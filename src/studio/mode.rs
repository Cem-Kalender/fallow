//! 015 (research R-1 to R-5): Manual and Auto, the same for the lead's spec sessions and the
//! assistant's solo sessions. Every request an agent's runtime sends to Fallow's permission
//! prompt is decided here, on Fallow's own request kinds (constitution IX), never on a runtime's
//! tool names: allowed, asked (Manual only) or refused at a fixed limit, never silently.

use std::path::Path;

use serde_json::Value;

use super::judge;
use crate::agent::{self, RequestKind};
pub use crate::config::settings::ChatMode;

/// 015 (FR-010): what the activity and Fallow's records show instead of an output that may
/// hold environment or secret values.
pub const HIDDEN_OUTPUT: &str = "Output hidden: it may hold secret values";

/// Which agent asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Lead,
    Assistant,
}

/// A fixed limit (spec FR-009), as the refusal records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// The assistant outside its working copy; the lead outside the spec documents.
    Edit,
    /// A push onto the protected target branch.
    Target,
    /// A tool Fallow cannot judge, in Manual.
    Tool,
    /// The runtime's own permission or settings files: never allowed (constitution VI).
    Control,
}

impl Limit {
    pub fn name(self) -> &'static str {
        match self {
            Limit::Edit => "edit",
            Limit::Target => "target",
            Limit::Tool => "tool",
            Limit::Control => "control",
        }
    }
}

/// Why a command the judge has nothing against is still asked in Manual: an ask always says
/// why (Windows CI, PR #3).
pub const NOT_APPROVED: &str =
    "it is not an approved command, and this chat is in Manual: Fallow asks first";

/// What Fallow answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Allow,
    /// Put to the engineer (Manual only), with the judge's reasons: never none.
    Ask(Vec<String>),
    Refuse {
        limit: Limit,
        reason: String,
        facts: Option<Value>,
    },
    /// 015 (2026-11-24 amendment, the engineer): stopped at a limit the engineer may open, in
    /// either mode: the request waits for their answer (Allow once, Always allow, Deny), unless
    /// the project already allows it past that limit (`Project::allowed_past_limits`).
    Hold {
        limit: Limit,
        reason: String,
        facts: Option<Value>,
    },
}

/// The project's target branch on its remote, as 010 last read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// Not known to be unprotected: `yes`, `unknown` or `checking` (research R-3).
    pub protected: bool,
    /// When the target is not known (no record, no target read in it): the other branches it
    /// may be, each held to the same limit.
    pub also: Vec<String>,
}

/// The branches a target that is not known yet may be, besides the folder's checked-out one.
const LIKELY_TARGETS: &[&str] = &["main", "master"];

impl Target {
    /// From the project's remote record (010). Without a record (not read yet), or with a
    /// remote but no target read in it, the folder's checked-out branch and the usual target
    /// names (`main`, `master`) count as protected targets, a detached `HEAD` too: only a
    /// target known to be unprotected is not a limit (FR-009). Only a project known to have no
    /// remote has no target.
    pub fn resolve(
        remote: Option<&crate::publish::Remote>,
        checked_out: impl FnOnce() -> Option<String>,
    ) -> Option<Target> {
        if let Some(r) = remote {
            if r.state == "none" {
                return None;
            }
            if let Some(name) = r.target.clone().filter(|t| !t.is_empty()) {
                return Some(Target {
                    name,
                    protected: r.protected != "no",
                    also: Vec::new(),
                });
            }
        }
        let mut names: Vec<String> = checked_out()
            .filter(|b| !b.is_empty())
            .into_iter()
            .collect();
        for n in LIKELY_TARGETS {
            if !names.iter().any(|x| x == n) {
                names.push(n.to_string());
            }
        }
        let name = names.remove(0);
        Some(Target {
            name,
            protected: true,
            also: names,
        })
    }

    /// The target's name and, when it is not known, the others it may be.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.name).chain(&self.also)
    }
}

/// What a request is, in words, for the card and the record: the command, the path, the URL
/// or the tool.
pub fn what(req: &agent::PermissionRequest) -> String {
    match req.kind {
        RequestKind::Run => req.command.clone().unwrap_or_default(),
        RequestKind::Edit | RequestKind::Read => req.path.clone().unwrap_or_default(),
        RequestKind::Web => req
            .url
            .clone()
            .or_else(|| req.query.clone())
            .unwrap_or_default(),
        RequestKind::Other => req.tool.clone(),
    }
}

fn tool_name(req: &agent::PermissionRequest) -> String {
    if req.tool.is_empty() {
        "That".into()
    } else {
        req.tool.clone()
    }
}

/// The record's kind of a request: `edit`, `run`, `read`, `web` or `tool`.
pub fn kind_name(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Run => "run",
        RequestKind::Edit => "edit",
        RequestKind::Read => "read",
        RequestKind::Web => "web",
        RequestKind::Other => "tool",
    }
}

/// 015 (2026-11-24 amendment): a request waiting for the engineer at a limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub limit: Limit,
    /// The record's kind: `run`, `edit`, `read`, `web` or `tool`.
    pub kind: &'static str,
    pub reason: String,
}

impl Held {
    /// Whether "Always allow" may be offered: past the edit or the target limit, never a tool
    /// Fallow cannot judge (nor a control file, which is never held).
    pub fn always(&self) -> bool {
        matches!(self.limit, Limit::Edit | Limit::Target)
    }

    /// What the agent is told when the engineer denies it (R-4's reason).
    pub fn denied(&self) -> String {
        format!(
            "Fallow refused this: {}. The engineer denied it; do not try another way to do it.",
            self.reason
        )
    }
}

/// A one-shot grant from "Allow this once" (R-5): exactly this kind and this command or path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub refusal: String,
    pub kind: &'static str,
    pub what: String,
}

/// The refusal message the agent receives at once (R-4).
pub fn refusal_message(reason: &str, limit: Limit) -> String {
    if limit == Limit::Control {
        format!(
            "Fallow refused this: {reason}. This file is never editable, in any mode; do not try another way to change it."
        )
    } else {
        format!(
            "Fallow refused this: {reason}. The engineer can allow exactly this once from the chat; if they do, you will be told."
        )
    }
}

/// What the agent is told when the engineer allowed a refusal once (R-5).
pub fn allowed_message(what: &str) -> String {
    format!(
        "The engineer allowed, once, exactly: `{what}`. Run exactly that again if it is still needed; anything else is decided as before."
    )
}

/// The assistant's context: its judge, gates and approved commands, and the remote's target.
pub struct AssistantCx<'a> {
    pub judge: &'a judge::Ctx<'a>,
    /// The runtime's own settings files (`Capabilities::control_files`): never editable.
    pub control_files: &'a [String],
    pub gates: &'a [String],
    pub approved: &'a [String],
    pub target: Option<&'a Target>,
}

/// The push limit of both agents: a push onto the protected target.
fn target_refusal(command: &str, target: Option<&Target>) -> Option<Decision> {
    let t = target.filter(|t| t.protected)?;
    let name = t.names().find(|n| judge::pushes_onto(command, n))?;
    Some(Decision::Hold {
        limit: Limit::Target,
        reason: format!(
            "it pushes onto {name}, the remote's protected target branch (Fallow only opens a pull request there)"
        ),
        facts: None,
    })
}

/// 015 (R-2): the assistant's request, decided. `folder_facts` gives 012's facts for a
/// command that changes the engineer's folder (only called when it does).
pub fn assistant(
    mode: ChatMode,
    req: &agent::PermissionRequest,
    cx: &AssistantCx,
    folder_facts: impl FnOnce(&str) -> Option<Value>,
) -> Decision {
    let ctx = cx.judge;
    match req.kind {
        // The web switch decides whether the assistant has web tools at all (FR-003).
        RequestKind::Web => Decision::Allow,
        RequestKind::Edit => {
            let path = req.path.clone().unwrap_or_default();
            let Some(why) = judge::edit_problem(&path, ctx) else {
                return Decision::Allow;
            };
            let reason = format!("the assistant may not change {path}: {why}");
            // The files that control the agents, and git's own record: never (constitution VI).
            let opaque = path.trim().starts_with('~') || path.contains('$');
            let protected = control_file(&path, cx.control_files, ctx.wt)
                || (!opaque && judge::rel_inside(ctx.wt, &path).is_some());
            if protected {
                return Decision::Refuse {
                    limit: Limit::Control,
                    reason,
                    facts: None,
                };
            }
            // The system's temporary folder is scratch space, not the engineer's project
            // (research R-3, 2026-11-24 amendment); the engineer's folder never is.
            let in_folder = judge::in_folder_path(ctx.folder, ctx.wt, &path);
            if !in_folder && judge::in_temp(&path) {
                return Decision::Allow;
            }
            Decision::Hold {
                limit: Limit::Edit,
                reason,
                facts: None,
            }
        }
        RequestKind::Read => {
            if mode == ChatMode::Auto {
                return Decision::Allow;
            }
            let path = req.path.as_deref();
            let problem = match req.tool.as_str() {
                "Grep" => judge::search_problem(path, req.query.as_deref(), ctx),
                "Glob" => judge::listing_problem(path, req.query.as_deref(), ctx),
                _ => judge::read_problem(path, ctx),
            };
            match problem {
                None => Decision::Allow,
                Some(why) => Decision::Ask(vec![why]),
            }
        }
        RequestKind::Other => match mode {
            ChatMode::Auto => Decision::Allow,
            ChatMode::Manual => Decision::Hold {
                limit: Limit::Tool,
                reason: format!("{} is not a tool Fallow can judge", tool_name(req)),
                facts: None,
            },
        },
        RequestKind::Run => {
            let command = req.command.clone().unwrap_or_default();
            if let Some(r) = target_refusal(&command, cx.target) {
                return r;
            }
            // The fixed limits, read from the command as written (final review, 1): a runtime
            // settings file in either mode; the rest in Auto (Manual asks, with 012's facts).
            let limit = judge::limit_problem(&command, ctx, cx.control_files);
            if let Some((true, why)) = &limit {
                return Decision::Refuse {
                    limit: Limit::Control,
                    reason: format!("it changes the runtime's own settings: {why}"),
                    facts: None,
                };
            }
            if judge::runs_unasked(&command, cx.gates, cx.approved, ctx) {
                return Decision::Allow;
            }
            // 015 (2026-11-24 amendment): in Auto, git's own record and Fallow's control files
            // written are never allowed; the engineer's folder changed waits for the engineer,
            // in both modes. Reads, `cd` and writes elsewhere (the temp folder) are not limits.
            if mode == ChatMode::Auto {
                if let Some(why) = judge::protected_write(&command, ctx) {
                    return Decision::Refuse {
                        limit: Limit::Control,
                        reason: why,
                        facts: None,
                    };
                }
                if let Some(reason) = judge::protected_unread(&command, ctx) {
                    return Decision::Hold {
                        limit: Limit::Edit,
                        reason,
                        facts: None,
                    };
                }
            }
            if let Some(reason) = folder_limit(&command, ctx) {
                return Decision::Hold {
                    limit: Limit::Edit,
                    reason,
                    facts: folder_facts(&command),
                };
            }
            match mode {
                ChatMode::Auto => Decision::Allow,
                ChatMode::Manual => match judge::assess(&command, ctx) {
                    judge::Verdict::Allow => Decision::Ask(vec![NOT_APPROVED.into()]),
                    judge::Verdict::Ask(why) => Decision::Ask(why),
                },
            }
        }
    }
}

/// 015 (2026-11-24 amendment): why a command is at the folder limit: it runs git in the
/// engineer's folder (`refwatch::in_folder`), or otherwise changes it (`judge::folder_change`).
fn folder_limit(command: &str, ctx: &judge::Ctx) -> Option<String> {
    if super::refwatch::in_folder(command, ctx.folder, ctx.wt) {
        return Some("it changes your project folder, outside the session's working copy".into());
    }
    judge::folder_change(command, ctx).map(|w| format!("{w}, outside the session's working copy"))
}

/// Whether `path` is one of the runtime's own permission or settings files (constitution VI,
/// `Capabilities::control_files`) or one of Fallow's files that control the agents
/// (`rights::CONTROL_GLOBS`, under a moved project's spec root too), however it is spelled:
/// never editable, never allowed once. Compared without regard to case on every platform: the
/// default macOS filesystem reads `.CLAUDE/settings.json` as `.claude/settings.json` (015
/// hardening).
pub fn control_file(path: &str, control_files: &[String], base: &Path) -> bool {
    let Some(rel) = judge::rel_inside(base, path) else {
        return false;
    };
    let rel = rel.to_ascii_lowercase();
    if control_files.iter().any(|f| f.eq_ignore_ascii_case(&rel)) {
        return true;
    }
    // Fallow's control files, at the top or below a folder (a moved project's spec root).
    let fallow = |r: &str| {
        crate::agent::rights::CONTROL_GLOBS
            .iter()
            .any(|g| match g.strip_suffix("/**") {
                Some(dir) => r.starts_with(&format!("{dir}/")),
                None => r == *g,
            })
    };
    fallow(&rel) || rel.match_indices('/').any(|(i, _)| fallow(&rel[i + 1..]))
}

/// The lead's context: where it works, the runtime's settings files, the commands that run
/// without asking, the remote's target and the project's folder.
pub struct LeadCx<'a> {
    /// Its working directory: the spec root of its working copy.
    pub workdir: &'a Path,
    pub control_files: &'a [String],
    /// The project's approved commands, with the turn's gates and Spec Kit scripts.
    pub approved: &'a [String],
    pub target: Option<&'a Target>,
    pub folder: &'a Path,
}

/// 015 (R-2): the lead's request, decided. The runtime allows the lead's reads and its edits of
/// the spec documents by rule (Fallow's own narrow rules, constitution VI); every command comes
/// here, the approved ones too, so that the limits hold before any of them is allowed (015
/// hardening: a runtime rule for `git` let `git push origin main` past the target limit).
pub fn lead(mode: ChatMode, req: &agent::PermissionRequest, cx: &LeadCx) -> Decision {
    let (workdir, control_files) = (cx.workdir, cx.control_files);
    match req.kind {
        RequestKind::Read | RequestKind::Web => Decision::Allow,
        RequestKind::Edit => {
            let path = req.path.clone().unwrap_or_default();
            if control_file(&path, control_files, workdir) {
                Decision::Refuse {
                    limit: Limit::Control,
                    reason: format!(
                        "{path} controls what agents may do (the runtime's or Fallow's own settings)"
                    ),
                    facts: None,
                }
            } else if spec_document(&path, workdir) {
                Decision::Allow
            } else {
                Decision::Hold {
                    limit: Limit::Edit,
                    reason: format!(
                        "the lead changes only spec documents (specs/, the constitution, .fallow/drafts/), not {path}"
                    ),
                    facts: None,
                }
            }
        }
        RequestKind::Other => match mode {
            ChatMode::Auto => Decision::Allow,
            ChatMode::Manual => Decision::Hold {
                limit: Limit::Tool,
                reason: format!("{} is not a tool Fallow can judge", tool_name(req)),
                facts: None,
            },
        },
        RequestKind::Run => {
            let command = req.command.clone().unwrap_or_default();
            if let Some(r) = target_refusal(&command, cx.target) {
                return r;
            }
            // The lead's copy is its working copy; what a command writes there outside the spec
            // documents is thrown away at the turn's end and listed in the chat (research R-3).
            // What it writes beyond that copy, in the runtime's settings, in Fallow's control
            // files or in git's own record, is refused.
            let mut protect: Vec<String> = control_files.to_vec();
            protect.extend(
                crate::agent::rights::CONTROL_GLOBS
                    .iter()
                    .map(|g| g.to_string()),
            );
            protect.extend([".git".to_string(), ".git/**".to_string()]);
            let ctx = judge::Ctx {
                wt: workdir,
                branch: "",
                folder: cx.folder,
                protect: &protect,
            };
            let limit = judge::limit_problem(&command, &ctx, control_files);
            if let Some((true, why)) = &limit {
                return Decision::Refuse {
                    limit: Limit::Control,
                    reason: format!("it changes the runtime's own settings: {why}"),
                    facts: None,
                };
            }
            // The approved commands, gates and Spec Kit scripts, and a bare `cd` inside its
            // copy, run in both modes, within the limits: an approved `git` does not write
            // `.git/hooks` or Fallow's control files unasked.
            if limit.is_none()
                && (lead_approved(&command, cx.approved) || moves_within(&command, workdir))
            {
                return Decision::Allow;
            }
            // 015 (2026-11-24 amendment): as the assistant's (`assistant`).
            if mode == ChatMode::Auto {
                if let Some(why) = judge::protected_write(&command, &ctx) {
                    return Decision::Refuse {
                        limit: Limit::Control,
                        reason: why,
                        facts: None,
                    };
                }
                if let Some(reason) = judge::protected_unread(&command, &ctx) {
                    return Decision::Hold {
                        limit: Limit::Edit,
                        reason,
                        facts: None,
                    };
                }
            }
            if let Some(reason) = folder_limit(&command, &ctx) {
                return Decision::Hold {
                    limit: Limit::Edit,
                    reason: reason.replace("the session's", "the lead's"),
                    facts: None,
                };
            }
            match mode {
                ChatMode::Auto => Decision::Allow,
                ChatMode::Manual => Decision::Ask(vec![NOT_APPROVED.into()]),
            }
        }
    }
}

/// The lead's approved commands as a runtime's prefix rules read them: every part of the
/// command is one of them, or starts with one and a space, and redirects nothing.
fn lead_approved(command: &str, approved: &[String]) -> bool {
    let Ok(parts) = judge::split(command) else {
        return false;
    };
    !parts.is_empty()
        && parts.iter().all(|p| {
            p.redirects.is_empty()
                && approved
                    .iter()
                    .map(|a| a.trim())
                    .filter(|a| !a.is_empty())
                    .any(|a| p.text == a || p.text.starts_with(&format!("{a} ")))
        })
}

/// 015 (hardening): whether every part of `command` is a bare `cd` that stays inside `wt`, read
/// as written, each from where the one before it went (`cd specs/015-x && cd ..`). Not `cd`
/// alone, `cd -`, `cd ~`, a variable, a glob, an option, an escape, a redirection or a pipe:
/// those are decided as any other command. A bare `cd` changes nothing; every later command is
/// a request of its own, decided with its limits.
pub fn moves_within(command: &str, wt: &Path) -> bool {
    let Ok(parts) = judge::split(command) else {
        return false;
    };
    // Where the shell is, as parts below `wt`.
    let mut at: Vec<String> = Vec::new();
    !parts.is_empty()
        && parts.iter().all(|p| {
            let [cd, to] = p.words.as_slice() else {
                return false;
            };
            if !cd.eq_ignore_ascii_case("cd")
                || !p.redirects.is_empty()
                || p.piped
                || p.input
                || p.escaped.iter().any(|e| *e)
                || to.is_empty()
                || to.starts_with(['-', '~'])
                || to.contains(['$', '%', '`', '*', '?', '[', '{'])
            {
                return false;
            }
            let norm = to.replace('\\', "/");
            let absolute =
                Path::new(&norm).is_absolute() || (norm.len() >= 2 && norm.as_bytes()[1] == b':');
            if absolute {
                match judge::rel_inside(wt, &norm) {
                    Some(r) => {
                        at = r
                            .split('/')
                            .filter(|x| !x.is_empty())
                            .map(str::to_string)
                            .collect();
                        return true;
                    }
                    None => return false,
                }
            }
            for c in Path::new(&norm).components() {
                match c {
                    std::path::Component::Normal(s) => at.push(s.to_string_lossy().into_owned()),
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        if at.pop().is_none() {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            true
        })
}

/// Whether the lead may write `path`: under one of the spec folders of its working copy,
/// however it is spelled (`..`, symlinks, another absolute spelling).
fn spec_document(path: &str, workdir: &Path) -> bool {
    judge::rel_inside(workdir, path).is_some_and(|rel| {
        crate::agent::rights::SPEC_DIRS
            .iter()
            .any(|d| rel.starts_with(&format!("{d}/")))
    })
}

/// 015 (R-6): a chat loaded after a restart is in `default`; when the mark says it was in
/// another mode, the chat says so once. 012's `until_switch` mark counts as Auto.
pub fn at_load(c: &mut super::conv::Conv, default: ChatMode) {
    let old = c.dir.join("until_switch");
    let was = if old.exists() {
        let _ = std::fs::remove_file(&old);
        Some(ChatMode::Auto)
    } else {
        std::fs::read_to_string(c.dir.join("mode"))
            .ok()
            .and_then(|m| ChatMode::parse(m.trim()).ok())
    };
    c.mode = default;
    c.mode_note = was.filter(|w| *w != default).map(|_| {
        format!(
            "Fallow restarted: this chat is in {}, your default.",
            default.label()
        )
    });
    let _ = std::fs::write(c.dir.join("mode"), default.name());
}

/// 015 (R-3): a tool call the runtime refused by its own rule, which Fallow did not answer:
/// an edit of the runtime's own settings files (or of Fallow's control files the runtime
/// denies the lead). Recorded as a refusal that is never allowed once.
pub fn runtime_refused(c: &mut super::conv::Conv, id: &str) {
    use crate::conversation::events::Event as CEvent;
    if id.is_empty() || c.answered.iter().any(|a| a == id) {
        return;
    }
    let Some(call) = c
        .activity
        .iter()
        .find(|a| a.id == id && matches!(a.kind.as_str(), "edit" | "write"))
    else {
        return;
    };
    let path = call.text.clone();
    // Recorded once per call.
    c.answered.push(id.to_string());
    let rid = c.state.next_refusal_id();
    let _ = c.append(CEvent::Refused {
        id: rid,
        kind: "edit".into(),
        what: path.clone(),
        reason: format!("{path} is one of the files that control the agents, never editable"),
        limit: Limit::Control.name().into(),
        facts: None,
    });
}

/// 015: the lead's last answer asks the engineer a question that waits for them: a note from
/// Fallow then goes with their answer, not as a turn of its own.
fn lead_question_open(c: &super::conv::Conv) -> bool {
    c.state.kind == crate::conversation::events::Kind::Spec
        && c.state
            .turns
            .iter()
            .rev()
            .find(|t| !t.withdrawn)
            .and_then(|t| t.result.as_ref())
            .is_some_and(|r| !r["question"].is_null())
}

impl super::Studio {
    /// 015 (R-5): "Allow this once" or "Dismiss" on a refusal. Allowing tells the agent first
    /// (the assistant through its waiting messages, 012; the lead at once when idle, or when its
    /// turn ends, or with the engineer's next message while its question waits), and only then
    /// records the allow and puts the one-shot grant on the chat: an agent that cannot be told
    /// leaves the refusal as it was. The agent's next request is handled after this returns, on
    /// the same thread, so the grant is there in time. Clicking it is the engineer's go-ahead
    /// for the session's waiting messages too, as their own message is (012 R-9).
    pub fn answer_refusal(
        &mut self,
        ctx: &crate::run::engine::Ctx,
        conv: &str,
        id: &str,
        answer: &str,
    ) -> Result<Value, String> {
        use crate::conversation::events::{Event as CEvent, Kind, RefusalState};
        let c = self.conv_mut(conv)?;
        if c.state.solo.as_ref().is_some_and(|s| s.closed.is_some()) {
            return Err("this session is closed".into());
        }
        let r = c
            .state
            .refusal(id)
            .cloned()
            .ok_or_else(|| format!("there is no refusal {id}"))?;
        if r.state != RefusalState::Refused {
            return Err("this refusal was already answered".into());
        }
        let withdrawn = c.state.refusal_turn(id).is_some_and(|t| t.withdrawn);
        match answer {
            "dismiss" => {
                c.append(CEvent::RefusalDismissed { id: id.into() })?;
                Ok(serde_json::json!({}))
            }
            "allow_once" => {
                if r.limit == Limit::Control.name() {
                    return Err(
                        "this file is never editable: it controls the agent's own permissions"
                            .into(),
                    );
                }
                if withdrawn {
                    return Err("you cancelled that answer: ask for it again instead".into());
                }
                let kind = match r.kind.as_str() {
                    "edit" => "edit",
                    "run" => "run",
                    "read" => "read",
                    "web" => "web",
                    _ => "tool",
                };
                let msg = allowed_message(&r.what);
                let told = if c.state.kind == Kind::Solo {
                    let v = self.say_solo(ctx, conv, &msg, Vec::new())?;
                    match v["queued"].as_str() {
                        Some(q) => {
                            if let Some(c) = self.convs.get_mut(conv) {
                                c.note_queue.push((id.to_string(), q.to_string()));
                            }
                            "queued"
                        }
                        None => "sent",
                    }
                } else if c.state.busy || lead_question_open(c) {
                    c.lead_notes.push((id.to_string(), msg));
                    "queued"
                } else {
                    self.say(ctx, conv, &msg, Vec::new())?;
                    "sent"
                };
                let c = self.conv_mut(conv)?;
                c.append(CEvent::RefusalAllowed { id: id.into() })?;
                c.grants.push(Grant {
                    refusal: id.into(),
                    kind,
                    what: r.what.clone(),
                });
                c.tick();
                Ok(serde_json::json!({ "told": told }))
            }
            other => Err(format!("answer {other:?}: allow_once or dismiss")),
        }
    }

    /// 015 (R-5): the lead's waiting notes from Fallow, sent as one message once its turn ended
    /// (not while its question waits: then after the engineer's answer and the turn it starts). When the lead
    /// cannot be told, the grants go and the refusals are refused again, saying why.
    pub fn lead_notes_tick(&mut self, ctx: &crate::run::engine::Ctx) {
        use crate::conversation::events::Event as CEvent;
        let ready: Vec<String> = self
            .convs
            .iter()
            .filter(|(_, c)| !c.state.busy && !c.lead_notes.is_empty() && !lead_question_open(c))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            let Some(c) = self.convs.get_mut(&id) else {
                continue;
            };
            let mut notes = std::mem::take(&mut c.lead_notes);
            // A note for a refusal in an answer the engineer cancelled goes, with its grant
            // (review of US2, round 2, 3).
            let gone: Vec<String> = notes
                .iter()
                .filter(|(r, _)| c.state.refusal_turn(r).is_none_or(|t| t.withdrawn))
                .map(|(r, _)| r.clone())
                .collect();
            notes.retain(|(r, _)| !gone.contains(r));
            c.grants.retain(|g| !gone.contains(&g.refusal));
            if notes.is_empty() {
                continue;
            }
            let text = notes
                .iter()
                .map(|(_, m)| m.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if let Err(e) = self.say(ctx, &id, &text, Vec::new()) {
                self.console.push(format!("conversation {id}: {e}"));
                if let Some(c) = self.convs.get_mut(&id) {
                    for (rid, _) in &notes {
                        c.grants.retain(|g| &g.refusal != rid);
                        let _ = c.append(CEvent::RefusalReopened {
                            id: rid.clone(),
                            why: format!("Fallow could not tell the lead: {e}"),
                        });
                    }
                }
            }
        }
    }

    /// 015 (R-9): the refusals waiting for the engineer, as "Needs you" items.
    pub fn refusals_waiting(&self) -> Vec<Value> {
        use crate::conversation::events::{Kind, RefusalState};
        let mut out = Vec::new();
        for (id, c) in &self.convs {
            if c.state.solo.as_ref().is_some_and(|s| s.closed.is_some()) {
                continue;
            }
            let project = c.state.project.clone();
            let project_name = self
                .store
                .get(&project)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| project.clone());
            let chat = match c.state.kind {
                Kind::Solo => c
                    .state
                    .solo
                    .as_ref()
                    .and_then(|s| s.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "a session".into()),
                _ => c
                    .state
                    .feature
                    .clone()
                    .unwrap_or_else(|| "a spec session".into()),
            };
            // 015 (2026-11-24 amendment): a request waiting at a limit for the engineer.
            if let Some(p) = c.pending.as_ref().filter(|p| p.held.is_some()) {
                let short: String = p.command.chars().take(80).collect();
                out.push(serde_json::json!({
                    "kind": "limit",
                    "key": format!("{id}:limit:{}", p.id),
                    "project": project,
                    "project_name": project_name,
                    "conv": id,
                    "solo": c.state.kind == Kind::Solo,
                    "feature": c.state.feature,
                    "chat": chat,
                    "refusal": p.id,
                    "text": format!("{chat} — waits at a limit: {short}"),
                    "at": c.state.turns.last().map(|t| t.started.clone()),
                }));
            }
            // A cancelled answer's refusals are gone with it (review of US2, 1).
            for t in c.state.turns.iter().filter(|t| !t.withdrawn) {
                for r in t
                    .refusals
                    .iter()
                    .filter(|r| r.state == RefusalState::Refused)
                {
                    let short: String = r.what.chars().take(80).collect();
                    out.push(serde_json::json!({
                        "kind": "refused",
                        "key": format!("{id}:refused:{}", r.id),
                        "project": project,
                        "project_name": project_name,
                        "conv": id,
                        "solo": c.state.kind == Kind::Solo,
                        "feature": c.state.feature,
                        "chat": chat,
                        "refusal": r.id,
                        "text": format!("{chat} — Fallow refused: {short}"),
                        "at": t.started,
                    }));
                }
            }
        }
        out
    }

    /// 015 (R-8): switching to Auto answers the request waiting for the engineer as Auto
    /// would answer it: allowed, or refused at a fixed limit.
    pub(super) fn pending_under_auto(&mut self, conv: &str) {
        use crate::conversation::events::{ApprovalAnswer, Event as CEvent, Kind};
        use crate::run::engine::Reply;
        let Some(c) = self.convs.get_mut(conv) else {
            return;
        };
        let Some(p) = c.pending.take() else {
            return;
        };
        let solo = c.state.kind == Kind::Solo;
        let before: usize = c.state.turns.iter().map(|t| t.refusals.len()).sum();
        let req = agent::registry::permission_request(&p.runtime, &p.raw);
        let (tx, rx) = std::sync::mpsc::channel();
        let r = if solo {
            self.solo_tool_call(conv, &p.runtime, &req, &p.raw, tx)
        } else {
            self.lead_tool_call(conv, &p.runtime, &req, &p.raw, tx)
        };
        let failed = r.is_err();
        let out = match r {
            Ok(()) => rx.try_recv().ok(),
            Err(why) => Some(Reply::Ok(p.answer(false, &why))),
        };
        let Some(c) = self.convs.get_mut(conv) else {
            return;
        };
        // Held at a limit under Auto too (2026-11-24 amendment): the new card answers the
        // agent's same request when the engineer answers it.
        if let Some(np) = c.pending.as_mut() {
            np.reply = p.reply.clone();
        }
        let after: usize = c.state.turns.iter().map(|t| t.refusals.len()).sum();
        let allowed = !failed && after == before && out.is_some();
        let _ = c.append(CEvent::ApprovalAnswered {
            id: p.id.clone(),
            answer: if allowed {
                ApprovalAnswer::Once
            } else {
                ApprovalAnswer::Deny
            },
            prefix: None,
            auto: true,
            why: Some("switched to Auto".into()),
        });
        // Asked again (it cannot be in Auto): the new request waits in its place.
        if let Some(o) = out {
            let _ = p.reply.send(o);
        }
    }

    /// 015 (R-4, R-5): acts on a request's decision for either agent: a matching grant first,
    /// then allowed, asked (Manual) or refused at once and recorded, never silently.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn answer_request(
        &mut self,
        conv: &str,
        runtime: &str,
        agent_kind: Agent,
        req: &agent::PermissionRequest,
        raw: &Value,
        reply: std::sync::mpsc::Sender<crate::run::engine::Reply>,
        decision: Decision,
        ask_facts: Option<Value>,
    ) -> Result<(), String> {
        use crate::conversation::events::Event as CEvent;
        use crate::run::engine::Reply;
        let allow = |reply: &std::sync::mpsc::Sender<Reply>| {
            let _ = reply.send(Reply::Ok(agent::registry::permission_reply(
                runtime, true, "", raw,
            )));
        };
        let kind = kind_name(req.kind);
        // A tool Fallow cannot judge is named with its input, so a grant is for exactly that
        // call (review of US2, 7).
        let what = if req.kind == RequestKind::Other {
            format!("{} {}", what(req), raw["input"])
        } else {
            what(req)
        };
        // Where the lead's paths start: its copy's spec root (012 R-18), as `lead_tool_call` judges.
        let lead_cwd = self.convs.get(conv).and_then(|c| {
            let p = self.project(&c.state.project).ok()?;
            Some((c.state.workdir.clone(), c.state.workdir.join(&p.spec_root)))
        });
        // 015 (2026-11-24 amendment): what the engineer allowed always, in this project, past
        // this limit, goes without waiting, in both modes.
        let past = match &decision {
            Decision::Hold { limit, .. } if matches!(limit, Limit::Edit | Limit::Target) => self
                .convs
                .get(conv)
                .and_then(|c| self.project(&c.state.project).ok())
                .is_some_and(|p| {
                    p.allowed_past_limits
                        .iter()
                        .any(|e| e.allows(limit.name(), kind, &what))
                }),
            _ => false,
        };
        let decision = if past { Decision::Allow } else { decision };
        let c = self.convs.get_mut(conv).ok_or("the conversation is gone")?;
        // "Allow this once": exactly this kind and this command or path (R-5).
        let granted = c
            .grants
            .iter()
            .position(|g| g.kind == kind && g.what == what);
        // A grant never opens the runtime's own files (constitution VI).
        let control = matches!(
            decision,
            Decision::Refuse {
                limit: Limit::Control,
                ..
            }
        );
        let decision = match granted.filter(|_| !control) {
            Some(i) => {
                let g = c.grants.remove(i);
                // Its note, if still waiting, has nothing left to say (review of US2, 5).
                c.lead_notes.retain(|(r, _)| r != &g.refusal);
                let queued: Vec<String> = c
                    .note_queue
                    .iter()
                    .filter(|(r, _)| r == &g.refusal)
                    .map(|(_, q)| q.clone())
                    .collect();
                c.note_queue.retain(|(r, _)| r != &g.refusal);
                for q in queued {
                    if c.state
                        .solo
                        .as_ref()
                        .is_some_and(|s| s.queue.iter().any(|w| w.id == q))
                    {
                        c.append(CEvent::Solo(
                            crate::conversation::solo::SoloEvent::QueueRemoved { id: q },
                        ))?;
                    }
                }
                // A lead's allowed edit is kept at the turn's end (review of US2, 3).
                if agent_kind == Agent::Lead && req.kind == RequestKind::Edit {
                    if let Some((wd, cwd)) = &lead_cwd {
                        let abs = cwd.join(&what);
                        if let Some(rel) = judge::rel_inside(wd, &abs.display().to_string()) {
                            c.allowed_paths.push(rel);
                        }
                    }
                }
                c.append(CEvent::RefusalUsed { id: g.refusal })?;
                Decision::Allow
            }
            None => decision,
        };
        match decision {
            Decision::Allow => {
                if agent_kind == Agent::Assistant && req.kind == RequestKind::Run {
                    // 012 (R-10): the ref watch attributes the command's branch changes.
                    self.open_call(conv, raw, &what);
                }
                allow(&reply);
                Ok(())
            }
            Decision::Ask(reasons) => {
                if c.pending.is_some() {
                    return Err(
                        "Another request is waiting for the engineer: ask again once it is answered."
                            .into(),
                    );
                }
                let read = req.kind == RequestKind::Read;
                let shown = if read {
                    super::Studio::read_words(req)
                } else {
                    what
                };
                let tool = req.tool.clone();
                self.ask_engineer(
                    conv, runtime, &tool, read, shown, reasons, raw, reply, ask_facts, None,
                )
            }
            // 015 (2026-11-24 amendment): the agent waits for the engineer's answer, the same
            // card as Manual's, with "Always allow" past the limit.
            Decision::Hold {
                limit,
                reason,
                facts,
            } => {
                if c.pending.is_some() {
                    return Err(
                        "Another request is waiting for the engineer: ask again once it is answered."
                            .into(),
                    );
                }
                let tool = req.tool.clone();
                let held = Held {
                    limit,
                    kind,
                    reason: reason.clone(),
                };
                self.ask_engineer(
                    conv,
                    runtime,
                    &tool,
                    false,
                    what,
                    vec![reason],
                    raw,
                    reply,
                    facts,
                    Some(held),
                )
            }
            Decision::Refuse {
                limit,
                reason,
                facts,
            } => {
                let id = c.state.next_refusal_id();
                c.append(CEvent::Refused {
                    id,
                    kind: kind.to_string(),
                    what,
                    reason: reason.clone(),
                    limit: limit.name().to_string(),
                    facts,
                })?;
                let _ = reply.send(Reply::Ok(agent::registry::permission_reply(
                    runtime,
                    false,
                    &refusal_message(&reason, limit),
                    raw,
                )));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn req(kind: RequestKind, tool: &str) -> agent::PermissionRequest {
        agent::PermissionRequest {
            kind,
            command: None,
            path: None,
            url: None,
            query: None,
            tool: tool.into(),
        }
    }

    fn run(cmd: &str) -> agent::PermissionRequest {
        agent::PermissionRequest {
            command: Some(cmd.into()),
            ..req(RequestKind::Run, "Bash")
        }
    }

    fn edit(path: &str) -> agent::PermissionRequest {
        agent::PermissionRequest {
            path: Some(path.into()),
            ..req(RequestKind::Edit, "Edit")
        }
    }

    fn read(path: &str) -> agent::PermissionRequest {
        agent::PermissionRequest {
            path: Some(path.into()),
            ..req(RequestKind::Read, "Read")
        }
    }

    struct Dirs {
        root: PathBuf,
        wt: PathBuf,
        folder: PathBuf,
    }

    fn dirs(name: &str) -> Dirs {
        let root = std::env::temp_dir().join(format!("fallow-mode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wt = root.join("wt");
        let folder = root.join("folder");
        std::fs::create_dir_all(wt.join("src")).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        Dirs {
            root,
            wt: wt.canonicalize().unwrap(),
            folder: folder.canonicalize().unwrap(),
        }
    }

    fn assistant_decides(
        mode: ChatMode,
        r: &agent::PermissionRequest,
        d: &Dirs,
        target: Option<&Target>,
    ) -> Decision {
        let protect = vec![
            ".fallow/squad.toml".to_string(),
            ".claude/settings.json".to_string(),
            ".claude/settings.local.json".to_string(),
            ".git".to_string(),
            ".git/**".to_string(),
        ];
        let ctx = judge::Ctx {
            wt: &d.wt,
            branch: "fallow/solo/fix",
            folder: &d.folder,
            protect: &protect,
        };
        let gates = vec!["cargo test".to_string()];
        let approved = vec!["npm run lint".to_string()];
        assistant(
            mode,
            r,
            &AssistantCx {
                judge: &ctx,
                control_files: &[
                    ".claude/settings.json".to_string(),
                    ".claude/settings.local.json".to_string(),
                ],
                gates: &gates,
                approved: &approved,
                target,
            },
            |_| Some(serde_json::json!({ "folder": true })),
        )
    }

    /// The limit a request is refused or held at.
    fn limit_of(d: &Decision) -> Option<Limit> {
        match d {
            Decision::Refuse { limit, .. } | Decision::Hold { limit, .. } => Some(*limit),
            _ => None,
        }
    }

    /// Whether it waits for the engineer at a limit (2026-11-24 amendment).
    fn held(d: &Decision) -> bool {
        matches!(d, Decision::Hold { .. })
    }

    #[test]
    fn auto_allows_the_assistant_everything_within_its_limits() {
        let d = dirs("auto");
        let outside = d.root.join("elsewhere.txt").display().to_string();
        for r in [
            read(&outside),
            read(&d.wt.join(".env").display().to_string()),
            edit(&d.wt.join("src/a.rs").display().to_string()),
            run("python -c \"print(1)\""),
            run("npx prettier --check ."),
            run("echo x > out.txt"),
            run("git push --force origin HEAD"),
            run("env"),
            run("cargo test"),
            req(RequestKind::Web, "WebFetch"),
            req(RequestKind::Other, "mcp__x__y"),
        ] {
            assert_eq!(
                assistant_decides(ChatMode::Auto, &r, &d, None),
                Decision::Allow,
                "{r:?}"
            );
        }
        // An edit in the copy however the platform spells it (Windows CI on 0.3.0 refused
        // `\\?\C:\…\wt\src/a.rs` as outside the copy): `.` and `..`, mixed separators, the
        // verbatim prefix or none, and another case where the filesystem ignores case.
        let wt = d.wt.display().to_string();
        let mut spellings = vec![
            format!("{wt}/x/.././src/a.rs"),
            d.wt.join("src").join("a.rs").display().to_string(),
        ];
        if cfg!(windows) {
            let bare = wt.trim_start_matches(r"\\?\").to_string();
            spellings.push(format!(r"{bare}\src\a.rs"));
            spellings.push(format!("{}/src/a.rs", bare.replace('\\', "/")));
            spellings.push(format!(r"\\?\{bare}\src\a.rs"));
        }
        if cfg!(windows) || cfg!(target_os = "macos") {
            spellings.push(format!("{}/src/a.rs", wt.to_uppercase()));
        }
        for p in spellings {
            assert_eq!(
                assistant_decides(ChatMode::Auto, &edit(&p), &d, None),
                Decision::Allow,
                "{p}"
            );
        }
        let _ = std::fs::remove_dir_all(&d.root);
    }

    #[test]
    fn manual_asks_the_assistant_what_012_asked() {
        let d = dirs("manual");
        let outside = d.root.join("elsewhere.txt").display().to_string();
        for r in [read(&outside), run("python -c \"print(1)\""), run("env")] {
            assert!(
                matches!(
                    assistant_decides(ChatMode::Manual, &r, &d, None),
                    Decision::Ask(_)
                ),
                "{r:?}"
            );
        }
        // A plain command the judge has no reason to ask about is still asked: it is not
        // approved (012 FR-008).
        assert!(matches!(
            assistant_decides(ChatMode::Manual, &run("ls -la"), &d, None),
            Decision::Ask(_)
        ));
        for r in [
            run("cargo test"),
            run("npm run lint --fix"),
            read("src/a.rs"),
        ] {
            assert_eq!(
                assistant_decides(ChatMode::Manual, &r, &d, None),
                Decision::Allow,
                "{r:?}"
            );
        }
        assert_eq!(
            limit_of(&assistant_decides(
                ChatMode::Manual,
                &req(RequestKind::Other, "mcp__x__y"),
                &d,
                None
            )),
            Some(Limit::Tool)
        );
        let _ = std::fs::remove_dir_all(&d.root);
    }

    #[test]
    fn the_assistant_s_limits_hold_in_both_modes() {
        let d = dirs("limits");
        let target = Target {
            name: "main".into(),
            protected: true,
            also: Vec::new(),
        };
        let open = Target {
            name: "main".into(),
            protected: false,
            also: Vec::new(),
        };
        let in_folder = d.folder.join("x.txt").display().to_string();
        for mode in [ChatMode::Manual, ChatMode::Auto] {
            assert_eq!(
                limit_of(&assistant_decides(mode, &edit(&in_folder), &d, None)),
                Some(Limit::Edit),
                "{mode:?}"
            );
            assert_eq!(
                limit_of(&assistant_decides(
                    mode,
                    &edit(&d.wt.join(".claude/settings.json").display().to_string()),
                    &d,
                    None
                )),
                Some(Limit::Control),
                "{mode:?}"
            );
            assert_eq!(
                limit_of(&assistant_decides(
                    mode,
                    &run("git push origin HEAD:main"),
                    &d,
                    Some(&target)
                )),
                Some(Limit::Target),
                "{mode:?}"
            );
            // Known to be unprotected: not a limit.
            assert_eq!(
                limit_of(&assistant_decides(
                    mode,
                    &run("git push origin HEAD:main"),
                    &d,
                    Some(&open)
                )),
                None
            );
        }
        // The target push and the edit outside the copy wait for the engineer.
        for mode in [ChatMode::Manual, ChatMode::Auto] {
            assert!(held(&assistant_decides(mode, &edit(&in_folder), &d, None)));
            assert!(held(&assistant_decides(
                mode,
                &run("git push origin HEAD:main"),
                &d,
                Some(&target)
            )));
        }
        // Shell commands that change the engineer's folder, read as written (final review, 1;
        // 2026-11-24 amendment): they wait at the folder limit, in both modes.
        let f = d.folder.display().to_string();
        for cmd in [
            format!("echo x > {f}/README.md"),
            format!("rm -rf {f}/src"),
            format!("cp a.txt {f}/src/a.txt"),
            format!("cd {f} && rm x"),
            format!("cd {f} && npm install"),
            format!("bash -c 'rm -rf {f}/src'"),
        ] {
            for mode in [ChatMode::Manual, ChatMode::Auto] {
                let got = assistant_decides(mode, &run(&cmd), &d, None);
                assert!(held(&got), "{cmd} {mode:?}: {got:?}");
                assert_eq!(limit_of(&got), Some(Limit::Edit), "{cmd}");
            }
        }
        // Writing elsewhere, outside the folder, is not a limit in Auto; Manual asks.
        for cmd in [
            "echo x > ../../elsewhere.txt".to_string(),
            format!("mv notes.txt {}/away.txt", d.root.display()),
            format!("cat {f}/README.md"),
            format!("cp {f}/README.md /tmp/x.md"),
        ] {
            assert_eq!(
                assistant_decides(ChatMode::Auto, &run(&cmd), &d, None),
                Decision::Allow,
                "{cmd}"
            );
            assert!(
                matches!(
                    assistant_decides(ChatMode::Manual, &run(&cmd), &d, None),
                    Decision::Ask(_)
                ),
                "{cmd}: Manual asks, as 012 does"
            );
        }
        // Git's own record, written: never, in Auto (Manual asks, as 012 does).
        assert_eq!(
            limit_of(&assistant_decides(
                ChatMode::Auto,
                &run("rm -rf .git"),
                &d,
                None
            )),
            Some(Limit::Control)
        );
        for cmd in [
            "echo '{}' > .claude/settings.local.json",
            "cp x .claude/settings.json",
            // Final review, round 2: in either order, behind another protected file; and the
            // shapes the judge does not read as paths.
            "cp x .fallow/squad.toml && echo '{}' > .claude/settings.json",
            "echo '{}' > .claude/settings.json; cp x .fallow/squad.toml",
            "sed -i s/a/b/ .claude/settings.json",
            "rm -rf .claude",
            "mv .claude .claude2",
            "git mv .claude/settings.json x.json",
            // Final review, round 3: after a `cd`, through `of=`, behind a wrapper or a shell.
            "cd .claude && echo '{}' > settings.json",
            "cd .claude && rm settings.json",
            "cd .claude && cp ../x settings.local.json",
            "cd src && echo x > ../.claude/settings.json",
            "dd if=x of=.claude/settings.json",
            "timeout 5 cp x .claude/settings.json",
            "env cp x .claude/settings.json",
            "bash -c \"echo '{}' > .claude/settings.json\"",
        ] {
            for mode in [ChatMode::Manual, ChatMode::Auto] {
                assert_eq!(
                    limit_of(&assistant_decides(mode, &run(cmd), &d, None)),
                    Some(Limit::Control),
                    "{cmd} {mode:?}"
                );
            }
        }
        // Reading a settings file is not a limit (Manual asks, as 012 does; Auto runs it).
        assert_eq!(
            assistant_decides(ChatMode::Auto, &run("cat .claude/settings.json"), &d, None),
            Decision::Allow
        );
        // A protected file of Fallow's, written: refused in Auto, named as such, never allowed.
        match assistant_decides(ChatMode::Auto, &run("cp x .fallow/squad.toml"), &d, None) {
            Decision::Refuse {
                limit: Limit::Control,
                reason,
                ..
            } => assert!(reason.contains("may not change"), "{reason}"),
            other => panic!("{other:?}"),
        }
        // Reads outside the copy, and what Fallow cannot read, still run in Auto.
        for cmd in [
            "ls ~".to_string(),
            "cat /etc/hosts".to_string(),
            format!("ls {}", d.root.display()),
            "python -c \"open('/tmp/x','w')\"".to_string(),
        ] {
            assert_eq!(
                assistant_decides(ChatMode::Auto, &run(&cmd), &d, None),
                Decision::Allow,
                "{cmd}"
            );
        }
        // The engineer's folder: waits for the engineer in both modes, with 012's facts.
        let cmd = format!("git -C {} checkout -b x", d.folder.display());
        for mode in [ChatMode::Manual, ChatMode::Auto] {
            match assistant_decides(mode, &run(&cmd), &d, None) {
                Decision::Hold {
                    limit: Limit::Edit,
                    facts: Some(_),
                    ..
                } => {}
                other => panic!("{other:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(&d.root);
    }

    /// SC-004: allowing a request in Auto costs what allowing an approved command costs. The
    /// medians of 400 decisions each are compared with a margin that holds on a busy CI runner
    /// (Windows' file calls are slower): at most three times the approved one, plus 20 ms.
    #[test]
    fn an_auto_allow_is_as_quick_as_an_approved_command() {
        let d = dirs("timing");
        let median = |mode: ChatMode, r: &agent::PermissionRequest| {
            let mut t: Vec<std::time::Duration> = (0..400)
                .map(|_| {
                    let s = std::time::Instant::now();
                    assert_eq!(assistant_decides(mode, r, &d, None), Decision::Allow);
                    s.elapsed()
                })
                .collect();
            t.sort();
            t[200]
        };
        let auto = median(ChatMode::Auto, &run("python -c \"print(1)\""));
        let approved = median(ChatMode::Manual, &run("npm run lint --fix"));
        assert!(
            auto <= approved * 3 + std::time::Duration::from_millis(20),
            "{auto:?} vs {approved:?}"
        );
        let _ = std::fs::remove_dir_all(&d.root);
    }

    #[test]
    fn the_lead_gets_the_same_two_modes() {
        // Real absolute paths for the platform (on Windows, with its drive), not created.
        let root = std::env::temp_dir().join(format!("fallow-mode-lead-{}", std::process::id()));
        let w = root.display().to_string();
        let wd = root.join("spec");
        let cf = vec![".claude/settings.json".to_string()];
        let fold = root.join("folder");
        let target = Target {
            name: "main".into(),
            protected: true,
            also: Vec::new(),
        };
        let approved = vec!["git".to_string(), "npm test".to_string()];
        let lead = |m: ChatMode,
                    r: &agent::PermissionRequest,
                    _: &PathBuf,
                    cf: &Vec<String>,
                    target: Option<&Target>,
                    fold: &PathBuf| {
            lead(
                m,
                r,
                &LeadCx {
                    workdir: &wd,
                    control_files: cf,
                    approved: &approved,
                    target,
                    folder: fold,
                },
            )
        };
        assert_eq!(
            lead(ChatMode::Auto, &run("cargo test"), &wd, &cf, None, &fold),
            Decision::Allow
        );
        assert_eq!(
            lead(ChatMode::Manual, &run("cargo test"), &wd, &cf, None, &fold),
            Decision::Ask(vec![NOT_APPROVED.into()])
        );
        for m in [ChatMode::Manual, ChatMode::Auto] {
            assert_eq!(
                lead(
                    m,
                    &edit(&format!("{w}/spec/specs/015-x/spec.md")),
                    &wd,
                    &cf,
                    None,
                    &fold
                ),
                Decision::Allow
            );
            assert_eq!(
                lead(m, &edit("specs/015-x/plan.md"), &wd, &cf, None, &fold),
                Decision::Allow
            );
            assert_eq!(
                limit_of(&lead(
                    m,
                    &edit(&format!("{w}/spec/src/main.rs")),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Edit)
            );
            assert_eq!(
                limit_of(&lead(
                    m,
                    &edit(&format!("{w}/spec/specs/../src/main.rs")),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Edit)
            );
            assert_eq!(
                limit_of(&lead(
                    m,
                    &edit(&format!("{w}/spec/specs/../.claude/settings.json")),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Control)
            );
            assert_eq!(
                limit_of(&lead(
                    m,
                    &edit(&format!("{w}/spec/.claude/settings.json")),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Control)
            );
            assert_eq!(
                limit_of(&lead(
                    m,
                    &run("git push origin main"),
                    &wd,
                    &cf,
                    Some(&target),
                    &fold
                )),
                Some(Limit::Target)
            );
            // A command writing the runtime's settings: never, in either mode.
            for cmd in ["sed -i s/a/b/ .claude/settings.json", "rm -rf .claude"] {
                assert_eq!(
                    limit_of(&lead(m, &run(cmd), &wd, &cf, None, &fold)),
                    Some(Limit::Control),
                    "{cmd}"
                );
            }
            assert_eq!(
                limit_of(&lead(
                    m,
                    &run("echo '{}' > .claude/settings.json"),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Control)
            );
        }
        // The engineer's folder changed: waits for the engineer, in both modes; elsewhere it
        // runs in Auto (2026-11-24 amendment).
        let far = run(&format!("echo x > {w}/folder/README.md"));
        for m in [ChatMode::Manual, ChatMode::Auto] {
            let got = lead(m, &far, &wd, &cf, None, &fold);
            assert!(held(&got), "{got:?}");
            assert_eq!(limit_of(&got), Some(Limit::Edit));
        }
        assert_eq!(
            lead(
                ChatMode::Auto,
                &run(&format!("echo x > {w}/elsewhere.txt")),
                &wd,
                &cf,
                None,
                &fold
            ),
            Decision::Allow
        );
        assert_eq!(
            lead(
                ChatMode::Auto,
                &req(RequestKind::Other, "mcp__x__y"),
                &wd,
                &cf,
                None,
                &fold
            ),
            Decision::Allow
        );
        assert_eq!(
            limit_of(&lead(
                ChatMode::Manual,
                &req(RequestKind::Other, "mcp__x__y"),
                &wd,
                &cf,
                None,
                &fold
            )),
            Some(Limit::Tool)
        );
        // 015 hardening: the approved commands run without asking, but only after the limits:
        // `git` approved never lets a push onto the target, or a write of a control file, by.
        for m in [ChatMode::Manual, ChatMode::Auto] {
            assert_eq!(
                lead(m, &run("git status"), &wd, &cf, Some(&target), &fold),
                Decision::Allow
            );
            assert_eq!(
                lead(m, &run("npm test && git log"), &wd, &cf, None, &fold),
                Decision::Allow
            );
            for cmd in [
                "git push origin main",
                "cd x && git push origin main",
                "git push origin HEAD:main",
            ] {
                assert_eq!(
                    limit_of(&lead(m, &run(cmd), &wd, &cf, Some(&target), &fold)),
                    Some(Limit::Target),
                    "{cmd}"
                );
            }
            assert_eq!(
                limit_of(&lead(
                    m,
                    &run("git diff --output=.claude/settings.json"),
                    &wd,
                    &cf,
                    None,
                    &fold
                )),
                Some(Limit::Control)
            );
            // A bare `cd` inside its copy is Fallow's to allow, not a runtime rule's.
            for cmd in [
                "cd specs".to_string(),
                "cd specs/015-x && cd ..".to_string(),
                format!("cd {w}/spec/specs"),
            ] {
                assert_eq!(
                    lead(m, &run(&cmd), &wd, &cf, None, &fold),
                    Decision::Allow,
                    "{cmd}"
                );
            }
        }
        // An approved `git` does not write git's own record or Fallow's control files: never,
        // in Auto.
        for cmd in [
            "git log --output=.git/hooks/pre-commit",
            "git diff --output=.fallow/squad.toml",
        ] {
            assert_eq!(
                limit_of(&lead(ChatMode::Auto, &run(cmd), &wd, &cf, None, &fold)),
                Some(Limit::Control),
                "{cmd}"
            );
            assert!(
                matches!(
                    lead(ChatMode::Manual, &run(cmd), &wd, &cf, None, &fold),
                    Decision::Ask(_)
                ),
                "{cmd}"
            );
        }
        // `cd` out of its copy, or with what Fallow cannot read, is any other command.
        for cmd in ["cd ..", "cd", "cd ~", "cd $X", "cd /tmp", "cd specs > x"] {
            assert!(
                matches!(
                    lead(ChatMode::Manual, &run(cmd), &wd, &cf, None, &fold),
                    Decision::Ask(_)
                ),
                "{cmd}"
            );
        }
        // Fallow's control files and the runtime's, however spelled, at the control limit.
        for path in [
            format!("{w}/spec/.CLAUDE/settings.json"),
            format!("{w}/spec/.fallow/squad.toml"),
            format!("{w}/spec/.fallow/agents/lead.md"),
            ".Fallow/pipelines/x.toml".to_string(),
        ] {
            assert_eq!(
                limit_of(&lead(ChatMode::Auto, &edit(&path), &wd, &cf, None, &fold)),
                Some(Limit::Control),
                "{path}"
            );
        }
    }

    /// 015 hardening: an edit of a control file that reaches Fallow, however spelled, is at the
    /// control limit (never "Allow this once"), not the edit limit.
    #[test]
    fn a_control_file_is_one_however_spelled() {
        let base = Path::new("/w/copy");
        let cf = vec![
            ".claude/settings.json".to_string(),
            ".claude/settings.local.json".to_string(),
        ];
        for p in [
            ".claude/settings.json",
            ".CLAUDE/Settings.json",
            "/w/copy/.claude/settings.local.json",
            "/w/copy/src/../.claude/settings.json",
            ".fallow/squad.toml",
            ".Fallow/agents/lead.md",
            "/w/copy/.fallow/pipelines/p.toml",
            "sub/.fallow/squad.toml",
        ] {
            assert!(control_file(p, &cf, base), "{p}");
        }
        for p in [
            ".claude/notes.md",
            ".fallow/drafts/x.md",
            "src/settings.json",
            "/elsewhere/.claude/settings.json",
        ] {
            assert!(!control_file(p, &cf, base), "{p}");
        }
    }

    #[test]
    fn a_target_is_protected_unless_known_not_to_be() {
        let mut r = crate::publish::Remote {
            state: "known".into(),
            remote: Some("origin".into()),
            service: None,
            repo: None,
            target: Some("main".into()),
            protected: "unknown".into(),
            why: None,
            account: None,
            checked_at: None,
            stale: false,
            checking: false,
        };
        let main = || Some("main".to_string());
        assert!(Target::resolve(Some(&r), || None).unwrap().protected);
        r.protected = "checking".into();
        assert!(Target::resolve(Some(&r), || None).unwrap().protected);
        r.protected = "no".into();
        assert!(!Target::resolve(Some(&r), || None).unwrap().protected);
        // No target read: the checked-out branch counts as protected.
        r.target = None;
        let t = Target::resolve(Some(&r), main).unwrap();
        assert!(t.protected && t.name == "main");
        // Not read yet: the same (review of phase 2, 1).
        let t = Target::resolve(None, main).unwrap();
        assert!(t.protected && t.name == "main");
        // Unknown, and the folder's HEAD detached: the usual names count (015 hardening).
        let t = Target::resolve(None, || None).unwrap();
        assert!(t.protected && t.names().eq(["main", "master"].iter()));
        let dev = || Some("dev".to_string());
        let t = Target::resolve(Some(&r), dev).unwrap();
        assert!(t.names().eq(["dev", "main", "master"].iter()));
        assert!(target_refusal("git push origin master", Some(&t)).is_some());
        assert!(target_refusal("git push origin feature", Some(&t)).is_none());
        // Known to have no remote: no target.
        r.state = "none".into();
        assert!(Target::resolve(Some(&r), main).is_none());
    }
}
