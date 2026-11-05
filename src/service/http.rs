//! The page on loopback (006 research R-6; 007 research S-1, S-3, S-4, contracts/web-api.md;
//! constitution V).
//!
//! - Bound to `127.0.0.1` only, on a free port.
//! - A per-launch secret, given only to the command line over the user-only pipe, is exchanged on
//!   the first visit for an `HttpOnly; SameSite=Strict` cookie; every request needs it.
//! - `Host` must name this server, and a `POST` must come from the page's own origin, against
//!   DNS rebinding and other sites.
//! - The page is embedded and loads nothing from anywhere else (`default-src 'self'`).

use std::io::Read;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use serde_json::{Value, json};
use tiny_http::{Header, Method, Response, Server, StatusCode};

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

/// The largest request body accepted.
const MAX_BODY: usize = 1024 * 1024;

pub struct Web {
    pub port: u16,
    server: Arc<Server>,
}

impl Drop for Web {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// What the security checks decide about a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Serve it.
    Pass,
    /// The first visit with the secret: set the cookie and redirect to `to`.
    SetCookie {
        to: String,
    },
    Refuse {
        code: u16,
        why: &'static str,
    },
}

/// The cookie's name for this server (cookies are not separated by port).
pub fn cookie_name(port: u16) -> String {
    format!("fallow_{port}")
}

/// The security checks of research R-6, on the request's method, URL and headers.
pub fn check(
    port: u16,
    secret: &str,
    method: &str,
    url: &str,
    host: Option<&str>,
    origin: Option<&str>,
    cookie: Option<&str>,
) -> Gate {
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| hosts.iter().any(|x| x == h)) {
        return Gate::Refuse {
            code: 403,
            why: "this server answers only to its own address",
        };
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if let Some(k) = query.split('&').find_map(|kv| kv.strip_prefix("k=")) {
        if method == "GET" && crate::platform::random::same(k, secret) {
            return Gate::SetCookie {
                to: path.to_string(),
            };
        }
    }
    let name = cookie_name(port);
    let has = cookie.is_some_and(|c| {
        c.split(';').any(|kv| {
            let kv = kv.trim();
            kv.strip_prefix(&format!("{name}="))
                .is_some_and(|v| crate::platform::random::same(v, secret))
        })
    });
    if !has {
        return Gate::Refuse {
            code: 401,
            why: "open Fallow with the `fallow` command to sign in to this page",
        };
    }
    if method != "GET" && method != "HEAD" {
        let origins = [
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ];
        if !origin.is_some_and(|o| origins.iter().any(|x| x == o)) {
            return Gate::Refuse {
                code: 403,
                why: "requests that change something must come from this page",
            };
        }
    }
    Gate::Pass
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn common_headers() -> Vec<Header> {
    vec![
        header(
            "Content-Security-Policy",
            "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'",
        ),
        header("X-Frame-Options", "DENY"),
        header("Cache-Control", "no-store"),
        header("X-Content-Type-Options", "nosniff"),
        header("Referrer-Policy", "no-referrer"),
    ]
}

fn respond(req: tiny_http::Request, code: u16, content_type: &str, body: String) {
    let mut r = Response::from_string(body).with_status_code(code);
    for h in common_headers() {
        r.add_header(h);
    }
    r.add_header(header("Content-Type", content_type));
    let _ = req.respond(r);
}

fn respond_bytes(req: tiny_http::Request, content_type: &str, body: &'static [u8]) {
    let mut r = Response::from_data(body).with_status_code(200);
    for h in common_headers() {
        r.add_header(h);
    }
    r.add_header(header("Content-Type", content_type));
    let _ = req.respond(r);
}

fn json_reply(req: tiny_http::Request, reply: Reply) {
    match reply {
        Reply::Ok(v) => respond(
            req,
            200,
            "application/json",
            json!({ "ok": true, "value": v }).to_string(),
        ),
        Reply::Err(e) => respond(
            req,
            409,
            "application/json",
            json!({ "ok": false, "error": e }).to_string(),
        ),
    }
}

impl Web {
    pub fn start(tx: Sender<Msg>, secret: String) -> Result<Web, String> {
        let server =
            Server::http("127.0.0.1:0").map_err(|e| format!("cannot open the page's port: {e}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or("the page's server has no address")?;
        let server = Arc::new(server);
        let s2 = Arc::clone(&server);
        std::thread::Builder::new()
            .name("http-accept".into())
            .spawn(move || {
                for req in s2.incoming_requests() {
                    let tx = tx.clone();
                    let secret = secret.clone();
                    let _ = std::thread::Builder::new()
                        .name("http-req".into())
                        .spawn(move || handle(req, &tx, port, &secret));
                }
            })
            .map_err(|e| format!("cannot start the page's server: {e}"))?;
        Ok(Web { port, server })
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
        // 008: only the fields the page's agent form changed (contracts/agents-api.md).
        ("PUT", ["api", "agents", name]) if body.get("fields").is_some() => {
            match body.get("fields").and_then(Value::as_object) {
                Some(fields) => r(Req::PutAgentFields {
                    name: name.to_string(),
                    scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
                    fields: fields.clone(),
                    body: s("body"),
                }),
                None => Some(Err("fields is a map of field names to values".into())),
            }
        }
        ("PUT", ["api", "agents", name]) => match need("text") {
            Ok(text) => r(Req::PutAgent {
                name: name.to_string(),
                scope: query_arg(url, "scope").unwrap_or_else(|| "user".into()),
                text,
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
        ("POST", ["api", "projects", p, "git-init"]) => {
            Some(confirmed().map(|()| Request::Studio(Req::GitInit { id: p.to_string() })))
        }
        ("POST", ["api", "projects", p, "conversation"]) => r(Req::ProjectConversation {
            project: p.to_string(),
        }),
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
        ("POST", ["api", "projects", p, "sessions"]) => match need("text") {
            Ok(t) => r(Req::NewSession {
                project: p.to_string(),
                agent: s("agent").filter(|a| !a.trim().is_empty()),
                text: t,
            }),
            Err(e) => Some(Err(e)),
        },
        ("GET", ["api", "sessions", c]) => r(Req::Conversation { id: c.to_string() }),
        ("POST", ["api", "sessions", c, "say"]) => match need("text") {
            Ok(t) => r(Req::Say {
                conv: c.to_string(),
                text: t,
            }),
            Err(e) => Some(Err(e)),
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
            match s("mode")
                .as_deref()
                .and_then(crate::agent::rights::Mode::parse)
            {
                Some(mode) => r(Req::SetMode {
                    conv: c.to_string(),
                    mode,
                }),
                None => Some(Err("mode: read_only, approved, ask or run_anything".into())),
            }
        }
        ("POST", ["api", "conversations" | "sessions", c, "approve"]) => {
            use crate::conversation::events::ApprovalAnswer;
            let answer = match s("answer").as_deref() {
                Some("once") => ApprovalAnswer::Once,
                Some("always") => ApprovalAnswer::Always,
                Some("deny") => ApprovalAnswer::Deny,
                _ => return Some(Err("answer: once, always or deny".into())),
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

fn header_value<'a>(req: &'a tiny_http::Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn handle(mut req: tiny_http::Request, tx: &Sender<Msg>, port: u16, secret: &str) {
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let gate = check(
        port,
        secret,
        &method,
        &url,
        header_value(&req, "Host"),
        header_value(&req, "Origin"),
        header_value(&req, "Cookie"),
    );
    match gate {
        Gate::Refuse { code, why } => {
            return respond(req, code, "text/plain; charset=utf-8", why.to_string());
        }
        Gate::SetCookie { to } => {
            let mut r = Response::empty(StatusCode(302));
            for h in common_headers() {
                r.add_header(h);
            }
            r.add_header(header(
                "Set-Cookie",
                &format!(
                    "{}={secret}; HttpOnly; SameSite=Strict; Path=/",
                    cookie_name(port)
                ),
            ));
            r.add_header(header("Location", if to.is_empty() { "/" } else { &to }));
            let _ = req.respond(r);
            return;
        }
        Gate::Pass => {}
    }
    let path = url.split('?').next().unwrap_or("/").to_string();
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let is_get = *req.method() == Method::Get;
    let body: Value = if is_get {
        Value::Null
    } else {
        let mut b = Vec::new();
        let _ = req
            .as_reader()
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut b);
        if b.len() > MAX_BODY {
            return respond(req, 413, "text/plain", "too large".into());
        }
        if b.is_empty() {
            json!({})
        } else {
            match serde_json::from_slice(&b) {
                Ok(v) => v,
                Err(_) => return respond(req, 400, "text/plain", "the body is not JSON".into()),
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
        return json_reply(req, reply);
    }
    match studio_route(&method, &dparts, &body, &url) {
        Some(Ok(r)) => return json_reply(req, ask(tx, r)),
        Some(Err(e)) => return json_reply(req, Reply::Err(e)),
        None => {}
    }
    match (is_get, parts.as_slice()) {
        (true, []) => respond(req, 200, "text/html; charset=utf-8", INDEX.into()),
        (true, ["app.js"]) => respond(req, 200, "text/javascript; charset=utf-8", APP_JS.into()),
        (true, ["screens", name]) => match SCREENS.iter().find(|(n, _)| n == name) {
            Some((_, js)) => respond(req, 200, "text/javascript; charset=utf-8", (*js).into()),
            None => respond(req, 404, "text/plain", "not found".into()),
        },
        (true, ["fonts", name]) => match FONTS.iter().find(|(n, _)| n == name) {
            Some((_, data)) => respond_bytes(req, "font/woff2", data),
            None => respond(req, 404, "text/plain", "not found".into()),
        },
        (true, ["api", "dashboard"]) => json_reply(req, ask(tx, Request::Dashboard)),
        (true, ["api", "data"]) => json_reply(req, ask(tx, Request::Data)),
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
                return respond(req, 503, "text/plain", "the service is stopping".into());
            }
            // Long polling (research S-4): the next change, or after 25 s with nothing new.
            match rrx.recv_timeout(std::time::Duration::from_secs(25)) {
                Ok(reply) => json_reply(req, reply),
                Err(_) => json_reply(req, Reply::Ok(json!({ "seq": after, "timeout": true }))),
            }
        }
        (true, ["app.css"]) => respond(req, 200, "text/css; charset=utf-8", APP_CSS.into()),
        // A loop (contracts/web-api.md, Loops).
        (true, ["api", "loops", r]) => match id(r) {
            Some(i) => json_reply(req, ask(tx, Request::Loop { id: i })),
            None => respond(req, 404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "activity"]) => match id(r) {
            Some(i) => {
                let arg = |k: &str| query_arg(&url, k).filter(|v| !v.is_empty());
                json_reply(
                    req,
                    ask(
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
                    ),
                )
            }
            None => respond(req, 404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "tasks", t]) => match id(r) {
            Some(i) => json_reply(
                req,
                ask(
                    tx,
                    Request::Task {
                        id: i,
                        task: percent_decode(t),
                    },
                ),
            ),
            None => respond(req, 404, "text/plain", "no such loop".into()),
        },
        (true, ["api", "loops", r, "steps", n, "activity"]) => {
            match (id(r), n.parse::<u32>().ok()) {
                (Some(i), Some(n)) => json_reply(req, ask(tx, Request::Activity { id: i, n })),
                _ => respond(req, 404, "text/plain", "no such step".into()),
            }
        }
        (false, ["api", "loops", r, action @ ..]) => {
            let Some(i) = id(r) else {
                return respond(req, 404, "text/plain", "no such loop".into());
            };
            let text = body["text"].as_str().unwrap_or_default().to_string();
            let confirmed = body["confirm"].as_bool() == Some(true);
            let request = match action {
                ["answer"] => Request::Answer { id: i, text },
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
                    return json_reply(req, Reply::Err("this needs {\"confirm\": true}".into()));
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
                    text,
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
                _ => return respond(req, 404, "text/plain", "no such action".into()),
            };
            json_reply(req, ask(tx, request))
        }
        _ => respond(req, 404, "text/plain", "not found".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "abc123";

    #[test]
    fn requests_need_the_host_the_cookie_and_for_posts_the_origin() {
        let ok_host = Some("127.0.0.1:5000");
        let cookie = Some("fallow_5000=abc123");
        assert_eq!(
            check(5000, S, "GET", "/", Some("evil.example:5000"), None, cookie),
            Gate::Refuse {
                code: 403,
                why: "this server answers only to its own address"
            }
        );
        assert!(matches!(
            check(5000, S, "GET", "/", ok_host, None, None),
            Gate::Refuse { code: 401, .. }
        ));
        assert!(matches!(
            check(
                5000,
                S,
                "GET",
                "/",
                ok_host,
                None,
                Some("fallow_5000=wrong")
            ),
            Gate::Refuse { code: 401, .. }
        ));
        assert!(matches!(
            check(
                5000,
                S,
                "GET",
                "/",
                ok_host,
                None,
                Some("fallow_4000=abc123")
            ),
            Gate::Refuse { code: 401, .. }
        ));
        assert_eq!(
            check(5000, S, "GET", "/api/runs", ok_host, None, cookie),
            Gate::Pass
        );
        assert_eq!(
            check(5000, S, "GET", "/runs/3?k=abc123", ok_host, None, None),
            Gate::SetCookie {
                to: "/runs/3".into()
            }
        );
        assert!(matches!(
            check(5000, S, "GET", "/?k=nope", ok_host, None, None),
            Gate::Refuse { code: 401, .. }
        ));
        assert!(matches!(
            check(
                5000,
                S,
                "POST",
                "/api/runs/1/merge",
                ok_host,
                Some("http://evil.example"),
                cookie
            ),
            Gate::Refuse { code: 403, .. }
        ));
        assert!(matches!(
            check(5000, S, "POST", "/api/runs/1/merge", ok_host, None, cookie),
            Gate::Refuse { code: 403, .. }
        ));
        assert_eq!(
            check(
                5000,
                S,
                "POST",
                "/api/runs/1/merge",
                ok_host,
                Some("http://127.0.0.1:5000"),
                cookie
            ),
            Gate::Pass
        );
        assert!(matches!(
            check(
                5000,
                S,
                "POST",
                "/api/runs/1/merge?k=abc123",
                ok_host,
                None,
                None
            ),
            Gate::Refuse { code: 401, .. }
        ));
    }

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
    fn studios_routes_need_the_secret_like_every_other() {
        // 007 T021 (constitution V): the new pages, scripts, fonts and APIs pass the same checks.
        let ok_host = Some("127.0.0.1:5000");
        let cookie = Some("fallow_5000=abc123");
        for path in [
            "/api/events?scope=global&after=0",
            "/api/dashboard",
            "/fonts/inter-400.woff2",
            "/screens/dashboard.js",
        ] {
            assert!(
                matches!(
                    check(5000, S, "GET", path, ok_host, None, None),
                    Gate::Refuse { code: 401, .. }
                ),
                "{path} without the cookie"
            );
            assert!(
                matches!(
                    check(
                        5000,
                        S,
                        "GET",
                        path,
                        Some("evil.example:5000"),
                        None,
                        cookie
                    ),
                    Gate::Refuse { code: 403, .. }
                ),
                "{path} from another host name"
            );
            assert_eq!(
                check(5000, S, "GET", path, ok_host, None, cookie),
                Gate::Pass,
                "{path}"
            );
        }
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
