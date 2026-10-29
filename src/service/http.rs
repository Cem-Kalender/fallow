//! The run page on loopback (006 research R-6, contracts/web-api.md, constitution V).
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
use crate::run::engine::{Create, Msg, Reply, Request};

const INDEX: &str = include_str!("web/index.html");
const RUN: &str = include_str!("web/run.html");
const APP_JS: &str = include_str!("web/app.js");
const APP_CSS: &str = include_str!("web/app.css");

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
            "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'",
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
    match (is_get, parts.as_slice()) {
        (true, []) => respond(req, 200, "text/html; charset=utf-8", INDEX.into()),
        (true, ["runs", r]) if id(r).is_some() => {
            respond(req, 200, "text/html; charset=utf-8", RUN.into())
        }
        (true, ["app.js"]) => respond(req, 200, "text/javascript; charset=utf-8", APP_JS.into()),
        (true, ["app.css"]) => respond(req, 200, "text/css; charset=utf-8", APP_CSS.into()),
        (true, ["api", "runs"]) => json_reply(req, ask(tx, Request::List)),
        (true, ["api", "runs", r]) => match id(r) {
            Some(i) => json_reply(req, ask(tx, Request::Get { id: i })),
            None => respond(req, 404, "text/plain", "no such run".into()),
        },
        (true, ["api", "runs", r, "steps", n, "activity"]) => {
            match (id(r), n.parse::<u32>().ok()) {
                (Some(i), Some(n)) => json_reply(req, ask(tx, Request::Activity { id: i, n })),
                _ => respond(req, 404, "text/plain", "no such step".into()),
            }
        }
        (true, ["api", "runs", r, "wait"]) => {
            let Some(i) = id(r) else {
                return respond(req, 404, "text/plain", "no such run".into());
            };
            let q = url.split_once('?').map(|(_, q)| q).unwrap_or("");
            let num = |k: &str| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let (rtx, rrx) = std::sync::mpsc::channel();
            let wait = Request::Wait {
                id: i,
                seq: num("seq"),
                activity: num("activity"),
            };
            if tx.send(Msg::Request(wait, rtx)).is_err() {
                return respond(req, 503, "text/plain", "the service is stopping".into());
            }
            // Long polling (research R-6): the answer comes with the next change, or after
            // 25 s with nothing new.
            match rrx.recv_timeout(std::time::Duration::from_secs(25)) {
                Ok(reply) => json_reply(req, reply),
                Err(_) => json_reply(
                    req,
                    Reply::Ok(
                        json!({ "seq": num("seq"), "activity": num("activity"), "timeout": true }),
                    ),
                ),
            }
        }
        (false, ["api", "runs"]) => {
            let mut roles = std::collections::BTreeMap::new();
            if let Some(m) = body["roles"].as_object() {
                for (role, v) in m {
                    roles.insert(
                        role.clone(),
                        crate::config::RoleChoice {
                            agent: v["agent"].as_str().unwrap_or_default().to_string(),
                            model: v["model"].as_str().unwrap_or_default().to_string(),
                        },
                    );
                }
            }
            let c = Create {
                repo: body["repo"].as_str().unwrap_or_default().into(),
                task: body["task"].as_str().unwrap_or_default().to_string(),
                target: body["target"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                autonomy: body["autonomy"]
                    .as_str()
                    .and_then(crate::config::Autonomy::parse),
                limit_usd: body["limit_usd"].as_f64(),
                limit_min: body["limit_min"].as_u64(),
                roles,
            };
            json_reply(req, ask(tx, Request::Create(c)))
        }
        (false, ["api", "runs", r, action @ ..]) => {
            let Some(i) = id(r) else {
                return respond(req, 404, "text/plain", "no such run".into());
            };
            let text = body["text"].as_str().unwrap_or_default().to_string();
            let request = match action {
                ["plan", "say"] => Request::Say { id: i, text },
                ["plan", "approve"] => Request::Approve { id: i },
                ["answer"] => Request::Answer { id: i, text },
                ["limits"] => Request::Limits {
                    id: i,
                    usd: body["usd"].as_f64(),
                    minutes: body["minutes"].as_u64(),
                    repair_cap: body["repair_cap"].as_u64().map(|r| r as u32),
                },
                ["merge"] => Request::Merge { id: i },
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
    fn the_page_loads_nothing_from_elsewhere() {
        for (name, text) in [
            ("index.html", INDEX),
            ("run.html", RUN),
            ("app.js", APP_JS),
            ("app.css", APP_CSS),
        ] {
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
