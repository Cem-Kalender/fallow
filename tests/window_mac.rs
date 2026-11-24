//! 016 (SC-006, US2 AS1, T025): on macOS, closing Fallow's window does not stop work: the real
//! service (the `automation` build, with its test page for driving it), its real window opened
//! with `fallow`, a session turn started on the fake runtime, the window closed as its red button
//! closes it (`window-close`, an `automation`-only command), and the turn finishes. Needs the
//! Mac's desktop: skipped without a display (CI's macOS runners have one).

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The service, stopped and its folder removed when the test ends however it ends.
struct Service {
    child: Child,
    home: PathBuf,
    port: u16,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = ask(&self.home, json!({ "cmd": "stop" }));
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end && self.child.try_wait().ok().flatten().is_none() {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn sock(home: &Path) -> PathBuf {
    home.join("Library/Application Support/Fallow/service.sock")
}

/// One request on the service's socket, as `fallow` makes it.
fn ask(home: &Path, req: Value) -> Result<Value, String> {
    let mut s = UnixStream::connect(sock(home)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    writeln!(s, "{}", json!({ "kind": "cli", "request": req })).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))
}

fn http(port: u16, method: &str, path: &str, cookie: &str, body: &Value) -> (String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let body = body.to_string();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, rest) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    (head.to_string(), rest.to_string())
}

fn call(svc: &Service, cookie: &str, method: &str, path: &str, body: Value) -> Value {
    let (_, b) = http(svc.port, method, path, cookie, &body);
    let v: Value = serde_json::from_str(&b).unwrap_or_else(|e| panic!("{path}: {e}: {b}"));
    assert_ne!(v["ok"], false, "{method} {path}: {v}");
    v["value"].clone()
}

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(s.success(), "git {args:?}");
}

#[test]
fn closing_the_window_does_not_stop_a_session_turn() {
    if !fallow::platform::display::interactive() {
        eprintln!("skipped: no display (the window needs the Mac's desktop)");
        return;
    }
    // Short: the socket's path must stay under 104 bytes. Its own HOME: its own service.
    let home = PathBuf::from(format!("/tmp/f016w-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("config")).unwrap();
    let repo = home.join("work");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.email", "t@example.invalid"),
        ("user.name", "t"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("README.md"), "calc\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "seed"]);
    let scenario = home.join("scenario.toml");
    std::fs::write(
        &scenario,
        "[[step]]\nkind = \"assistant_turn\"\nsleep_ms = 8000\noutput = { reply = \"done\" }\n",
    )
    .unwrap();
    std::fs::write(
        home.join("config/settings.toml"),
        format!(
            "[agents.claude]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            env!("CARGO_BIN_EXE_fallow-fake-agent"),
            scenario.display().to_string()
        ),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .env("HOME", &home)
        .env("FALLOW_AUTOMATION", "1")
        .args(["serve", "--config-dir"])
        .arg(home.join("config"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let err = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if let Some(rest) = line
                .split("http://127.0.0.1:")
                .nth(1)
                .or_else(|| line.split("page on port ").nth(1))
            {
                let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let _ = tx.send(port.parse::<u16>().unwrap_or(0));
            }
        }
    });
    let port = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the service said its page's port");
    let svc = Service { child, home, port };

    // The page's cookie, from its address with the secret (as the bench gets it).
    let url = ask(&svc.home, json!({ "cmd": "url", "path": "/" })).unwrap();
    let url = url["url"].as_str().unwrap().to_string();
    let path_q = url.split_once(&format!(":{port}")).unwrap().1.to_string();
    let (head, _) = http(port, "GET", &path_q, "", &json!(null));
    let cookie = head
        .lines()
        .find_map(|l| {
            l.strip_prefix("Set-Cookie: ")
                .or_else(|| l.strip_prefix("set-cookie: "))
        })
        .and_then(|c| c.split(';').next())
        .expect("a cookie")
        .to_string();
    let p = call(
        &svc,
        &cookie,
        "POST",
        "/api/projects",
        json!({ "path": repo }),
    )["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);

    // The real window, as the engineer opens it.
    let opened = Command::new(env!("CARGO_BIN_EXE_fallow"))
        .env("HOME", &svc.home)
        .status()
        .unwrap();
    assert!(opened.success(), "`fallow` opens the window");

    let v = call(
        &svc,
        &cookie,
        "POST",
        &format!("/api/projects/{p}/sessions"),
        json!({ "text": "keep working" }),
    );
    let s = v["session"].as_str().expect("a session").to_string();
    let closed = ask(&svc.home, json!({ "cmd": "window-close" })).unwrap();
    assert_eq!(closed["ok"], true, "{closed}");
    // Asked of its page, the window closes within 3 s at most; the turn (8 s) is still running.
    std::thread::sleep(Duration::from_secs(4));
    let v = call(
        &svc,
        &cookie,
        "GET",
        &format!("/api/sessions/{s}"),
        json!({}),
    );
    assert_eq!(v["busy"], true, "the turn outlives the window: {v}");
    assert!(
        svc.home
            .join("Library/Application Support/Fallow/window.json")
            .exists(),
        "the window's place is kept when it closes"
    );

    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let v = call(
            &svc,
            &cookie,
            "GET",
            &format!("/api/sessions/{s}"),
            json!({}),
        );
        let done = v["busy"] == false
            && v["turns"]
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["reply"] == "done"));
        if done {
            break;
        }
        assert!(
            Instant::now() < end,
            "the turn did not finish with the window closed: {v}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    // The service still answers: closing the window kept it.
    assert_eq!(
        ask(&svc.home, json!({ "cmd": "status" })).unwrap()["ok"],
        true
    );
}
