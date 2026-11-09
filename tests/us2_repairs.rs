//! 007 US2 (T041; FR-013): a task whose gates fail is repaired in its phase's session, and after
//! the cap the loop asks the engineer; a story whose review fails is repaired, gated, and
//! checked again.

mod common;

use common::*;
use serde_json::Value;

const ADD_WRONG: &str = "def add(a, b):\n    if not isinstance(a, int) or not isinstance(b, int):\n        raise TypeError('ints only')\n    return a - b\n";

fn repair_step(part: &str, files: &[(&str, &str)]) -> String {
    task_step(part, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
}

fn steps_of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect()
}

#[test]
fn failing_gates_are_repaired_in_the_session_and_the_cap_asks_the_engineer() {
    let root = scratch("repairs");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        // T002 ships a wrong add() with a test that catches it: the gates fail.
        task_step("T002", &[("calc/__init__.py", ADD_WRONG), ("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        repair_step("T002", &[("calc/__init__.py", CALC_INIT)]),
        // T003 keeps failing: its build and three repairs all leave a failing test.
        task_step("T003", &[("tests/test_more.py", "import unittest\n\n\nclass More(unittest.TestCase):\n    def test_x(self):\n        self.assertEqual(1, 2)\n")], ""),
        repair_step("T003", &[("tests/note1.txt", "1")]),
        repair_step("T003", &[("tests/note2.txt", "2")]),
        repair_step("T003", &[("tests/note3.txt", "3")]),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the engineer asked", |v| !v["needs"].is_null());
    let lp = &v["feature_loop"];
    // T002: one repair, resumed in the phase's session, then accepted.
    assert_eq!(lp["tasks"]["T002"]["state"], "accepted");
    assert_eq!(lp["tasks"]["T002"]["repairs"], 1);
    let b = steps_of(&v, "build", "T002");
    let r = steps_of(&v, "repair", "T002");
    assert_eq!(
        r[0]["resume"], b[0]["session"],
        "the repair resumed the phase's session"
    );
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", r[0]["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("gate tests"),
        "the reasons go to the builder: {prompt}"
    );
    // T003: three repairs, then the engineer.
    assert_eq!(steps_of(&v, "repair", "T003").len(), 3);
    assert_eq!(lp["tasks"]["T003"]["state"], "failed");
    let why = v["needs"]["why"].as_str().unwrap();
    assert!(
        why.contains("T003 still fails after 3 repair round(s)"),
        "{why}"
    );
    assert_eq!(v["needs_engineer"].as_str().unwrap(), why);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failing_review_is_repaired_gated_and_checked_again() {
    let root = scratch("review-repair");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", false, ""),
        repair_step(
            "US1",
            &[("calc/__init__.py", &format!("{CALC_INIT}# fixed\n"))],
        ),
        check_step("US1", true, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", &format!("{CALC_INIT_SUB}# fixed\n")),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let reviews = steps_of(&v, "review", "US1");
    assert_eq!(reviews.len(), 2, "checked again after the repair");
    assert_eq!(reviews[0]["verified"][0], false);
    assert_eq!(reviews[1]["verified"][0], true);
    let repairs = steps_of(&v, "repair", "US1");
    assert_eq!(repairs.len(), 1);
    let phase2 = steps_of(&v, "build", "T002")[0]["session"].clone();
    assert_eq!(repairs[0]["resume"], phase2, "in the phase's session");
    let gates = v["gate_runs"].as_array().unwrap();
    assert!(
        gates.iter().any(|g| g["part"] == "US1"),
        "the story's repair was gated"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_engineers_options_let_the_loop_go_on() {
    use fallow::run::engine::Request;
    let root = scratch("options");
    let note = |n: &str| repair_step("T002", &[(&*format!("tests/note{n}.txt"), n)]);
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", ADD_WRONG), ("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        note("1"),
        note("2"),
        note("3"),
        // "Allow one more repair": exactly one more, which fails too.
        note("4"),
        // And once more: this one fixes it.
        repair_step("T002", &[("calc/__init__.py", CALC_INIT)]),
        task_step("T003", &[("tests/test_add2.py", "")], ""),
        handover_step(2, ""),
        check_step("US1", true, ""),
        // T004's builder asks a question, then goes on with the answer in its session.
        "[[step]]\nkind = \"build\"\npart = \"T004\"\noutput = { status = \"blocked\", task = \"T004\", question = \"Should sub() accept floats?\", summary = \"asking\" }\n\n".to_string(),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], "when = \"ints only, please\""),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, _repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the cap", |v| !v["needs"].is_null());
    assert!(
        v["needs"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "allow one more repair"),
        "{}",
        v["needs"]
    );
    ask(tx, Request::Retry { id }).unwrap();
    let v = wait_for(tx, id, "the cap again", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("T002"))
            && steps_of(v, "repair", "T002").len() == 4
    });
    assert!(
        v["needs"]["why"].as_str().unwrap().contains("still fails"),
        "{}",
        v["needs"]
    );
    ask(tx, Request::Retry { id }).unwrap();
    let v = wait_for(tx, id, "the question", |v| {
        v["needs"]["why"]
            .as_str()
            .is_some_and(|w| w.contains("floats"))
    });
    assert_eq!(v["feature_loop"]["tasks"]["T002"]["state"], "accepted");
    assert_eq!(
        steps_of(&v, "repair", "T002").len(),
        5,
        "one more repair each time it was allowed"
    );
    ask(
        tx,
        Request::Answer {
            id,
            text: "No, ints only, please.".into(),
        },
    )
    .unwrap();
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let b = steps_of(&v, "build", "T004");
    assert_eq!(b.len(), 2);
    assert_eq!(
        b[1]["resume"], b[0]["session"],
        "the answer goes to the asking session"
    );
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 2026-11-02: the builder's program could not be started (Claude Code updating itself: npm's
/// shim was there, its `.exe` not, and the shim refused the agent's arguments). A step whose
/// agent never started is no attempt at the task: it uses no repair round and goes again as
/// the same step, with its instruction; after three such starts in a row the engineer is
/// asked, and "retry" starts it.
#[cfg(windows)]
#[test]
fn an_agent_that_could_not_be_started_uses_no_repair_and_goes_again_as_the_same_step() {
    use fallow::run::engine::Request;
    let root = scratch("not-started");
    // The builder runs on the fake runtime, through a batch file that cannot take its
    // arguments: Rust refuses to start it ("batch file arguments are invalid").
    let shims = root.join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let shim = shims.join("agent.cmd");
    std::fs::write(&shim, "@echo off\r\nexit /b 1\r\n").unwrap();
    let agents = root.join("config").join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    let shipped = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "builder")
        .unwrap()
        .1;
    std::fs::write(
        agents.join("builder.md"),
        shipped.replace("runtime: claude", "runtime: fake"),
    )
    .unwrap();
    std::fs::write(
        root.join("settings.extra.toml"),
        format!(
            "[agents.fake]\ncommand = {:?}\nargs = [\"--scenario\", {:?}]\n",
            shim.display().to_string(),
            root.join("scenario.toml").display().to_string()
        ),
    )
    .unwrap();
    let (e, _repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the engineer asked", |v| !v["needs"].is_null());
    let why = v["needs"]["why"].as_str().unwrap();
    assert!(
        why.contains("builder could not be started 3 times in a row")
            && why.contains("batch file arguments are invalid"),
        "{why}"
    );
    let b = steps_of(&v, "build", "T001");
    // Three starts that failed, and the fourth queued for the retry; no repair at all.
    assert_eq!(b.len(), 4, "{b:?}");
    assert!(steps_of(&v, "repair", "T001").is_empty(), "no repair round");
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["repairs"], 0);
    let prompt = |s: &Value| {
        std::fs::read_to_string(
            root.join("data/runs")
                .join(id.to_string())
                .join("steps")
                .join(format!("{}.prompt", s["n"])),
        )
        .unwrap_or_default()
    };
    let first = prompt(b[0]);
    assert!(first.contains("T001"), "{first}");
    // The program is back (the update is done): npm's shim and the `.exe` it starts.
    let bin = shims.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_fallow-fake-agent"),
        bin.join("agent.exe"),
    )
    .unwrap();
    std::fs::write(
        &shim,
        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\bin\\agent.exe\"   %*\r\n",
    )
    .unwrap();
    ask(tx, Request::Retry { id }).unwrap();
    let v = wait_for(tx, id, "T001 accepted", |v| {
        v["feature_loop"]["tasks"]["T001"]["state"] == "accepted"
    });
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["repairs"], 0);
    assert!(steps_of(&v, "repair", "T001").is_empty(), "no repair round");
    let b = steps_of(&v, "build", "T001");
    assert_eq!(b.len(), 4, "{b:?}");
    // Every start was given the task's own instruction, never a repair of a failure.
    for s in &b {
        assert_eq!(prompt(s), first, "step {}", s["n"]);
    }
    // The activity says so, and shows no failure of the task.
    let a = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: fallow::run::activity::Filter::default(),
        },
    )
    .unwrap()
    .to_string();
    assert_eq!(
        a.matches("build T001 did not start, it goes again").count(),
        3,
        "{a}"
    );
    assert!(!a.contains("T001 not accepted"), "{a}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 2026-11-09: a loop spent its four repair rounds on "Failed to authenticate: OAuth session
/// expired" (the engineer's `claude` login had run out). An agent that is not signed in did no
/// work that counts: no repair round, the engineer is asked at once to sign in, and "retry"
/// runs the same step with its own instruction.
#[test]
fn an_agent_that_is_not_signed_in_uses_no_repair_and_the_engineer_is_asked_to_sign_in() {
    use fallow::run::engine::Request;
    let root = scratch("signed-out");
    let signed_out = "[[step]]\nkind = \"build\"\npart = \"T001\"\nis_error = true\nresult = \"Failed to authenticate: OAuth session expired and could not be refreshed\"\n\n";
    let (e, _repo, id) = locked_calc(&root, &format!("{signed_out}{}", calc_loop_ok()), None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the engineer asked", |v| !v["needs"].is_null());
    let why = v["needs"]["why"].as_str().unwrap();
    assert!(why.contains("is not signed in") && why.contains("OAuth session expired") && why.contains("/login"), "{why}");
    assert!(!why.contains("repair"), "{why}");
    assert!(steps_of(&v, "repair", "T001").is_empty(), "no repair round");
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["repairs"], 0);
    let b = steps_of(&v, "build", "T001");
    assert_eq!(b.len(), 2, "the failed start and the one queued for the retry: {b:?}");
    // Signed in again: the retry runs the queued step, and the task is accepted unrepaired.
    ask(tx, Request::Retry { id }).unwrap();
    let v = wait_for(tx, id, "T001 accepted", |v| v["feature_loop"]["tasks"]["T001"]["state"] == "accepted");
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["repairs"], 0);
    assert!(steps_of(&v, "repair", "T001").is_empty(), "no repair round");
    stop(e);
    let _ = std::fs::remove_dir_all(&root);
}
