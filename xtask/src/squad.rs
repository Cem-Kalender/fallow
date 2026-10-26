//! `cargo xtask squad` (004 T017; SC-009, quickstart Q1, Q2, Q8).
//!
//! Fallow runs in the PTY harness with an `mcp-agent` lead (#1). The first session offers
//! coordination; the harness declines it and starts the burst from the palette instead. In the
//! burst the lead starts a builder (#2) with a first task:
//! - no request reaches the engineer (the burst covers the start and the task);
//! - the builder receives the task and reports it done;
//! - the lead's `whoami` names its role and carries its briefing;
//! - after `Shift-X` the lead's next delivery waits for the engineer again (SC-007).
//!
//! A run where the lead never connects is INVALID.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::pty::{self, Session};

const LEAVE: &[u8] = b"\x1d";
const ESC: &[u8] = b"\x1b";
const CTRL_P: &[u8] = b"\x10";

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
                eprintln!("squad: unknown argument {other:?}");
                return 2;
            }
        }
    }
    let base = std::env::temp_dir().join(format!("fallow-squad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let result = run_all(&bin, &base);
    let _ = std::fs::remove_dir_all(&base);
    match result {
        Ok(code) => code,
        Err(e) => {
            println!("invalid: {e}");
            println!("os={} squad result=INVALID", std::env::consts::OS);
            pty::step_summary(&format!(
                "### Squads ({})\n\nINVALID: {e}\n",
                std::env::consts::OS
            ));
            2
        }
    }
}

/// Waits up to `limit` for `path` to hold text containing `want`; the last text read.
fn wait_for(path: &Path, want: &str, limit: Duration) -> String {
    let until = Instant::now() + limit;
    let mut last = String::new();
    while Instant::now() < until {
        if let Ok(t) = std::fs::read_to_string(path) {
            if t.contains(want) {
                return t;
            }
            last = t;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    last
}

fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn run_all(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let f = |name: &str| base.join(name).display().to_string();
    // The lead waits for the harness's files instead of typed lines: Fallow types its own
    // "coordination started" message into the lead.
    let lead = format!(
        "print lead-ready\nuntil-file {}\ncall whoami {{}}\nwrite {}\ncall session_start {{\"profile\":\"b\",\"project\":\"project\",\"first_task\":\"zz-build-the-page\",\"role\":\"builder\"}}\nwrite {}\nsleep 15000\ncall inbox {{}}\nwrite {}\nuntil-file {}\ncall deliver {{\"session\":2,\"text\":\"zz-after-end\"}}\nwrite {}\nprint lead-done\n",
        f("go"),
        f("lead-whoami"),
        f("lead-start"),
        f("lead-inbox"),
        f("ended"),
        f("lead-after"),
    );
    // The builder tries what its role may not do (SC-002).
    let builder = format!(
        "report-tasks {}\ncall whoami {{}}\nwrite {}\ncall session_start {{\"profile\":\"b\",\"project\":\"project\",\"role\":\"builder\"}}\nwrite {}\ncall event_signal {{\"name\":\"review-approved\"}}\nwrite {}\n",
        f("b-tasks"),
        f("b-whoami"),
        f("b-start"),
        f("b-signal"),
    );
    std::fs::write(base.join("lead.script"), lead).map_err(|e| e.to_string())?;
    std::fs::write(base.join("b.script"), builder).map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    for (id, script) in [("lead", "lead.script"), ("b", "b.script")] {
        profiles.push_str(&format!(
            "[[profile]]\nid = \"{id}\"\nname = \"{id}\"\ncommand = {:?}\nargs = [\"mcp-agent\", {:?}]\norchestration = \"claude\"\n\n",
            me.display().to_string(),
            f(script),
        ));
    }
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;
    println!(
        "squad: {} (burst from the palette, start with a first task, no prompt, Shift-X)",
        bin.display()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let flow = (|| -> Result<(), String> {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".into());
        }
        // The lead (#1); its start offers coordination: decline for now (Esc).
        s.expect(b"don't");
        s.send(b"n");
        pause(300);
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("the first session did not offer coordination")?;
        s.send(ESC);
        pause(300);
        s.send(LEAVE);
        pause(300);
        // Palette: "Coordinate this project…" with the lead selected; Enter starts 4 h.
        s.expect(b"usage.");
        s.send(CTRL_P);
        pause(300);
        s.send(b"coordinate this");
        pause(300);
        s.send(b"\r");
        s.seen(Duration::from_secs(10))
            .ok_or("the coordination dialog never opened")?;
        s.expect(b"Coordinating");
        s.send(b"\r");
        s.seen(Duration::from_secs(10))
            .ok_or("no countdown in the hint bar")?;
        std::fs::write(base.join("go"), "go").map_err(|e| e.to_string())?;
        // The builder gets its task and reports it; the lead reads its inbox.
        wait_for(&base.join("b-tasks"), "done", Duration::from_secs(60));
        wait_for(&base.join("lead-inbox"), "{", Duration::from_secs(60));
        // Shift-X in the orchestration view ends the burst.
        s.send(b"o");
        pause(500);
        s.send(b"X");
        pause(500);
        s.send(ESC);
        pause(300);
        std::fs::write(base.join("ended"), "ended").map_err(|e| e.to_string())?;
        Ok(())
    })();
    let whoami = wait_for(&base.join("lead-whoami"), "{", Duration::from_secs(5));
    let start = std::fs::read_to_string(base.join("lead-start")).unwrap_or_default();
    let inbox = std::fs::read_to_string(base.join("lead-inbox")).unwrap_or_default();
    let b_tasks = std::fs::read_to_string(base.join("b-tasks")).unwrap_or_default();
    let after = wait_for(&base.join("lead-after"), "{", Duration::from_secs(30));
    let b_whoami = std::fs::read_to_string(base.join("b-whoami")).unwrap_or_default();
    let b_start = std::fs::read_to_string(base.join("b-start")).unwrap_or_default();
    let b_signal = std::fs::read_to_string(base.join("b-signal")).unwrap_or_default();
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    println!("evidence: {}", outcome.evidence());
    if let Err(e) = flow {
        problems.push(e);
    }
    if !whoami.contains("\"session\":1") {
        problems.push(format!("the lead never called whoami: {whoami:?}"));
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let role_ok = whoami.contains("\"role\":\"lead\"") && whoami.contains("\"briefing\"");
    // Covered by the burst: the start is not pending, it happened, and its task was handed over.
    let start_ok = !start.contains("pending") && inbox.contains("started as #2");
    let task_ok = b_tasks.trim() == "T1 done";
    // After the burst: the delivery waits for the engineer again.
    let ended_ok = after.contains("pending");
    // SC-002: the builder has its role, and may neither start sessions nor approve reviews.
    let refusals_ok = b_whoami.contains("\"role\":\"builder\"")
        && b_start.contains("not_allowed")
        && b_signal.contains("not_allowed");
    let pass = role_ok && start_ok && task_ok && ended_ok && refusals_ok;
    let result = if pass { "PASS" } else { "FAIL" };
    let ok = |b: bool| if b { "ok" } else { "WRONG" };
    println!(
        "os={} role={} start={} task={} after_end={} builder_refusals={} result={result}",
        std::env::consts::OS,
        ok(role_ok),
        ok(start_ok),
        ok(task_ok),
        ok(ended_ok),
        ok(refusals_ok),
    );
    if !pass {
        println!(
            "whoami: {whoami:?}\nstart: {start:?}\ninbox: {inbox:?}\nbuilder: {b_tasks:?}\nafter: {after:?}\nbuilder whoami: {b_whoami:?}\nbuilder start: {b_start:?}\nbuilder signal: {b_signal:?}"
        );
    }
    pty::step_summary(&format!(
        "### Squads ({})\n\n| check | result | expected |\n|---|---|---|\n| the lead's whoami: role and briefing | {} | lead, briefing |\n| the lead starts a builder with a first task | {} | no prompt, started |\n| the builder receives the task and reports it | {} | T1 done |\n| a delivery after Shift-X | {} | pending again |\n| a builder's session_start and review-approved | {} | refused (SC-002) |\n\n**{result}**\n",
        std::env::consts::OS,
        ok(role_ok),
        ok(start_ok),
        ok(task_ok),
        ok(ended_ok),
        ok(refusals_ok),
    ));
    Ok(if pass { 0 } else { 1 })
}
