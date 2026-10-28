//! `cargo xtask squad` (004 T017, T022, T028, T035; SC-002, SC-003, SC-004, SC-009).
//!
//! Fallow runs in the PTY harness on a git project whose squad file declares a gate (it passes
//! unless `gate.fail` exists) and a merge that needs the gate, the builders' work and a review.
//! Every agent is an `mcp-agent` script:
//! - The lead (#1) starts; Fallow offers coordination; the harness declines and starts the burst
//!   from the palette instead. The baseline gate runs at once (#2).
//! - The lead starts two builders (#3, #4) with first tasks and a reviewer (#5): no request
//!   reaches the engineer. Builders commit and report with evidence; one of them also tries to
//!   start a session and approve a review (refused, SC-002). The lead tries to fake the gate
//!   (refused, SC-004).
//! - The lead acts only on Fallow's squad updates typed into its session, never polling (SC-010):
//!   when both builders' reports have arrived it merges their branches into its worktree; Fallow
//!   runs the final gate on that commit and signals it; the reviewer approves; Fallow tells the
//!   lead every merge condition holds. Before that the merge stays closed (SC-003).
//! - When the merge is ready, the harness presses `m`, `y` in the Squad tab: the project's main
//!   branch then contains the gated commit, and no agent was ever handed a merge (SC-009).
//! - After `Shift-X` the lead's next delivery waits for the engineer again (SC-007).
//!
//! A run where the lead never connects is INVALID.
//!
//! A second run, `exits` (T054, FR-039 to FR-042), has no gate and the burst's 3 starts:
//! - The lead starts two builders (#2 api, #3 page); a third builder's start waits for the
//!   engineer, the last start being kept for the reviewer the merge needs; the reviewer (#4)
//!   starts at once.
//! - The reviewer exits before its verdict and builder #2 exits with its task in progress. Fallow
//!   tells the lead both; the lead hands the api part to #3, withdraws #2's task, and starts a
//!   new reviewer, which waits for the engineer (no starts left). The harness approves in the
//!   Requests tab.
//! - The new reviewer approves and #3 reports both parts: the merge is ready without the exited
//!   sessions, and says why.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::pty::{self, Session};

const LEAVE: &[u8] = b"\x1d";
const ESC: &[u8] = b"\x1b";
const CTRL_P: &[u8] = b"\x10";
const BACKTAB: &[u8] = b"\x1b[Z";

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
    let mut code = 0;
    for (name, run) in [
        ("squad", run_all as fn(&Path, &Path) -> Result<i32, String>),
        ("exits", run_exits),
        ("solo-loop", run_solo_loop),
    ] {
        let base = std::env::temp_dir().join(format!("fallow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let result = run(&bin, &base);
        let _ = std::fs::remove_dir_all(&base);
        code = code.max(match result {
            Ok(code) => code,
            Err(e) => {
                println!("invalid: {e}");
                println!("os={} {name} result=INVALID", std::env::consts::OS);
                pty::step_summary(&format!(
                    "### Squads, {name} ({})\n\nINVALID: {e}\n",
                    std::env::consts::OS
                ));
                2
            }
        });
    }
    code
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

/// The squad the harness's project declares.
const SQUAD: &str = r#"name = "harness"
[[gate]]
name = "tests"
command = "test ! -f gate.fail"
windows = "if (Test-Path gate.fail) { exit 1 }"
timeout_min = 5
baseline = true

[merge]
requires = ["gate:tests", "done:builder", "signal:review-approved@reviewer"]
"#;

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=harness",
            "-c",
            "user.email=harness@example.com",
        ])
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The `solo-loop` run's squad (005): the built-in roles and merge, plus a gate.
const SQUAD_SOLO: &str = r#"name = "solo"
[[gate]]
name = "tests"
command = "test ! -f gate.fail"
windows = "if (Test-Path gate.fail) { exit 1 }"
timeout_min = 5
baseline = true

[merge]
requires = ["gate:tests", "work:done", "review:approved"]
"#;

/// The `exits` run's squad: no gate, the built-in roles.
const SQUAD_EXITS: &str = r#"name = "exits"
[merge]
requires = ["done:builder", "signal:review-approved@reviewer"]
"#;

/// A git project with the squad file, committed on main (the burst's starting commit).
fn make_project(project: &Path, squad: &str) -> Result<(), String> {
    git(project, &["init", "-q", "-b", "main"])?;
    std::fs::create_dir_all(project.join(".fallow")).map_err(|e| e.to_string())?;
    std::fs::write(project.join(".fallow").join("squad.toml"), squad).map_err(|e| e.to_string())?;
    std::fs::write(project.join("README.md"), "harness\n").map_err(|e| e.to_string())?;
    git(project, &["add", "."])?;
    git(project, &["commit", "-q", "-m", "start"]).map(|_| ())
}

fn start(profile: &str, role: &str, task: Option<&str>) -> String {
    let task = task.map_or(String::new(), |t| format!(",\"first_task\":\"{t}\""));
    format!(
        "call session_start {{\"profile\":\"{profile}\",\"project\":\"project\",\"role\":\"{role}\"{task}}}\n"
    )
}

fn run_all(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    make_project(&project, SQUAD)?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let f = |name: &str| base.join(name).display().to_string();
    // The lead waits for the harness's files instead of typed lines: Fallow types its own
    // messages into it.
    let lead = format!(
        "print lead-ready\nuntil-file {go}\ncall whoami {{}}\nwrite {whoami}\ncall event_signal {{\"name\":\"gate-tests-passed\"}}\nwrite {fake}\n{b}write {start}\n{c}{r}call squad_status {{}}\nwrite {early}\nuntil-all was reported done by #3|was reported done by #4\nmerge-branches fallow/\nuntil Every merge condition holds\ncall squad_status {{}}\nwrite {ready}\ncall inbox {{}}\nwrite {inbox}\ncall task_list {{\"mine\":false}}\nwrite {tasks}\nuntil-file {ended}\ncall deliver {{\"session\":3,\"text\":\"zz-after-end\"}}\nwrite {after}\nprint lead-done\n",
        go = f("go"),
        whoami = f("lead-whoami"),
        fake = f("lead-gate"),
        b = start("b", "builder", Some("zz-build-the-api")),
        start = f("lead-start"),
        c = start("c", "builder", Some("zz-build-the-page")),
        r = start("r", "reviewer", None),
        early = f("lead-status-early"),
        ready = f("lead-ready-status"),
        inbox = f("lead-inbox"),
        tasks = f("lead-tasks"),
        ended = f("ended"),
        after = f("lead-after"),
    );
    // The first builder also tries what its role may not do (SC-002).
    let builder_b = format!(
        "report-tasks {}\ncommit-file api.txt\ncall whoami {{}}\nwrite {}\n{}write {}\ncall event_signal {{\"name\":\"review-approved\"}}\nwrite {}\ncall event_signal {{\"name\":\"api-done\"}}\n",
        f("b-tasks"),
        f("b-whoami"),
        start("b", "builder", None),
        f("b-start"),
        f("b-signal"),
    );
    let builder_c = format!(
        "report-tasks {}\ncommit-file web.txt\ncall event_signal {{\"name\":\"web-done\"}}\n",
        f("c-tasks"),
    );
    // The reviewer approves once Fallow signalled the gate.
    let reviewer = format!(
        "wait-loop gate-tests-passed 300 {}\nverdict approved - api and page checked\nwrite {}\n",
        f("r-gate"),
        f("r-verdict"),
    );
    for (name, text) in [
        ("lead.script", lead),
        ("b.script", builder_b),
        ("c.script", builder_c),
        ("r.script", reviewer),
    ] {
        std::fs::write(base.join(name), text).map_err(|e| e.to_string())?;
    }
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    for id in ["lead", "b", "c", "r"] {
        profiles.push_str(&format!(
            "[[profile]]\nid = \"{id}\"\nname = \"{id}\"\ncommand = {:?}\nargs = [\"mcp-agent\", {:?}]\norchestration = \"claude\"\n\n",
            me.display().to_string(),
            f(&format!("{id}.script")),
        ));
    }
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;
    println!(
        "squad: {} (lead, two builders, a reviewer and a gate; the engineer merges)",
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
        // The countdown shows once the start notices have gone.
        s.expect(b"coordinating");
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("no countdown in the hint bar")?;
        std::fs::write(base.join("go"), "go").map_err(|e| e.to_string())?;
        // Every condition holds: merge from the Squad tab (Board → Requests → Squad).
        let ready = wait_for(
            &base.join("lead-ready-status"),
            "\"ready\":true",
            Duration::from_secs(400),
        );
        if !ready.contains("\"ready\":true") {
            return Err(format!("the merge never became ready: {ready}"));
        }
        s.send(b"o");
        pause(500);
        s.send(BACKTAB);
        pause(300);
        s.send(BACKTAB);
        pause(300);
        s.expect(b"Merge");
        s.send(b"m");
        s.seen(Duration::from_secs(10)).ok_or("no merge dialog")?;
        pause(300);
        s.send(b"y");
        pause(2000);
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
    let read = |n: &str| std::fs::read_to_string(base.join(n)).unwrap_or_default();
    let whoami = wait_for(&base.join("lead-whoami"), "{", Duration::from_secs(5));
    let after = wait_for(&base.join("lead-after"), "{", Duration::from_secs(30));
    let (start_, inbox, tasks, ready) = (
        read("lead-start"),
        read("lead-inbox"),
        read("lead-tasks"),
        read("lead-ready-status"),
    );
    let (early, fake_gate, r_gate) = (read("lead-status-early"), read("lead-gate"), read("r-gate"));
    let (b_tasks, c_tasks, b_whoami, b_start, b_signal) = (
        read("b-tasks"),
        read("c-tasks"),
        read("b-whoami"),
        read("b-start"),
        read("b-signal"),
    );
    // The gated commit, as squad_status named it when the merge became ready.
    let gated = ready
        .split("\"merge\":{\"commit\":\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or_default()
        .to_string();
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
    // Covered by the burst: no start waited; the baseline gate is #2, so the builders are #3, #4.
    let start_ok = !start_.contains("pending")
        && inbox.contains("started as #3")
        && inbox.contains("started as #4")
        && inbox.contains("started as #5");
    let task_ok =
        b_tasks.trim().starts_with("T") && b_tasks.contains("done") && c_tasks.contains("done");
    let ended_ok = after.contains("pending");
    let refusals_ok = b_whoami.contains("\"role\":\"builder\"")
        && b_start.contains("not_allowed")
        && b_signal.contains("not_allowed");
    let closed_ok = early.contains("\"ready\":false");
    let gate_ok = fake_gate.contains("not_allowed")
        && r_gate.contains("status=signalled")
        && ready.contains("\"kind\":\"baseline\"")
        && ready.contains("\"class\":\"pass\"");
    // SC-009: the target branch has exactly the gated commit, merged by Fallow on the
    // engineer's press, and no agent was handed a merge.
    let on_main = !gated.is_empty()
        && git(&project, &["merge-base", "--is-ancestor", &gated, "main"]).is_ok()
        && git(&project, &["log", "-1", "--format=%s", "main"])
            .is_ok_and(|m| m.contains("merged by Fallow"));
    let no_merge_task = !tasks.is_empty() && !tasks.to_lowercase().contains("git merge");
    let merge_ok = on_main && no_merge_task;
    let pass = role_ok
        && start_ok
        && task_ok
        && ended_ok
        && refusals_ok
        && closed_ok
        && gate_ok
        && merge_ok;
    let result = if pass { "PASS" } else { "FAIL" };
    let ok = |b: bool| if b { "ok" } else { "WRONG" };
    println!(
        "os={} role={} starts={} tasks={} after_end={} builder_refusals={} merge_closed={} gate={} merged={} result={result}",
        std::env::consts::OS,
        ok(role_ok),
        ok(start_ok),
        ok(task_ok),
        ok(ended_ok),
        ok(refusals_ok),
        ok(closed_ok),
        ok(gate_ok),
        ok(merge_ok),
    );
    if !pass {
        println!(
            "whoami: {whoami:?}\nstart: {start_:?}\ninbox: {inbox:?}\nbuilder b: {b_tasks:?}\nbuilder c: {c_tasks:?}\nafter: {after:?}\nbuilder whoami: {b_whoami:?}\nbuilder start: {b_start:?}\nbuilder signal: {b_signal:?}\nready: {ready:?}\nearly: {early:?}\nfake gate: {fake_gate:?}\nreviewer gate: {r_gate:?}\ntasks: {tasks:?}\ngated: {gated:?} on main: {on_main}"
        );
    }
    pty::step_summary(&format!(
        "### Squads ({})\n\n| check | result | expected |\n|---|---|---|\n| the lead's whoami: role and briefing | {} | lead, briefing |\n| the lead starts two builders and a reviewer | {} | no prompt, started |\n| builders receive their tasks and report them with evidence | {} | done |\n| a delivery after Shift-X | {} | pending again |\n| a builder's session_start and review-approved | {} | refused (SC-002) |\n| merge before the work, gate and review | {} | closed (SC-003) |\n| gate: baseline, final on the lead's commit, Fallow's signal; an agent's refused | {} | pass (SC-004) |\n| the engineer's merge of the gated commit, no merge task for agents | {} | on main (SC-009) |\n\n**{result}**\n",
        std::env::consts::OS,
        ok(role_ok),
        ok(start_ok),
        ok(task_ok),
        ok(ended_ok),
        ok(refusals_ok),
        ok(closed_ok),
        ok(gate_ok),
        ok(merge_ok),
    ));
    Ok(if pass { 0 } else { 1 })
}

/// Writes the agents' scripts and a profile for each (`<name>.script`).
fn write_agents(base: &Path, config: &Path, agents: &[(&str, String)]) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    for (id, text) in agents {
        let script = base.join(format!("{id}.script"));
        std::fs::write(&script, text).map_err(|e| e.to_string())?;
        profiles.push_str(&format!(
            "[[profile]]\nid = \"{id}\"\nname = \"{id}\"\ncommand = {:?}\nargs = [\"mcp-agent\", {:?}]\norchestration = \"claude\"\n\n",
            me.display().to_string(),
            script.display().to_string(),
        ));
    }
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())
}

/// Starts the burst from the palette, as `run_all` does (the lead's offer declined first).
fn start_burst(s: &mut Session) -> Result<(), String> {
    if s.wait_frame(Duration::from_secs(10)).is_none() {
        return Err("no first frame".into());
    }
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
    s.expect(b"usage.");
    s.send(CTRL_P);
    pause(300);
    s.send(b"coordinate this");
    pause(300);
    s.send(b"\r");
    s.seen(Duration::from_secs(10))
        .ok_or("the coordination dialog never opened")?;
    s.expect(b"coordinating");
    s.send(b"\r");
    s.seen(Duration::from_secs(30))
        .ok_or("no countdown in the hint bar")?;
    Ok(())
}

fn run_exits(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    make_project(&project, SQUAD_EXITS)?;
    let f = |name: &str| base.join(name).display().to_string();
    let start_part = |profile: &str, task: &str, part: &str| {
        format!(
            "call session_start {{\"profile\":\"{profile}\",\"project\":\"project\",\"role\":\"builder\",\"first_task\":\"{task}\",\"part\":\"{part}\"}}\n"
        )
    };
    let lead = format!(
        "print lead-ready\nuntil-file {go}\n{b}write {start_b}\n{c}{d}write {held}\n{r1}write {start_r1}\nuntil-all #2 builder exited with unfinished work|#4 reviewer exited without signalling\ncall task_create {{\"text\":\"zz-build-the-api-again\",\"target\":{{\"session\":3}},\"part\":\"api\"}}\nwrite {rehand}\ncall task_report {{\"task\":1,\"outcome\":\"cancelled\",\"note\":\"handed to #3\"}}\nwrite {withdraw}\n{r2}write {start_r2}\nuntil Every merge condition holds\ncall squad_status {{}}\nwrite {ready}\nprint lead-done\n",
        go = f("go"),
        b = start_part("b", "zz-build-the-api", "api"),
        start_b = f("lead-start-b"),
        c = start_part("c", "zz-build-the-page", "page"),
        d = start("d", "builder", None),
        held = f("lead-held"),
        r1 = start("r1", "reviewer", None),
        start_r1 = f("lead-start-r1"),
        rehand = f("lead-rehand"),
        withdraw = f("lead-withdraw"),
        r2 = start("r2", "reviewer", None),
        start_r2 = f("lead-start-r2"),
        ready = f("lead-ready-status"),
    );
    write_agents(
        base,
        &config,
        &[
            ("lead", lead),
            // Exits with its task in progress.
            ("b", "until zz-build-the-api\nsleep 500\nexit\n".to_string()),
            (
                "c",
                format!(
                    "report-tasks {}\ncommit-file web.txt\ncommit-file api.txt\n",
                    f("c-tasks")
                ),
            ),
            ("d", "print idle\n".to_string()),
            // Exits before its verdict.
            ("r1", "sleep 3000\nexit\n".to_string()),
            (
                "r2",
                format!(
                    "sleep 2000\nverdict approved - api and page checked\nwrite {}\n",
                    f("r2-verdict")
                ),
            ),
        ],
    )?;
    println!(
        "exits: {} (a reviewer and a builder exit mid-work; the lead replaces them)",
        bin.display()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let flow = (|| -> Result<(), String> {
        start_burst(&mut s)?;
        std::fs::write(base.join("go"), "go").map_err(|e| e.to_string())?;
        // The new reviewer's start waits for the engineer: approve every waiting start (the
        // held builder's too) in the Requests tab, which opens first while requests wait.
        let r2 = wait_for(&base.join("lead-start-r2"), "{", Duration::from_secs(120));
        if r2.is_empty() {
            return Err("the lead never asked for a new reviewer".into());
        }
        pause(500);
        s.send(b"o");
        pause(800);
        // One press per waiting start; a press with none left does nothing.
        for _ in 0..3 {
            s.send(b"y");
            pause(800);
        }
        s.send(ESC);
        pause(300);
        let ready = wait_for(
            &base.join("lead-ready-status"),
            "\"ready\":true",
            Duration::from_secs(180),
        );
        if !ready.contains("\"ready\":true") {
            return Err(format!("the merge never became ready: {ready}"));
        }
        Ok(())
    })();
    let read = |n: &str| std::fs::read_to_string(base.join(n)).unwrap_or_default();
    let (held, start_r1, rehand, withdraw, start_r2, ready, c_tasks) = (
        read("lead-held"),
        read("lead-start-r1"),
        read("lead-rehand"),
        read("lead-withdraw"),
        read("lead-start-r2"),
        read("lead-ready-status"),
        read("c-tasks"),
    );
    let details = format!(
        "held: {held:?}\nr1: {start_r1:?}\nrehand: {rehand:?}\nwithdraw: {withdraw:?}\nr2: {start_r2:?}\nready: {ready:?}\nc: {c_tasks:?}"
    );
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    println!("evidence: {}", outcome.evidence());
    if let Err(e) = flow {
        problems.push(e);
    }
    if !problems.is_empty() {
        println!("{details}");
        return Err(problems.join("; "));
    }
    // FR-039: the last start is kept for the reviewer; the extra builder waits for the engineer.
    let kept_ok = held.contains("pending")
        && held.contains("kept for reviewer")
        && start_r1.contains("\"start\"")
        && !start_r1.contains("pending");
    // FR-042: the lead was told of #2's task, handed the part again and withdrew the old task.
    let builder_ok = rehand.contains("\"task\"")
        && withdraw.contains("cancelled")
        && c_tasks
            .lines()
            .filter(|l| l.starts_with('T') && l.trim_end().ends_with(" done"))
            .count()
            >= 2;
    // FR-041 and FR-039: the new reviewer's start waited (no starts left), and the merge is
    // ready without the exited reviewer, saying why.
    let reviewer_ok = start_r2.contains("pending") && ready.contains("#4 exited without it");
    let ready_ok = ready.contains("\"ready\":true");
    let pass = kept_ok && builder_ok && reviewer_ok && ready_ok;
    let result = if pass { "PASS" } else { "FAIL" };
    let ok = |b: bool| if b { "ok" } else { "WRONG" };
    println!(
        "os={} kept_start={} builder_exit={} reviewer_exit={} merge_ready={} result={result}",
        std::env::consts::OS,
        ok(kept_ok),
        ok(builder_ok),
        ok(reviewer_ok),
        ok(ready_ok),
    );
    if !pass {
        println!("{details}");
    }
    pty::step_summary(&format!(
        "### Squads, exits ({})\n\n| check | result | expected |\n|---|---|---|\n| the last start kept for the reviewer | {} | extra builder waits, reviewer starts (FR-039) |\n| a builder exits mid-task | {} | lead told, part handed again, task withdrawn (FR-042) |\n| a reviewer exits before its verdict | {} | new reviewer waits for approval; not waited for (FR-041) |\n| the merge | {} | ready |\n\n**{result}**\n",
        std::env::consts::OS,
        ok(kept_ok),
        ok(builder_ok),
        ok(reviewer_ok),
        ok(ready_ok),
    ));
    Ok(if pass { 0 } else { 1 })
}

/// 005 T014: one loop by default. The lead builds the work itself (no manifest), a builder's
/// start is refused with the reason, the lead reports its own piece with evidence, Fallow runs
/// the final gate on the lead's commit, a reviewer approves, and the merge is ready.
fn run_solo_loop(bin: &Path, base: &Path) -> Result<i32, String> {
    let config = base.join("config");
    let project = base.join("project");
    for d in [&config, &project] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    make_project(&project, SQUAD_SOLO)?;
    let f = |name: &str| base.join(name).display().to_string();
    let lead = format!(
        "print lead-ready\nuntil-file {go}\ncall whoami {{}}\nwrite {whoami}\ncall session_start {{\"profile\":\"b\",\"project\":\"project\",\"role\":\"builder\",\"work\":\"feature\"}}\nwrite {builder}\ncommit-file feature.txt\nreport-own feature.txt\nwrite {report}\n{r}write {start_r}\nuntil Every merge condition holds\ncall squad_status {{}}\nwrite {ready}\nprint lead-done\n",
        go = f("go"),
        whoami = f("lead-whoami"),
        builder = f("lead-builder"),
        report = f("lead-report"),
        r = start("r", "reviewer", None),
        start_r = f("lead-start-r"),
        ready = f("lead-ready-status"),
    );
    write_agents(
        base,
        &config,
        &[
            ("lead", lead),
            ("b", "print idle\n".to_string()),
            (
                "r",
                format!(
                    "wait-loop gate-tests-passed 300 {}\nverdict approved - feature checked\nwrite {}\n",
                    f("r-gate"),
                    f("r-verdict")
                ),
            ),
        ],
    )?;
    println!(
        "solo-loop: {} (the lead builds alone; the gate and a reviewer check it)",
        bin.display()
    );
    let cfg = config.display().to_string();
    let proj = project.display().to_string();
    let mut s = Session::spawn(bin, &["--config-dir", &cfg, &proj], Duration::from_secs(1))?;
    let flow = (|| -> Result<(), String> {
        start_burst(&mut s)?;
        std::fs::write(base.join("go"), "go").map_err(|e| e.to_string())?;
        let ready = wait_for(
            &base.join("lead-ready-status"),
            "\"ready\":true",
            Duration::from_secs(240),
        );
        if !ready.contains("\"ready\":true") {
            return Err(format!("the merge never became ready: {ready}"));
        }
        Ok(())
    })();
    let read = |n: &str| std::fs::read_to_string(base.join(n)).unwrap_or_default();
    let (whoami, builder, report, start_r, ready, r_gate) = (
        read("lead-whoami"),
        read("lead-builder"),
        read("lead-report"),
        read("lead-start-r"),
        read("lead-ready-status"),
        read("r-gate"),
    );
    let details = format!(
        "whoami: {whoami:?}\nbuilder: {builder:?}\nreport: {report:?}\nreviewer: {start_r:?}\nready: {ready:?}\nr gate: {r_gate:?}"
    );
    s.quit_with_sessions();
    let outcome = s.finish();
    let mut problems = outcome.problems(None);
    println!("evidence: {}", outcome.evidence());
    if let Err(e) = flow {
        problems.push(e);
    }
    if !problems.is_empty() {
        println!("{details}");
        return Err(problems.join("; "));
    }
    let default_ok = whoami.contains("build each piece of work yourself");
    let refused_ok = builder.contains("not_allowed") && builder.contains("built by the lead");
    let report_ok = report.contains("done");
    let gate_ok = r_gate.contains("status=signalled") && ready.contains("\"class\":\"pass\"");
    let ready_ok = ready.contains("\"ready\":true") && ready.contains("work:done");
    let pass = default_ok && refused_ok && report_ok && gate_ok && ready_ok;
    let result = if pass { "PASS" } else { "FAIL" };
    let ok = |b: bool| if b { "ok" } else { "WRONG" };
    println!(
        "os={} lead_default={} builder_refused={} own_report={} gate={} merge_ready={} result={result}",
        std::env::consts::OS,
        ok(default_ok),
        ok(refused_ok),
        ok(report_ok),
        ok(gate_ok),
        ok(ready_ok),
    );
    if !pass {
        println!("{details}");
    }
    pty::step_summary(&format!(
        "### Squads, solo-loop ({})\n\n| check | result | expected |\n|---|---|---|\n| the lead's briefing | {} | builds each piece itself (FR-001) |\n| a builder without delegated work | {} | refused with the reason (FR-003) |\n| the lead reports its own piece | {} | done with evidence |\n| the final gate on the lead's commit | {} | pass |\n| the merge | {} | ready on work:done and review:approved |\n\n**{result}**\n",
        std::env::consts::OS,
        ok(default_ok),
        ok(refused_ok),
        ok(report_ok),
        ok(gate_ok),
        ok(ready_ok),
    ));
    Ok(if pass { 0 } else { 1 })
}
