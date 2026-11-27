//! 014 (research R-3): the Studio's requests, answered without any transport. The window's custom
//! scheme (Windows) and the loopback page of `http.rs` (other platforms, and the `automation` test
//! build) both hand their requests here, after their own checks.

use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::ask;
use crate::run::engine::{Msg, Reply, Request};

const INDEX: &str = include_str!("web/index.html");
const APP_JS: &str = include_str!("web/app.js");
const APP_CSS: &str = include_str!("web/app.css");

/// The screens' scripts (research S-2), served at `/screens/<name>.js`.
const SCREENS: &[(&str, &str)] = &[
    ("dashboard.js", include_str!("web/screens/dashboard.js")),
    ("project.js", include_str!("web/screens/project.js")),
    ("spec.js", include_str!("web/screens/spec.js")),
    ("loop.js", include_str!("web/screens/loop.js")),
    ("pipelines.js", include_str!("web/screens/pipelines.js")),
    ("agents.js", include_str!("web/screens/agents.js")),
    ("cmdmenu.js", include_str!("web/screens/cmdmenu.js")),
    ("files.js", include_str!("web/screens/files.js")),
    ("solo.js", include_str!("web/screens/solo.js")),
];

/// The fonts Fallow ships (research S-3), served at `/fonts/<name>`.
const FONTS: &[(&str, &[u8])] = &[
    (
        "inter-400.woff2",
        include_bytes!("../../assets/fonts/inter-400.woff2"),
    ),
    (
        "inter-500.woff2",
        include_bytes!("../../assets/fonts/inter-500.woff2"),
    ),
    (
        "inter-600.woff2",
        include_bytes!("../../assets/fonts/inter-600.woff2"),
    ),
    (
        "inter-700.woff2",
        include_bytes!("../../assets/fonts/inter-700.woff2"),
    ),
    (
        "jetbrains-mono-400.woff2",
        include_bytes!("../../assets/fonts/jetbrains-mono-400.woff2"),
    ),
    (
        "jetbrains-mono-500.woff2",
        include_bytes!("../../assets/fonts/jetbrains-mono-500.woff2"),
    ),
];

/// The largest JSON request body accepted (an image for a session has its own limit,
/// `extras::MAX_IMAGE`).
pub const MAX_BODY: usize = 1024 * 1024;

/// One request of the page: its method, its URL (path and query, as sent) and its body.
pub struct PageRequest {
    pub method: String,
    pub url: String,
    pub body: Vec<u8>,
}

/// The answer: its status, its content type and its body. Every answer carries `HEADERS` too.
pub struct PageResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// The headers of every answer: the page runs only Fallow's own scripts and loads nothing from
/// anywhere else (constitution V).
pub const HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'",
    ),
    ("X-Frame-Options", "DENY"),
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
];

/// The largest body a transport needs to read: a session's image, the largest request there
/// is. A transport reads one byte more, so `answer` sees what is too large.
pub const MAX_READ: usize = crate::studio::extras::MAX_IMAGE;

fn text(code: u16, content_type: &str, body: String) -> PageResponse {
    PageResponse {
        status: code,
        content_type: content_type.to_string(),
        body: body.into_bytes(),
    }
}

fn bytes(content_type: &str, body: &'static [u8]) -> PageResponse {
    data(content_type, body.to_vec())
}

/// 012 (R-8): a kept image, sent as itself; `nosniff` (in the headers) keeps it an image.
fn data(content_type: &str, body: Vec<u8>) -> PageResponse {
    PageResponse {
        status: 200,
        content_type: content_type.to_string(),
        body,
    }
}

fn json_reply(reply: Reply) -> PageResponse {
    match reply {
        Reply::Ok(v) => text(
            200,
            "application/json",
            json!({ "ok": true, "value": v }).to_string(),
        ),
        Reply::Err(e) => text(
            409,
            "application/json",
            json!({ "ok": false, "error": e }).to_string(),
        ),
    }
}

/// 013: the theme this server serves the page with (research R-3). `value` is held only to read or
/// write it, so a page is never served behind the engine. `saving` is held while the engine saves
/// a choice, so two choices from two pages are kept in the order they were saved (review).
#[derive(Default)]
pub struct KeptTheme {
    value: std::sync::Mutex<Option<String>>,
    saving: std::sync::Mutex<()>,
}

/// 013: `index.html` with the engineer's theme: `data-theme` on `<html>` and the matching
/// `color-scheme` hint for the browser's own canvas, for Light and Dark; as written for System.
pub fn page_with_theme(theme: &str) -> String {
    match theme {
        "light" | "dark" => INDEX
            .replacen(
                "<html lang=\"en\">",
                &format!("<html lang=\"en\" data-theme=\"{theme}\">"),
                1,
            )
            .replacen(
                "<meta name=\"color-scheme\" content=\"light dark\">",
                &format!("<meta name=\"color-scheme\" content=\"{theme}\">"),
                1,
            ),
        _ => INDEX.to_string(),
    }
}

/// `%2F` and friends, and `+` as a space in queries.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_arg(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?').map(|(_, q)| q)?;
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
        .map(|v| percent_decode(&v.replace('+', " ")))
}

/// The studio's routes (007 contracts/web-api.md): `None` when the path is not one of them.
fn studio_route(
    method: &str,
    parts: &[&str],
    body: &Value,
    url: &str,
) -> Option<Result<Request, String>> {
    use crate::studio::Req;
    let s = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);
    let need = |k: &str| s(k).ok_or_else(|| format!("{k} is required"));
    let confirmed = || {
        if body.get("confirm").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err("this needs the engineer's confirmation".to_string())
        }
    };
    let r = |req: Req| Some(Ok(Request::Studio(req)));
    match (method, parts) {
        ("GET", ["api", "agents"]) => r(Req::Agents {
            project: query_arg(url, "project").filter(|p| !p.is_empty()),
        }),
        // 017 (contracts/web-api.md): the template a new agent starts from.
        ("GET", ["api", "agents", "template"]) => match query_arg(url, "name") {
            Some(name) => r(Req::AgentTemplate { name }),
            None => Some(Err("name is required".into())),
        },
        // 017: 008's form save is gone; an agent is saved as its whole text.
        ("PUT", ["api", "agents", _]) if body.get("fields").is_some() => {
            Some(Err("an agent is saved as its whole text".into()))
        }
        ("PUT", ["api", "agents", name]) => match need("text") {
            Ok(text) => r(Req::PutAgent {
                name: name.to_string(),
                scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
                text,
                base: s("base"),
                replace: body.get("replace").and_then(Value::as_bool) == Some(true),
            }),
            Err(e) => Some(Err(e)),
        },
        ("DELETE", ["api", "agents", name]) => Some(confirmed().map(|()| {
            Request::Studio(Req::DeleteAgent {
                name: name.to_string(),
                scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
            })
        })),
        ("GET", ["api", "pipelines"]) => r(Req::Pipelines {
            project: query_arg(url, "project").filter(|p| !p.is_empty()),
        }),
        ("POST", ["api", "pipelines", "check"]) => match need("toml") {
            Ok(toml) => r(Req::CheckPipeline {
                project: s("project").filter(|p| !p.is_empty()),
                toml,
            }),
            Err(e) => Some(Err(e)),
        },
        ("PUT", ["api", "pipelines", name]) => match need("toml") {
            Ok(toml) => r(Req::PutPipeline {
                name: name.to_string(),
                scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
                toml,
            }),
            Err(e) => Some(Err(e)),
        },
        ("DELETE", ["api", "pipelines", name]) => Some(confirmed().map(|()| {
            Request::Studio(Req::DeletePipeline {
                name: name.to_string(),
                scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
            })
        })),
        ("GET", ["api", "runtimes"]) => r(Req::Runtimes),
        ("POST", ["api", "projects"]) => match need("path") {
            Ok(p) => r(Req::AddProject { path: p.into() }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "projects", p]) => r(Req::Project { id: p.to_string() }),
        ("POST", ["api", "projects", p, "remote", "check"]) => {
            r(Req::CheckRemote { id: p.to_string() })
        }
        ("DELETE", ["api", "features", p, f]) => {
            Some(confirmed().map(|()| Request::DeleteFeature {
                project: p.to_string(),
                feature: f.to_string(),
            }))
        }
        ("DELETE", ["api", "data", kind, id]) => Some(confirmed().map(|()| Request::DeleteData {
            kind: kind.to_string(),
            id: id.to_string(),
        })),
        ("DELETE", ["api", "projects", p]) => {
            Some(confirmed().map(|()| Request::Studio(Req::RemoveProject { id: p.to_string() })))
        }
        ("PATCH", ["api", "projects", p]) => r(Req::PatchProject {
            id: p.to_string(),
            patch: body.clone(),
        }),
        ("PUT", ["api", "projects", p, "commands"]) => {
            let commands = body
                .get("commands")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            r(Req::SetCommands {
                id: p.to_string(),
                commands,
            })
        }
        // The chat's `/` menu before the chat has a conversation (`PUT` is the approved commands).
        ("GET", ["api", "projects", p, "commands"]) => r(Req::ProjectCommands {
            project: p.to_string(),
        }),
        ("GET", ["api", "projects", p, "git-init"]) => r(Req::GitScan { id: p.to_string() }),
        ("POST", ["api", "projects", p, "git-init"]) => Some(confirmed().map(|()| {
            Request::Studio(Req::GitInit {
                id: p.to_string(),
                gitignore: s("gitignore"),
            })
        })),
        ("GET", ["api", "projects", p, "move"]) => r(Req::Move {
            id: p.to_string(),
            confirmed: false,
        }),
        ("POST", ["api", "projects", p, "move"]) => Some(confirmed().map(|()| {
            Request::Studio(Req::Move {
                id: p.to_string(),
                confirmed: true,
            })
        })),
        // 012 (FR-035): the project conversation with the lead is retired.
        ("POST", ["api", "projects", _p, "conversation"]) => Some(Err(
            "The project conversation with the lead is retired: start a session with the assistant, or a feature".into(),
        )),
        ("POST", ["api", "projects", p, "features"]) => match need("title") {
            Ok(t) => r(Req::NewFeature {
                project: p.to_string(),
                title: t,
                lead: body
                    .get("lead")
                    .and_then(Value::as_str)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string),
            }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "conversations", c]) => r(Req::Conversation { id: c.to_string() }),
        // 012: solo sessions (contracts/web-api.md, "Sessions"). A session is a conversation:
        // its view, messages, stop and approvals go through the same requests.
        ("POST", ["api", "projects", p, "sessions"]) => {
            // 015: the mode chosen in the composer before the first message.
            let mode = match s("mode").filter(|m| !m.is_empty()) {
                Some(m) => match crate::studio::mode::ChatMode::parse(&m) {
                    Ok(m) => Some(m),
                    Err(e) => return Some(Err(e)),
                },
                None => None,
            };
            match need("text") {
                Ok(t) => r(Req::NewSession {
                    project: p.to_string(),
                    agent: s("agent").filter(|a| !a.trim().is_empty()),
                    text: t,
                    mode,
                }),
                Err(e) => Some(Err(e)),
            }
        }
        ("GET", ["api", "sessions", c]) => r(Req::Conversation { id: c.to_string() }),
        ("POST", ["api", "sessions", c, "pending"]) => match need("choice") {
            Ok(choice) => r(Req::SoloPending {
                session: c.to_string(),
                choice,
                confirmed: body.get("confirmed").and_then(Value::as_bool) == Some(true),
            }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "sessions", c, "undo"]) => match query_arg(url, "turn").and_then(|t| t.parse().ok()) {
            Some(turn) => r(Req::UndoPreview { session: c.to_string(), turn }),
            None => Some(Err("turn is required".into())),
        },
        ("POST", ["api", "sessions", c, "undo"]) => match body.get("turn").and_then(Value::as_u64) {
            Some(turn) => r(Req::Undo {
                session: c.to_string(),
                turn: turn as u32,
                confirmed: body.get("confirmed").and_then(Value::as_bool) == Some(true),
            }),
            None => Some(Err("turn is required".into())),
        },
        ("GET", ["api", "sessions", c, "changes"]) => r(Req::SoloChanges { session: c.to_string() }),
        ("GET", ["api", "projects", p, "git-changes"]) => r(Req::GitChanges { project: p.to_string() }),
        ("GET", ["api", "sessions", c, "feature-draft"]) => r(Req::FeatureDraft {
            session: c.to_string(),
            suggestion: query_arg(url, "suggestion").and_then(|n| n.parse().ok()),
        }),
        ("POST", ["api", "sessions", c, "feature"]) => r(Req::FeatureFromSession {
            session: c.to_string(),
            title: s("title").unwrap_or_default(),
            brief: s("brief").unwrap_or_default(),
            lead: s("lead").filter(|l| !l.is_empty()),
        }),
        ("POST", ["api", "conversations", c, "feature"]) => r(Req::FeatureFromConversation {
            conv: c.to_string(),
            title: s("title").unwrap_or_default(),
            brief: s("brief").unwrap_or_default(),
            lead: s("lead").filter(|l| !l.is_empty()),
        }),
        ("POST", ["api", "sessions", c, "gates"]) => r(Req::SoloGates { session: c.to_string() }),
        ("GET", ["api", "projects", p, "sessions"]) => r(Req::Sessions {
            project: p.to_string(),
        }),
        ("POST", ["api", "sessions", c, "rename"]) => match need("name") {
            Ok(n) => r(Req::RenameSession {
                session: c.to_string(),
                name: n,
            }),
            Err(e) => Some(Err(e)),
        },
        ("POST", ["api", "sessions", c, "open"]) => r(Req::OpenSession {
            session: c.to_string(),
        }),
        ("POST", ["api", "sessions", c, "say"]) if body.get("attachments").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) => {
            r(Req::SoloSay {
                session: c.to_string(),
                text: s("text").unwrap_or_default(),
                attachments: body["attachments"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default(),
            })
        }
        ("POST", ["api", "sessions", c, "say"]) => match need("text") {
            Ok(t) => r(Req::Say {
                conv: c.to_string(),
                text: t,
            }),
            Err(e) => Some(Err(e)),
        },
        ("POST", ["api", "sessions", c, "queue", id]) => r(Req::SoloQueue {
            session: c.to_string(),
            id: id.to_string(),
            text: s("text"),
            remove: body.get("remove").and_then(Value::as_bool) == Some(true),
            send: body.get("send").and_then(Value::as_bool) == Some(true),
        }),
        ("POST", ["api", "sessions", c, "close"]) => r(Req::CloseSession {
            session: c.to_string(),
            choice: s("choice"),
            confirmed: body.get("confirmed").and_then(Value::as_bool) == Some(true),
            digest: s("digest"),
        }),
        ("POST", ["api", "sessions", c, "remove-copy"]) => r(Req::RemoveCopy {
            session: c.to_string(),
            confirmed: body.get("confirmed").and_then(Value::as_bool) == Some(true),
        }),
        ("DELETE", ["api", "sessions", c]) => r(Req::DeleteSession {
            session: c.to_string(),
            confirmed: body.get("confirmed").and_then(Value::as_bool) == Some(true),
        }),
        ("POST", ["api", "sessions", c, "web"]) => match body.get("on").and_then(Value::as_bool) {
            Some(on) => r(Req::SoloWeb { session: c.to_string(), on }),
            None => Some(Err("say whether web access is on".into())),
        },
        ("POST", ["api", "sessions", c, "cancel"]) => r(Req::Cancel {
            conv: c.to_string(),
        }),
        ("POST", ["api", "sessions", c, "mode"]) => match need("mode") {
            Ok(m) => r(Req::SessionMode {
                session: c.to_string(),
                mode: m,
            }),
            Err(e) => Some(Err(e)),
        },
        ("POST", ["api", "conversations", c, "say"]) => match need("text") {
            Ok(t) => r(Req::Say {
                conv: c.to_string(),
                text: t,
            }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "projects", p, "files", "versions"]) => r(Req::FileVersions {
            project: p.to_string(),
        }),
        ("GET", ["api", "projects", p, "files"]) => r(Req::Files {
            project: p.to_string(),
            version: query_arg(url, "version").unwrap_or_default(),
        }),
        ("GET", ["api", "projects", p, "file"]) => match query_arg(url, "path") {
            Some(path) => r(Req::File {
                project: p.to_string(),
                version: query_arg(url, "version").unwrap_or_default(),
                path,
                changes: query_arg(url, "view").as_deref() == Some("changes"),
            }),
            None => Some(Err("path is required".into())),
        },
        ("POST", ["api", "files", "open"]) => match (need("project"), need("path")) {
            (Ok(project), Ok(path)) => r(Req::OpenFile {
                project,
                version: s("version").unwrap_or_default(),
                path,
                editor: body
                    .get("editor")
                    .and_then(|e| serde_json::from_value(e.clone()).ok()),
                command: s("command").filter(|c| !c.trim().is_empty()),
            }),
            (Err(e), _) | (_, Err(e)) => Some(Err(e)),
        },
        ("GET", ["api", "files", "editor"]) => r(Req::GetEditor),
        ("POST", ["api", "theme"]) => match need("theme") {
            Ok(theme) => r(Req::SetTheme { theme }),
            Err(e) => Some(Err(e)),
        },
        // 016 (FR-033, data-model "Page settings"): the page's settings beside the theme.
        ("GET", ["api", "prefs"]) => r(Req::GetChatWide),
        ("POST", ["api", "prefs"]) => match body.get("chat_wide").and_then(Value::as_object) {
            Some(m) => r(Req::SetChatWide { screens: m.clone() }),
            None => Some(Err("chat_wide is an object of screen names to true or false".into())),
        },
        ("POST", ["api", "files", "editor"]) => r(Req::SetEditor {
            editor: serde_json::from_value(body.clone())
                .ok()
                .filter(|e: &crate::studio::files::Editor| !e.program.is_empty()),
            command: s("command").filter(|c| !c.trim().is_empty()),
            clear: body.get("clear").and_then(Value::as_bool).unwrap_or(false),
        }),
        ("POST", ["api", "conversations", c, "lead"]) => match need("lead") {
            Ok(l) => r(Req::SetLead {
                conv: c.to_string(),
                lead: l,
            }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "conversations", c, "commands"]) => r(Req::Commands {
            conv: c.to_string(),
        }),
        ("POST", ["api", "conversations", c, "cancel"]) => r(Req::Cancel {
            conv: c.to_string(),
        }),
        ("POST", ["api", "conversations", c, "mode"]) => {
            match crate::studio::mode::ChatMode::parse(s("mode").as_deref().unwrap_or_default()) {
                Ok(mode) => r(Req::SetMode {
                    conv: c.to_string(),
                    mode,
                }),
                Err(e) => Some(Err(e)),
            }
        }
        ("POST", ["api", "conversations" | "sessions", c, "refusals", id]) => match need("answer") {
            Ok(a) => r(Req::Refusal {
                conv: c.to_string(),
                id: id.to_string(),
                answer: a,
            }),
            Err(e) => Some(Err(e)),
        },
        ("POST", ["api", "conversations" | "sessions", c, "approve"]) => {
            use crate::conversation::events::ApprovalAnswer;
            let answer = match s("answer").as_deref() {
                Some("once") => ApprovalAnswer::Once,
                Some("always") => ApprovalAnswer::Always,
                Some("deny") => ApprovalAnswer::Deny,
                // 015 (FR-007): "Switch to Auto": this request allowed, then the chat in Auto.
                Some("auto") => {
                    return match need("id") {
                        Ok(id) => r(Req::ApproveAuto {
                            conv: c.to_string(),
                            id,
                        }),
                        Err(e) => Some(Err(e)),
                    };
                }
                _ => return Some(Err("answer: once, always, deny or auto".into())),
            };
            match need("id") {
                Ok(id) => r(Req::Approve {
                    conv: c.to_string(),
                    id,
                    answer,
                    prefix: s("prefix"),
                }),
                Err(e) => Some(Err(e)),
            }
        }
        ("GET", ["api", "features", p, f]) => r(Req::Feature {
            project: p.to_string(),
            feature: f.to_string(),
        }),
        ("GET", ["api", "features", p, f, "doc"]) => match query_arg(url, "path") {
            Some(path) => r(Req::Doc {
                project: p.to_string(),
                feature: f.to_string(),
                path,
            }),
            None => Some(Err("path is required".into())),
        },
        ("PUT", ["api", "features", p, f, "doc"]) => {
            match (query_arg(url, "path"), need("text"), need("version")) {
                (Some(path), Ok(text), Ok(version)) => r(Req::PutDoc {
                    project: p.to_string(),
                    feature: f.to_string(),
                    path,
                    text,
                    version,
                }),
                _ => Some(Err("path, text and version are required".into())),
            }
        }
        ("POST", ["api", "features", p, f, "step"]) => match need("to") {
            Ok(to) => r(Req::SetStep {
                project: p.to_string(),
                feature: f.to_string(),
                to,
            }),
            Err(e) => Some(Err(e)),
        },
        ("POST", ["api", "features", p, f, "dismiss"]) => {
            match body.get("finding").and_then(Value::as_u64) {
                Some(i) => r(Req::Dismiss {
                    project: p.to_string(),
                    feature: f.to_string(),
                    finding: i as usize,
                }),
                None => Some(Err("finding is required".into())),
            }
        }
        ("POST", ["api", "features", p, f, "unlock"]) => {
            if body.get("confirm").and_then(Value::as_bool) != Some(true) {
                return Some(Err("unlocking needs {\"confirm\": true}".into()));
            }
            Some(Ok(Request::Unlock {
                project: p.to_string(),
                feature: f.to_string(),
            }))
        }
        // 011 FR-013: the overlap read before the lock (contracts/web-api.md).
        ("GET", ["api", "features", p, f, "overlap"]) => Some(Ok(Request::Overlap {
            project: p.to_string(),
            feature: f.to_string(),
        })),
        ("GET", ["api", "features", p, f, "relock-preview"]) => Some(Ok(Request::RelockPreview {
            project: p.to_string(),
            feature: f.to_string(),
        })),
        ("POST", ["api", "features", p, f, "lock"]) => {
            let level = match s("autonomy").as_deref() {
                None => None,
                Some("strict") => Some(crate::run::events::Level::Strict),
                Some("normal") => Some(crate::run::events::Level::Normal),
                Some("free") => Some(crate::run::events::Level::Free),
                Some(o) => return Some(Err(format!("autonomy {o:?}: strict, normal or free"))),
            };
            let budget = body.get("budget");
            let when_verified = match s("when_verified").as_deref() {
                None => None,
                Some(w) => match crate::run::events::WhenVerified::parse(w) {
                    Some(w) => Some(w),
                    None => return Some(Err(format!("when_verified {w:?}: wait or pull_request"))),
                },
            };
            let update_limit = match body.get("update_limit") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_u64() {
                    Some(n) if (1..=20).contains(&n) => Some(n as u32),
                    _ => return Some(Err("update_limit: a number from 1 to 20".into())),
                },
            };
            let run_after = body.get("run_after").and_then(Value::as_u64);
            Some(Ok(Request::Lock {
                when_verified,
                update_limit,
                run_after,
                project: p.to_string(),
                feature: f.to_string(),
                pipeline: s("pipeline"),
                level,
                budget_usd: budget.and_then(|b| b.get("usd")).and_then(Value::as_f64),
                budget_minutes: budget
                    .and_then(|b| b.get("minutes"))
                    .and_then(Value::as_u64),
            }))
        }
        _ => None,
    }
}

/// The Studio's answer to one request (007 to 013 contracts/web-api.md), after the transport's
/// own checks.
pub fn answer(req: PageRequest, tx: &Sender<Msg>, theme: &KeptTheme) -> PageResponse {
    let method = req.method.clone();
    let url = req.url.clone();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let is_get = method == "GET";
    // 012 (R-8): an image for a session's message: the raw bytes, up to 5 MiB.
    if let ("POST", ["api", "sessions", s, "attachments"]) = (method.as_str(), parts.as_slice()) {
        let session = percent_decode(s);
        let b = req.body;
        if b.len() > crate::studio::extras::MAX_IMAGE {
            return json_reply(Reply::Err("an image can be up to 5 MiB".into()));
        }
        let name = query_arg(&url, "name").unwrap_or_else(|| "image".into());
        return json_reply(ask(
            tx,
            Request::Studio(crate::studio::Req::SoloAttach {
                session,
                name,
                bytes: b,
            }),
        ));
    }
    if let (true, ["api", "sessions", s, "attachments", sha]) = (is_get, parts.as_slice()) {
        let r = ask(
            tx,
            Request::Studio(crate::studio::Req::SoloAttachment {
                session: percent_decode(s),
                sha: percent_decode(sha),
            }),
        );
        return match r {
            Reply::Ok(v) => match std::fs::read(v["path"].as_str().unwrap_or_default()) {
                Ok(bytes) => data(
                    v["media_type"]
                        .as_str()
                        .unwrap_or("application/octet-stream"),
                    bytes,
                ),
                Err(_) => text(404, "text/plain", "not found".into()),
            },
            Reply::Err(e) => text(404, "text/plain", e),
        };
    }
    let body: Value = if is_get {
        Value::Null
    } else {
        let b = &req.body;
        if b.len() > MAX_BODY {
            return text(413, "text/plain", "too large".into());
        }
        if b.is_empty() {
            json!({})
        } else {
            match serde_json::from_slice(b) {
                Ok(v) => v,
                Err(_) => return text(400, "text/plain", "the body is not JSON".into()),
            }
        }
    };
    let id = |s: &str| s.parse::<u64>().ok();
    let decoded: Vec<String> = parts.iter().map(|p| percent_decode(p)).collect();
    let dparts: Vec<&str> = decoded.iter().map(String::as_str).collect();
    // 008 T044: the folder picker opens on the engineer's desktop; this request waits for it.
    if method == "POST" && dparts == ["api", "projects", "pick"] {
        let reply = match crate::platform::picker::pick_folder("Add a project folder to Fallow") {
            Ok(Some(p)) => Reply::Ok(json!({ "path": p })),
            Ok(None) => Reply::Ok(json!({ "cancelled": true })),
            Err(e) => Reply::Err(e),
        };
        return json_reply(reply);
    }
    // 016 (FR-014, T028): "Show banners": macOS's prompt for full permission; this request
    // waits for the engineer's answer.
    #[cfg(target_os = "macos")]
    if method == "POST" && dparts == ["api", "notifications", "banners"] {
        let p = crate::platform::notify::mac::ask_banners();
        return json_reply(Reply::Ok(json!({ "permission": p.name() })));
    }
    match studio_route(&method, &dparts, &body, &url) {
        // 013: a theme Fallow kept is the one this server serves from now on (`KeptTheme`).
        Some(Ok(r @ Request::Studio(crate::studio::Req::SetTheme { .. }))) => {
            let order = theme.saving.lock();
            let reply = ask(tx, r);
            if let (Reply::Ok(v), Ok(mut k)) = (&reply, theme.value.lock()) {
                if let Some(t) = v["theme"].as_str() {
                    *k = Some(t.to_string());
                }
            }
            drop(order);
            return json_reply(reply);
        }
        Some(Ok(r)) => return json_reply(ask(tx, r)),
        Some(Err(e)) => return json_reply(Reply::Err(e)),
        None => {}
    }
    match (is_get, parts.as_slice()) {
        (true, []) => {
            // 013 (research R-3): the engineer's theme on the page as served, so its first frame
            // is right. This server keeps it: the engine is asked once, for the first page, and
            // told of every choice through POST /api/theme, so a page never waits behind a slow
            // request (the review's 200 ms limit served System, and a light frame, on a busy
            // engine). System when it cannot be read.
            let kept = theme.value.lock().ok().and_then(|k| k.clone());
            let t = match kept {
                Some(t) => t,
                None => {
                    let t = match ask(tx, Request::Studio(crate::studio::Req::GetTheme)) {
                        Reply::Ok(v) => v["theme"].as_str().unwrap_or("system").to_string(),
                        Reply::Err(_) => "system".to_string(),
                    };
                    if let Ok(mut k) = theme.value.lock() {
                        k.get_or_insert(t.clone());
                    }
                    t
                }
            };
            text(200, "text/html; charset=utf-8", page_with_theme(&t))
        }
        (true, ["app.js"]) => text(200, "text/javascript; charset=utf-8", APP_JS.into()),
        (true, ["screens", name]) => match SCREENS.iter().find(|(n, _)| n == name) {
            Some((_, js)) => text(200, "text/javascript; charset=utf-8", (*js).into()),
            None => text(404, "text/plain", "not found".into()),
        },
        (true, ["fonts", name]) => match FONTS.iter().find(|(n, _)| n == name) {
            Some((_, data)) => bytes("font/woff2", data),
            None => text(404, "text/plain", "not found".into()),
        },
        (true, ["api", "dashboard"]) => json_reply(ask(tx, Request::Dashboard)),
        (true, ["api", "git"]) => json_reply(ask(tx, Request::Git { check: false })),
        (false, ["api", "git", "check"]) if method == "POST" => {
            json_reply(ask(tx, Request::Git { check: true }))
        }
        (true, ["api", "data"]) => json_reply(ask(tx, Request::Data)),
        (true, ["api", "events"]) => {
            let q = url.split_once('?').map(|(_, q)| q).unwrap_or("");
            let arg = |k: &str| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                    .map(|v| v.replace("%3A", ":").replace("%3a", ":"))
            };
            let after = arg("after")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let scope = arg("scope").unwrap_or_else(|| "global".into());
            let (rtx, rrx) = std::sync::mpsc::channel();
            if tx
                .send(Msg::Request(Request::Events { scope, after }, rtx))
                .is_err()
            {
                return text(503, "text/plain", "the service is stopping".into());
            }
            // Long polling (research S-4): the next change, or after 25 s with nothing new.
            match rrx.recv_timeout(std::time::Duration::from_secs(25)) {
                Ok(reply) => json_reply(reply),
                Err(_) => json_reply(Reply::Ok(json!({ "seq": after, "timeout": true }))),
            }
        }
        (true, ["app.css"]) => text(200, "text/css; charset=utf-8", APP_CSS.into()),
        // A loop (contracts/web-api.md, Loops).
        (true, ["api", "loops", r]) => match id(r) {
            Some(i) => json_reply(ask(tx, Request::Loop { id: i })),
            None => text(404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "activity"]) => match id(r) {
            Some(i) => {
                let arg = |k: &str| query_arg(&url, k).filter(|v| !v.is_empty());
                json_reply(ask(
                    tx,
                    Request::LoopActivity {
                        id: i,
                        filter: crate::run::activity::Filter {
                            agent: arg("agent"),
                            kind: arg("kind"),
                            task: arg("task"),
                            after: arg("after").and_then(|v| v.parse().ok()).unwrap_or(0),
                        },
                    },
                ))
            }
            None => text(404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "tasks", t]) => match id(r) {
            Some(i) => json_reply(ask(
                tx,
                Request::Task {
                    id: i,
                    task: percent_decode(t),
                },
            )),
            None => text(404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "steps", n, "activity"]) => {
            match (id(r), n.parse::<u32>().ok()) {
                (Some(i), Some(n)) => json_reply(ask(tx, Request::Activity { id: i, n })),
                _ => text(404, "text/plain", "no such step".into()),
            }
        }
        (false, ["api", "loops", r, action @ ..]) => {
            let Some(i) = id(r) else {
                return text(404, "text/plain", "no such loop".into());
            };
            let said = body["text"].as_str().unwrap_or_default().to_string();
            let confirmed = body["confirm"].as_bool() == Some(true);
            let request = match action {
                ["answer"] => Request::Answer { id: i, text: said },
                ["limits"] => Request::Limits {
                    id: i,
                    usd: body["usd"].as_f64(),
                    minutes: body["minutes"].as_u64(),
                    repair_cap: body["repair_cap"]
                        .as_u64()
                        .map(|r| r.min(u32::MAX as u64) as u32),
                    once: body["once"].as_bool().unwrap_or(false),
                },
                ["merge"] | ["stop"] | ["publish"] if !confirmed => {
                    return json_reply(Reply::Err("this needs {\"confirm\": true}".into()));
                }
                ["merge"] => Request::Merge {
                    id: i,
                    without_latest_check: body["without_latest_check"] == true,
                },
                ["publish"] => Request::Publish {
                    id: i,
                    without_latest_check: body["without_latest_check"] == true,
                },
                // 011: bring the loop up to date with its target (contracts/web-api.md).
                ["update"] => Request::Update { id: i },
                ["lock-again"] => Request::LockAgain { id: i },
                ["leave-pr"] => Request::LeavePr { id: i },
                ["wait"] => Request::Wait {
                    id: i,
                    other: body["for"].as_u64().unwrap_or(0),
                },
                ["stop-waiting"] => Request::StopWaiting { id: i },
                ["questions", q, "answer"] => Request::QuestionAnswer {
                    id: i,
                    q: percent_decode(q),
                    answer: body["answer"].as_str().unwrap_or_default().to_string(),
                },
                ["questions", q, "overrule"] => Request::Overrule {
                    id: i,
                    q: percent_decode(q),
                    answer: body["answer"].as_str().unwrap_or_default().to_string(),
                },
                ["questions", q, "confirm"] => Request::QuestionConfirm {
                    id: i,
                    q: percent_decode(q),
                },
                ["retry"] => Request::Retry { id: i },
                ["pause"] => Request::Pause { id: i },
                ["resume"] => Request::GoOn { id: i },
                ["note"] => Request::Note {
                    id: i,
                    text: said,
                    agent: body["agent"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                },
                ["stop"] => Request::Stop {
                    id: i,
                    reason: body["reason"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                },
                _ => return text(404, "text/plain", "no such action".into()),
            };
            json_reply(ask(tx, request))
        }
        _ => text(404, "text/plain", "not found".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_studio_routes_map_to_requests() {
        use crate::studio::Req;
        let b = json!({ "text": "hi", "version": "v1", "confirm": true });
        let r = studio_route(
            "PUT",
            &["api", "features", "shop", "001-cart", "doc"],
            &b,
            "/api/features/shop/001-cart/doc?path=specs%2F001-cart%2Fspec.md",
        );
        match r {
            Some(Ok(Request::Studio(Req::PutDoc { path, version, .. }))) => {
                assert_eq!(path, "specs/001-cart/spec.md");
                assert_eq!(version, "v1");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(
                studio_route("DELETE", &["api", "projects", "shop"], &json!({}), "/"),
                Some(Err(_))
            ),
            "removing needs the confirmation"
        );
        assert!(matches!(
            studio_route("DELETE", &["api", "projects", "shop"], &b, "/"),
            Some(Ok(Request::Studio(Req::RemoveProject { .. })))
        ));
        assert!(studio_route("GET", &["api", "runs"], &b, "/").is_none());
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn the_fonts_and_screens_are_all_served() {
        let css_fonts: Vec<&str> = APP_CSS
            .split("url(/fonts/")
            .skip(1)
            .filter_map(|s| s.split(')').next())
            .collect();
        assert_eq!(css_fonts.len(), FONTS.len());
        for f in css_fonts {
            assert!(
                FONTS.iter().any(|(n, _)| *n == f),
                "app.css names {f}, which is not served"
            );
        }
        for (_, data) in FONTS {
            assert!(data.starts_with(b"wOF2"), "a woff2 file");
        }
        for s in INDEX.split("src=\"/screens/").skip(1) {
            let name = s.split('"').next().unwrap();
            assert!(
                SCREENS.iter().any(|(n, _)| *n == name),
                "index.html loads {name}, which is not served"
            );
        }
    }

    #[test]
    fn the_page_uses_no_inline_style_or_script() {
        // The page's policy refuses inline styles and scripts (style-src and script-src 'self').
        for (name, text) in [("index.html", INDEX), ("app.js", APP_JS)]
            .into_iter()
            .chain(SCREENS.iter().copied())
        {
            assert!(!text.contains(" style=\""), "{name} has an inline style");
            assert!(!text.contains(" onclick="), "{name} has an inline handler");
        }
        assert!(
            !INDEX.contains("<script>"),
            "an inline script in index.html"
        );
    }

    #[test]
    fn the_page_loads_nothing_from_elsewhere() {
        for (name, text) in [
            ("index.html", INDEX),
            ("app.js", APP_JS),
            ("app.css", APP_CSS),
        ]
        .into_iter()
        .chain(SCREENS.iter().copied())
        {
            assert!(
                !text.contains("http://") && !text.contains("https://"),
                "{name} names an outside URL"
            );
            assert!(
                !text.contains("//fonts") && !text.contains("cdn"),
                "{name} loads from a CDN"
            );
        }
    }
}
