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
use std::time::Duration;

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

/// The span of the builds of `tasks` (from the first one's start to the last one's finish), from
/// their own `step_started` and `step_finished` events in the log: neither Fallow's work before
/// and after them nor the machine's load outside them counts (US4 review 1).
fn build_span(ev: &[Value], tasks: &[String]) -> Duration {
    let at = |e: &Value| fallow::run::time::parse_rfc3339(e["at"].as_str().unwrap()).unwrap();
    let builds: std::collections::BTreeSet<u64> = ev
        .iter()
        .filter(|e| {
            e["type"] == "step_queued"
                && e["kind"] == "build"
                && tasks.iter().any(|t| e["task"].as_str() == Some(t.as_str()))
        })
        .filter_map(|e| e["n"].as_u64())
        .collect();
    let of = |t: &str| -> Vec<u64> {
        ev.iter()
            .filter(|e| e["type"] == t && e["n"].as_u64().is_some_and(|n| builds.contains(&n)))
            .map(at)
            .collect()
    };
    let (starts, ends) = (of("step_started"), of("step_finished"));
    assert_eq!(starts.len(), tasks.len(), "a build per task");
    assert_eq!(ends.len(), tasks.len(), "each build finished");
    Duration::from_secs(ends.iter().max().unwrap() - starts.iter().min().unwrap())
}

/// A loop over the feature with `builders` builders, each build taking 12 s, run until it is
/// ready; returns the engine, the loop and the span of the group's builds (`build_span`).
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
    let (e, id) = lock(&root, &f, &whole(&f, "sleep_ms = 12000"), (5.0, 60));
    wait_for(&e.tx, id, "ready", |v| v["stage"] == "ready");
    let took = build_span(&events(&root, id), &f.parallel);
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
    // SC-002: with 3 builders the group's builds take at most 60 % of their time with one,
    // measured from the builds' own start and finish in each loop's log (US4 review 1).
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
    pair_gate(&f);
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

/// Unlocks the feature, changes its `tasks.md` with `change`, analyzes again and locks it again
/// (the scenario holds `LEAD_ANALYZES_AGAIN`).
fn lock_again(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    v: &Value,
    change: impl Fn(&str) -> String,
) {
    use fallow::studio::Req;
    let project = v["feature_loop"]["project"].as_str().unwrap().to_string();
    let feature = "001-calc".to_string();
    ask(
        tx,
        Request::Unlock {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    let path = "specs/001-calc/tasks.md".to_string();
    let doc = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: path.clone(),
        },
    )
    .unwrap();
    let text = change(doc["text"].as_str().unwrap());
    sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path,
            text,
            version: doc["version"].as_str().unwrap().to_string(),
        },
    )
    .unwrap();
    analyze_again(tx, &project, &feature);
    ask(
        tx,
        Request::Lock {
            project,
            feature,
            pipeline: None,
            level: None,
            budget_usd: None,
            budget_minutes: None,
            when_verified: None,
            update_limit: None,
            run_after: None,
        },
    )
    .unwrap();
}

/// 018 US3 (decided after review 1; FR-020a): a new lock while a group is in progress is taken
/// in only when no step of the group runs. The tasks of the group not merged by then are dropped
/// (their branches and working copies removed, built again under the new lock), and the history
/// says so; `tasks.md` is ticked only for tasks merged into the build branch; no two builders ever
/// share a working copy and the builders cap holds.
#[test]
fn a_new_lock_during_a_group_drops_its_unmerged_tasks_and_builds_them_again() {
    let root = scratch("builders-relock");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    // T004 is still at work when the engineer locks again; T005 and T006 are accepted by then,
    // not merged (they wait for T004).
    steps.push_str(&f.build("T004", "sleep_ms = 20000"));
    steps.push_str(&f.build("T005", ""));
    steps.push_str(&f.build("T006", ""));
    // Under the new lock: US1 brought in line, then T004 to T007.
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    for t in ["T004", "T005", "T006", "T007"] {
        steps.push_str(&f.build(t, ""));
    }
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "T005 and T006 accepted while T004 builds", |v| {
        ["T005", "T006"]
            .iter()
            .all(|t| v["feature_loop"]["tasks"][t]["state"] == "accepted")
    });
    // The new lock: T004's text changes and T006 is no longer [P].
    lock_again(tx, &v, |t| {
        t.replace(
            "T004 [P] [US1] Write calc/t004.py",
            "T004 [P] [US1] Write calc/t004.py with its name",
        )
        .replace("T006 [P] [US1]", "T006 [US1]")
    });
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let relocked = of_type(&ev, "relocked")[0]["seq"].as_u64().unwrap();
    let old4 = of(&v, "build", "T004")[0]["n"].as_u64().unwrap();
    let old4_end = ev
        .iter()
        .find(|e| e["type"] == "step_finished" && e["n"].as_u64() == Some(old4))
        .and_then(|e| e["seq"].as_u64())
        .unwrap();
    assert!(
        old4_end > relocked,
        "the new lock came while T004 was at work"
    );
    // Must fail: nothing of the group starts while T004's builder is at work; never two
    // builders of one task at once (one working copy); never more than the cap.
    let between: Vec<&Value> = of_type(&ev, "step_queued")
        .into_iter()
        .filter(|e| (relocked..old4_end).contains(&e["seq"].as_u64().unwrap()))
        .collect();
    assert!(
        between.is_empty(),
        "queued under the old group: {between:?}"
    );
    for t in ["T004", "T005", "T006"] {
        assert_eq!(most_at_once(&ev, &[t.to_string()]), 1, "{t}");
    }
    assert!(most_at_once(&ev, &f.parallel) <= 3);
    // Must fail: the old group is dropped once (none of its tasks was merged), each task
    // reset, and the history says so.
    let dropped = of_type(&ev, "group_dropped");
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(
        dropped[0]["tasks"],
        serde_json::json!(["T004", "T005", "T006"])
    );
    assert_eq!(
        dropped[0]["dropped"],
        serde_json::json!(["T004", "T005", "T006"])
    );
    assert!(dropped[0]["seq"].as_u64().unwrap() > old4_end);
    let feed = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: fallow::run::activity::Filter::default(),
        },
    )
    .unwrap()
    .to_string();
    for t in ["T004", "T005", "T006"] {
        assert!(
            feed.contains(&format!(
                "{t} built alongside was not merged before the new lock: built again"
            )),
            "{feed}"
        );
    }
    // Must fail: the new lock is taken into the build branch without a tick for a task that
    // was accepted but never merged.
    let build_branch = text(&v["feature_loop"], "build_branch");
    let taken = git(&f.repo, &["log", "--format=%H %s", &build_branch])
        .lines()
        .find(|l| l.contains("lock 2 of"))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .expect("the new lock's merge");
    let at_lock = git(
        &f.repo,
        &["show", &format!("{taken}:specs/001-calc/tasks.md")],
    );
    for t in ["T004", "T005", "T006"] {
        assert!(!at_lock.contains(&format!("- [X] {t} ")), "{t}: {at_lock}");
    }
    // Each built again under the new lock: T004 and T005 as the new group, each in its own
    // working copy started from the build branch holding the new lock; T006 in the build
    // worktree.
    for t in ["T004", "T005", "T006"] {
        let b = of(&v, "build", t);
        assert_eq!(b.len(), 2, "{t}: {b:?}");
        let base = text(b[1], "base");
        let docs = git(
            &f.repo,
            &["show", &format!("{base}:specs/001-calc/tasks.md")],
        );
        assert!(docs.contains("calc/t004.py with its name"), "{t} on {base}");
        let wt = text(b[1], "worktree").replace('\\', "/");
        let own = if t == "T006" {
            "/build".to_string()
        } else {
            format!("/task-{t}")
        };
        assert!(wt.ends_with(&own), "{t} in {wt}");
    }
    let started = of_type(&ev, "group_started");
    assert_eq!(started.len(), 2);
    assert_eq!(started[1]["tasks"], serde_json::json!(["T004", "T005"]));
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005"]);
    let tasks_md = git(
        &f.repo,
        &["show", &format!("{build_branch}:specs/001-calc/tasks.md")],
    );
    for t in ["T004", "T005", "T006", "T007"] {
        assert!(tasks_md.contains(&format!("- [X] {t} ")), "{t}: {tasks_md}");
    }
    // The old group is recorded as dropped; no working copy or branch of either group is left.
    let lv = ask(tx, Request::Loop { id }).unwrap();
    assert_eq!(lv["groups"][0]["dropped"], true, "{}", lv["groups"]);
    assert_eq!(lv["groups"][0]["tasks"][1]["state"], "dropped");
    assert_eq!(lv["groups"][1]["done"], true);
    let wt = root.join("data/runs").join(id.to_string()).join("wt");
    for t in ["T004", "T005", "T006"] {
        assert!(!wt.join(format!("task-{t}")).exists(), "{t}'s working copy");
    }
    let left: Vec<String> = branches(&f.repo)
        .into_iter()
        .filter(|b| {
            ["T004", "T005", "T006"]
                .iter()
                .any(|t| b.ends_with(&format!("-{t}")))
        })
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 AS10 (FR-018; review 1): two tasks of a group ask questions the engineer must answer.
/// Each holds only its own task; neither question replaces the other; the group's other tasks go
/// on; each answer goes to the step of the task that asked.
#[test]
fn two_questions_in_a_group_each_hold_only_their_task() {
    let root = scratch("builders-questions");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 4, &[]);
    let mut steps = f.phase_1();
    for t in &f.parallel {
        if t == "T005" || t == "T006" {
            steps.push_str(
                &f.build(t, &format!("ask = \"May {t} import os?\""))
                    .replacen(
                        "status = \"done\"",
                        "status = \"blocked\", question = \"$ANSWER\"",
                        1,
                    ),
            );
        } else {
            steps.push_str(&f.build(t, "sleep_ms = 1500"));
        }
    }
    for q in ["q1", "q2"] {
        steps.push_str(&format!("[[step]]\nkind = \"lead_answer\"\npart = \"{q}\"\noutput = {{ outcome = \"relay\", answer = \"\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{{ label = \"no\", recommended = true }}] }}\n\n"));
    }
    for t in ["T005", "T006"] {
        steps.push_str(&f.build(t, "when = \"The engineer answers\""));
    }
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "both questions", |v| {
        ["q1", "q2"]
            .iter()
            .all(|q| v["feature_loop"]["questions"][q]["state"] == "waiting_engineer")
    });
    let task_of = |v: &Value, q: &str| text(&v["feature_loop"]["questions"][q], "task");
    let (t1, t2) = (task_of(&v, "q1"), task_of(&v, "q2"));
    let mut asked = vec![t1.clone(), t2.clone()];
    asked.sort();
    assert_eq!(asked, ["T005", "T006"]);
    // The other tasks go on and are accepted while both wait (AS10); T008 does not start.
    let v = wait_for(tx, id, "T004 and T007 accepted", |v| {
        ["T004", "T007"]
            .iter()
            .all(|t| v["feature_loop"]["tasks"][t]["state"] == "accepted")
    });
    std::thread::sleep(Duration::from_secs(2));
    let v2 = ask(tx, Request::Get { id }).unwrap();
    for v in [&v, &v2] {
        for t in ["T005", "T006"] {
            assert_eq!(v["feature_loop"]["tasks"][t]["state"], "waiting", "{t}");
        }
        // Must fail: the engineer is asked the first question; the second does not replace it.
        let needs = v["needs_engineer"].as_str().unwrap_or_default().to_string();
        assert!(
            needs.contains(&format!("The builder of {t1} asks (q1): ")),
            "{needs}"
        );
    }
    assert!(
        of(&v2, "build", "T008").is_empty(),
        "T008 waits for the answers"
    );
    // Answering (the loop's answer, not naming the question) answers the question asked; its
    // task's builder gets it, the other still waits.
    ask(
        tx,
        Request::Answer {
            id,
            text: "First answer".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "the first answered", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "answered"
            && of(v, "build", &t1).len() == 2
    });
    assert_eq!(
        v["feature_loop"]["questions"]["q2"]["state"],
        "waiting_engineer"
    );
    assert_eq!(of(&v, "build", &t2).len(), 1, "{t2} still waits");
    assert!(prompt(&root, id, of(&v, "build", &t1)[1]).contains("First answer"));
    let v = wait_for(tx, id, "the second question asked", |v| {
        v["needs_engineer"]
            .as_str()
            .is_some_and(|n| n.contains(&format!("The builder of {t2} asks (q2): ")))
    });
    assert!(of(&v, "build", "T008").is_empty());
    ask(
        tx,
        Request::Answer {
            id,
            text: "Second answer".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let b = of(&v, "build", &t2);
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!(b[1]["resume"], b[0]["session"]);
    assert!(prompt(&root, id, b[1]).contains("Second answer"));
    let ev = events(&root, id);
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006", "T007"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 (decided after review 1; FR-020a): a task of the group merged before the new lock
/// stays merged and is not built again; the tasks not merged are built again; the group the new
/// `tasks.md` gives is gated on its merged result.
#[test]
fn a_new_lock_keeps_the_tasks_of_the_group_already_merged() {
    let root = scratch("builders-relock-merged");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    // T004 is merged; T005 is still at work at the new lock; T006 is accepted, not merged.
    steps.push_str(&f.build("T004", ""));
    steps.push_str(&f.build("T005", "sleep_ms = 20000"));
    steps.push_str(&f.build("T006", ""));
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    for t in ["T005", "T006", "T007"] {
        steps.push_str(&f.build(t, ""));
    }
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(
        tx,
        id,
        "T004 merged and T006 accepted while T005 builds",
        |v| {
            v["feature_loop"]["tasks"]["T006"]["state"] == "accepted"
                && v["feature_loop"]["groups"][0]["tasks"][0]["merged"].is_string()
        },
    );
    lock_again(tx, &v, |t| {
        t.replace(
            "T005 [P] [US1] Write calc/t005.py",
            "T005 [P] [US1] Write calc/t005.py with its name",
        )
    });
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let dropped = of_type(&ev, "group_dropped");
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(dropped[0]["dropped"], serde_json::json!(["T005", "T006"]));
    // T004 is built and merged once; T005 and T006 again.
    assert_eq!(of(&v, "build", "T004").len(), 1);
    for t in ["T005", "T006"] {
        assert_eq!(of(&v, "build", t).len(), 2, "{t}");
    }
    let merged: Vec<&str> = of_type(&ev, "task_merged")
        .iter()
        .map(|e| e["task"].as_str().unwrap())
        .collect();
    assert_eq!(merged, ["T004", "T005", "T006"]);
    // The new group, of the same three tasks, ran and was gated on its merged result.
    let started = of_type(&ev, "group_started");
    assert_eq!(started.len(), 2);
    assert_eq!(
        started[1]["tasks"],
        serde_json::json!(["T004", "T005", "T006"])
    );
    let done = of_type(&ev, "group_done");
    assert_eq!(done.len(), 1);
    let last_merge = of_type(&ev, "task_merged").last().unwrap()["seq"]
        .as_u64()
        .unwrap();
    assert!(
        of_type(&ev, "gate_run")
            .iter()
            .any(|g| g["part"] == "group-T004-T006" && g["seq"].as_u64().unwrap() > last_merge),
        "the new group's gates"
    );
    let build_branch = text(&v["feature_loop"], "build_branch");
    let taken = git(&f.repo, &["log", "--format=%H %s", &build_branch])
        .lines()
        .find(|l| l.contains("lock 2 of"))
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .expect("the new lock's merge");
    let at_lock = git(
        &f.repo,
        &["show", &format!("{taken}:specs/001-calc/tasks.md")],
    );
    assert!(at_lock.contains("- [X] T004 "), "{at_lock}");
    assert!(!at_lock.contains("- [X] T006 "), "{at_lock}");
    let left: Vec<String> = branches(&f.repo)
        .into_iter()
        .filter(|b| {
            ["T004", "T005", "T006"]
                .iter()
                .any(|t| b.ends_with(&format!("-{t}")))
        })
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A gate that fails only on the merged result of T004 and T005 until a repair adds
/// calc/fixed.py (as `a_merged_group_that_fails_the_gates_is_repaired_before_the_next_task`).
fn pair_gate(f: &ParallelFeature) {
    std::fs::write(
        f.repo.join(".fallow/squad.toml"),
        format!(
            "repair_cap = 2\n[[gate]]\nname = \"tests\"\ncommand = \"{PYTHON} -m unittest\"\nbaseline = true\n\n[[gate]]\nname = \"pair\"\ncommand = \"{PYTHON} -c \\\"import os,sys; print('Ran 1 test'); both = os.path.exists('calc/t004.py') and os.path.exists('calc/t005.py'); fixed = os.path.exists('calc/fixed.py'); sys.exit(1 if both and not fixed else 0)\\\"\"\nbaseline = true\n"
        ),
    )
    .unwrap();
}

/// 018 US3 (review 2, FR-020a, research R-6a): the engineer locks again while the repair of a
/// merged group (part `group-<first>-<last>`, tied to no one task) is at work, with a restart of
/// Fallow before it ends or not. The group is dropped; its repair is never queued again, and
/// never counts as the repair of the group the new lock gives.
fn relock_during_a_group_repair(name: &'static str, restart: bool) {
    let root = scratch(name);
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    pair_gate(&f);
    let part = "group-T004-T006";
    let fixed = [("calc/fixed.py", "FIXED = True\n")];
    let mut steps = f.phase_1();
    for t in &f.parallel {
        steps.push_str(&f.build(t, ""));
    }
    // The old group's repair, at work when the engineer locks again.
    steps.push_str(&task_step(part, &fixed, "sleep_ms = 15000").replacen(
        "kind = \"build\"",
        "kind = \"repair\"",
        1,
    ));
    // The story brought in line with the new lock (gated at the checkpoint on the build head,
    // FR-020b: the group's repair is in it).
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    // A second repair of the part: the new group's, when the old one was cut short by the
    // restart and its merged result (T004 and T005 kept) still fails (were the old one run
    // again, it would take this one).
    steps.push_str(&repair(part, &fixed));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let v = wait_for(&e.tx, id, "the group's repair at work", |v| {
        of(v, "repair", part)
            .first()
            .is_some_and(|s| !s["started"].is_null())
    });
    lock_again(&e.tx, &v, |t| {
        t.replace(
            "T007 [US1] Join them in calc/all.py",
            "T007 [US1] Join them all in calc/all.py",
        )
    });
    let e = if restart {
        stop(e);
        start_studio(&root, &root.join("scenario.toml"), None)
    } else {
        e
    };
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let dropped = of_type(&ev, "group_dropped");
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert_eq!(dropped[0]["dropped"], serde_json::json!([]));
    let dropped_at = dropped[0]["seq"].as_u64().unwrap();
    let started = of_type(&ev, "group_started");
    assert_eq!(started.len(), 2, "{started:?}");
    let new_group = started[1]["seq"].as_u64().unwrap();
    // Must fail (restart): the old repair, interrupted, is never queued again after the drop.
    let reps = of(&v, "repair", part);
    for r in &reps {
        let q = queued_seq(&ev, r["n"].as_u64().unwrap());
        assert!(
            q < dropped_at || q > new_group,
            "a repair of the dropped group queued after its drop: {r}"
        );
        assert!(
            !text(r, "reason").starts_with("resumed") && !text(r, "reason").starts_with("again"),
            "{r}"
        );
    }
    // The new group's merged result: it passes when the old repair ended before the drop;
    // after a restart (the old repair cut short) it fails and is repaired as the new group's.
    let own: Vec<&&Value> = reps
        .iter()
        .filter(|r| queued_seq(&ev, r["n"].as_u64().unwrap()) > new_group)
        .collect();
    assert_eq!(own.len(), usize::from(restart), "{reps:?}");
    assert_eq!(reps.len(), 1 + own.len(), "{reps:?}");
    assert_eq!(of_type(&ev, "group_done").len(), 1);
    // FR-020b: the story is gated on the build head at its checkpoint, never charged the
    // group's failure: its relock repair only.
    let story = of(&v, "repair", "US1");
    assert_eq!(story.len(), 1, "{story:?}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_lock_during_a_group_repair_never_runs_it_again() {
    relock_during_a_group_repair("builders-relock-repair", false);
}

#[test]
fn a_new_lock_during_a_group_repair_then_a_restart_never_runs_it_again() {
    relock_during_a_group_repair("builders-relock-repair-restart", true);
}

/// 018 US3 (review 3; engineer's decision 2026-11-27, FR-020b): a new lock keeps the merged
/// tasks of a group whose merged result fails the gates (the old group's repair fixed nothing).
/// The story is brought in line with the new lock; the new group gates the merged result again
/// and repairs it, counted as the group's. At the story's checkpoint the gates run on the build
/// head (everything that will be merged), not on the story repair's own commit: the story is
/// never charged the group's failure.
#[test]
fn a_story_after_a_new_lock_is_gated_on_the_build_head_not_charged_the_groups_failure() {
    let root = scratch("builders-relock-story-head");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    pair_gate(&f);
    let part = "group-T004-T006";
    let mut steps = f.phase_1();
    for t in &f.parallel {
        steps.push_str(&f.build(t, ""));
    }
    // The old group's repair, at work at the new lock: it fixes nothing.
    steps.push_str(
        &task_step(part, &[("calc/note.py", "NOTE = 1\n")], "sleep_ms = 3000").replacen(
            "kind = \"build\"",
            "kind = \"repair\"",
            1,
        ),
    );
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    // The new group's repair of the merged result it gates again.
    steps.push_str(&repair(part, &[("calc/fixed.py", "FIXED = True\n")]));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "the group's repair at work", |v| {
        of(v, "repair", part)
            .first()
            .is_some_and(|s| !s["started"].is_null())
    });
    lock_again(tx, &v, |t| {
        t.replace(
            "T007 [US1] Join them in calc/all.py",
            "T007 [US1] Join them all in calc/all.py",
        )
    });
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    // Must fail (before the decision): the story's relock repair, gated on its own commit
    // (without the group's repair), was charged repair rounds and went to the engineer.
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let new_group = of_type(&ev, "group_started")[1]["seq"].as_u64().unwrap();
    // The new group's merged result failed and was repaired as the group's.
    let gates = of_type(&ev, "gate_run");
    assert!(
        gates.iter().any(|g| g["part"] == part
            && g["result"]["gate"] == "pair"
            && g["class"] != "pass"
            && g["seq"].as_u64().unwrap() > new_group),
        "{gates:?}"
    );
    let group_repairs: Vec<&Value> = of(&v, "repair", part)
        .into_iter()
        .filter(|r| queued_seq(&ev, r["n"].as_u64().unwrap()) > new_group)
        .collect();
    assert_eq!(group_repairs.len(), 1, "{group_repairs:?}");
    // The story: only its relock repair, never a repair round.
    let story = of(&v, "repair", "US1");
    assert_eq!(story.len(), 1, "{story:?}");
    assert!(
        text(story[0], "reason").starts_with("the new lock"),
        "{}",
        story[0]
    );
    let own = text(story[0], "verified_commit");
    // Its gates ran on the build head at the checkpoint: the group's repair is in it.
    let at_story: Vec<&&Value> = gates.iter().filter(|g| g["part"] == "US1").collect();
    assert!(!at_story.is_empty());
    for g in &at_story {
        assert_eq!(g["class"], "pass", "{g}");
        let c = g["result"]["commit"].as_str().unwrap();
        assert_ne!(c, own, "gated on the story repair's own commit");
        git(&f.repo, &["cat-file", "-e", &format!("{c}:calc/fixed.py")]);
        git(&f.repo, &["merge-base", "--is-ancestor", &own, c]);
    }
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 (review 2, research R-6): a new lock that takes a merged task out of every group (its
/// [P] removed) leaves neither its working copy nor its branch: no branch of the run outlives
/// its group.
#[test]
fn a_new_lock_removes_the_copies_of_a_merged_task_no_longer_in_a_group() {
    let root = scratch("builders-relock-ungroup");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    // T004 is merged; T005 is still at work at the new lock; T006 is accepted, not merged.
    steps.push_str(&f.build("T004", ""));
    steps.push_str(&f.build("T005", "sleep_ms = 20000"));
    steps.push_str(&f.build("T006", ""));
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    for t in ["T005", "T006", "T007"] {
        steps.push_str(&f.build(t, ""));
    }
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(
        tx,
        id,
        "T004 merged and T006 accepted while T005 builds",
        |v| {
            v["feature_loop"]["tasks"]["T006"]["state"] == "accepted"
                && v["feature_loop"]["groups"][0]["tasks"][0]["merged"].is_string()
        },
    );
    lock_again(tx, &v, |t| t.replace("T004 [P] [US1]", "T004 [US1]"));
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let started = of_type(&ev, "group_started");
    assert_eq!(started.len(), 2);
    assert_eq!(started[1]["tasks"], serde_json::json!(["T005", "T006"]));
    assert_eq!(of(&v, "build", "T004").len(), 1);
    // Must fail: T004's working copy and branch are gone with the group it was in.
    let wt = root.join("data/runs").join(id.to_string()).join("wt");
    for t in ["T004", "T005", "T006"] {
        assert!(!wt.join(format!("task-{t}")).exists(), "{t}'s working copy");
    }
    let left: Vec<String> = branches(&f.repo)
        .into_iter()
        .filter(|b| {
            ["T004", "T005", "T006"]
                .iter()
                .any(|t| b.ends_with(&format!("-{t}")))
        })
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

const LINT: &str = "---\nname: lint-checker\ndescription: Runs the linter on what a task changed\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nRun the linter on the task's changes.\n";

/// A checker's verdict at `part`: no findings.
fn clean(part: &str, extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"review\"\npart = {part:?}\n{extra}\noutput = {{ findings = [], summary = \"checked\" }}\n\n"
    )
}

const RELAY_Q1: &str = "[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = { outcome = \"relay\", answer = \"\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{ label = \"no\", recommended = true }] }\n\n";

/// 018 US3 (review 2, FR-018): a checker after a task of the group asks the engineer. The
/// engineer is told who asks about which task (the checker, not a builder); only that task is
/// held; the answer goes to the checker that asked.
#[test]
fn a_checker_after_a_group_task_asks_as_itself_and_holds_only_its_task() {
    let root = scratch("builders-checker-asks");
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("builders.toml"),
        pipeline_with(
            3,
            "[[step]]\nagent = \"lint-checker\"\nwhen = \"task\"\nat = \"end\"\non_fail = \"repair\"\n",
        ),
    )
    .unwrap();
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("lint-checker.md"), LINT).unwrap();
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    for t in ["T001", "T002", "T003"] {
        steps.push_str(&clean(&format!("lint-checker@{t}-end"), ""));
    }
    for t in &f.parallel {
        steps.push_str(&f.build(t, ""));
        let extra = if t == "T005" {
            "ask = \"May T005 import os?\""
        } else {
            ""
        };
        steps.push_str(&clean(&format!("lint-checker@{t}-end"), extra));
    }
    steps.push_str(RELAY_Q1);
    steps.push_str(&clean(
        "lint-checker@T005-end",
        "when = \"The engineer answers\"",
    ));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&clean(&format!("lint-checker@{}-end", f.last), ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "the question", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
            && !v["needs_engineer"].is_null()
    });
    let needs = v["needs_engineer"].as_str().unwrap_or_default().to_string();
    // Must fail: the checker asks about T005, not "the builder" or its point's name.
    assert!(
        needs.contains("The lint-checker of T005 asks (q1): "),
        "{needs}"
    );
    // Only T005 is held: T004 and T006 are accepted meanwhile.
    let v = wait_for(tx, id, "T004 and T006 accepted, T005 waits", |v| {
        ["T004", "T006"]
            .iter()
            .all(|t| v["feature_loop"]["tasks"][t]["state"] == "accepted")
            && v["feature_loop"]["tasks"]["T005"]["state"] == "waiting"
    });
    assert!(of(&v, "build", &f.last).is_empty());
    ask(
        tx,
        Request::Answer {
            id,
            text: "Yes, os is fine".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || (v["stage"] != "building" && !v["needs_engineer"].is_null())
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let r = of(&v, "review", "lint-checker@T005-end");
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(prompt(&root, id, r[1]).contains("Yes, os is fine"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 (review 2, research R-6a): a task whose question still waits for the engineer is
/// dropped by a new lock. Its question is the loop's, no longer the task's: the task is not
/// shown waiting for an answer.
#[test]
fn a_dropped_task_with_a_waiting_question_is_not_shown_waiting() {
    let root = scratch("builders-relock-question");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    let mut steps = f.phase_1();
    steps.push_str(&f.build("T004", ""));
    steps.push_str(&f.build("T005", "ask = \"May T005 import os?\"").replacen(
        "status = \"done\"",
        "status = \"blocked\", question = \"$ANSWER\"",
        1,
    ));
    steps.push_str(&f.build("T006", ""));
    steps.push_str(RELAY_Q1);
    steps.push_str(
        &task_step(
            "US1",
            &[("calc/relock.py", "LOCK = 2\n")],
            "when = \"lock 2\"",
        )
        .replacen("kind = \"build\"", "kind = \"repair\"", 1),
    );
    for t in ["T005", "T006", "T007"] {
        steps.push_str(&f.build(t, ""));
    }
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    steps.push_str(LEAD_ANALYZES_AGAIN);
    let (e, id) = lock(&root, &f, &steps, (5.0, 60));
    let tx = &e.tx;
    let v = wait_for(tx, id, "T005's question, T004 merged", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
            && v["feature_loop"]["groups"][0]["tasks"][0]["merged"].is_string()
            && v["feature_loop"]["tasks"]["T006"]["state"] == "accepted"
    });
    lock_again(tx, &v, |t| {
        t.replace(
            "T005 [P] [US1] Write calc/t005.py",
            "T005 [P] [US1] Write calc/t005.py with its name",
        )
    });
    wait_for(tx, id, "the group dropped", |v| {
        v["feature_loop"]["groups"][0]["dropped"] == true
    });
    std::thread::sleep(Duration::from_secs(2));
    let v = ask(tx, Request::Get { id }).unwrap();
    assert_eq!(
        v["feature_loop"]["questions"]["q1"]["state"],
        "waiting_engineer"
    );
    // Must fail: T005 is not built and not waiting for an answer; the question is the loop's.
    assert_eq!(v["feature_loop"]["tasks"]["T005"]["state"], "pending");
    let needs = v["needs_engineer"].as_str().unwrap_or_default().to_string();
    assert!(needs.contains("(q1)"), "{needs}");
    assert!(!needs.contains(" of T005 asks"), "{needs}");
    let lv = ask(tx, Request::Loop { id }).unwrap();
    for g in lv["groups"].as_array().unwrap() {
        for t in g["tasks"].as_array().unwrap() {
            assert_ne!(t["state"], "waiting", "{g}");
        }
    }
    for p in lv["phases"].as_array().unwrap() {
        for t in p["tasks"].as_array().unwrap() {
            assert_ne!(t["state"], "waiting", "{t}");
        }
    }
    ask(
        tx,
        Request::Answer {
            id,
            text: "Yes, os is fine".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || (v["stage"] != "building" && !v["needs_engineer"].is_null())
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    assert_eq!(of(&v, "build", "T005").len(), 2);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 018 US3 (review 4, FR-020b): the engineer overrules the lead's decision while a group runs.
/// The story's overrule repair is gated on its own commit; the merged group that then fails the
/// gates is repaired as the group's, never charged to the story. Only the checkpoint gates the
/// build head.
#[test]
fn an_overrule_repair_is_not_charged_a_group_merged_after_it() {
    let root = scratch("builders-overrule-group");
    setup(&root, 3, None);
    let f = parallel_tasks_feature(&root, 3, &[]);
    pair_gate(&f);
    let part = "group-T004-T006";
    let mut steps = f.phase_1();
    for t in &f.parallel {
        if t == "T004" {
            // T004 asks; the lead decides (Free) and T004 goes on.
            steps.push_str(&f.build(t, "ask = \"May t004.py be empty?\"").replacen(
                "status = \"done\"",
                "status = \"done\", question = \"$ANSWER\"",
                1,
            ));
            steps.push_str("[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = { outcome = \"decided\", answer = \"No\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{ label = \"no\", recommended = true }, { label = \"yes\" }] }\n\n");
        } else {
            steps.push_str(&f.build(t, "sleep_ms = 6000"));
        }
    }
    // The overrule repair: no session holds the whole phase, so it starts with the phase's own
    // introduction, a repair of the story, never the instruction of a task built alongside.
    steps.push_str("[[step]]\nkind = \"repair\"\npart = \"US1\"\nwhen = \"overruled\"\nfiles = [{ path = \"calc/overruled.py\", text = \"YES = True\\n\" }]\noutput = { status = \"done\", task = \"US1\", files = [\"calc/overruled.py\"], summary = \"yes\" }\n\n");
    steps.push_str(&repair(part, &[("calc/fixed.py", "FIXED = True\n")]));
    steps.push_str(&f.build(&f.last, ""));
    steps.push_str(&handover_step(2, ""));
    steps.push_str(&check_step("US1", true, ""));
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", f.lead)).unwrap();
    let e = start_studio(&root, &scenario, None);
    let id = lock_calc_in(
        &e.tx,
        &f.repo,
        Some("builders"),
        Some((5.0, 60)),
        Some(fallow::run::events::Level::Free),
    );
    let tx = &e.tx;
    wait_for(tx, id, "q1 decided", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "decided"
    });
    ask(
        tx,
        Request::Overrule {
            id,
            q: "q1".into(),
            answer: "Yes".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let ev = events(&root, id);
    let gates = of_type(&ev, "gate_run");
    // The story's only repair is the overrule's, accepted before the merged group failed.
    let story = of(&v, "repair", "US1");
    assert_eq!(story.len(), 1, "{story:?}");
    assert!(text(story[0], "reason").starts_with("the engineer overruled"));
    let accepted = of_type(&ev, "verified")
        .iter()
        .find(|e| e["n"] == story[0]["n"] && e["accepted"] == true)
        .map(|e| e["seq"].as_u64().unwrap())
        .expect("the overrule repair accepted");
    // The merged group failed and was repaired as the group's.
    let failed = gates
        .iter()
        .find(|g| g["part"] == part && g["result"]["gate"] == "pair" && g["class"] != "pass")
        .unwrap_or_else(|| panic!("{gates:?}"));
    assert!(failed["seq"].as_u64().unwrap() > accepted, "{failed}");
    assert_eq!(of(&v, "repair", part).len(), 1);
    // No gate run of the story ever failed.
    // Its instruction never starts as the build of a task built alongside.
    let first = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", story[0]["n"])),
    )
    .unwrap();
    assert!(
        first.starts_with("Fallow step: repair (part US1)") && !first.contains("build (part T00"),
        "{first}"
    );
    let us1: Vec<&&Value> = gates.iter().filter(|g| g["part"] == "US1").collect();
    assert!(!us1.is_empty());
    assert!(us1.iter().all(|g| g["class"] == "pass"), "{us1:?}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
