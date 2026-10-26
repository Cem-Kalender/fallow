//! The squad file (004 contracts/squad-file.md, spec FR-016 to FR-019).
//!
//! A squad declares the roles of a coordination burst, the gates whose results Fallow observes,
//! and the conditions for publishing the work. It lives in the project at `.fallow/squad.toml`,
//! versioned with the code; Fallow reads it when a burst starts and never writes it. Without the
//! file, the built-in squad is used. Roles for all projects may also come from `roles.toml` in
//! Fallow's config folder; the squad's own roles override them by name.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Spanned;

use crate::config::merge::line_of;

pub const SQUAD_FILE: &str = ".fallow/squad.toml";
pub const ROLES_FILE: &str = "roles.toml";
pub const MAX_ROLES: usize = 16;
pub const MAX_GATES: usize = 8;

/// What a role may do in the repository; mapped to each agent CLI's own flags (research R-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    ReadOnly,
    Edit,
    EditRun,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s {
            "read-only" => Level::ReadOnly,
            "edit" => Level::Edit,
            "edit-run" => Level::EditRun,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::ReadOnly => "read-only",
            Level::Edit => "edit",
            Level::EditRun => "edit-run",
        }
    }
}

/// A group of Fallow's tools a role may use (contracts/mcp-tools.md "Tool list per role").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tool {
    Board,
    Events,
    Deliver,
    Tasks,
    Report,
    Start,
    Sessions,
}

impl Tool {
    pub fn parse(s: &str) -> Option<Tool> {
        Some(match s {
            "board" => Tool::Board,
            "events" => Tool::Events,
            "deliver" => Tool::Deliver,
            "tasks" => Tool::Tasks,
            "report" => Tool::Report,
            "start" => Tool::Start,
            "sessions" => Tool::Sessions,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tools {
    All,
    Some(BTreeSet<Tool>),
}

impl Tools {
    pub fn allows(&self, t: Tool) -> bool {
        match self {
            Tools::All => true,
            Tools::Some(s) => s.contains(&t),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub name: String,
    pub description: String,
    pub level: Level,
    /// A profile id; the agent CLI this role runs.
    pub agent: String,
    pub model: Option<String>,
    pub tools: Tools,
    /// Event name patterns this role may signal (`*` matches any characters).
    pub signals: Vec<String>,
    pub max: u8,
    /// Per-agent flags replacing Fallow's mapping for this role's level.
    pub flags: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failures {
    /// Recognize common test runners' output (research R-5).
    Auto,
    ExitCode,
    /// A regex with one capture group naming a failing check.
    Regex(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub name: String,
    pub command: String,
    pub windows: Option<String>,
    pub macos: Option<String>,
    pub linux: Option<String>,
    pub timeout_min: u32,
    pub baseline: bool,
    pub failures: Failures,
}

impl Gate {
    /// The command for an OS name as `std::env::consts::OS` gives it.
    pub fn command_for(&self, os: &str) -> &str {
        let o = match os {
            "windows" => &self.windows,
            "macos" => &self.macos,
            _ => &self.linux,
        };
        o.as_deref()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(&self.command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publish {
    /// A draft pull request if the project has a remote, else a local merge.
    Auto,
    DraftPr,
    LocalMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    OnApproval,
    Automatic,
}

/// One condition for publishing the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// The final run of this gate passed (or failed only as the baseline did).
    Gate(String),
    /// Every task handed to a session in this role is reported done.
    AllDone(String),
    /// Signalled by a session in this role (every one of them, if several).
    Signal { event: String, role: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Squad {
    pub name: String,
    pub source: Source,
    pub publish: Publish,
    pub merge: Merge,
    pub target_branch: Option<String>,
    pub repair_cap: u8,
    pub wait_notice_min: u32,
    pub roles: Vec<Role>,
    pub gates: Vec<Gate>,
    pub merge_requires: Vec<Condition>,
}

impl Squad {
    pub fn role(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.name == name)
    }
}

fn tools(list: &[Tool]) -> Tools {
    Tools::Some(list.iter().copied().collect())
}

fn builtin_roles() -> Vec<Role> {
    let role = |name: &str, description: &str, level, t: Tools, signals: &[&str], max| Role {
        name: name.into(),
        description: description.into(),
        level,
        agent: "claude".into(),
        model: None,
        tools: t,
        signals: signals.iter().map(|s| (*s).to_string()).collect(),
        max,
        flags: BTreeMap::new(),
    };
    vec![
        role(
            "lead",
            "Plans the work with the engineer, starts sessions, hands out tasks and publishes.",
            Level::EditRun,
            Tools::All,
            &["*"],
            1,
        ),
        role(
            "builder",
            "Implements its assigned tasks in its own worktree and reports them with evidence.",
            Level::EditRun,
            tools(&[
                Tool::Board,
                Tool::Events,
                Tool::Deliver,
                Tool::Report,
                Tool::Sessions,
            ]),
            &["*-ready", "*-done", "*-blocked"],
            3,
        ),
        role(
            "reviewer",
            "Reviews the builders' work against the spec, plan and tasks. Read-only.",
            Level::ReadOnly,
            tools(&[Tool::Board, Tool::Events, Tool::Report, Tool::Sessions]),
            &["review-*"],
            2,
        ),
    ]
}

/// The squad used without a squad file (spec FR-019).
pub fn builtin() -> Squad {
    Squad {
        name: "built-in".into(),
        source: Source::Builtin,
        publish: Publish::Auto,
        merge: Merge::OnApproval,
        target_branch: None,
        repair_cap: 2,
        wait_notice_min: 10,
        roles: builtin_roles(),
        gates: Vec::new(),
        merge_requires: vec![
            Condition::AllDone("builder".into()),
            Condition::Signal {
                event: "review-approved".into(),
                role: "reviewer".into(),
            },
        ],
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRole {
    name: String,
    description: String,
    level: String,
    agent: Option<String>,
    model: Option<String>,
    tools: Option<toml::Value>,
    signals: Option<Vec<String>>,
    max: Option<i64>,
    #[serde(default)]
    flags: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGate {
    name: String,
    command: String,
    windows: Option<String>,
    macos: Option<String>,
    linux: Option<String>,
    timeout_min: Option<i64>,
    baseline: Option<bool>,
    failures: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMerge {
    mode: Option<String>,
    #[serde(default)]
    requires: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSquad {
    name: Option<String>,
    publish: Option<Spanned<String>>,
    merge: Option<Spanned<RawMerge>>,
    target_branch: Option<String>,
    repair_cap: Option<Spanned<i64>>,
    wait_notice_min: Option<Spanned<i64>>,
    #[serde(default)]
    role: Vec<Spanned<RawRole>>,
    #[serde(default)]
    gate: Vec<Spanned<RawGate>>,
}

#[derive(Debug, Deserialize)]
struct RawRolesFile {
    #[serde(default)]
    role: Vec<Spanned<RawRole>>,
}

fn valid_name(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn role_from(raw: RawRole, at: &str, problems: &mut Vec<String>) -> Option<Role> {
    let mut ok = true;
    let mut bad = |m: String| {
        problems.push(format!("{at}: {m}"));
        ok = false;
    };
    if !valid_name(&raw.name) {
        bad(format!(
            "role name {:?}: expected [a-z][a-z0-9-]{{0,31}}",
            raw.name
        ));
    }
    if !(1..=200).contains(&raw.description.chars().count()) {
        bad(format!(
            "role {:?}: description must be 1..=200 chars",
            raw.name
        ));
    }
    let level = Level::parse(&raw.level);
    if level.is_none() {
        bad(format!(
            "role {:?}: level {:?}: expected \"read-only\", \"edit\" or \"edit-run\"",
            raw.name, raw.level
        ));
    }
    let max = raw.max.unwrap_or(1);
    if !(1..=16).contains(&max) {
        bad(format!("role {:?}: max {max}: expected 1..=16", raw.name));
    }
    let tools = match &raw.tools {
        None => Some(Tools::Some(
            [Tool::Board, Tool::Events, Tool::Report, Tool::Sessions].into(),
        )),
        Some(toml::Value::String(s)) if s == "all" => Some(Tools::All),
        Some(toml::Value::Array(a)) => {
            let mut set = BTreeSet::new();
            let mut fine = true;
            for v in a {
                match v.as_str().and_then(Tool::parse) {
                    Some(t) => {
                        set.insert(t);
                    }
                    None => {
                        bad(format!(
                            "role {:?}: unknown tool {}: expected board, events, deliver, tasks, report, start, sessions",
                            raw.name,
                            v.as_str()
                                .map_or_else(|| format!("{v:?}"), |t| format!("{t:?}"))
                        ));
                        fine = false;
                    }
                }
            }
            fine.then_some(Tools::Some(set))
        }
        Some(other) => {
            bad(format!(
                "role {:?}: tools {other:?}: expected \"all\" or a list",
                raw.name
            ));
            None
        }
    };
    if !ok {
        return None;
    }
    Some(Role {
        name: raw.name,
        description: raw.description,
        level: level?,
        agent: raw
            .agent
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "claude".into()),
        model: raw.model.filter(|m| !m.is_empty()),
        tools: tools?,
        signals: raw.signals.unwrap_or_default(),
        max: max as u8,
        flags: raw.flags,
    })
}

fn gate_from(raw: RawGate, at: &str, problems: &mut Vec<String>) -> Option<Gate> {
    let before = problems.len();
    if !valid_name(&raw.name) {
        problems.push(format!(
            "{at}: gate name {:?}: expected [a-z][a-z0-9-]{{0,31}}",
            raw.name
        ));
    }
    if raw.command.trim().is_empty() {
        problems.push(format!(
            "{at}: gate {:?}: command must not be empty",
            raw.name
        ));
    }
    let timeout = raw.timeout_min.unwrap_or(30);
    if !(1..=240).contains(&timeout) {
        problems.push(format!(
            "{at}: gate {:?}: timeout_min {timeout}: expected 1..=240",
            raw.name
        ));
    }
    let failures = match raw.failures.as_deref() {
        None | Some("auto") => Failures::Auto,
        Some("exit-code") => Failures::ExitCode,
        Some(re) => match regex::Regex::new(re) {
            Ok(r) if r.captures_len() == 2 => Failures::Regex(re.to_string()),
            Ok(_) => {
                problems.push(format!(
                    "{at}: gate {:?}: failures regex needs exactly one capture group",
                    raw.name
                ));
                Failures::ExitCode
            }
            Err(e) => {
                problems.push(format!("{at}: gate {:?}: failures: {e}", raw.name));
                Failures::ExitCode
            }
        },
    };
    (problems.len() == before).then(|| Gate {
        name: raw.name,
        command: raw.command,
        windows: raw.windows,
        macos: raw.macos,
        linux: raw.linux,
        timeout_min: timeout as u32,
        baseline: raw.baseline.unwrap_or(true),
        failures,
    })
}

fn condition(s: &str) -> Option<Condition> {
    if let Some(g) = s.strip_prefix("gate:") {
        return Some(Condition::Gate(g.to_string()));
    }
    if let Some(r) = s.strip_prefix("done:") {
        return Some(Condition::AllDone(r.to_string()));
    }
    let rest = s.strip_prefix("signal:")?;
    let (event, role) = rest.split_once('@')?;
    Some(Condition::Signal {
        event: event.to_string(),
        role: role.to_string(),
    })
}

/// Parses `roles.toml`: roles for all projects. Problems are `file:line: message`.
pub fn parse_roles(file: &str, text: &str) -> (Vec<Role>, Vec<String>) {
    let mut problems = Vec::new();
    let raw: RawRolesFile = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return (Vec::new(), vec![format!("{file}:{line}: {}", e.message())]);
        }
    };
    let mut out = Vec::new();
    for r in raw.role {
        let at = format!("{file}:{}", line_of(text, r.span().start));
        if let Some(role) = role_from(r.into_inner(), &at, &mut problems) {
            out.push(role);
        }
    }
    (out, problems)
}

/// Parses a squad file. `base_roles` (from `roles.toml`) are added unless the squad defines a role
/// of the same name; the built-in lead is added if no role is named `lead`.
pub fn parse(file: &str, text: &str, base_roles: &[Role]) -> Result<Squad, Vec<String>> {
    let mut problems = Vec::new();
    let raw: RawSquad = match toml::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let line = e.span().map_or(1, |s| line_of(text, s.start));
            return Err(vec![format!("{file}:{line}: {}", e.message())]);
        }
    };
    finish(file, text, raw, base_roles, &mut problems)
}

fn finish(
    file: &str,
    text: &str,
    raw: RawSquad,
    base_roles: &[Role],
    problems: &mut Vec<String>,
) -> Result<Squad, Vec<String>> {
    let at = |span: std::ops::Range<usize>| format!("{file}:{}", line_of(text, span.start));
    let mut squad = builtin();
    squad.name = raw.name.unwrap_or_else(|| "squad".into());
    squad.source = Source::File(PathBuf::from(file));
    if let Some(p) = raw.publish {
        match p.get_ref().as_str() {
            "auto" => squad.publish = Publish::Auto,
            "draft-pr" => squad.publish = Publish::DraftPr,
            "local-merge" => squad.publish = Publish::LocalMerge,
            other => problems.push(format!(
                "{}: publish {other:?}: expected \"auto\", \"draft-pr\" or \"local-merge\"",
                at(p.span())
            )),
        }
    }
    let merge_line = raw.merge.as_ref().map(|m| at(m.span()));
    let mut requires = None;
    if let Some(m) = raw.merge {
        let where_ = at(m.span());
        let m = m.into_inner();
        match m.mode.as_deref() {
            None | Some("on-approval") => squad.merge = Merge::OnApproval,
            Some("automatic") => squad.merge = Merge::Automatic,
            Some(other) => problems.push(format!(
                "{where_}: merge mode {other:?}: expected \"on-approval\" or \"automatic\""
            )),
        }
        requires = Some(m.requires);
    }
    squad.target_branch = raw.target_branch.filter(|b| !b.trim().is_empty());
    if let Some(c) = raw.repair_cap {
        if (0..=10).contains(c.get_ref()) {
            squad.repair_cap = *c.get_ref() as u8;
        } else {
            problems.push(format!(
                "{}: repair_cap {}: expected 0..=10",
                at(c.span()),
                c.get_ref()
            ));
        }
    }
    if let Some(w) = raw.wait_notice_min {
        if (1..=120).contains(w.get_ref()) {
            squad.wait_notice_min = *w.get_ref() as u32;
        } else {
            problems.push(format!(
                "{}: wait_notice_min {}: expected 1..=120",
                at(w.span()),
                w.get_ref()
            ));
        }
    }
    let mut roles: Vec<Role> = Vec::new();
    if raw.role.len() > MAX_ROLES {
        problems.push(format!("{file}:1: at most {MAX_ROLES} roles"));
    }
    for r in raw.role {
        let where_ = at(r.span());
        if let Some(role) = role_from(r.into_inner(), &where_, problems) {
            if roles.iter().any(|x| x.name == role.name) {
                problems.push(format!(
                    "{where_}: role names must be unique: {:?}",
                    role.name
                ));
            } else {
                roles.push(role);
            }
        }
    }
    let defined_here = !roles.is_empty();
    for b in base_roles {
        if !roles.iter().any(|r| r.name == b.name) {
            roles.push(b.clone());
        }
    }
    if !roles.iter().any(|r| r.name == "lead") {
        roles.insert(0, builtin_roles().remove(0));
    }
    if defined_here || !base_roles.is_empty() {
        squad.roles = roles;
    }
    if raw.gate.len() > MAX_GATES {
        problems.push(format!("{file}:1: at most {MAX_GATES} gates"));
    }
    squad.gates.clear();
    for g in raw.gate {
        let where_ = at(g.span());
        if let Some(gate) = gate_from(g.into_inner(), &where_, problems) {
            squad.gates.push(gate);
        }
    }
    if let Some(reqs) = requires {
        let line = merge_line.unwrap_or_else(|| format!("{file}:1"));
        let mut conds = Vec::new();
        for r in reqs {
            match condition(&r) {
                None => problems.push(format!(
                    "{line}: merge condition {r:?}: expected gate:<name>, done:<role> or signal:<event>@<role>"
                )),
                Some(c) => {
                    let known = match &c {
                        Condition::Gate(g) => squad.gates.iter().any(|x| &x.name == g),
                        Condition::AllDone(role) | Condition::Signal { role, .. } => {
                            squad.roles.iter().any(|x| &x.name == role)
                        }
                    };
                    if known {
                        conds.push(c);
                    } else {
                        problems.push(format!(
                            "{line}: merge condition {r:?} names an undefined gate or role"
                        ));
                    }
                }
            }
        }
        squad.merge_requires = conds;
    }
    if problems.is_empty() {
        Ok(squad)
    } else {
        Err(std::mem::take(problems))
    }
}

/// The squad for a project: `.fallow/squad.toml` if present, else the built-in squad; roles from
/// `<config_dir>/roles.toml` are added either way.
pub fn load(project: &Path, config_dir: Option<&Path>) -> Result<Squad, Vec<String>> {
    let base = match config_dir.map(|d| d.join(ROLES_FILE)) {
        Some(p) if p.exists() => {
            let text =
                std::fs::read_to_string(&p).map_err(|e| vec![format!("{}: {e}", p.display())])?;
            let (roles, problems) = parse_roles(&p.display().to_string(), &text);
            if !problems.is_empty() {
                return Err(problems);
            }
            roles
        }
        _ => Vec::new(),
    };
    let path = project.join(SQUAD_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&path.display().to_string(), &text, &base).map(|mut s| {
            s.source = Source::File(path.clone());
            s
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut s = builtin();
            for b in base {
                match s.roles.iter_mut().find(|r| r.name == b.name) {
                    Some(r) => *r = b,
                    None => s.roles.push(b),
                }
            }
            Ok(s)
        }
        Err(e) => Err(vec![format!("{}: {e}", path.display())]),
    }
}
