//! Configuration types and their validation rules (contracts/config.md).
//!
//! Each file is deserialized into `Raw*` types that keep source spans, then validated into the
//! public types. A validation failure rejects only the offending value or entry and is reported
//! as `file:line: message`.

use std::fmt;
use std::net::IpAddr;
use std::ops::Range;

use globset::{GlobBuilder, GlobMatcher};
use ipnet::IpNet;
use regex::Regex;
use serde::Deserialize;
use toml::Spanned;

/// Runtime settings (`settings.toml`).
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub refresh_interval_ms: u64,
    /// How often each process's footprint is re-read (R-3).
    pub footprint_interval_ms: u64,
    pub recent_window_minutes: u64,
    pub detection: Detection,
    pub dns: Dns,
    pub windows: WindowsSettings,
    pub editor: EditorSettings,
    pub ui: UiSettings,
    pub sessions: SessionSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub browsers: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dns {
    pub enabled: bool,
    pub refresh_minutes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowsSettings {
    pub read_working_directory: CwdPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditorSettings {
    pub read_only_threshold_mb: u64,
    pub tab_width: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    pub mouse: bool,
}

/// `[sessions]` (002 contracts/config.md).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSettings {
    pub scrollback_lines: u64,
    pub attention_delay_ms: u64,
    pub max_sessions: u64,
    /// The leave-session key as written (`ctrl-]`, `f12`, …); parsed by `session::keys`.
    pub leave_key: String,
    /// A new git worktree by default for git projects (FR-031).
    pub worktrees: bool,
    /// Where new worktrees go; `None`: next to the project (FR-034).
    pub worktree_root: Option<std::path::PathBuf>,
}

/// When Fallow reads a Windows process's working directory (research R-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CwdPolicy {
    OnDetect,
    OnSelect,
    Never,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            refresh_interval_ms: 2000,
            footprint_interval_ms: 10_000,
            recent_window_minutes: 30,
            detection: Detection { browsers: false },
            dns: Dns {
                enabled: true,
                refresh_minutes: 15,
            },
            windows: WindowsSettings {
                read_working_directory: CwdPolicy::OnDetect,
            },
            editor: EditorSettings {
                read_only_threshold_mb: 10,
                tab_width: 4,
            },
            ui: UiSettings { mouse: true },
            sessions: SessionSettings {
                scrollback_lines: 10_000,
                attention_delay_ms: 5_000,
                max_sessions: 8,
                leave_key: "ctrl-]".into(),
                worktrees: true,
                worktree_root: None,
            },
        }
    }
}

/// What a signature identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    AgentCli,
    Script,
    Editor,
    LocalModelServer,
    Unknown,
    Browser,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "agent_cli" => Kind::AgentCli,
            "script" => Kind::Script,
            "editor" => Kind::Editor,
            "local_model_server" => Kind::LocalModelServer,
            "unknown" => Kind::Unknown,
            "browser" => Kind::Browser,
            _ => return None,
        })
    }
}

/// A validated detection signature (`signatures.toml`).
#[derive(Debug, Clone)]
pub struct Signature {
    pub id: String,
    pub enabled: bool,
    pub kind: Kind,
    pub provider: Option<String>,
    pub match_name: Option<NameGlob>,
    pub match_cmdline: Option<Regex>,
    pub model_from_cmdline: Option<Regex>,
    pub local_server: Option<LocalServer>,
}

impl Signature {
    /// True when every specified matcher matches (AND within one signature): `match.name`
    /// against the process name, and `match.cmdline` against the argv joined with spaces.
    /// A signature with a `match.cmdline` never matches a process whose command line is
    /// unknown. Across signatures, any matching signature counts (OR); see
    /// contracts/config.md.
    pub fn matches(&self, name: &str, cmdline: Option<&str>) -> bool {
        if let Some(glob) = &self.match_name {
            if !glob.matcher.is_match(name) {
                return false;
            }
        }
        if let Some(re) = &self.match_cmdline {
            match cmdline {
                Some(c) if re.is_match(c) => {}
                _ => return false,
            }
        }
        true
    }
}

/// A compiled `match.name` glob, keeping its source text for display.
#[derive(Debug, Clone)]
pub struct NameGlob {
    pub pattern: String,
    pub matcher: GlobMatcher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalServer {
    pub default_ports: Vec<u16>,
}

/// A validated LLM API provider (`providers.toml`).
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub hostnames: Vec<String>,
    pub ranges: Vec<IpNet>,
    pub enabled: bool,
}

/// A rejected value or entry: the byte span in the source file and the reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Invalid {
    pub span: Range<usize>,
    pub message: String,
}

impl Invalid {
    fn new(span: Range<usize>, message: impl fmt::Display) -> Self {
        Invalid {
            span,
            message: message.to_string(),
        }
    }
}

// ---- Raw (as written) forms -----------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawSettings {
    refresh_interval_ms: Option<Spanned<i64>>,
    footprint_interval_ms: Option<Spanned<i64>>,
    recent_window_minutes: Option<Spanned<i64>>,
    detection: Option<RawDetection>,
    dns: Option<RawDns>,
    windows: Option<RawWindows>,
    editor: Option<RawEditor>,
    ui: Option<RawUi>,
    sessions: Option<RawSessions>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSessions {
    scrollback_lines: Option<Spanned<i64>>,
    attention_delay_ms: Option<Spanned<i64>>,
    max_sessions: Option<Spanned<i64>>,
    leave_key: Option<Spanned<String>>,
    worktrees: Option<Spanned<String>>,
    worktree_root: Option<Spanned<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDetection {
    browsers: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDns {
    enabled: Option<bool>,
    refresh_minutes: Option<Spanned<i64>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawWindows {
    read_working_directory: Option<Spanned<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawEditor {
    read_only_threshold_mb: Option<Spanned<i64>>,
    tab_width: Option<Spanned<i64>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUi {
    mouse: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawSignatureFile {
    #[serde(default)]
    pub(crate) signature: Vec<Spanned<RawSignature>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawSignature {
    pub(crate) id: Option<Spanned<String>>,
    kind: Option<Spanned<String>>,
    enabled: Option<bool>,
    provider: Option<Spanned<String>>,
    #[serde(rename = "match")]
    matcher: Option<RawMatch>,
    model_from_cmdline: Option<Spanned<String>>,
    local_server: Option<Spanned<RawLocalServer>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMatch {
    name: Option<Spanned<String>>,
    cmdline: Option<Spanned<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLocalServer {
    #[serde(default)]
    default_ports: Vec<Spanned<i64>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawProviderFile {
    #[serde(default)]
    pub(crate) provider: Vec<Spanned<RawProvider>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawProvider {
    pub(crate) id: Option<Spanned<String>>,
    display_name: Option<String>,
    #[serde(default)]
    hostnames: Vec<Spanned<String>>,
    #[serde(default)]
    ranges: Vec<Spanned<String>>,
    enabled: Option<bool>,
}

/// Keys each file may contain, for unknown-key warnings. `Table` nests; `Tables` is an array
/// of tables whose entries share one key set.
pub(crate) enum Shape {
    Leaf,
    Table(&'static [(&'static str, Shape)]),
    Tables(&'static [(&'static str, Shape)]),
}

pub(crate) const SETTINGS_SHAPE: Shape = Shape::Table(&[
    ("refresh_interval_ms", Shape::Leaf),
    ("footprint_interval_ms", Shape::Leaf),
    ("recent_window_minutes", Shape::Leaf),
    ("detection", Shape::Table(&[("browsers", Shape::Leaf)])),
    (
        "dns",
        Shape::Table(&[("enabled", Shape::Leaf), ("refresh_minutes", Shape::Leaf)]),
    ),
    (
        "windows",
        Shape::Table(&[("read_working_directory", Shape::Leaf)]),
    ),
    (
        "editor",
        Shape::Table(&[
            ("read_only_threshold_mb", Shape::Leaf),
            ("tab_width", Shape::Leaf),
        ]),
    ),
    ("ui", Shape::Table(&[("mouse", Shape::Leaf)])),
    (
        "sessions",
        Shape::Table(&[
            ("scrollback_lines", Shape::Leaf),
            ("attention_delay_ms", Shape::Leaf),
            ("max_sessions", Shape::Leaf),
            ("leave_key", Shape::Leaf),
            ("worktrees", Shape::Leaf),
            ("worktree_root", Shape::Leaf),
        ]),
    ),
]);

pub(crate) const SIGNATURES_SHAPE: Shape = Shape::Table(&[(
    "signature",
    Shape::Tables(&[
        ("id", Shape::Leaf),
        ("kind", Shape::Leaf),
        ("enabled", Shape::Leaf),
        ("provider", Shape::Leaf),
        (
            "match",
            Shape::Table(&[("name", Shape::Leaf), ("cmdline", Shape::Leaf)]),
        ),
        ("model_from_cmdline", Shape::Leaf),
        (
            "local_server",
            Shape::Table(&[("default_ports", Shape::Leaf)]),
        ),
    ]),
)]);

pub(crate) const PROVIDERS_SHAPE: Shape = Shape::Table(&[(
    "provider",
    Shape::Tables(&[
        ("id", Shape::Leaf),
        ("display_name", Shape::Leaf),
        ("hostnames", Shape::Leaf),
        ("ranges", Shape::Leaf),
        ("enabled", Shape::Leaf),
    ]),
)]);

// ---- Validation -----------------------------------------------------------------------------

/// Overlays the values present in `raw` onto `base`. Invalid values keep the `base` value and
/// are returned as errors.
pub(crate) fn apply_settings(base: &mut Settings, raw: RawSettings) -> Vec<Invalid> {
    let mut errors = Vec::new();
    let mut int = |v: Option<Spanned<i64>>, min: i64, max: i64, rule: &str, out: &mut u64| {
        if let Some(v) = v {
            let n = *v.get_ref();
            if (min..=max).contains(&n) {
                *out = n as u64;
            } else {
                errors.push(Invalid::new(
                    v.span(),
                    format!("{n} is out of range: {rule}"),
                ));
            }
        }
    };
    int(
        raw.refresh_interval_ms,
        500,
        i64::MAX,
        "minimum 500",
        &mut base.refresh_interval_ms,
    );
    int(
        raw.footprint_interval_ms,
        500,
        3_600_000,
        "500..=3600000",
        &mut base.footprint_interval_ms,
    );
    int(
        raw.recent_window_minutes,
        1,
        i64::MAX,
        "minimum 1",
        &mut base.recent_window_minutes,
    );
    if let Some(d) = raw.detection {
        if let Some(b) = d.browsers {
            base.detection.browsers = b;
        }
    }
    if let Some(d) = raw.dns {
        if let Some(b) = d.enabled {
            base.dns.enabled = b;
        }
        int(
            d.refresh_minutes,
            1,
            60,
            "1..=60",
            &mut base.dns.refresh_minutes,
        );
    }
    if let Some(e) = raw.editor {
        int(
            e.read_only_threshold_mb,
            1,
            i64::MAX,
            "minimum 1",
            &mut base.editor.read_only_threshold_mb,
        );
        int(e.tab_width, 1, 16, "1..=16", &mut base.editor.tab_width);
    }
    if let Some(u) = raw.ui {
        if let Some(b) = u.mouse {
            base.ui.mouse = b;
        }
    }
    if let Some(sx) = raw.sessions {
        int(
            sx.scrollback_lines,
            1000,
            100_000,
            "1000..=100000",
            &mut base.sessions.scrollback_lines,
        );
        int(
            sx.attention_delay_ms,
            1000,
            600_000,
            "1000..=600000",
            &mut base.sessions.attention_delay_ms,
        );
        int(
            sx.max_sessions,
            1,
            16,
            "1..=16",
            &mut base.sessions.max_sessions,
        );
        if let Some(w) = sx.worktrees {
            match w.get_ref().as_str() {
                "auto" => base.sessions.worktrees = true,
                "never" => base.sessions.worktrees = false,
                other => errors.push(Invalid::new(
                    w.span(),
                    format!("worktrees = {other:?}: expected \"auto\" or \"never\""),
                )),
            }
        }
        if let Some(r) = sx.worktree_root {
            let v = r.get_ref().trim();
            if v.is_empty() {
                base.sessions.worktree_root = None;
            } else if std::path::Path::new(v).is_absolute() {
                base.sessions.worktree_root = Some(v.into());
            } else {
                errors.push(Invalid::new(
                    r.span(),
                    format!("worktree_root = {v:?}: expected an absolute folder path or \"\""),
                ));
            }
        }
        if let Some(k) = sx.leave_key {
            if crate::session::keys::LeaveKey::parse(k.get_ref()).is_some() {
                base.sessions.leave_key = k.get_ref().clone();
            } else {
                errors.push(Invalid::new(
                    k.span(),
                    format!(
                        "leave_key = {:?}: expected one key such as \"ctrl-]\", \"ctrl-\\\" or \"f12\"",
                        k.get_ref()
                    ),
                ));
            }
        }
    }
    if let Some(w) = raw.windows {
        if let Some(v) = w.read_working_directory {
            match v.get_ref().as_str() {
                "on_detect" => base.windows.read_working_directory = CwdPolicy::OnDetect,
                "on_select" => base.windows.read_working_directory = CwdPolicy::OnSelect,
                "never" => base.windows.read_working_directory = CwdPolicy::Never,
                other => errors.push(Invalid::new(
                    v.span(),
                    format!(
                        "read_working_directory = {other:?}: expected \"on_detect\", \"on_select\" or \"never\""
                    ),
                )),
            }
        }
    }
    errors
}

/// Validates one signature entry. `entry` is the span of the whole entry, used when the
/// problem is a missing key.
pub(crate) fn validate_signature(
    raw: RawSignature,
    entry: Range<usize>,
    provider_ids: &[&str],
) -> Result<Signature, Invalid> {
    let id = raw
        .id
        .ok_or_else(|| Invalid::new(entry.clone(), "signature without `id`"))?;
    let id_text = id.get_ref().trim().to_string();
    if id_text.is_empty() {
        return Err(Invalid::new(id.span(), "signature `id` is empty"));
    }
    let ctx = |m: String| format!("signature {id_text:?}: {m}");
    let kind_raw = raw
        .kind
        .ok_or_else(|| Invalid::new(entry.clone(), ctx("`kind` is required".into())))?;
    let kind = Kind::parse(kind_raw.get_ref()).ok_or_else(|| {
        Invalid::new(
            kind_raw.span(),
            ctx(format!(
                "unknown kind {:?}: expected agent_cli, script, editor, local_model_server, unknown or browser",
                kind_raw.get_ref()
            )),
        )
    })?;
    let matcher = raw.matcher.unwrap_or_default();
    if matcher.name.is_none() && matcher.cmdline.is_none() {
        return Err(Invalid::new(
            entry,
            ctx("needs at least one of `match.name` and `match.cmdline`".into()),
        ));
    }
    let match_name = match matcher.name {
        None => None,
        Some(g) => {
            let glob = GlobBuilder::new(g.get_ref())
                .case_insensitive(cfg!(any(windows, target_os = "macos")))
                .build()
                .map_err(|e| {
                    Invalid::new(g.span(), ctx(format!("invalid match.name glob: {e}")))
                })?;
            Some(NameGlob {
                pattern: g.get_ref().clone(),
                matcher: glob.compile_matcher(),
            })
        }
    };
    let regex = |r: &Spanned<String>, what: &str| {
        Regex::new(r.get_ref())
            .map_err(|e| Invalid::new(r.span(), ctx(format!("invalid {what} regex: {e}"))))
    };
    let match_cmdline = matcher
        .cmdline
        .as_ref()
        .map(|r| regex(r, "match.cmdline"))
        .transpose()?;
    let model_from_cmdline = match &raw.model_from_cmdline {
        None => None,
        Some(r) => {
            let re = regex(r, "model_from_cmdline")?;
            if re.captures_len() < 2 {
                return Err(Invalid::new(
                    r.span(),
                    ctx("model_from_cmdline needs a capture group for the model name".into()),
                ));
            }
            Some(re)
        }
    };
    let local_server = match raw.local_server {
        None => None,
        Some(ls) if kind != Kind::LocalModelServer => {
            return Err(Invalid::new(
                ls.span(),
                ctx("`local_server` is allowed only with kind = \"local_model_server\"".into()),
            ));
        }
        Some(ls) => {
            let mut ports = Vec::new();
            for p in &ls.get_ref().default_ports {
                let n = *p.get_ref();
                let port = u16::try_from(n).ok().filter(|&n| n != 0).ok_or_else(|| {
                    Invalid::new(p.span(), ctx(format!("port {n} is not in 1..=65535")))
                })?;
                ports.push(port);
            }
            Some(LocalServer {
                default_ports: ports,
            })
        }
    };
    let provider = match raw.provider {
        None => None,
        Some(p) if provider_ids.contains(&p.get_ref().as_str()) => Some(p.get_ref().clone()),
        Some(p) => {
            return Err(Invalid::new(
                p.span(),
                ctx(format!(
                    "provider {:?} does not match any provider id",
                    p.get_ref()
                )),
            ));
        }
    };
    Ok(Signature {
        id: id_text,
        enabled: raw.enabled.unwrap_or(true),
        kind,
        provider,
        match_name,
        match_cmdline,
        model_from_cmdline,
        local_server,
    })
}

/// Validates one provider entry.
pub(crate) fn validate_provider(
    raw: RawProvider,
    entry: Range<usize>,
) -> Result<Provider, Invalid> {
    let id = raw
        .id
        .ok_or_else(|| Invalid::new(entry.clone(), "provider without `id`"))?;
    let id_text = id.get_ref().trim().to_string();
    if id_text.is_empty() {
        return Err(Invalid::new(id.span(), "provider `id` is empty"));
    }
    let ctx = |m: String| format!("provider {id_text:?}: {m}");
    if raw.hostnames.is_empty() && raw.ranges.is_empty() {
        return Err(Invalid::new(
            entry,
            ctx("needs at least one hostname or range".into()),
        ));
    }
    let mut hostnames = Vec::new();
    for h in &raw.hostnames {
        let name = h.get_ref();
        if let Err(why) = check_hostname(name) {
            return Err(Invalid::new(
                h.span(),
                ctx(format!("hostname {name:?} {why}")),
            ));
        }
        hostnames.push(name.to_ascii_lowercase());
    }
    let mut ranges = Vec::new();
    for r in &raw.ranges {
        let net: IpNet = r.get_ref().parse().map_err(|_| {
            Invalid::new(
                r.span(),
                ctx(format!("range {:?} is not valid CIDR", r.get_ref())),
            )
        })?;
        ranges.push(net.trunc());
    }
    Ok(Provider {
        display_name: raw.display_name.unwrap_or_else(|| id_text.clone()),
        id: id_text,
        hostnames,
        ranges,
        enabled: raw.enabled.unwrap_or(true),
    })
}

/// A plain DNS name: no wildcard, no IP literal, letters/digits/hyphens in 1–63 character
/// labels, at most 253 characters.
fn check_hostname(name: &str) -> Result<(), &'static str> {
    if name.contains('*') {
        return Err("must not contain wildcards");
    }
    if name.parse::<IpAddr>().is_ok() || name.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() {
        return Err("is an IP literal, not a DNS name");
    }
    if name.is_empty() || name.len() > 253 {
        return Err("is not a valid DNS name (length)");
    }
    let labels: Vec<&str> = name.split('.').collect();
    for label in &labels {
        let ok = !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !ok {
            return Err("is not a valid DNS name");
        }
    }
    if labels
        .last()
        .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("is not a valid DNS name (numeric top-level label)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_hostname;

    #[test]
    fn hostnames() {
        assert!(check_hostname("api.anthropic.com").is_ok());
        assert!(check_hostname("openrouter.ai").is_ok());
        assert!(check_hostname("*.openai.com").is_err());
        assert!(check_hostname("1.2.3.4").is_err());
        assert!(check_hostname("::1").is_err());
        assert!(check_hostname("[::1]").is_err());
        assert!(check_hostname("api..com").is_err());
        assert!(check_hostname("-api.com").is_err());
        assert!(check_hostname("api.com.").is_err());
        assert!(check_hostname("a_b.com").is_err());
    }
}
