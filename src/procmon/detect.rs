//! LLM process detection (FR-020, FR-022, FR-023, FR-025, FR-026, FR-027).
//!
//! Inputs are process metadata and connection endpoints only. There is no environment-based
//! signal (FR-024) and no payload inspection (FR-027).

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use compact_str::CompactString;
use ipnet::IpNet;
use smallvec::SmallVec;

use super::platform::parse::is_local;
use super::snapshot::{
    ConnState, Connection, DetectionSignal, Kind, LlmProcess, ProcessKey, SignalKind, Status, Via,
};
use crate::config::{Provider, Signature};

/// Provider addresses: resolved hostnames plus built-in ranges (data-model.md
/// "ResolvedProviderSet"). Replaced as a whole after each DNS pass.
#[derive(Debug, Clone, Default)]
pub struct ProviderSet {
    resolved: HashMap<IpAddr, CompactString>,
    ranges: Vec<(IpNet, CompactString)>,
}

impl ProviderSet {
    /// The enabled providers' built-in ranges plus `resolved` addresses.
    pub fn new(
        providers: &[Provider],
        resolved: impl IntoIterator<Item = (IpAddr, CompactString)>,
    ) -> Self {
        let enabled: HashSet<&str> = providers
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.id.as_str())
            .collect();
        let ranges = providers
            .iter()
            .filter(|p| p.enabled)
            .flat_map(|p| {
                p.ranges
                    .iter()
                    .map(|r| (*r, CompactString::from(p.id.as_str())))
            })
            .collect();
        let resolved = resolved
            .into_iter()
            .filter(|(_, id)| enabled.contains(id.as_str()))
            .collect();
        ProviderSet { resolved, ranges }
    }

    /// The provider an address belongs to, and how that was known. Resolved addresses win.
    pub fn lookup(&self, ip: IpAddr) -> Option<(&str, Via)> {
        let ip = canonical(ip);
        if let Some(id) = self.resolved.get(&ip) {
            return Some((id.as_str(), Via::Resolved));
        }
        self.ranges
            .iter()
            .find(|(net, _)| net.contains(&ip))
            .map(|(_, id)| (id.as_str(), Via::Range))
    }

    pub fn resolved_count(&self) -> usize {
        self.resolved.len()
    }
}

/// IPv4-mapped IPv6 addresses (dual-stack sockets) compare as IPv4.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// One process as detection sees it.
#[derive(Debug, Clone)]
pub struct ProcView<'a> {
    pub key: ProcessKey,
    pub name: &'a str,
    /// argv joined with spaces, if it could be read.
    pub cmdline: Option<&'a str>,
}

/// Compiled signatures and the browser switch.
#[derive(Debug, Clone)]
pub struct Detector {
    signatures: Vec<Signature>,
    browsers: bool,
}

/// Process names that suggest a script when a process is seen only through a connection.
const INTERPRETERS: &[&str] = &[
    "python",
    "python3",
    "node",
    "deno",
    "bun",
    "ruby",
    "perl",
    "php",
    "java",
    "dotnet",
    "pwsh",
    "powershell",
];

impl Detector {
    pub fn new(signatures: &[Signature], browsers: bool) -> Self {
        Detector {
            signatures: signatures.iter().filter(|s| s.enabled).cloned().collect(),
            browsers,
        }
    }

    /// Indexes of the matching signatures (OR across signatures; each one is AND within).
    /// On Windows a trailing `.exe` is ignored, so `match.name = "claude"` matches
    /// `claude.exe`.
    pub fn hits(&self, name: &str, cmdline: Option<&str>) -> SmallVec<[u16; 2]> {
        let name = normalize_name(name);
        self.signatures
            .iter()
            .enumerate()
            .filter(|(_, s)| s.matches(name, cmdline))
            .map(|(i, _)| i as u16)
            .collect()
    }

    pub fn signature(&self, i: u16) -> &Signature {
        &self.signatures[i as usize]
    }

    /// Detects the LLM processes among `procs` (FR-020–FR-026). `hits` holds each process's
    /// signature matches (cached by the caller). Status is left `Idle`; `status::evaluate`
    /// sets it.
    pub fn detect(
        &self,
        procs: &[ProcView<'_>],
        hits: &HashMap<ProcessKey, SmallVec<[u16; 2]>>,
        conns: &[Connection],
        providers: &ProviderSet,
    ) -> HashMap<ProcessKey, LlmProcess> {
        let mut out: HashMap<ProcessKey, LlmProcess> = HashMap::new();
        let mut excluded: HashSet<ProcessKey> = HashSet::new();
        let mut models: HashMap<ProcessKey, CompactString> = HashMap::new();

        for p in procs {
            let Some(h) = hits.get(&p.key) else { continue };
            let is_browser = h.iter().any(|&i| self.signature(i).kind == Kind::Browser);
            if is_browser && !self.browsers {
                // FR-026: browsers are excluded from detection, connections included.
                excluded.insert(p.key);
                continue;
            }
            let sigs: SmallVec<[&Signature; 2]> = h.iter().map(|&i| self.signature(i)).collect();
            let Some(first) = sigs.first() else { continue };
            let mut llm = new_llm(p.key, first.kind);
            for s in &sigs {
                llm.signals
                    .push(DetectionSignal::new(SignalKind::Signature {
                        id: s.id.as_str().into(),
                    }));
            }
            llm.provider = sigs
                .iter()
                .find_map(|s| s.provider.as_deref())
                .map(Into::into);
            if let Some(model) = sigs.iter().find_map(|s| {
                let re = s.model_from_cmdline.as_ref()?;
                let c = re.captures(p.cmdline?)?;
                c.iter()
                    .skip(1)
                    .flatten()
                    .next()
                    .map(|m| CompactString::from(m.as_str()))
            }) {
                models.insert(p.key, model.clone());
                llm.model = Some(model);
            }
            out.insert(p.key, llm);
        }

        // Local model servers and the ports they listen on.
        let servers: HashSet<ProcessKey> = out
            .values()
            .filter(|l| l.kind == Kind::LocalModelServer)
            .map(|l| l.key)
            .collect();
        let mut server_ports: HashMap<u16, ProcessKey> = HashMap::new();
        for c in conns {
            if c.state == ConnState::Listen && servers.contains(&c.owner) {
                server_ports.insert(c.local.port(), c.owner);
            }
        }

        let names: HashMap<ProcessKey, &str> = procs.iter().map(|p| (p.key, p.name)).collect();
        for c in conns {
            if c.state != ConnState::Established || excluded.contains(&c.owner) {
                continue;
            }
            let Some(name) = names.get(&c.owner) else {
                continue;
            };
            let signal = if c.remote.port() == 443 {
                providers
                    .lookup(c.remote.ip())
                    .map(|(id, via)| SignalKind::ProviderConnection {
                        provider_id: id.into(),
                        via,
                    })
            } else {
                server_ports
                    .get(&c.remote.port())
                    .filter(|&&server| server != c.owner && is_local(c.local, c.remote))
                    .map(|&server| SignalKind::LocalModelConnection { server })
            };
            let Some(kind) = signal else { continue };
            let llm = out.entry(c.owner).or_insert_with(|| {
                let interpreter =
                    INTERPRETERS.contains(&normalize_name(name).to_ascii_lowercase().as_str());
                new_llm(
                    c.owner,
                    if interpreter {
                        Kind::Script
                    } else {
                        Kind::Unknown
                    },
                )
            });
            let signal = DetectionSignal::new(kind);
            if llm.signals.contains(&signal) {
                continue;
            }
            match &signal.kind {
                SignalKind::ProviderConnection { provider_id, .. } if llm.provider.is_none() => {
                    llm.provider = Some(provider_id.clone());
                }
                SignalKind::LocalModelConnection { server } if llm.model.is_none() => {
                    llm.model = models.get(server).cloned();
                }
                _ => {}
            }
            llm.signals.push(signal);
        }

        for llm in out.values_mut() {
            llm.confidence = llm
                .signals
                .iter()
                .map(|s| s.confidence)
                .max()
                .unwrap_or(llm.confidence);
        }
        out
    }
}

fn new_llm(key: ProcessKey, kind: Kind) -> LlmProcess {
    LlmProcess {
        key,
        kind,
        signals: SmallVec::new(),
        confidence: super::snapshot::Confidence::Likely,
        provider: None,
        model: None,
        status: Status::Idle,
    }
}

/// The name signatures are matched against: on Windows without a trailing `.exe`.
pub fn normalize_name(name: &str) -> &str {
    if cfg!(windows) {
        let n = name.len();
        if n > 4 && name[n - 4..].eq_ignore_ascii_case(".exe") {
            return &name[..n - 4];
        }
    }
    name
}
