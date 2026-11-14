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

use tiny_http::{Header, Response, Server, StatusCode};

use super::answer::{HEADERS, KeptTheme, MAX_READ, PageRequest, answer};
use crate::run::engine::Msg;

/// 013: kept at its path for the tests (`answer.rs` holds it since 014).
pub use super::answer::page_with_theme;

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

fn respond(req: tiny_http::Request, code: u16, content_type: &str, body: Vec<u8>) {
    let mut r = Response::from_data(body).with_status_code(code);
    for (k, v) in HEADERS {
        r.add_header(header(k, v));
    }
    r.add_header(header("Content-Type", content_type));
    let _ = req.respond(r);
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
                // 013: the engineer's theme, kept by this server so the page is served with it
                // without waiting on the engine (research R-3).
                let theme: Arc<KeptTheme> = Arc::default();
                for req in s2.incoming_requests() {
                    let tx = tx.clone();
                    let secret = secret.clone();
                    let theme = Arc::clone(&theme);
                    let _ = std::thread::Builder::new()
                        .name("http-req".into())
                        .spawn(move || handle(req, &tx, port, &secret, &theme));
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

/// The checks of this transport (`check`), then the Studio's answer.
fn handle(
    mut req: tiny_http::Request,
    tx: &Sender<Msg>,
    port: u16,
    secret: &str,
    theme: &KeptTheme,
) {
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
            return respond(
                req,
                code,
                "text/plain; charset=utf-8",
                why.as_bytes().to_vec(),
            );
        }
        Gate::SetCookie { to } => {
            let mut r = Response::empty(StatusCode(302));
            for (k, v) in HEADERS {
                r.add_header(header(k, v));
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
    let mut body = Vec::new();
    if method != "GET" && method != "HEAD" {
        let _ = req
            .as_reader()
            .take(MAX_READ as u64 + 1)
            .read_to_end(&mut body);
        if body.len() > MAX_READ {
            return respond(req, 413, "text/plain", b"too large".to_vec());
        }
    }
    let a = answer(PageRequest { method, url, body }, tx, theme);
    respond(req, a.status, &a.content_type, a.body)
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
}
