//! `cargo xtask orchestration` (003 T023; SC-002, FR-003, research R-9).
//!
//! Fallow runs in the PTY harness with two `mcp-agent` sessions (A, B) whose profiles use the
//! "claude" orchestration style, so Fallow starts each with its own bridge.
//! - **Board latency (SC-002)**: A posts 50 timestamped messages; B polls the board and
//!   measures post-to-read. p95 < 1,000 ms.
//! - **Engineer's view (SC-002)**: with the orchestration view open on the project board, the
//!   time from A's last post to the message on Fallow's screen. < 1,000 ms.
//! - **Identity (FR-003)**: B runs A's bridge command (A's session number and token) from inside
//!   session B; Fallow refuses it. The same command run by this harness (outside every session),
//!   and with a wrong token, exits 3.
//!
//! A run where a session never starts or A never connects is INVALID.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::pty::{self, Session};

const POSTS: usize = 50;
const BUDGET_MS: f64 = 1000.0;
const LEAVE: &[u8] = b"\x1d";
const CTRL_B: &[u8] = b"\x02";
const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bin" => match it.next() {
                Some(b) => bin = PathBuf::from(b),
                None => {
                    eprintln!("--bin needs a value");
                    return 2;
                }
            },
            other => {
                eprintln!("orchestration: unknown argument {other:?}");
                return 2;
            }
        }
    }
    let base = std::env::temp_dir().join(format!("fallow-orch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let result = run_all(&bin, &base);
    let _ = std::fs::remove_dir_all(&base);
    match result {
        Ok(code) => code,
        Err(e) => {
            println!("invalid: {e}");
            println!("os={} orchestration result=INVALID", std::env::consts::OS);
            pty::step_summary(&format!(
                "### Orchestration ({})\n\nINVALID: {e}\n",
                std::env::consts::OS
            ));
            2
        }
    }
}

/// A's requests to type into B (US3): rejected, approved, approved with a permission, covered by
/// it, and (after "revoke all") pending again.
fn delivery_script(f: &dyn Fn(&str) -> String) -> String {
    let deliver = |t: &str| format!("call deliver {{\"session\":2,\"text\":\"{t}\"}}\n");
    format!(
        "sleep 1000\n{}sleep 4000\ncall inbox {{}}\nwrite {}\n{}sleep 4000\n{}sleep 4000\n{}sleep 7000\n{}write {}\nprint a-all-sent\nsleep 3000\ncall event_signal {{\"name\":\"pr-merge-x\",\"message\":\"PR merged\"}}\nsleep 15000\ncall task_list {{\"mine\":false}}\nwrite {}\ncall session_start {{\"profile\":\"c\",\"project\":\"project\",\"first_task\":\"say hello\"}}\nsleep 25000\ncall inbox {{}}\nwrite {}\n",
        deliver("zz-first"),
        f("a-inbox"),
        deliver("zz-second"),
        deliver("zz-third"),
        deliver("zz-fourth"),
        deliver("zz-fifth"),
        f("a-fifth"),
        f("a-tasks"),
        f("a-start"),
    )
}

fn epoch_ms(t: SystemTime) -> u128 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Waits up to `limit` for `path` to exist with content.
fn wait_file(path: &Path, limit: Duration) -> Option<String> {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if let Ok(t) = std::fs::read_to_string(path) {
            if !t.is_empty() {
                return Some(t);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// The engineer's side of A's delivery requests, in the Requests tab: reject the first,
/// approve the second, grant a permission with the third; the fourth needs no prompt; then
/// revoke all before the fifth.
fn deliveries(s: &mut Session) -> Result<(), String> {
    // Opened before any request is pending, the view shows the Board: Shift-Tab to Requests.
    s.send(b"o");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"[Z");
    std::thread::sleep(Duration::from_millis(300));
    for (text, keys) in [
        (&b"zz-first"[..], &b"x"[..]),
        (b"zz-second", b"y"),
        (b"zz-third", b"g"),
    ] {
        s.expect(text);
        s.seen(Duration::from_secs(30)).ok_or(format!(
            "request {} never showed",
            String::from_utf8_lossy(text)
        ))?;
        std::thread::sleep(Duration::from_millis(200));
        s.send(keys);
        std::thread::sleep(Duration::from_millis(300));
    }
    // The grant dialog is open, pre-filled "#1 → #2, deliver": grant it.
    s.send(b"\r");
    // The fourth request arrives ~4 s later and is covered; revoke all before the fifth (~11 s).
    std::thread::sleep(Duration::from_secs(6));
    s.send(b"X");
    std::thread::sleep(Duration::from_millis(300));
    s.send(ESC);
    std::thread::sleep(Duration::from_millis(300));
    // US4 (SC-001): queue "merge feature/x" for B after pr-merge-x, from the palette. A signals
    // the event; the engineer's task goes to B without any approval; B reports it done.
    s.send(b"\x10");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"new task");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"\r");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"#2 merge feature/x after:pr-merge-x");
    std::thread::sleep(Duration::from_millis(200));
    s.send(b"\r");
    std::thread::sleep(Duration::from_millis(300));
    s.send(ESC);
    std::thread::sleep(Duration::from_millis(300));
    // US5: A asks to start "c" with a first task. Review requests (palette), reject the fifth
    // delivery still waiting at the top, then approve the start.
    s.expect(b"start c in project");
    s.send(b"\x10");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"review requests");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"\r");
    s.seen(Duration::from_secs(90))
        .ok_or("A's start request never showed")?;
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"x");
    std::thread::sleep(Duration::from_millis(300));
    s.send(b"y");
    std::thread::sleep(Duration::from_millis(300));
    s.send(ESC);
    Ok(())
}

/// Runs a bridge command outside every session; its exit code (or `None` if it hung).
fn outside_bridge(cmd: &[String]) -> Option<i32> {
    let (exe, args) = cmd.split_first()?;
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut i) = child.stdin.take() {
        let _ = writeln!(
            i,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"outside","version":"1"}}}}}}"#
        );
        // Keep stdin open: only Fallow closing the channel may end the bridge.
        std::mem::forget(i);
    }
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if let Ok(Some(st)) = child.try_wait() {
            return st.code();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    None
}

fn run_all(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let f = |name: &str| base.join(name).display().to_string();
    let script_a = format!(
        "call whoami {{}}\nwrite {}\nsave-bridge {}\nprint a-ready\nuntil go\npost-timed {POSTS} 100 project\nsleep 1500\npost-stamped {} zz-visible-now\nsleep 2000\ncall event_signal {{\"name\":\"pr-123-merged\",\"message\":\"merged\"}}\nprint a-done\n{}",
        f("a-whoami"),
        f("bridge-a.json"),
        f("a-stamp"),
        delivery_script(&f),
    );
    let script_b = format!(
        "echo-to {}\nreport-tasks {}\ncall whoami {{}}\nbridge-from {}\nwrite {}\nprint b-measuring\nmeasure-reads {POSTS} project {}\nwait-loop pr-123-merged - {}\nwait-loop never 8 {}\n",
        f("b-typed"),
        f("b-tasks"),
        f("bridge-a.json"),
        f("b-verdict"),
        f("b-latency"),
        f("b-wait-signal"),
        f("b-wait-timeout"),
    );
    // C is started by A's request (US5); its first task is reported done.
    let script_c = format!(
        "report-tasks {}\ncall whoami {{}}\nwrite {}\n",
        f("c-tasks"),
        f("c-whoami"),
    );
    std::fs::write(base.join("c.script"), script_c).map_err(|e| e.to_string())?;
    std::fs::write(base.join("a.script"), script_a).map_err(|e| e.to_string())?;
    std::fs::write(base.join("b.script"), script_b).map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    for id in ["a", "b", "c"] {
        profiles.push_str(&format!(
            "[[profile]]\nid = \"{id}\"\nname = \"{id}\"\ncommand = {:?}\nargs = [\"mcp-agent\", {:?}]\norchestration = \"claude\"\n\n",
            me.display().to_string(),
            f(&format!("{id}.script")),
        ));
    }
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;
    // The latency run posts faster than the default 30 per minute.
    std::fs::write(
        config.join("settings.toml"),
        "[orchestration]\nrate_per_min = 600\nwait_block_s = 5\n",
    )
    .map_err(|e| e.to_string())?;
    println!(
        "orchestration: {} (post-to-read p95 < {BUDGET_MS} ms, on screen < {BUDGET_MS} ms, identity refusals)",
        bin.display()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let flow = (|| -> Result<f64, String> {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".into());
        }
        // A (#1): connects, saves its bridge command, waits for "go".
        s.expect(b"a-ready");
        s.send(b"n");
        std::thread::sleep(Duration::from_millis(300));
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("session A never connected (no a-ready)")?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        // B (#2): tries A's bridge from inside B, then measures reads.
        s.expect(b"b-measuring");
        s.send(b"n");
        std::thread::sleep(Duration::from_millis(300));
        s.send(DOWN);
        std::thread::sleep(Duration::from_millis(100));
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("session B never started measuring")?;
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        // To A, start the posts, leave, and watch the board in the orchestration view.
        s.send(CTRL_B);
        std::thread::sleep(Duration::from_millis(500));
        s.send(b"go\r");
        std::thread::sleep(Duration::from_millis(300));
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(300));
        s.expect(b"zz-visible-now");
        s.send(b"o");
        s.seen(Duration::from_secs(60))
            .ok_or("the last post never showed in the orchestration view")?;
        let shown = SystemTime::now();
        s.send(ESC);
        deliveries(&mut s)?;
        let stamp: u128 = wait_file(&base.join("a-stamp"), Duration::from_secs(10))
            .and_then(|t| t.trim().parse().ok())
            .ok_or("A did not record its post time")?;
        Ok(epoch_ms(shown).saturating_sub(stamp) as f64)
    })();
    let latency = wait_file(&base.join("b-latency"), Duration::from_secs(60));
    let fifth = wait_file(&base.join("a-fifth"), Duration::from_secs(60)).unwrap_or_default();
    let inbox = std::fs::read_to_string(base.join("a-inbox")).unwrap_or_default();
    // A writes its task list last; read B's typed lines after it.
    let tasks = wait_file(&base.join("a-tasks"), Duration::from_secs(60)).unwrap_or_default();
    let typed = std::fs::read_to_string(base.join("b-typed")).unwrap_or_default();
    let b_tasks = std::fs::read_to_string(base.join("b-tasks")).unwrap_or_default();
    let a_start = wait_file(&base.join("a-start"), Duration::from_secs(60)).unwrap_or_default();
    let c_tasks = wait_file(&base.join("c-tasks"), Duration::from_secs(30)).unwrap_or_default();
    // B then waits for A's signal, and for an event nobody signals (8 s limit, 5 s blocks).
    let wait_signal =
        wait_file(&base.join("b-wait-signal"), Duration::from_secs(30)).unwrap_or_default();
    let wait_timeout =
        wait_file(&base.join("b-wait-timeout"), Duration::from_secs(30)).unwrap_or_default();
    let cmd: Option<Vec<String>> = std::fs::read_to_string(base.join("bridge-a.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let outside = cmd.as_deref().map(outside_bridge);
    let wrong_token = cmd.clone().map(|mut c| {
        if let Some(last) = c.last_mut() {
            *last = "00".repeat(32);
        }
        outside_bridge(&c)
    });
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    println!("evidence: {}", outcome.evidence());
    let shown_ms = match flow {
        Ok(v) => v,
        Err(e) => {
            problems.push(e);
            f64::NAN
        }
    };
    let whoami = std::fs::read_to_string(base.join("a-whoami")).unwrap_or_default();
    if !whoami.contains("\"session\":1") {
        problems.push(format!("A's whoami did not name session 1: {whoami:?}"));
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let latency = latency.unwrap_or_default();
    let field = |k: &str| -> f64 {
        latency
            .split_whitespace()
            .find_map(|w| w.strip_prefix(k))
            .and_then(|v| v.parse().ok())
            .unwrap_or(f64::NAN)
    };
    let (p95, max, n) = (field("p95_ms="), field("max_ms="), field("n="));
    let verdict = std::fs::read_to_string(base.join("b-verdict")).unwrap_or_default();
    let inside_refused = verdict.trim() == "refused";
    let outside_refused = outside == Some(Some(3));
    let wrong_refused = wrong_token == Some(Some(3));
    let latency_ok = n as usize == POSTS && p95 < BUDGET_MS;
    let shown_ok = shown_ms < BUDGET_MS;
    // US2: the signal reaches the waiting agent with A as sender; a limited wait times out after
    // one "still_waiting" repeat.
    let signal_ok = wait_signal.contains("status=signalled") && wait_signal.contains("from=1");
    let timeout_ok = wait_timeout.trim() == "status=timed_out repeats=1 from=null";
    // US3: only approved or covered requests were typed, each with its label; the fifth waits.
    let label = "[Fallow: message from agent #1";
    let got = |t: &str| typed.lines().any(|l| l.contains(t) && l.starts_with(label));
    let delivery_ok = !typed.contains("zz-first")
        && got("zz-second")
        && got("zz-third")
        && got("zz-fourth")
        && !typed.contains("zz-fifth")
        && inbox.contains("rejected")
        && fifth.contains("pending");
    // US4 / SC-001: the queued task waited for the event, went to B with no approval, and B's
    // report made it done.
    let queue_ok = b_tasks.trim() == "T1 done"
        && tasks.contains("\"state\":\"done\"")
        && tasks.contains("merge feature/x")
        && typed.lines().any(|l| {
            l.starts_with("[Fallow: task T1 from the user's queue.")
                && l.ends_with("merge feature/x")
        });
    // US5: the approved start became session #3, whose first task was handed over without
    // another prompt and reported done.
    let start_ok = a_start.contains("started as #3") && c_tasks.trim() == "T2 done";
    if !start_ok {
        println!("A's inbox after the start: {a_start:?}; C's task reports: {c_tasks:?}");
    }
    let pass = start_ok
        && queue_ok
        && delivery_ok
        && latency_ok
        && shown_ok
        && inside_refused
        && outside_refused
        && wrong_refused
        && signal_ok
        && timeout_ok;
    let result = if pass { "PASS" } else { "FAIL" };
    println!(
        "os={} posts={POSTS} reads={n} post_read_p95_ms={p95:.0} post_read_max_ms={max:.0} on_screen_ms={shown_ms:.0} budget_ms={BUDGET_MS} other_session_token={} outside_bridge={:?} wrong_token={:?} wait_signal={:?} wait_timeout={:?} deliveries={} queue={} start={} result={result}",
        std::env::consts::OS,
        if inside_refused {
            "refused"
        } else {
            "ACCEPTED"
        },
        outside.flatten(),
        wrong_token.flatten(),
        wait_signal.trim(),
        wait_timeout.trim(),
        if delivery_ok { "ok" } else { "WRONG" },
        if queue_ok { "ok" } else { "WRONG" },
        if start_ok { "ok" } else { "WRONG" },
    );
    if !queue_ok {
        println!(
            "B's task reports: {b_tasks:?}; A's task_list: {tasks:?}; typed into B: {typed:?}"
        );
    }
    if !delivery_ok {
        println!("typed into B: {typed:?}; A's inbox: {inbox:?}; fifth: {fifth:?}");
    }
    pty::step_summary(&format!(
        "### Orchestration ({})\n\n| check | result | budget |\n|---|---|---|\n| post → read by another agent, p95 of {POSTS} | {p95:.0} ms | < {BUDGET_MS} ms |\n| post → on the engineer's screen | {shown_ms:.0} ms | < {BUDGET_MS} ms |\n| another session's token, from inside a session | {} | refused |\n| bridge run outside every session | exit {:?} | exit 3 |\n| wrong token | exit {:?} | exit 3 |\n| wait, then A signals | {} | signalled by #1 |\n| wait with an 8 s limit (5 s blocks) | {} | timed out after one repeat |\n| deliveries: rejected, approved, granted, covered, revoked | {} | only approved or covered ones typed, labelled |\n| PR scenario: queued task after pr-merge-x, done by B | {} | no approval, done |\n| A asks to start a session with a first task; approved | {} | started, task done |\n\n**{result}**\n",
        std::env::consts::OS,
        if inside_refused {
            "refused"
        } else {
            "ACCEPTED"
        },
        outside.flatten(),
        wrong_token.flatten(),
        wait_signal.trim(),
        wait_timeout.trim(),
        if delivery_ok { "ok" } else { "WRONG" },
        if queue_ok { "ok" } else { "WRONG" },
        if start_ok { "ok" } else { "WRONG" },
    ));
    Ok(if pass { 0 } else { 1 })
}
