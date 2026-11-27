//! 018 US2 (T012-T015; FR-004 to FR-010; contracts/pipeline.md "Points"): agents at every
//! point of a loop, one after another or side by side. A checker after each task checks that
//! task's commit and its finding is a repair of the task; a maker before each task runs first
//! and its commit is the builder's base; checkers alongside each other at a phase's end overlap
//! and are judged together; and the points follow `tasks.md` in a fixed order.

mod common;

use common::*;
use serde_json::Value;
use std::path::Path;

const LINT: &str = "---\nname: lint-checker\ndescription: Runs the linter on what a task changed\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nRun the linter on the task's changes.\n";

/// Standard (0.4.0's text) named `points`, with `extra` steps after its own.
fn pipeline(extra: &str) -> String {
    format!(
        "{}\n{}",
        PIPELINE_040_STANDARD.replace("name = \"standard\"", "name = \"points\""),
        extra.trim_start()
    )
}

fn setup(root: &Path, pipeline: &str) {
    let dir = root.join("config").join("pipelines");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("points.toml"), pipeline).unwrap();
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(agents.join("lint-checker.md"), LINT).unwrap();
}

/// A checker's findings at `part`: none, or one high finding saying `what`.
fn findings(part: &str, what: Option<&str>, extra: &str) -> String {
    let f = match what {
        None => String::new(),
        Some(w) => format!(
            "{{ severity = \"high\", where = \"calc/__init__.py\", ran = \"python -m lint calc\", result = {w:?} }}"
        ),
    };
    format!(
        "[[step]]\nkind = \"review\"\npart = {part:?}\n{extra}\noutput = {{ findings = [{f}], summary = \"checked\" }}\n\n"
    )
}

/// A maker's step at `part`, writing `file`.
fn make(part: &str, file: &str) -> String {
    format!(
        "[[step]]\nkind = \"make\"\npart = {part:?}\nfiles = [{{ path = {file:?}, text = \"{part}\\n\" }}]\noutput = {{ status = \"done\", task = \"maker\", files = [{file:?}], summary = \"made\" }}\n\n"
    )
}

fn repair(part: &str, files: &[(&str, &str)]) -> String {
    task_step(part, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
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

#[test]
fn a_checker_after_each_task_checks_its_commit_and_its_finding_is_a_task_repair() {
    let root = scratch("points-task-end");
    let pl = pipeline(
        "[[step]]\nagent = \"lint-checker\"\nwhen = \"task\"\nat = \"end\"\non_fail = \"repair\"\n",
    )
    .replace("task_repairs = 3", "task_repairs = 1");
    setup(&root, &pl);
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        findings("lint-checker@T001-end", None, ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        findings("lint-checker@T002-end", Some("unused import os"), ""),
        repair(
            "T002",
            &[("calc/__init__.py", &format!("{CALC_INIT}# lint\n"))],
        ),
        findings("lint-checker@T002-end", Some("line too long"), ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("points"));
    let v = wait_for(&e.tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.to_lowercase().contains("allow one more repair") && why.contains("line too long"),
        "{why}"
    );
    // T001's checker ran on T001's commit, after its gates and before the handover.
    let t1 = of(&v, "build", "T001")[0];
    let l1 = of(&v, "review", "lint-checker@T001-end");
    assert_eq!(l1.len(), 1, "{l1:?}");
    assert_eq!(l1[0]["base"], t1["verified_commit"], "{}", l1[0]);
    let gates = v["gate_runs"].as_array().unwrap();
    assert!(
        gates
            .iter()
            .any(|g| g["part"] == "T001" && g["result"]["commit"] == t1["verified_commit"]),
        "T001 was gated: {gates:?}"
    );
    assert!(l1[0]["n"].as_u64() < of(&v, "handover", "phase-1")[0]["n"].as_u64());
    // T002's: on T002's accepted commit, queued after T002's build; T003 never queued.
    let t2 = of(&v, "build", "T002")[0];
    let l2 = of(&v, "review", "lint-checker@T002-end");
    assert_eq!(l2.len(), 2, "{l2:?}");
    assert_eq!(l2[0]["base"], t2["verified_commit"], "{}", l2[0]);
    assert!(l2[0]["n"].as_u64() > t2["n"].as_u64());
    assert!(of(&v, "build", "T003").is_empty(), "T003 waits for T002");
    // The finding is a repair of the task itself, counted against its repairs.
    let r = of(&v, "repair", "T002");
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0]["task"], "T002");
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", r[0]["n"])),
    )
    .unwrap();
    assert!(prompt.contains("unused import os"), "{prompt}");
    // The repair was gated before the checker looked again, at the repaired commit.
    assert!(
        gates
            .iter()
            .any(|g| g["part"] == "T002" && g["result"]["commit"] == r[0]["verified_commit"]),
        "the repair was gated: {gates:?}"
    );
    assert_eq!(l2[1]["base"], r[0]["verified_commit"], "{}", l2[1]);
    let t = &v["feature_loop"]["tasks"]["T002"];
    assert_eq!(t["repairs"], 1, "{t}");
    assert_ne!(t["state"], "accepted", "{t}");
    // The loop view (T020): every step's point, task steps included, and the builders' cap.
    let view = ask(&e.tx, fallow::run::engine::Request::Loop { id }).unwrap();
    let pl = &view["pipeline"];
    assert_eq!(pl["caps"]["builders"], 1, "{pl}");
    let at: Vec<(&str, &str)> = pl["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["agent"].as_str().unwrap(), s["at"].as_str().unwrap()))
        .collect();
    assert_eq!(
        at,
        [
            ("builder", "end"),
            ("fallow:gates", "end"),
            ("reviewer", "end"),
            ("lint-checker", "end")
        ]
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_maker_before_each_task_runs_first() {
    let root = scratch("points-task-start");
    setup(
        &root,
        &pipeline(
            "[[step]]\nagent = \"test-writer\"\nwhen = \"task\"\nat = \"start\"\non_fail = \"engineer\"\n",
        ),
    );
    let tasks = ["T001", "T002", "T003", "T004"];
    let mut steps = String::new();
    for t in tasks {
        steps.push_str(&make(
            &format!("test-writer@{t}-start"),
            &format!("notes/{t}.md"),
        ));
    }
    steps.push_str(&calc_loop_ok());
    let (e, _repo, id) = locked_calc(&root, &steps, Some("points"));
    let v = wait_for(&e.tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let gates = v["gate_runs"].as_array().unwrap();
    for t in tasks {
        let part = format!("test-writer@{t}-start");
        let m = of(&v, "make", &part);
        assert_eq!(m.len(), 1, "{t}: {m:?}");
        let b = of(&v, "build", t);
        assert_eq!(b.len(), 1, "{t}: {b:?}");
        assert!(
            m[0]["n"].as_u64() < b[0]["n"].as_u64(),
            "{t}: the maker first"
        );
        let c = &m[0]["verified_commit"];
        assert!(c.is_string(), "{t}: {}", m[0]);
        assert_eq!(
            &b[0]["base"], c,
            "{t}: the builder starts from the maker's commit"
        );
        assert!(
            gates.iter().any(|g| g["kind"] == "baseline"
                && g["part"] == part
                && &g["result"]["commit"] == c),
            "{t}: the maker's commit is a baseline: {gates:?}"
        );
    }
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn checkers_alongside_each_other_at_a_phase_end_overlap_and_are_judged_together() {
    let root = scratch("points-phase-end");
    let pl = pipeline(
        "[[step]]\nagent = \"reviewer\"\nwhen = \"phase\"\nat = \"end\"\non_fail = \"repair\"\nparallel_with = [\"security-reviewer\"]\n\n[[step]]\nagent = \"security-reviewer\"\nwhen = \"phase\"\nat = \"end\"\non_fail = \"repair\"\n",
    )
    .replace("task_repairs = 3", "task_repairs = 1");
    setup(&root, &pl);
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        findings(
            "reviewer@phase-1-end",
            Some("the reviewer saw no docstring"),
            "sleep_ms = 2000",
        ),
        findings(
            "security-reviewer@phase-1-end",
            Some("the security reviewer saw eval"),
            "sleep_ms = 2000",
        ),
        repair(
            "phase-1-end",
            &[("calc/__init__.py", "\"\"\"Calc.\"\"\"\n")],
        ),
        findings(
            "reviewer@phase-1-end",
            Some("the reviewer still saw no tests"),
            "",
        ),
        findings("security-reviewer@phase-1-end", None, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, Some("points"));
    let v = wait_for(&e.tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.contains("phase-1-end") && why.contains("still saw no tests"),
        "{why}"
    );
    let rv = of(&v, "review", "reviewer@phase-1-end");
    let sec = of(&v, "review", "security-reviewer@phase-1-end");
    assert_eq!(rv.len(), 2, "both ran again after the repair: {rv:?}");
    assert_eq!(sec.len(), 2, "both ran again after the repair: {sec:?}");
    let (a, b) = (rv[0], sec[0]);
    assert!(
        text(a, "started") < text(b, "finished") && text(b, "started") < text(a, "finished"),
        "they overlap in time: {a} {b}"
    );
    // Both checked the same commit.
    assert_eq!(a["base"], b["base"]);
    let r = of(&v, "repair", "phase-1-end");
    assert_eq!(r.len(), 1, "one repair for both: {r:?}");
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", r[0]["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("no docstring") && prompt.contains("saw eval"),
        "the repair holds both checkers' findings: {prompt}"
    );
    // Round 2 started after the repair, both at once on the repaired commit.
    assert!(rv[1]["n"].as_u64() > r[0]["n"].as_u64());
    assert!(sec[1]["n"].as_u64() > r[0]["n"].as_u64());
    assert_eq!(rv[1]["base"], sec[1]["base"]);
    // One failing and one passing: the phase does not end.
    let p1 = v["feature_loop"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["spec"]["n"] == 1)
        .unwrap();
    assert_ne!(p1["state"], "done", "{p1}");
    assert!(of(&v, "build", "T002").is_empty(), "phase 2 waits");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_order_of_points_follows_tasks_md() {
    let root = scratch("points-order");
    // The checkpoint's maker is listed before the phase's checker: the points' order wins.
    let pl = pipeline(
        r#"[[step]]
agent = "docs-writer"
when = "once"
at = "start"
on_fail = "engineer"

[[step]]
agent = "test-writer"
when = "checkpoint"
at = "start"
on_fail = "engineer"

[[step]]
agent = "reviewer"
when = "phase"
at = "start"
on_fail = "engineer"

[[step]]
agent = "docs-writer"
when = "task"
at = "start"
on_fail = "engineer"

[[step]]
agent = "lint-checker"
when = "task"
at = "end"
on_fail = "repair"

[[step]]
agent = "security-reviewer"
when = "phase"
at = "end"
on_fail = "repair"

[[step]]
agent = "docs-writer"
when = "once"
at = "end"
on_fail = "engineer"
"#,
    );
    setup(&root, &pl);
    let mut expected: Vec<(&str, String)> = vec![("make", "docs-writer@once-start".into())];
    let mut steps = make("docs-writer@once-start", "notes/once-start.md");
    let phases: [(u32, Option<&str>, &[&str]); 3] = [
        (1, None, &["T001"]),
        (2, Some("US1"), &["T002", "T003"]),
        (3, Some("US2"), &["T004"]),
    ];
    let builds = [
        ("T001", vec![("calc/__init__.py", "")]),
        ("T002", vec![("calc/__init__.py", CALC_INIT)]),
        (
            "T003",
            vec![("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
        ),
        (
            "T004",
            vec![
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
        ),
    ];
    for (n, story, tasks) in phases {
        let ps = format!("reviewer@phase-{n}-start");
        steps.push_str(&findings(&ps, None, ""));
        expected.push(("review", ps));
        if story.is_some() {
            let cs = format!("test-writer@phase-{n}-start");
            steps.push_str(&make(&cs, &format!("notes/story-{n}.md")));
            expected.push(("make", cs));
        }
        for t in tasks {
            let ts = format!("docs-writer@{t}-start");
            steps.push_str(&make(&ts, &format!("notes/{t}.md")));
            expected.push(("make", ts));
            let files = &builds.iter().find(|(x, _)| x == t).unwrap().1;
            steps.push_str(&task_step(t, files, ""));
            expected.push(("build", t.to_string()));
            let te = format!("lint-checker@{t}-end");
            steps.push_str(&findings(&te, None, ""));
            expected.push(("review", te));
        }
        steps.push_str(&handover_step(n, ""));
        expected.push(("handover", format!("phase-{n}")));
        let pe = format!("security-reviewer@phase-{n}-end");
        steps.push_str(&findings(&pe, None, ""));
        expected.push(("review", pe));
        if let Some(s) = story {
            steps.push_str(&check_step(s, true, ""));
            expected.push(("review", s.to_string()));
        }
    }
    steps.push_str(&make("docs-writer@once-end", "notes/once-end.md"));
    expected.push(("make", "docs-writer@once-end".into()));
    let (e, _repo, id) = locked_calc(&root, &steps, Some("points"));
    let v = wait_for(&e.tx, id, "ready", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);
    let got: Vec<(String, String)> = in_order(&v)
        .iter()
        .map(|s| (text(s, "kind"), text(s, "part")))
        .collect();
    let want: Vec<(String, String)> = expected
        .into_iter()
        .map(|(k, p)| (k.to_string(), p))
        .collect();
    assert_eq!(got, want);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
