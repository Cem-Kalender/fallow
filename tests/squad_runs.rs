//! 005 T015, T018, T022, T024, T026, T039: task sets, runs, cost reports, run records (with the
//! constitution's marker test) and series (US2, FR-011 to FR-019a).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use fallow::app::runs::build_record;
use fallow::orchestration::record::{self, RunRecord};
use fallow::orchestration::runs::{self, End, Measured, Mode, Plan, Run, Series, Snapshot, Stage};
use fallow::orchestration::usage;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-runs-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const SET: &str = r#"
setup = [{ from = "setup/local.json", to = ".claude/settings.local.json" }]

[[task]]
name = "cart-add"
project = "C:/Repos/shop"
start = "4f2a9c1"
description = "cart-add/description.md"
several_components = true
time_limit_min = 90
hidden = [{ from = "cart-add/hidden/t.rs", to = "tests/t.rs" }]
tests = [
  { command = "cargo test --test t add", windows = "cargo test --test t add" },
  { command = "cargo test --test t zero" },
]
"#;

fn set_dir(name: &str) -> PathBuf {
    let d = tmp(name);
    std::fs::create_dir_all(d.join("setup")).unwrap();
    std::fs::create_dir_all(d.join("cart-add/hidden")).unwrap();
    std::fs::write(d.join("setup/local.json"), "{}").unwrap();
    std::fs::write(
        d.join("cart-add/description.md"),
        "Add items to the cart.\n",
    )
    .unwrap();
    std::fs::write(d.join("cart-add/hidden/t.rs"), "#[test] fn add() {}\n").unwrap();
    std::fs::write(d.join("taskset.toml"), SET).unwrap();
    d
}

#[test]
fn a_task_set_is_read_and_checked() {
    // T015, contracts/task-set.md.
    let d = set_dir("read");
    let s = runs::load(&d).unwrap();
    assert_eq!(s.setup[0].to, ".claude/settings.local.json");
    let t = s.task("cart-add").unwrap();
    assert_eq!(t.start, "4f2a9c1");
    assert!(t.several_components);
    assert_eq!(t.time_limit, Duration::from_secs(90 * 60));
    assert_eq!(t.description, "Add items to the cart.\n");
    assert_eq!(t.hidden[0].to, "tests/t.rs");
    assert_eq!(t.tests.len(), 2);
    assert_eq!(t.tests[1].command_for("linux"), "cargo test --test t zero");
    // Problems name the file and line.
    let bad = |text: &str| {
        runs::parse(&d, "taskset.toml", text)
            .unwrap_err()
            .join("\n")
    };
    let e =
        bad("[[task]]\nname = \"x\"\nproject = \"p\"\nstart = \"s\"\ndescription = \"nope.md\"\n");
    assert!(
        e.contains("taskset.toml:1:") && e.contains("nope.md"),
        "{e}"
    );
    assert!(e.contains("at least one hidden test"), "{e}");
    let e = bad(
        "[[task]]\nname = \"x\"\nproject = \"p\"\nstart = \"s\"\ndescription = \"cart-add/description.md\"\nhidden = [{ from = \"../secret\", to = \"t\" }]\ntests = [{ command = \"true\" }]\n",
    );
    assert!(e.contains("not a file in the task set"), "{e}");
    let e = bad(
        "[[task]]\nname = \"x\"\nproject = \"p\"\nstart = \"s\"\ndescription = \"cart-add/description.md\"\nhidden = [{ from = \"cart-add/hidden/t.rs\", to = \"../outside\" }]\ntests = [{ command = \"true\" }]\n",
    );
    assert!(e.contains("without .."), "{e}");
    let e = bad(
        "[[task]]\nname = \"x\"\nproject = \"p\"\nstart = \"s\"\ndescription = \"cart-add/description.md\"\ntime_limit_min = 0\ntests = [{ command = \"true\" }]\n",
    );
    assert!(e.contains("1..=480"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn fingerprints_follow_the_text_not_the_line_endings() {
    // R-12, FR-016, SC-009.
    let d = set_dir("fp");
    let t = runs::load(&d).unwrap().tasks.remove(0);
    let a = runs::task_fingerprint(&t).unwrap();
    let mut crlf = t.clone();
    crlf.description = "Add items to the cart.\r\n".into();
    assert_eq!(runs::task_fingerprint(&crlf).unwrap(), a);
    let h = runs::hidden_fingerprint(&t).unwrap();
    std::fs::write(
        d.join("cart-add/hidden/t.rs"),
        "#[test] fn add() { panic!() }\n",
    )
    .unwrap();
    assert_ne!(
        runs::hidden_fingerprint(&t).unwrap(),
        h,
        "content changes it"
    );
    let mut other = t.clone();
    other.tests[0].command = "cargo test".into();
    assert_ne!(
        runs::hidden_fingerprint(&other).unwrap(),
        runs::hidden_fingerprint(&t).unwrap()
    );
    assert!(!a.contains("cart") && a.len() == 40);
    let _ = std::fs::remove_dir_all(&d);
}

fn run(task: runs::Task) -> Run {
    let now = Instant::now();
    Run {
        id: 1,
        plan: Plan {
            set: PathBuf::from("set"),
            task: task.name.clone(),
            mode: Mode::Squad,
            off: BTreeSet::new(),
            record: true,
            agent: "claude".into(),
        },
        task,
        task_fp: "a".repeat(40),
        hidden_fp: "b".repeat(40),
        copy: PathBuf::from("copy"),
        root: PathBuf::from("root"),
        project: 9,
        start_commit: "c".repeat(40),
        first: Some(1),
        sessions: [1u32, 2, 3].into(),
        started_at: now,
        started: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        ended_at: Some(now + Duration::from_secs(600)),
        stage: Stage::Running,
        typed: true,
        idle_since: None,
        idle_end: Duration::from_secs(180),
        measured: Default::default(),
        last_seen: None,
        final_commit: Some("d".repeat(40)),
        tested_dir: None,
        snapshot: None,
    }
}

fn task() -> runs::Task {
    runs::Task {
        name: "cart-add".into(),
        project: PathBuf::from("p"),
        start: "s".into(),
        description: "Add items.".into(),
        several_components: false,
        time_limit: Duration::from_secs(3600),
        hidden: Vec::new(),
        tests: vec![runs::HiddenTest {
            command: "true".into(),
            windows: None,
            macos: None,
            linux: None,
        }],
    }
}

#[test]
fn a_run_ends_when_nothing_works_and_nothing_is_in_progress() {
    // T018, FR-013, R-13.
    let mut r = run(task());
    let t0 = r.started_at;
    r.observe_idle(t0, false, false);
    assert_eq!(r.idle_since, Some(t0));
    // Not yet: idle for less than idle_end.
    assert_eq!(r.due_end(t0 + Duration::from_secs(60), false, false), None);
    // Not while something is in progress (a gate, a start waiting for memory, a hand-over).
    assert_eq!(r.due_end(t0 + Duration::from_secs(200), false, true), None);
    r.observe_idle(t0 + Duration::from_secs(100), false, true);
    assert_eq!(r.idle_since, None, "in progress resets the idle clock");
    r.observe_idle(t0 + Duration::from_secs(110), false, false);
    assert_eq!(r.due_end(t0 + Duration::from_secs(250), false, false), None);
    assert_eq!(
        r.due_end(t0 + Duration::from_secs(300), false, false),
        Some(End::Idle)
    );
    // The time limit ends it whatever happens.
    assert_eq!(
        r.due_end(t0 + Duration::from_secs(3600), true, true),
        Some(End::TimeLimit)
    );
    // Nothing is idle before the description was typed.
    let mut fresh = run(task());
    fresh.typed = false;
    fresh.observe_idle(t0, false, false);
    assert_eq!(fresh.idle_since, None);
    // The description ends with the same closing line in every mode.
    assert!(r.description().ends_with(runs::CLOSING_LINE));
}

#[test]
fn a_series_runs_every_task_mode_and_repeat_in_a_shuffled_order() {
    // T026, FR-018.
    let tasks = vec!["a".to_string(), "b".to_string()];
    let s = Series::new(
        Path::new("set"),
        &tasks,
        &Mode::ALL,
        3,
        &BTreeSet::new(),
        42,
    );
    assert_eq!(s.queue.len(), 18);
    for t in &tasks {
        for m in Mode::ALL {
            assert_eq!(
                s.queue
                    .iter()
                    .filter(|p| &p.task == t && p.mode == m)
                    .count(),
                3
            );
        }
    }
    let ordered: Vec<(String, Mode)> = s.queue.iter().map(|p| (p.task.clone(), p.mode)).collect();
    let mut sorted = ordered.clone();
    sorted.sort();
    assert_ne!(ordered, sorted, "shuffled");
    let again = Series::new(
        Path::new("set"),
        &tasks,
        &Mode::ALL,
        3,
        &BTreeSet::new(),
        42,
    );
    assert_eq!(
        again
            .queue
            .iter()
            .map(|p| (p.task.clone(), p.mode))
            .collect::<Vec<_>>(),
        ordered,
        "the same seed, the same order"
    );
    let mut s = s;
    let first = s.queue[0].clone();
    assert_eq!(s.take_next(), Some(first));
    assert_eq!((s.done, s.queue.len()), (1, 17));
}

#[test]
fn the_agent_reports_its_cost_on_its_status_line() {
    // T022, R-10, FR-015.
    let m = usage::marker(
        r#"{"cost":{"total_cost_usd":1.23456},"model":{"id":"claude-opus-5-5","display_name":"Opus"},"version":"2.1.330","transcript_path":"/home/me/secret.jsonl"}"#,
    );
    assert_eq!(
        m,
        "fallow-usage cost=1.2346 model=claude-opus-5-5 version=2.1.330"
    );
    assert!(!m.contains("secret"), "only cost, model and version");
    let r = usage::read(&[
        "something".to_string(),
        format!("  {m}  "),
        "> ".to_string(),
    ])
    .unwrap();
    assert_eq!(r.cost_usd, Some(1.2346));
    assert_eq!(r.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(r.version.as_deref(), Some("2.1.330"));
    // Unknown stays unknown; an alias or text is never taken as a model.
    let u = usage::marker(r#"{"model":{"id":"my private alias"}}"#);
    assert_eq!(u, "fallow-usage cost=- model=- version=-");
    let r = usage::read(&[u]).unwrap();
    assert_eq!((r.cost_usd, r.model), (None, None));
    assert!(usage::read(&["no marker here".to_string()]).is_none());
    // The status line: forward slashes, quoted, before the list flags; none with the
    // profile's own --settings.
    let a = usage::status_line_args(Path::new(r"C:\Program Files\Fallow\fallow.exe"), &[]).unwrap();
    assert_eq!(a[0], "--settings");
    let v: serde_json::Value = serde_json::from_str(&a[1]).unwrap();
    assert_eq!(
        v["statusLine"]["command"],
        "\"C:/Program Files/Fallow/fallow.exe\" usage-report"
    );
    assert_eq!(v["statusLine"]["type"], "command");
    assert!(
        usage::status_line_args(
            Path::new("/usr/bin/fallow"),
            &["--settings".into(), "{}".into()]
        )
        .is_none()
    );
}

fn record_of(r: &Run, passed: u32, failed: u32) -> RunRecord {
    build_record(
        r,
        &fallow::session::SessionManager::default(),
        passed,
        failed,
    )
}

#[test]
fn records_hold_only_what_the_constitution_allows() {
    // T024, contracts/run-record.md, constitution VII (v5.0.0): the CI marker test.
    const MARK: &str = "ZZMARKERZZ";
    let mut t = task();
    t.name = format!("task-{MARK}");
    t.description = format!("Build {MARK} please");
    t.tests[0].command = format!("cargo test {MARK}");
    t.hidden.push(runs::Copy {
        from: PathBuf::from(format!("/home/{MARK}/hidden.rs")),
        to: format!("tests/{MARK}.rs"),
    });
    let mut r = run(t);
    r.copy = PathBuf::from(format!("/cache/{MARK}"));
    r.measured.insert(
        1,
        Measured {
            working: Duration::from_secs(300),
            blocked: Duration::from_secs(20),
            interventions: 2,
            cost_usd: Some(1.5),
            // A custom alias the agent reported as its model: dropped.
            model: record::model_id(&format!("{MARK}-alias")),
            version: record::version_id(MARK),
        },
    );
    r.measured.insert(
        2,
        Measured {
            working: Duration::from_secs(200),
            cost_usd: Some(0.5),
            model: record::model_id("claude-opus-5-5"),
            version: record::version_id("2.1.330"),
            ..Measured::default()
        },
    );
    r.snapshot = Some(Snapshot {
        roles: vec![
            (1, "lead".into()),
            (2, format!("builder-{MARK}")),
            (3, format!("reviewer-{MARK}")),
        ],
        parts: vec![(2, Some(format!("part-{MARK}")))],
        gates: vec!["pass".into()],
        reviews: vec!["approved".into()],
        repairs: vec![(Some(format!("part-{MARK}")), 1)],
        signals: vec![(2, 3)],
    });
    let rec = record_of(&r, 2, 1);
    rec.check().unwrap();
    let text = serde_json::to_string_pretty(&rec).unwrap();
    assert!(!text.contains(MARK), "{text}");
    assert!(!text.contains("cart") && !text.contains("/cache") && !text.contains("tests/"));
    // What it does hold.
    assert_eq!(rec.mode, "squad");
    assert_eq!(rec.sessions[0].n, 1);
    assert_eq!(rec.sessions[0].role, 1, "the first session is role 1");
    assert_eq!(rec.sessions[0].model, None);
    assert_eq!(rec.sessions[1].model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(rec.parts.len(), 1);
    assert_eq!((rec.parts[0].signals, rec.parts[0].repair_rounds), (3, 1));
    assert_eq!(rec.parts[0].working_s, 200, "the part's builder");
    assert_eq!((rec.hidden.passed, rec.hidden.failed), (2, 1));
    assert_eq!(rec.duration_s, 600);
    assert_eq!(rec.started, "2027-01-15T08:00:00Z");
    // Free text where a fixed value belongs is refused before writing.
    let mut bad = rec.clone();
    bad.parts[0].gates = vec![MARK.to_string()];
    assert!(bad.check().is_err());
    let mut bad = rec.clone();
    bad.sessions[0].agent = MARK.into();
    assert!(bad.check().is_err());
    let mut bad = rec.clone();
    bad.mode = MARK.into();
    assert!(bad.check().is_err());
}

#[test]
fn records_are_written_listed_compared_and_deleted() {
    // T024, T027, FR-016, FR-019.
    let cache = tmp("records");
    let mut a = record_of(&run(task()), 2, 0);
    a.per_role.iter_mut().for_each(|x| x.cost_usd = Some(1.0));
    let mut b = a.clone();
    b.duration_s = 900;
    b.hidden = record::Hidden {
        passed: 1,
        failed: 1,
    };
    let mut plain = a.clone();
    plain.mode = "plain".into();
    let mut unknown = a.clone();
    unknown.per_role[0].cost_usd = None;
    for r in [&a, &b, &plain] {
        record::write(&cache, r).unwrap();
    }
    let n = record::write(&cache, &unknown).unwrap();
    assert_eq!(n, 4);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(record::dir(&cache).join("1.json")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
    }
    let all = record::list(&cache);
    assert_eq!(
        all.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let groups = record::groups(&all);
    assert_eq!(groups.len(), 2);
    let squad = groups.iter().find(|g| g.mode == "squad").unwrap();
    assert_eq!((squad.runs, squad.passing), (3, 2));
    assert_eq!((squad.min_s, squad.max_s), (600, 900));
    assert_eq!(squad.cost_per_pass, None, "one run's cost is unknown");
    let plain = groups.iter().find(|g| g.mode == "plain").unwrap();
    assert!(plain.cost_per_pass.is_some());
    // Different settings are a different group.
    let mut other = a.clone();
    other.time_limit_s = 60;
    record::write(&cache, &other).unwrap();
    assert_eq!(record::groups(&record::list(&cache)).len(), 3);
    record::delete(&cache, 2).unwrap();
    assert_eq!(record::list(&cache).len(), 4);
    let (count, size) = record::usage(&cache);
    assert_eq!(count, 4);
    assert!(size > 0);
    let _ = std::fs::remove_dir_all(&cache);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A shell that stays running and echoes what is typed (the agent's stand-in): extra flags are
/// ignored.
fn shell_profile() -> fallow::session::profiles::AgentProfile {
    let (command, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/k", "echo", "ready"])
    } else {
        ("/bin/sh", vec!["-c", "echo ready; exec cat", "sh"])
    };
    fallow::session::profiles::AgentProfile {
        id: "agent".into(),
        name: "Agent".into(),
        command: command.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: Vec::new(),
        builtin: false,
        orchestration: fallow::session::profiles::Style::Claude,
    }
}

#[test]
fn a_plain_run_goes_from_its_copy_to_its_record() {
    // T018, T020, T025: the whole lifecycle in the app, with a shell as the agent.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use fallow::app::state::AppState;
    let base = tmp("lifecycle");
    let cache = base.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    // Records and copies go to this test's cache, never the user's.
    unsafe {
        std::env::set_var(
            if cfg!(windows) {
                "LOCALAPPDATA"
            } else {
                "XDG_CACHE_HOME"
            },
            &cache,
        );
    }
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["config", "user.email", "t@example.invalid"]);
    git(&project, &["config", "user.name", "t"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    std::fs::write(project.join("a.txt"), "a\n").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-q", "-m", "start"]);
    let start = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["branch", "fallow/earlier-run"]);
    let refs = git(&project, &["for-each-ref"]);
    let set = base.join("set");
    std::fs::create_dir_all(set.join("t/hidden")).unwrap();
    std::fs::write(set.join("t/description.md"), "zz-desc: do the thing\n").unwrap();
    std::fs::write(set.join("t/hidden/h.txt"), "h\n").unwrap();
    let (pass, fail) = if cfg!(windows) {
        ("if (Test-Path h.txt) { exit 0 } else { exit 1 }", "exit 3")
    } else {
        ("test -f h.txt", "exit 3")
    };
    std::fs::write(
        set.join("taskset.toml"),
        format!(
            "[[task]]\nname = \"t\"\nproject = '{}'\nstart = \"{start}\"\ndescription = \"t/description.md\"\nhidden = [{{ from = \"t/hidden/h.txt\", to = \"h.txt\" }}]\ntests = [{{ command = \"{pass}\" }}, {{ command = \"{fail}\" }}]\n",
            project.display()
        ),
    )
    .unwrap();

    let mut s = AppState::new(None, Vec::new());
    let (tx, rx) = std::sync::mpsc::channel();
    s.events = Some(tx);
    s.profiles = vec![shell_profile()];
    s.register_task_set(&set).unwrap();
    let plan = fallow::app::runs::parse_run_line("t plain agent=agent", s.task_sets()).unwrap();
    s.start_run(plan).unwrap();
    {
        let r = s.runs.run.as_mut().unwrap();
        r.idle_end = Duration::from_secs(2);
        assert!(r.copy.join("a.txt").exists(), "the copy has the project");
        assert_eq!(git(&r.copy, &["rev-parse", "HEAD"]), start);
        let log = git(&r.copy, &["log", "--all", "--oneline"]);
        assert_eq!(log.lines().count(), 1, "only the start's history: {log}");
    }
    let first = s.runs.run.as_ref().unwrap().first.unwrap();
    let cmd = s.sessions.get(first).unwrap().command_line.clone().unwrap();
    assert!(
        cmd.contains("dontAsk") && cmd.contains("AskUserQuestion"),
        "{cmd}"
    );
    assert!(
        cmd.contains("statusLine") && !cmd.contains("--mcp-config"),
        "{cmd}"
    );
    // What the event loop does for sessions.
    let pump = |s: &mut AppState| {
        while let Ok(ev) = rx.try_recv() {
            match ev {
                fallow::app::events::AppEvent::SessionExited(id, code) => {
                    s.session_exited(id, code);
                }
                fallow::app::events::AppEvent::SessionOutput(id) => {
                    s.session_output(id);
                }
                _ => {}
            }
        }
        s.sessions.rearm();
        s.tick_sessions();
        // The process monitor's quiet CPU makes a quiet session idle in the app; here a shell
        // quiet for a second counts as idle.
        for x in &mut s.sessions.sessions {
            if x.state == fallow::session::SessionState::Working
                && x.last_output.elapsed() > Duration::from_secs(1)
            {
                x.state = fallow::session::SessionState::NeedsAttention;
            }
        }
        s.tick_runs();
    };
    let until = Instant::now() + Duration::from_secs(90);
    let mut confirmed = false;
    loop {
        pump(&mut s);
        let stage = s.runs.run.as_ref().unwrap().stage.clone();
        if matches!(stage, Stage::Recorded) {
            break;
        }
        if matches!(stage, Stage::Ended(_))
            && matches!(s.overlay, fallow::app::state::Overlay::Confirm { .. })
            && !confirmed
        {
            // The one confirmation names the run's session.
            s.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
            confirmed = true;
        }
        assert!(Instant::now() < until, "stuck at {stage:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(confirmed, "the engineer confirmed closing the sessions");
    let typed = s.sessions.get(first).map(|_| ());
    let _ = typed;
    let root = fallow::config::cache_root().unwrap();
    let recs = record::list(&root);
    assert_eq!(recs.len(), 1, "{recs:?}");
    let r = &recs[0].1;
    assert_eq!((r.hidden.passed, r.hidden.failed), (1, 1));
    assert_eq!(r.mode, "plain");
    assert_eq!(r.start_commit, start);
    let text = std::fs::read_to_string(record::dir(&root).join("1.json")).unwrap();
    assert!(
        !text.contains("zz-desc") && !text.contains("\"t\""),
        "{text}"
    );
    // The engineer's repository is untouched and the copy is gone.
    assert_eq!(git(&project, &["for-each-ref"]), refs);
    assert!(!s.runs.run.as_ref().unwrap().root.exists());
    assert!(s.projects.projects.iter().all(|p| !p.temporary));
    let _ = std::fs::remove_dir_all(&base);
}
