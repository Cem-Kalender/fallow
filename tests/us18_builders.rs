//! 018 US3 (T024-T030; FR-011 to FR-020, FR-023, FR-024; research R-4 to R-7): builders fan out
//! over the `[P]` tasks of a phase. Each task of a group is built in its own working copy and
//! session, gated on its own commit, and merged into the build branch in `tasks.md` order after
//! the touched-files check; an overlap is repaired on top of the build branch by the task's own
//! builder; the merged group is gated before the task after it starts. One builder, or a phase
//! without `[P]` tasks, runs as 0.4.0 did.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;
use std::path::Path;
use std::time::{Duration, Instant};

/// `pipelines/builders.toml` (Standard with `builders` builders) and settings.
fn setup(root: &Path, builders: u32, settings: Option<&str>) {
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("builders.toml"), pipeline_with(builders, "")).unwrap();
    if let Some(s) = settings {
        std::fs::write(root.join("settings.extra.toml"), s).unwrap();
    }
}

/// Starts the engine on `steps` (after the lead's turns) and locks the feature with the
/// `builders` pipeline and a budget.
fn lock(
    root: &Path,
    f: &ParallelFeature,
    steps: &str,
    budget: (f64, u64),
) -> (fallow::service::ToolEngine, u64) {
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(root, &scenario, None);
    let id = lock_calc_in(&e.tx, &f.repo, Some("builders"), Some(budget), None);
    (e, id)
}

fn events(root: &Path, id: u64) -> Vec<Value> {
    std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("events.jsonl"),
    )
    .unwrap()
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect()
}

fn of_type<'a>(ev: &'a [Value], t: &str) -> Vec<&'a Value> {
    ev.iter().filter(|e| e["type"] == t).collect()
}

/// The steps in the order they were queued.
fn in_order(v: &Value) -> Vec<&Value> {
    let mut s: Vec<&Value> = v["steps"].as_object().unwrap().values().collect();
    s.sort_by_key(|x| x["n"].as_u64());
    s
}

fn of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    in_order(v)
        .into_iter()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect()
}

fn text(s: &Value, k: &str) -> String {
    s[k].as_str().unwrap_or_default().to_string()
}

fn prompt(root: &Path, id: u64, s: &Value) -> String {
    std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", s["n"])),
    )
    .unwrap()
}

/// The seq of the event that queued step `n`.
fn queued_seq(ev: &[Value], n: u64) -> u64 {
    ev.iter()
        .find(|e| e["type"] == "step_queued" && e["n"].as_u64() == Some(n))
        .and_then(|e| e["seq"].as_u64())
        .unwrap()
}

/// The most build steps of `tasks` running at once, from the log.
fn most_at_once(ev: &[Value], tasks: &[String]) -> usize {
    let mut builds = std::collections::BTreeSet::new();
    let (mut now, mut most) = (0usize, 0usize);
    for e in ev {
        let n = e["n"].as_u64();
        match e["type"].as_str() {
            Some("step_queued")
                if e["kind"] == "build"
                    && tasks.iter().any(|t| e["task"].as_str() == Some(t.as_str())) =>
            {
                builds.insert(n.unwrap());
            }
            Some("step_started") if n.is_some_and(|n| builds.contains(&n)) => {
                now += 1;
                most = most.max(now);
            }
            Some("step_finished") if n.is_some_and(|n| builds.contains(&n)) => now -= 1,
            _ => {}
        }
    }
    most
}

fn branches(repo: &Path) -> Vec<String> {
    git(repo, &["branch", "--list", "--format=%(refname:short)"])
        .lines()
        .map(str::to_string)
        .collect()
}

/// The scripted steps of a whole loop over the feature: phase 1, the group's builds (`extra`
/// each), the plain task, the handover and the checkpoint.
fn whole(f: &ParallelFeature, extra: &str) -> String {
    let mut s = f.phase_1();
    for t in &f.parallel {
        s.push_str(&f.build(t, extra));
    }
    s.push_str(&f.build(&f.last, ""));
    s.push_str(&handover_step(2, ""));
    s.push_str(&check_step("US1", true, ""));
    s
}

/// From the first group task's build starting to the plain task after the group being queued.
fn group_time(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, id: u64) -> Duration {
    wait_for(tx, id, "T004 building", |v| {
        of(v, "build", "T004")
            .first()
            .is_some_and(|s| !s["started"].is_null())
    });
    let from = Instant::now();
    wait_for(tx, id, "T008 queued", |v| {
        !of(v, "build", "T008").is_empty()
    });
    from.elapsed()
}

/// A loop over the feature with `builders` builders, each build taking 5 s; returns the engine,
/// the loop and the group's time (`group_time`).
fn timed(
    name: &'static str,
    builders: u32,
) -> (
    std::path::PathBuf,
    ParallelFeature,
    fallow::service::ToolEngine,
    u64,
    Duration,
) {
    let root = scratch(name);
    setup(&root, builders, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    let (e, id) = lock(&root, &f, &whole(&f, "sleep_ms = 5000"), (5.0, 60));
    let took = group_time(&e.tx, id);
    (root, f, e, id, took)
}

#[test]
fn three_builders_run_four_parallel_tasks_and_merge_them_in_order() {
    // SC-002: the same loop with one builder runs at the same time, under the same load.
    let serial = std::thread::spawn(|| timed("builders-four-one", 1));
    let (root, f, e, id, took) = timed("builders-four", 3);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    // One group, of the four [P] tasks.
    let started = of_type(&ev, "group_started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["tasks"],
        serde_json::json!(["T004", "T005", "T006", "T007"])
    );
    // Three builds at once, never more (AS1).
    assert_eq!(most_at_once(&ev, &f.parallel), 3);
    // Each in its own working copy and session; the fourth started when one was accepted.
    let mut sessions = Vec::new();
    for t in &f.parallel {
        let b = of(&v, "build", t);
        assert_eq!(b.len(), 1, "{t}: {b:?}");
        let wt = text(b[0], "worktree").replace('\\', "/");
        assert!(wt.ends_with(&format!("/task-{t}")), "{t} in {wt}");
        assert!(b[0]["resume"].is_null(), "{t} starts a session of its own");
        sessions.push(text(b[0], "session"));
        // Its builder is told the others are built alongside, and to do only its own.
        let p = prompt(&root, id, b[0]);
        assert!(
            p.contains("built at the same time in other working copies")
                && p.contains(&format!("do only {t}")),
            "{p}"
        );
    }
    sessions.sort();
    sessions.dedup();
    assert_eq!(sessions.len(), 4, "a session per task");
    let t7 = of(&v, "build", "T007")[0];
    let accepted_first = of_type(&ev, "task")
        .into_iter()
        .find(|e| e["state"] == "accepted" && f.parallel.iter().any(|t| e["id"] == *t))
        .unwrap();
    assert!(
        queued_seq(&ev, t7["n"].as_u64().unwrap()) > accepted_first["seq"].as_u64().unwrap(),
        "the fourth waits for a place"
    );
    // Merged in tasks.md order (AS3), each ticked on the build branch.
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006", "T007"]);
    let build_branch = text(&v["feature_loop"], "build_branch");
    let tasks_md = git(
        &f.repo,
        &["show", &format!("{build_branch}:specs/001-calc/tasks.md")],
    );
    for t in ["T004", "T005", "T006", "T007", "T008"] {
        assert!(tasks_md.contains(&format!("- [X] {t} ")), "{t}: {tasks_md}");
    }
    for t in &f.parallel {
        let file = format!("{build_branch}:calc/{}.py", t.to_lowercase());
        assert!(git(&f.repo, &["show", &file]).contains(t.as_str()));
    }
    // The merged group is gated on the merged head before T008 is queued (AS7, FR-016).
    let last_merge = of_type(&ev, "task_merged").last().unwrap()["commit"].clone();
    let group_gates: Vec<&Value> = of_type(&ev, "gate_run")
        .into_iter()
        .filter(|g| g["part"] == "group-T004-T007")
        .collect();
    assert!(!group_gates.is_empty(), "the group's gates ran");
    assert!(
        group_gates
            .iter()
            .all(|g| g["result"]["commit"] == last_merge)
    );
    let t8 = of(&v, "build", "T008")[0];
    let t8_queued = queued_seq(&ev, t8["n"].as_u64().unwrap());
    assert!(
        group_gates
            .iter()
            .all(|g| g["seq"].as_u64().unwrap() < t8_queued)
    );
    let done = of_type(&ev, "group_done");
    assert_eq!(done.len(), 1);
    assert!(done[0]["seq"].as_u64().unwrap() < t8_queued);
    // T008 in the build worktree and the phase's session: the handover resumes it.
    assert!(text(t8, "worktree").replace('\\', "/").ends_with("/build"));
    assert!(
        t8["resume"].is_null(),
        "the phase had no session of its own yet"
    );
    let h = of(&v, "handover", "phase-2")[0];
    assert_eq!(
        h["resume"], t8["session"],
        "the handover in the phase's session"
    );
    // The handover's instruction names the tasks built alongside, with their merge commits
    // (FR-016a).
    let hp = prompt(&root, id, h);
    for m in of_type(&ev, "task_merged") {
        let c = m["commit"].as_str().unwrap();
        assert!(
            hp.contains(m["task"].as_str().unwrap()) && hp.contains(&c[..7]),
            "{hp}"
        );
    }
    // No working copy or branch of the group is left (FR-017).
    let wt = root.join("data/runs").join(id.to_string()).join("wt");
    for t in &f.parallel {
        assert!(!wt.join(format!("task-{t}")).exists(), "{t}'s working copy");
    }
    let left: Vec<String> = branches(&f.repo)
        .into_iter()
        .filter(|b| f.parallel.iter().any(|t| b.ends_with(&format!("-{t}"))))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let (root1, _f1, e1, _id1, one) = serial.join().unwrap();
    // SC-002: with 3 builders the group takes at most 60 % of its time with one.
    assert!(
        took.as_secs_f64() <= 0.6 * one.as_secs_f64(),
        "3 builders: {took:?}; 1 builder: {one:?}"
    );
    drop(e1);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&root1);
}

#[test]
fn the_setting_for_agents_at_once_limits_the_builders() {
    let root = scratch("builders-slots");
    setup(&root, 3, Some("[defaults]\nmax_parallel = 2\n"));
    let f = parallel_tasks_feature(&root, 4, &[]);
    let (e, id) = lock(&root, &f, &whole(&f, "sleep_ms = 1500"), (5.0, 60));
    wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    assert_eq!(of_type(&ev, "group_started").len(), 1);
    assert_eq!(
        most_at_once(&ev, &f.parallel),
        2,
        "never more than the setting"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

fn repair(part: &str, files: &[(&str, &str)]) -> String {
    task_step(part, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
}

#[test]
fn an_overlap_is_repaired_on_top_and_reported() {
    let root = scratch("builders-overlap");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(
        &root,
        4,
        &[("T005", "calc/shared.py"), ("T007", "calc/shared.py")],
    );
    let mut steps = f.phase_1();
    for t in &f.parallel {
        // T007 finishes last, after T005 is merged.
        let extra = if t == "T007" { "sleep_ms = 1500" } else { "" };
        steps.push_str(&f.build(t, extra));
    }
    // A repair that leaves a marker is refused; the next one is clean.
    steps.push_str(&repair(
        "T007",
        &[(
            "calc/shared.py",
            "<<<<<<< T007\nBY = \"T007\"\n=======\nBY = \"T005\"\n>>>>>>> T005\n# half done\n",
        )],
    ));
    steps.push_str(&repair(
        "T007",
        &[("calc/shared.py", "BY = \"T005\"\nALSO = \"T007\"\n")],
    ));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    // T007 was held back, with the file and the task that changed it.
    let o = of_type(&ev, "task_overlap");
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(o[0]["task"], "T007");
    assert_eq!(o[0]["files"], serde_json::json!(["calc/shared.py"]));
    assert_eq!(o[0]["with"], serde_json::json!(["T005"]));
    // Must fail: T007's first commit is never merged as it was. Where it was held back, the
    // build branch did not hold it; no merge on the build branch takes it in directly.
    let first = text(of(&v, "build", "T007")[0], "verified_commit");
    let working = o[0]["working"].as_str().unwrap();
    let parents: Vec<String> = git(&f.repo, &["rev-list", "--parents", "-n", "1", working])
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect();
    assert_eq!(
        parents[0], first,
        "the working commit builds on T007's work"
    );
    let held_at = &parents[1];
    assert!(
        std::process::Command::new("git")
            .current_dir(&f.repo)
            .args(["merge-base", "--is-ancestor", &first, held_at])
            .status()
            .unwrap()
            .code()
            == Some(1),
        "the build branch did not hold T007 when it was held back"
    );
    let build_branch = text(&v["feature_loop"], "build_branch");
    for line in git(
        &f.repo,
        &[
            "rev-list",
            "--first-parent",
            "--parents",
            &format!("{}..{build_branch}", held_at),
        ],
    )
    .lines()
    {
        let ps: Vec<&str> = line.split_whitespace().collect();
        assert!(
            ps.get(2) != Some(&first.as_str()),
            "merged as it was: {line}"
        );
    }
    // The repair: T007's builder, in T007's session, on the working commit, naming the file,
    // T005 and both sides' changes.
    let r = of(&v, "repair", "T007");
    assert_eq!(r.len(), 2, "{r:?}");
    let b7 = of(&v, "build", "T007")[0];
    assert_eq!(r[0]["task"], "T007");
    assert_eq!(r[0]["resume"], b7["session"], "in T007's session");
    assert_eq!(r[0]["base"].as_str(), Some(working));
    assert!(
        text(r[0], "worktree")
            .replace('\\', "/")
            .ends_with("/task-T007")
    );
    let p = prompt(&root, id, r[0]);
    assert!(
        p.contains("calc/shared.py")
            && p.contains("T005")
            && p.contains("did T005")
            && p.contains("did T007")
            && p.contains("both tasks' work must keep working"),
        "{p}"
    );
    // The repair that left a marker was refused and counted.
    let why = r[0]["verified"].to_string();
    assert!(
        r[0]["verified"][0] == false
            && why.contains("calc/shared.py still holds a conflict marker"),
        "{why}"
    );
    assert_eq!(r[1]["resume"], b7["session"]);
    assert_eq!(v["feature_loop"]["tasks"]["T007"]["repairs"], 2);
    // The clean repair was gated and merged.
    let clean = text(r[1], "verified_commit");
    assert!(
        v["gate_runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["part"] == "T007" && g["result"]["commit"] == clean)
    );
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006", "T007"]);
    let shared = git(
        &f.repo,
        &["show", &format!("{build_branch}:calc/shared.py")],
    );
    assert!(shared.contains("ALSO = \"T007\"") && !shared.contains("<<<<<<<"));
    // The loop's view and history say which tasks changed which files (FR-024).
    let lv = ask(tx, Request::Loop { id }).unwrap();
    let t7 = lv["groups"][0]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "T007")
        .unwrap()
        .clone();
    assert_eq!(
        t7["overlaps"],
        serde_json::json!([{ "files": ["calc/shared.py"], "with": ["T005"] }])
    );
    assert_eq!(t7["state"], "merged");
    assert_eq!(t7["worktree"], "task-T007");
    let feed = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: fallow::run::activity::Filter::default(),
        },
    )
    .unwrap()
    .to_string();
    assert!(
        feed.contains("T007 changed calc/shared.py, which T005 changed: repaired on top of T005"),
        "{feed}"
    );
    assert!(feed.contains("T007 merged into the build branch"), "{feed}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_merged_group_that_fails_the_gates_is_repaired_before_the_next_task() {
    let root = scratch("builders-group-gate");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    // A gate that fails only on the merged result: T004's and T005's files together, until a
    // repair adds calc/fixed.py.
    std::fs::write(
        f.repo.join(".fallow/squad.toml"),
        format!(
            "repair_cap = 2\n[[gate]]\nname = \"tests\"\ncommand = \"{PYTHON} -m unittest\"\nbaseline = true\n\n[[gate]]\nname = \"pair\"\ncommand = \"{PYTHON} -c \\\"import os,sys; print('Ran 1 test'); both = os.path.exists('calc/t004.py') and os.path.exists('calc/t005.py'); sys.exit(1 if both and not os.path.exists('calc/fixed.py') else 0)\\\"\"\nbaseline = true\n"
        ),
    )
    .unwrap();
    let mut steps = f.phase_1();
    for t in &f.parallel {
        steps.push_str(&f.build(t, ""));
    }
    steps.push_str(&repair(
        "group-T004-T007",
        &[("calc/fixed.py", "FIXED = True\n")],
    ));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    // Each task passed its own gates.
    let gates = of_type(&ev, "gate_run");
    for t in &f.parallel {
        let own: Vec<&&Value> = gates.iter().filter(|g| g["part"] == t.as_str()).collect();
        assert!(!own.is_empty(), "{t} gated");
        assert!(own.iter().all(|g| g["class"] == "pass"), "{t}: {own:?}");
    }
    // The merged group failed the pair gate; one repair of the group; then it passed.
    let group: Vec<&&Value> = gates
        .iter()
        .filter(|g| g["part"] == "group-T004-T007")
        .collect();
    let failed: Vec<&&&Value> = group
        .iter()
        .filter(|g| g["result"]["gate"] == "pair" && g["class"] != "pass")
        .collect();
    assert_eq!(failed.len(), 1, "{group:?}");
    let r = of(&v, "repair", "group-T004-T007");
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0]["task"].is_null());
    assert!(
        text(r[0], "worktree")
            .replace('\\', "/")
            .ends_with("/build")
    );
    assert!(prompt(&root, id, r[0]).contains("gate pair"));
    let repaired = text(r[0], "verified_commit");
    assert!(
        group.iter().any(|g| g["result"]["gate"] == "pair"
            && g["result"]["commit"] == repaired
            && g["class"] == "pass"),
        "{group:?}"
    );
    // Must fail: T008 is not queued until the group passes (AS6).
    let t8 = queued_seq(&ev, of(&v, "build", "T008")[0]["n"].as_u64().unwrap());
    let passed = group
        .iter()
        .filter(|g| g["result"]["commit"] == repaired)
        .map(|g| g["seq"].as_u64().unwrap())
        .max()
        .unwrap();
    assert!(passed < t8);
    assert!(of_type(&ev, "group_done")[0]["seq"].as_u64().unwrap() < t8);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn one_builder_or_no_group_runs_as_before() {
    // One builder over [P] tasks: one after the other in the phase's session (AS9).
    let root = scratch("builders-one");
    setup(&root, 1, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    let (e, id) = lock(&root, &f, &whole(&f, ""), (5.0, 60));
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    assert!(of_type(&ev, "group_started").is_empty());
    let first = of(&v, "build", "T004")[0];
    for t in ["T004", "T005", "T006", "T007", "T008"] {
        let b = of(&v, "build", t)[0];
        assert!(
            text(b, "worktree").replace('\\', "/").ends_with("/build"),
            "{t}"
        );
        assert_eq!(b["session"], first["session"], "{t} in the phase's session");
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // Three builders on a feature without [P] tasks: as 0.4.0 (AS8).
    let root = scratch("builders-no-p");
    setup(&root, 3, None);
    let (e, _repo, id) = locked_calc(&root, &calc_loop_ok(), Some("builders"));
    let v = wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let ev = events(&root, id);
    assert!(of_type(&ev, "group_started").is_empty());
    for s in in_order(&v).into_iter().filter(|s| s["kind"] == "build") {
        assert!(text(s, "worktree").replace('\\', "/").ends_with("/build"));
    }
    let t2 = of(&v, "build", "T002")[0];
    let t3 = of(&v, "build", "T003")[0];
    assert_eq!(t2["session"], t3["session"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_question_holds_only_its_task() {
    let root = scratch("builders-question");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    let mut steps = f.phase_1();
    for t in &f.parallel {
        if t == "T005" {
            steps.push_str(&f.build(t, "ask = \"May t005.py import os?\"").replacen(
                "status = \"done\"",
                "status = \"blocked\", question = \"$ANSWER\"",
                1,
            ));
            steps.push_str("[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = { outcome = \"relay\", answer = \"\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{ label = \"no\", recommended = true }] }\n\n");
            steps.push_str(&f.build(t, "when = \"The engineer answers\""));
        } else {
            steps.push_str(&f.build(t, "sleep_ms = 500"));
        }
    }
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    wait_for(tx, id, "the question", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
    });
    // The other tasks go on and are accepted while it waits; the task after the group does not
    // start (AS10, FR-018).
    let v = wait_for(tx, id, "T004, T006 and T007 accepted", |v| {
        ["T004", "T006", "T007"]
            .iter()
            .all(|t| v["feature_loop"]["tasks"][t]["state"] == "accepted")
    });
    assert_eq!(
        v["feature_loop"]["questions"]["q1"]["state"],
        "waiting_engineer"
    );
    assert!(!v["needs_engineer"].is_null(), "the engineer is asked");
    assert_eq!(v["feature_loop"]["tasks"]["T005"]["state"], "waiting");
    std::thread::sleep(Duration::from_secs(2));
    let v = ask(tx, Request::Get { id }).unwrap();
    assert!(
        of(&v, "build", "T008").is_empty(),
        "T008 waits for the answer"
    );
    let ev = events(&root, id);
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004"], "nothing after T005 merges before it");
    ask(
        tx,
        Request::QuestionAnswer {
            id,
            q: "q1".into(),
            answer: "No, nothing from os".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    // The answer went to T005's builder, in its session and working copy.
    let b = of(&v, "build", "T005");
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!(b[1]["resume"], b[0]["session"]);
    assert!(
        text(b[1], "worktree")
            .replace('\\', "/")
            .ends_with("/task-T005")
    );
    let ev = events(&root, id);
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006", "T007"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_cost_limit_stops_new_builders() {
    let root = scratch("builders-cost");
    setup(&root, 3, None);
    let log = root.join("args.log");
    // Every fake agent this test binary starts writes its arguments there; this test reads only
    // its own (by its scenario).
    unsafe { std::env::set_var("FALLOW_FAKE_ARGS_LOG", &log) };
    let f = parallel_tasks_feature(&root, 4, &[]);
    let mut steps = f.phase_1();
    for (t, ms) in f.parallel.iter().zip([1000, 5000, 5000, 0]) {
        steps.push_str(&f.build(t, &format!("sleep_ms = {ms}\ncost = 1.0")));
    }
    steps.push_str(&f.build(&f.last, ""));
    // Phase 1 costs 0.04; the first group build to finish reaches the limit.
    let (e, id) = lock(&root, &f, &steps, (1.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "the limit", |v| {
        v["stage"] == "paused" && !v["needs_engineer"].is_null()
    });
    assert!(
        v["needs_engineer"].to_string().contains("limit is reached"),
        "{}",
        v["needs_engineer"]
    );
    // The running builds finish.
    let v = wait_for(tx, id, "T005 and T006 finished", |v| {
        ["T005", "T006"]
            .iter()
            .all(|t| of(v, "build", t).iter().all(|s| !s["finished"].is_null()))
    });
    let ev = events(&root, id);
    let limit = of_type(&ev, "limit_reached");
    assert_eq!(limit.len(), 1, "{limit:?}");
    let at = limit[0]["seq"].as_u64().unwrap();
    let late: Vec<&Value> = of_type(&ev, "step_started")
        .into_iter()
        .filter(|s| s["seq"].as_u64().unwrap() > at)
        .collect();
    assert!(late.is_empty(), "no step starts after the limit: {late:?}");
    assert!(
        of(&v, "build", "T007")
            .iter()
            .all(|s| s["started"].is_null())
    );
    assert_eq!(v["stage"], "paused");
    // The three builds started together, each with its share of what was left (FR-023).
    assert_eq!(most_at_once(&ev, &f.parallel), 3);
    let scenario = root.join("scenario.toml").display().to_string();
    let budgets: Vec<f64> = std::fs::read_to_string(&log)
        .unwrap()
        .split("---\n")
        .filter(|b| b.lines().any(|l| l == scenario) && b.lines().any(|l| l == "--session-id"))
        .filter_map(|b| {
            let ls: Vec<&str> = b.lines().collect();
            let i = ls.iter().position(|l| *l == "--max-budget-usd")?;
            ls.get(i + 1)?.parse().ok()
        })
        .collect();
    let group = &budgets[budgets.len() - 3..];
    let share = (1.0 - 0.04) / 3.0;
    for b in group {
        assert!(*b <= share + 0.005, "{b} of {share}: {budgets:?}");
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
