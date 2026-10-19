//! The procmon worker (R-1, R-6, FR-010, FR-015, FR-030): a thread that refreshes every
//! `refresh_interval_ms` and sends an `Arc<Snapshot>` to the UI.
//!
//! Enumeration, names, parents, owners, start times and CPU come from `sampler.rs`; command
//! lines, working directories, footprints and connections from the Fallow-owned platform layer
//! (R-1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use compact_str::CompactString;
use smallvec::SmallVec;

use super::detect::{Detector, ProcView, ProviderSet};
use super::group;
use super::platform::{OsPlatform, Platform, PlatformError, RawConn, TcpQuery};
use super::sampler::{Sample, Sampler};
use super::snapshot::{
    Confidence, Connection, CwdState, DetectionSignal, Footprint, FootprintMetric, Kind,
    LlmProcess, Note, NoteKind, ProcessKey, ProcessRecord, SignalKind, Snapshot, Status, Uid,
};
use super::status::ConnectionFacts;
use super::summary;
use crate::config::{CwdPolicy, Settings, Signature};
use crate::dns::SharedProviders;

/// One process as sampled from the OS this refresh.
#[derive(Debug, Clone)]
pub struct ProcSample {
    pub key: ProcessKey,
    pub parent_pid: Option<u32>,
    pub name: CompactString,
    pub owner: Option<Uid>,
    /// % of one core since the last refresh.
    pub cpu: f32,
}

/// Messages from the UI to the worker.
#[derive(Debug, Clone)]
pub enum Request {
    /// The user selected this process (Windows `on_select` working-directory policy).
    Selected(ProcessKey),
    /// 002 R-8: this PID is the agent of a Fallow session. It is always a group root.
    Track { pid: u32, session: u32 },
    /// The session's agent exited or the session was closed.
    Untrack { pid: u32 },
}

/// Per-process state carried between refreshes, keyed by (PID, start time) so a reused PID
/// never inherits anything.
#[derive(Debug, Default)]
struct Cached {
    cmdline: Option<Arc<[Box<str>]>>,
    joined: Option<Box<str>>,
    hits: SmallVec<[u16; 2]>,
    cpu: [f32; 5],
    cpu_filled: u8,
    cpu_next: u8,
    /// Windows working directory, read once under the R-10 policy.
    cwd_once: Option<CwdState>,
    cwd_requested: bool,
    /// The last footprint read and when (R-3: read at most every `footprint_interval_ms`).
    footprint: Option<(Footprint, Instant)>,
}

impl Cached {
    fn push_cpu(&mut self, v: f32) {
        self.cpu[self.cpu_next as usize] = v;
        self.cpu_next = (self.cpu_next + 1) % 5;
        self.cpu_filled = (self.cpu_filled + 1).min(5);
    }

    /// Mean over the samples taken so far (at most the last 5).
    fn mean_cpu(&self) -> f32 {
        if self.cpu_filled == 0 {
            return 0.0;
        }
        self.cpu.iter().sum::<f32>() / self.cpu_filled as f32
    }
}

/// Process names whose working directory is never read on Windows (R-10): protected and
/// system processes. Detected LLM processes are user processes, so this is a safety net.
const WINDOWS_PROTECTED: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "memory compression",
    "secure system",
];

/// The refresh logic, independent of the thread and of enumeration so it can be driven directly.
pub struct Monitor<P: Platform> {
    platform: P,
    detector: Detector,
    providers: SharedProviders,
    cwd_policy: CwdPolicy,
    footprint_every: Duration,
    all_users: bool,
    own_pid: u32,
    notes: Vec<Note>,
    cache: HashMap<ProcessKey, Cached>,
    conns: Vec<RawConn>,
    /// PIDs of session agents (002 R-8).
    tracked: HashMap<u32, u32>,
}

impl<P: Platform> Monitor<P> {
    pub fn new(
        platform: P,
        signatures: &[Signature],
        settings: &Settings,
        providers: SharedProviders,
        all_users: bool,
        extra_notes: Vec<Note>,
    ) -> Self {
        let mut coverage = String::from("Not covered: containers, remote machines");
        if cfg!(windows) {
            coverage.push_str(", WSL2");
        }
        let mut notes = vec![Note {
            kind: NoteKind::Coverage,
            text: coverage.into(),
        }];
        notes.extend(extra_notes);
        Monitor {
            platform,
            detector: Detector::new(signatures, settings.detection.browsers),
            providers,
            cwd_policy: settings.windows.read_working_directory,
            footprint_every: Duration::from_millis(settings.footprint_interval_ms),
            all_users,
            own_pid: std::process::id(),
            notes,
            cache: HashMap::new(),
            conns: Vec::new(),
            tracked: HashMap::new(),
        }
    }

    pub fn request(&mut self, r: Request) {
        match r {
            Request::Selected(key) => {
                if let Some(c) = self.cache.get_mut(&key) {
                    c.cwd_requested = true;
                }
            }
            Request::Track { pid, session } => {
                self.tracked.insert(pid, session);
            }
            Request::Untrack { pid } => {
                self.tracked.remove(&pid);
            }
        }
    }

    /// One refresh: detection, grouping, status and summary from this sample of processes.
    pub fn cycle(&mut self, procs: &[ProcSample], total_ram: u64, available_ram: u64) -> Snapshot {
        self.cycle_at(procs, total_ram, available_ram, Instant::now())
    }

    /// `cycle` with an explicit clock, for tests of the footprint interval.
    pub fn cycle_at(
        &mut self,
        procs: &[ProcSample],
        total_ram: u64,
        available_ram: u64,
        now: Instant,
    ) -> Snapshot {
        // "Records without a start time are dropped."
        let procs: Vec<&ProcSample> = procs.iter().filter(|p| p.key.start_time != 0).collect();
        let own_owner = procs
            .iter()
            .find(|p| p.key.pid == self.own_pid)
            .and_then(|p| p.owner.clone());
        // FR-030: only the current user's processes unless all-users mode is active.
        let visible: Vec<&ProcSample> = procs
            .into_iter()
            .filter(|p| self.all_users || own_owner.is_none() || p.owner == own_owner)
            .collect();

        let alive: HashSet<ProcessKey> = visible.iter().map(|p| p.key).collect();
        self.cache.retain(|k, _| alive.contains(k));
        for p in &visible {
            let entry = self.cache.entry(p.key);
            let c = entry.or_insert_with(|| {
                // Read once per (PID, start time) and cached (R-1).
                let cmdline = self.platform.cmdline(p.key).ok();
                let joined = cmdline.as_ref().map(|a| a.join(" ").into_boxed_str());
                let hits = self.detector.hits(&p.name, joined.as_deref());
                Cached {
                    cmdline,
                    joined,
                    hits,
                    ..Default::default()
                }
            });
            c.push_cpu(p.cpu);
        }

        let by_pid: HashMap<u32, ProcessKey> = visible.iter().map(|p| (p.key.pid, p.key)).collect();
        let providers: Arc<ProviderSet> =
            self.providers.lock().map(|s| s.clone()).unwrap_or_default();

        // Connections of the visible processes; servers first need their signature hits.
        let servers: HashSet<u32> = visible
            .iter()
            .filter(|p| {
                self.cache[&p.key]
                    .hits
                    .iter()
                    .any(|&i| self.detector.signature(i).kind == Kind::LocalModelServer)
            })
            .map(|p| p.key.pid)
            .collect();
        let candidates: HashSet<u32> = by_pid.keys().copied().collect();
        let is_provider = |ip| providers.lookup(ip).is_some();
        self.conns.clear();
        let query = TcpQuery {
            candidates: &candidates,
            servers: &servers,
            is_provider: &is_provider,
        };
        let _ = self.platform.tcp_table(&query, &mut self.conns);
        let conns: Vec<Connection> = self
            .conns
            .iter()
            .filter_map(|c| {
                Some(Connection {
                    owner: *by_pid.get(&c.pid)?,
                    local: c.local,
                    remote: c.remote,
                    state: c.state,
                })
            })
            .collect();

        let views: Vec<ProcView<'_>> = visible
            .iter()
            .map(|p| ProcView {
                key: p.key,
                name: &p.name,
                cmdline: self.cache[&p.key].joined.as_deref(),
            })
            .collect();
        let hits: HashMap<ProcessKey, SmallVec<[u16; 2]>> = self
            .cache
            .iter()
            .filter(|(_, c)| !c.hits.is_empty())
            .map(|(k, c)| (*k, c.hits.clone()))
            .collect();
        let mut llm = self.detector.detect(&views, &hits, &conns, &providers);

        let mut parents =
            group::link_parents(visible.iter().map(|p| (p.key, p.parent_pid)), &by_pid);
        // Session agents are always listed and always root their own group, even when Fallow
        // itself runs under another agent (R-8).
        let mut sessions = HashMap::new();
        for (&pid, &id) in &self.tracked {
            let Some(&key) = by_pid.get(&pid) else {
                continue;
            };
            sessions.insert(key, id);
            parents.insert(key, None);
            let signal = DetectionSignal::new(SignalKind::Session { id });
            llm.entry(key)
                .and_modify(|l| l.signals.push(signal.clone()))
                .or_insert_with(|| LlmProcess {
                    key,
                    kind: Kind::Session,
                    signals: smallvec::smallvec![signal],
                    confidence: Confidence::Certain,
                    provider: None,
                    model: None,
                    status: Status::Idle,
                });
        }
        let llm_keys: HashSet<ProcessKey> = llm.keys().copied().collect();
        let mut groups = group::build(&parents, &llm_keys);

        // Footprint and working directory only for group members (R-1 cost control).
        let members: Vec<ProcessKey> = groups
            .iter()
            .flat_map(|g| std::iter::once(g.root).chain(g.members.iter().copied()))
            .collect();
        let sample: HashMap<ProcessKey, &ProcSample> =
            visible.iter().map(|p| (p.key, *p)).collect();
        let mut records = HashMap::with_capacity(members.len());
        let mut footprints = HashMap::with_capacity(members.len());
        for key in members {
            if records.contains_key(&key) {
                continue;
            }
            // Footprints cost far more than the process list (smaps_rollup walks the page
            // tables, ~1 ms per GB mapped), so each process's figure is re-read at most every
            // `footprint_interval_ms`; CPU and status still update every refresh (R-3).
            let cached = self.cache.get(&key).and_then(|c| c.footprint);
            let footprint = match cached {
                Some((f, at)) if now.saturating_duration_since(at) < self.footprint_every => {
                    Some(f)
                }
                _ => {
                    let f = self.platform.footprint(key).ok();
                    if let (Some(c), Some(f)) = (self.cache.get_mut(&key), f) {
                        c.footprint = Some((f, now));
                    }
                    f
                }
            };
            if let Some(f) = footprint {
                footprints.insert(key, f);
            }
            let is_llm = llm.contains_key(&key);
            let cwd = self.cwd(key, sample[&key].name.as_str(), is_llm);
            let c = &self.cache[&key];
            let p = sample[&key];
            records.insert(
                key,
                ProcessRecord {
                    key,
                    parent: parents.get(&key).copied().flatten(),
                    name: p.name.clone(),
                    cmdline: c.cmdline.clone(),
                    owner: p.owner.clone(),
                    cpu_samples: c.cpu,
                    footprint,
                    cwd,
                },
            );
        }
        group::totals(
            &mut groups,
            |k| footprints.get(&k).map(|f| f.bytes),
            |k| self.cache.get(&k).map_or(0.0, |c| c.mean_cpu()),
        );

        let servers: HashSet<ProcessKey> = llm
            .values()
            .filter(|l| l.kind == Kind::LocalModelServer)
            .map(|l| l.key)
            .collect();
        let facts = ConnectionFacts::new(&conns, &servers, &providers);
        for l in llm.values_mut() {
            let cpu = self.cache.get(&l.key).map_or(0.0, |c| c.mean_cpu());
            l.status = super::status::evaluate(&facts.input(l.key, l.kind, cpu));
        }

        let own = ProcessKey {
            pid: self.own_pid,
            start_time: by_pid.get(&self.own_pid).map_or(0, |k| k.start_time),
        };
        let fallow = self.platform.footprint(own).unwrap_or(Footprint {
            bytes: 0,
            metric: default_metric(),
        });
        let summary =
            summary::summarize(&groups, &llm, &footprints, total_ram, available_ram, fallow);
        Snapshot {
            taken_at: SystemTime::now(),
            groups,
            llm,
            records,
            summary,
            notes: self.notes.clone(),
            sessions,
        }
    }

    /// Working directory under the platform's policy (R-10 on Windows).
    fn cwd(&mut self, key: ProcessKey, name: &str, is_llm: bool) -> CwdState {
        if !cfg!(windows) {
            return match self.platform.cwd(key) {
                Ok(p) => CwdState::Known(p),
                Err(e) => CwdState::Unavailable(e.to_string().into()),
            };
        }
        let lower = name.to_ascii_lowercase();
        if key.pid <= 4 || WINDOWS_PROTECTED.contains(&lower.as_str()) {
            return CwdState::Unavailable("protected process".into());
        }
        let policy = self.cwd_policy;
        let c = self.cache.get(&key);
        if let Some(done) = c.and_then(|c| c.cwd_once.clone()) {
            return done;
        }
        let requested = c.is_some_and(|c| c.cwd_requested);
        let read_now = match policy {
            CwdPolicy::Never => {
                return CwdState::Unavailable("windows.read_working_directory = never".into());
            }
            CwdPolicy::OnDetect => is_llm,
            CwdPolicy::OnSelect => requested,
        };
        if !read_now {
            return if requested {
                CwdState::Pending
            } else {
                CwdState::Unknown
            };
        }
        let state = match self.platform.cwd(key) {
            Ok(p) => CwdState::Known(p),
            Err(PlatformError::PermissionDenied) => CwdState::Unavailable("access denied".into()),
            Err(e) => CwdState::Unavailable(e.to_string().into()),
        };
        if let Some(c) = self.cache.get_mut(&key) {
            c.cwd_once = Some(state.clone());
        }
        state
    }
}

fn default_metric() -> FootprintMetric {
    if cfg!(target_os = "macos") {
        FootprintMetric::PhysFootprint
    } else if cfg!(windows) {
        FootprintMetric::PrivateWorkingSet
    } else {
        FootprintMetric::Pss
    }
}

/// The UI side of a running worker.
pub struct WorkerHandle {
    pub snapshots: Receiver<Arc<Snapshot>>,
    pub requests: Sender<Request>,
    pub cycles: Arc<AtomicU64>,
}

/// What the worker needs from configuration and the command line.
pub struct WorkerConfig {
    pub settings: Settings,
    pub signatures: Vec<Signature>,
    pub providers: SharedProviders,
    pub all_users: bool,
    pub notes: Vec<Note>,
}

/// Starts the `procmon` thread (256 KiB stack). Call it after the first frame (R-6).
pub fn spawn(cfg: WorkerConfig) -> std::io::Result<WorkerHandle> {
    let (tx, rx): (SyncSender<Arc<Snapshot>>, _) = mpsc::sync_channel(1);
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let cycles = Arc::new(AtomicU64::new(0));
    let counter = cycles.clone();
    let interval = Duration::from_millis(cfg.settings.refresh_interval_ms);
    thread::Builder::new()
        .name("procmon".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut sampler = Sampler::default();
            let mut monitor = Monitor::new(
                OsPlatform::default(),
                &cfg.signatures,
                &cfg.settings,
                cfg.providers,
                cfg.all_users,
                cfg.notes,
            );
            loop {
                let started = Instant::now();
                loop {
                    match req_rx.try_recv() {
                        Ok(r) => monitor.request(r),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
                let (procs, total, avail) = sampler.sample();
                let snap = monitor.cycle(&procs, total, avail);
                counter.fetch_add(1, Ordering::Relaxed);
                if tx.send(Arc::new(snap)).is_err() {
                    return;
                }
                if let Some(rest) = interval.checked_sub(started.elapsed()) {
                    thread::sleep(rest);
                }
            }
        })?;
    Ok(WorkerHandle {
        snapshots: rx,
        requests: req_tx,
        cycles,
    })
}
