//! 006 T021 (constitution V): the run page listens on loopback only, needs the per-launch
//! secret, refuses a foreign Host or Origin, and sends the security headers.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;

use fallow::run::engine::{Msg, Reply};
use fallow::service::http::Web;

fn raw(port: u16, req: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn get(port: u16, path: &str, host: &str, extra: &str) -> String {
    raw(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

#[test]
fn the_page_checks_every_request() {
    let (tx, rx) = mpsc::channel::<Msg>();
    // A stand-in engine: every request gets an empty list.
    std::thread::spawn(move || {
        while let Ok(m) = rx.recv() {
            if let Msg::Request(_, reply) = m {
                let _ = reply.send(Reply::Ok(serde_json::json!([])));
            }
        }
    });
    let secret = "f".repeat(64);
    let web = Web::start(tx, secret.clone()).unwrap();
    let port = web.port;
    let host = format!("127.0.0.1:{port}");

    // Bound to loopback only: the machine's own network address does not answer. (A UDP
    // "connect" picks the outgoing address without sending anything.)
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    if probe.connect("192.0.2.1:9").is_ok() {
        let local = probe.local_addr().unwrap().ip();
        if !local.is_loopback() && !local.is_unspecified() {
            let r = TcpStream::connect_timeout(
                &std::net::SocketAddr::new(local, port),
                std::time::Duration::from_secs(2),
            );
            assert!(r.is_err(), "the page answers on {local}");
        }
    }

    let r = get(port, "/", &host, "");
    assert!(r.starts_with("HTTP/1.1 401"), "{r}");
    let r = get(port, "/", &format!("evil.example:{port}"), "");
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");

    // The secret is exchanged for an HttpOnly, SameSite=Strict cookie and dropped from the URL.
    let r = get(port, &format!("/runs/3?k={secret}"), &host, "");
    assert!(r.starts_with("HTTP/1.1 302"), "{r}");
    let lower = r.to_lowercase();
    assert!(lower.contains("location: /runs/3"), "{r}");
    assert!(
        lower.contains(&format!(
            "set-cookie: fallow_{port}={secret}; httponly; samesite=strict"
        )),
        "{r}"
    );

    let cookie = format!("Cookie: fallow_{port}={secret}\r\n");
    let r = get(port, "/", &host, &cookie);
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    let lower = r.to_lowercase();
    assert!(
        lower.contains("content-security-policy: default-src 'self'"),
        "{r}"
    );
    assert!(lower.contains("x-frame-options: deny"));
    assert!(lower.contains("cache-control: no-store"));
    let r = get(port, "/api/runs", &host, &cookie);
    assert!(
        r.starts_with("HTTP/1.1 200") && r.contains("\"ok\":true"),
        "{r}"
    );

    // Changes need the page's own Origin.
    let post = |origin: &str| {
        raw(
            port,
            &format!(
                "POST /api/runs/1/merge HTTP/1.1\r\nHost: {host}\r\n{cookie}{origin}Content-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            ),
        )
    };
    assert!(post("").starts_with("HTTP/1.1 403"));
    assert!(post("Origin: http://evil.example\r\n").starts_with("HTTP/1.1 403"));
    let r = post(&format!("Origin: http://127.0.0.1:{port}\r\n"));
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    drop(web);
}
