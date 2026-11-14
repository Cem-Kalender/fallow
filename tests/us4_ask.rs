//! 007 US4 (FR-019 to FR-023; research S-10, contracts/mcp-tools.md): questions go through the
//! lead. An agent's `ask` is answered by the lead when the documents settle it, decided by the
//! lead only at the Free level, and relayed to the engineer otherwise; every answer is committed
//! to the spec's Clarifications and reaches the build branch at the next task boundary; a
//! relayed question makes the loop wait, and the step resumes with the answer; overruling
//! reopens the work built on the old answer and runs the checkpoint again.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::run::events::Level;
use serde_json::Value;

const FLOATS: &str = "Should add() accept floats?";
const BIG: &str = "What should add() do with numbers above a billion?";

fn lead_answer(q: &str, outcome: &str, answer: &str, basis: &str, extra: &str) -> String {
    format!(
        "[[step]]\nkind = \"lead_answer\"\npart = {q:?}\n{extra}\noutput = {{ outcome = {outcome:?}, answer = {answer:?}, basis = {basis:?}, reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false, options = [{{ label = \"refuse them\", recommended = true }}, {{ label = \"add them\" }}] }}\n\n"
    )
}

/// A builder step that asks `question` (and `also` right after), returning `status`.
fn asking_step(
    task: &str,
    question: &str,
    also: Option<&str>,
    files: &[(&str, &str)],
    status: &str,
) -> String {
    let also = also
        .map(|a| format!("also_ask = {a:?}\n"))
        .unwrap_or_default();
    task_step(task, files, &format!("ask = {question:?}\n{also}"))
        .replacen(
            "status = \"done\"",
            &format!("status = {status:?}, question = \"$ANSWER\""),
            1,
        )
        .replacen("summary = \"did", "summary = \"$ANSWER2 did", 1)
}

fn question<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["feature_loop"]["questions"]
        .get(id)
        .unwrap_or_else(|| panic!("no question {id}: {}", v["feature_loop"]["questions"]))
}

fn steps_of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    let mut s: Vec<&Value> = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect();
    s.sort_by_key(|s| s["n"].as_u64());
    s
}

fn spec_on_feature_branch(repo: &std::path::Path) -> String {
    git(repo, &["show", "001-calc:specs/001-calc/spec.md"])
}

fn lock_level(
    root: &std::path::Path,
    steps: &str,
    level: Level,
) -> (fallow::service::ToolEngine, std::path::PathBuf, u64) {
    locked_calc_level(root, steps, level)
}

#[test]
fn the_lead_answers_what_the_spec_settles_and_relays_the_rest() {
    let root = scratch("ask-normal");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        // Settled by the spec's TypeError scenario: answered, committed, and T002 goes on.
        asking_step(
            "T002",
            FLOATS,
            None,
            &[("calc/__init__.py", CALC_INIT)],
            "done",
        ),
        lead_answer(
            "q1",
            "answer",
            "No: ints only, as US1's TypeError scenario says",
            "spec.md US1 scenario 2",
            "",
        ),
        // Not settled: relayed; a second question while it waits is refused; the step ends
        // blocked and the loop waits.
        asking_step(
            "T003",
            BIG,
            Some("And floats?"),
            &[("tests/__init__.py", "")],
            "blocked",
        ),
        // A real lead explains its relay in `answer` (T086): reasoning, not an option.
        lead_answer(
            "q2",
            "relay",
            "Nothing settles it; I recommend refusing.",
            "none",
            "",
        ),
        // Resumed in its session with the engineer's answer.
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "when = \"The engineer answers\"",
        ),
        handover_step(2, ""),
        check_step("US1", true, ""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, repo, id) = lock_level(&root, &steps, Level::Normal);
    let tx = &e.tx;

    let v = wait_for(tx, id, "the relayed question", |v| {
        v["feature_loop"]["questions"]["q2"]["state"] == "waiting_engineer" && !v["needs"].is_null()
    });
    // q1: the lead's answer, with its basis, committed to the feature branch's Clarifications.
    let q1 = question(&v, "q1");
    assert_eq!(q1["state"], "answered", "{q1}");
    assert_eq!(q1["by"], "lead");
    let c1 = q1["commit"].as_str().expect("q1's commit");
    let spec = git(&repo, &["show", &format!("{c1}:specs/001-calc/spec.md")]);
    assert!(
        spec.contains(&format!(
            "- Q (T002, builder): {FLOATS} → A (lead): No: ints only"
        )),
        "{spec}"
    );
    // T002 got the answer within its step and was accepted.
    assert_eq!(v["feature_loop"]["tasks"]["T002"]["state"], "accepted");
    // q2: relayed with options and the lead's recommendation; the loop waits for it.
    let q2 = question(&v, "q2");
    assert_eq!(q2["recommended"], "refuse them", "{q2}");
    assert!(
        q2["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "add them")
    );
    assert_eq!(
        q2["options"],
        serde_json::json!(["refuse them", "add them"]),
        "the lead's reasoning is no option: {q2}"
    );
    assert!(
        v["needs"]["why"].as_str().unwrap().contains(BIG),
        "{}",
        v["needs"]
    );
    let t003 = steps_of(&v, "build", "T003");
    assert_eq!(t003.len(), 1, "nothing more of T003 runs while it waits");
    // The second ask while q2 waited was refused (one open question per step).
    assert!(v["feature_loop"]["questions"].get("q3").is_none());
    // The agent asks again only once its first ask has its reply, so the refusal can come after
    // q2 is relayed (issue #4): read the step's activity until it shows, within a minute.
    let n = t003[0]["n"].as_u64().unwrap() as u32;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let result = loop {
        let result = ask(tx, Request::Activity { id, n }).unwrap();
        if result.to_string().contains("already have a question open")
            || std::time::Instant::now() > end
        {
            break result;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert!(
        result.to_string().contains("already have a question open"),
        "the second ask is refused: {result}"
    );
    std::thread::sleep(std::time::Duration::from_secs(2));
    let still = ask(tx, Request::Get { id }).unwrap();
    assert_eq!(
        still["steps"].as_object().unwrap().len(),
        v["steps"].as_object().unwrap().len(),
        "the loop waits"
    );

    // The engineer answers: committed, and T003 resumes in its session with it.
    let a = ask(
        tx,
        Request::QuestionAnswer {
            id,
            q: "q2".into(),
            answer: "Refuse them with a ValueError".into(),
        },
    )
    .unwrap();
    assert!(a["commit"].is_string());
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let q2 = question(&v, "q2");
    assert_eq!(q2["state"], "answered");
    assert_eq!(q2["by"], "engineer");
    let t003 = steps_of(&v, "build", "T003");
    assert_eq!(t003.len(), 2);
    assert_eq!(t003[1]["resume"], t003[0]["session"], "the same session");
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", t003[1]["n"])),
    )
    .unwrap();
    assert!(prompt.contains("Refuse them with a ValueError"), "{prompt}");
    // Both answers are on the feature branch, and reached the build branch: they are in what
    // is merged, which Fallow accepted.
    let spec = spec_on_feature_branch(&repo);
    assert!(
        spec.contains("→ A (engineer): Refuse them with a ValueError"),
        "{spec}"
    );
    let merged = git(
        &repo,
        &[
            "show",
            &format!(
                "{}:specs/001-calc/spec.md",
                v["integrated"].as_str().unwrap()
            ),
        ],
    );
    assert!(
        merged.contains("A (lead): No: ints only") && merged.contains("A (engineer): Refuse"),
        "{merged}"
    );
    assert_eq!(
        v["feature_loop"]["outcome"][0], true,
        "{}",
        v["feature_loop"]["outcome"]
    );
    // The feed shows who answered, with the commit.
    let feed = ask(
        tx,
        Request::LoopActivity {
            id,
            filter: Default::default(),
        },
    )
    .unwrap();
    let entries = feed["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["kind"] == "answer"
        && e["agent"] == "lead"
        && e["text"].as_str().unwrap().starts_with("lead answered q1")
        && e["commit"].is_string()));
    assert!(entries.iter().any(|e| {
        e["kind"] == "answer"
            && e["agent"] == "lead"
            && e["text"]
                .as_str()
                .unwrap()
                .starts_with("relayed to you (q2)")
    }));
    assert!(entries.iter().any(|e| e["kind"] == "answer"
        && e["agent"] == "you"
        && e["text"].as_str().unwrap().contains("Refuse them")));
    assert!(
        entries
            .iter()
            .any(|e| e["kind"] == "ask" && e["text"] == FLOATS),
        "the agent's question"
    );

    // T086: unlocked, with only User Story 2 changed. The answers the loop committed to the
    // Clarifications are no change of the engineer's: only US2 is affected.
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
    let path = "specs/001-calc/spec.md".to_string();
    let doc = sreq(
        tx,
        fallow::studio::Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: path.clone(),
        },
    )
    .unwrap();
    let text = doc["text"].as_str().unwrap();
    assert!(text.contains("Refuse them"), "the session has the answers");
    sreq(
        tx,
        fallow::studio::Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path,
            text: text.replace("the result is 2.", "the result is the int 2."),
            version: doc["version"].as_str().unwrap().to_string(),
        },
    )
    .unwrap();
    let p = ask(tx, Request::RelockPreview { project, feature }).unwrap();
    assert_eq!(p["affected"], serde_json::json!(["US2"]), "{p}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn free_lets_the_lead_decide_and_overruling_reopens_the_story() {
    let root = scratch("ask-free");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        asking_step("T002", FLOATS, None, &[("calc/__init__.py", CALC_INIT)], "done"),
        lead_answer("q1", "decided", "No floats", "none", ""),
        task_step("T003", &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)], ""),
        handover_step(2, ""),
        check_step("US1", true, ""),
        task_step("T004", &[("calc/__init__.py", CALC_INIT_SUB), ("tests/test_sub.py", TEST_SUB)], ""),
        handover_step(3, ""),
        check_step("US2", true, ""),
        // After the overrule: the story's repair in phase 2's session, and its review again.
        "[[step]]\nkind = \"repair\"\npart = \"US1\"\nwhen = \"overruled\"\nfiles = [{ path = \"tests/test_float.py\", text = \"\" }]\noutput = { status = \"done\", task = \"US1\", files = [\"tests/test_float.py\"], summary = \"floats\" }\n\n".to_string(),
        check_step("US1", true, ""),
    ]
    .concat();
    let (e, repo, id) = lock_level(&root, &steps, Level::Free);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let q1 = question(&v, "q1");
    assert_eq!(q1["state"], "decided", "{q1}");
    assert!(spec_on_feature_branch(&repo).contains("→ A (lead, decided): No floats"));
    // A decision never blocks the loop; the page lists it to confirm or overrule.
    let view = ask(tx, Request::Loop { id }).unwrap();
    let qs = view["questions"].as_array().unwrap();
    assert_eq!(qs[0]["overrulable"], true, "{qs:?}");
    assert_eq!(qs[0]["confirmable"], true, "{qs:?}");

    let o = ask(
        tx,
        Request::Overrule {
            id,
            q: "q1".into(),
            answer: "Accept floats too".into(),
        },
    )
    .unwrap();
    assert_eq!(o["reopened"], serde_json::json!(["T002", "T003"]), "{o}");
    assert!(
        ask(
            tx,
            Request::Overrule {
                id,
                q: "q1".into(),
                answer: "again".into()
            }
        )
        .is_err(),
        "only the lead's answers are overruled"
    );
    assert!(
        ask(tx, Request::QuestionConfirm { id, q: "q1".into() }).is_err(),
        "only a decision still standing is confirmed"
    );
    let v = wait_for(tx, id, "ready again", |v| {
        v["stage"] == "ready" && steps_of(v, "review", "US1").len() == 2
    });
    let q1 = question(&v, "q1");
    assert_eq!(q1["state"], "overruled");
    let repairs = steps_of(&v, "repair", "US1");
    assert_eq!(repairs.len(), 1, "one repair of the story");
    assert!(
        repairs[0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("the engineer overruled")
    );
    // In phase 2's builder session.
    let phase2 = steps_of(&v, "build", "T003")[0]["session"].clone();
    assert_eq!(repairs[0]["resume"], phase2);
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", repairs[0]["n"])),
    )
    .unwrap();
    assert!(
        prompt.contains("Accept floats too")
            && prompt.contains("No floats")
            && prompt.contains("T002, T003"),
        "{prompt}"
    );
    // The story's review ran again after the repair; the reversal is committed and merged.
    let reviews = steps_of(&v, "review", "US1");
    assert!(reviews[1]["n"].as_u64() > repairs[0]["n"].as_u64());
    assert!(
        spec_on_feature_branch(&repo)
            .contains("A (engineer, overruling the lead): Accept floats too")
    );
    let merged = git(
        &repo,
        &[
            "show",
            &format!(
                "{}:specs/001-calc/spec.md",
                v["integrated"].as_str().unwrap()
            ),
        ],
    );
    assert!(merged.contains("overruling the lead"), "{merged}");
    assert_eq!(v["feature_loop"]["outcome"][0], true);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn strict_relays_every_answer_and_a_slow_lead_is_relayed() {
    // Strict: even an answer with a basis goes to the engineer.
    let root = scratch("ask-strict");
    let steps = [
        // The builder carries on although its question waits: the loop waits after the step.
        asking_step("T001", FLOATS, None, &[("calc/__init__.py", "")], "done"),
        lead_answer("q1", "answer", "No", "spec.md US1", ""),
    ]
    .concat();
    let (e, _repo, id) = lock_level(&root, &steps, Level::Strict);
    let v = wait_for(&e.tx, id, "the relayed question", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer" && !v["needs"].is_null()
    });
    assert_eq!(question(&v, "q1")["recommended"], "refuse them");
    assert_eq!(v["feature_loop"]["tasks"]["T001"]["state"], "waiting");
    let n = v["steps"].as_object().unwrap().len();
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(
        ask(&e.tx, Request::Get { id }).unwrap()["steps"]
            .as_object()
            .unwrap()
            .len(),
        n,
        "the loop waits"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // Normal, but the lead takes longer than its limit: relayed.
    let root = scratch("ask-slow");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nlead_answer_sec = 2\n",
    )
    .unwrap();
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        asking_step(
            "T002",
            FLOATS,
            None,
            &[("calc/__init__.py", CALC_INIT)],
            "blocked",
        ),
        lead_answer("q1", "answer", "No", "spec.md US1", "sleep_ms = 8000"),
    ]
    .concat();
    let (e, _repo, id) = lock_level(&root, &steps, Level::Normal);
    let v = wait_for(&e.tx, id, "the relayed question", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer" && !v["needs"].is_null()
    });
    assert!(question(&v, "q1")["answer"].is_null());
    assert_eq!(v["feature_loop"]["tasks"]["T002"]["state"], "waiting");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

fn restart(e: fallow::service::ToolEngine, root: &std::path::Path) -> fallow::service::ToolEngine {
    e.tx.send(fallow::run::engine::Msg::Shutdown).unwrap();
    let fallow::service::ToolEngine { handle, .. } = e;
    handle.join().unwrap();
    start_studio(root, &root.join("scenario.toml"), None)
}

fn prompt_of(root: &std::path::Path, id: u64, step: &Value) -> String {
    std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", step["n"])),
    )
    .unwrap()
}

#[test]
fn every_answer_reaches_the_step_that_asked_whenever_it_comes() {
    // Strict: every question goes to the engineer.
    let root = scratch("ask-timing");
    const CHECK: &str = "Is add(2, 3) the right check for US1?";
    let steps = [
        // The builder asks, then carries on for a while and ends done: its step still ends
        // blocked on the question, and the answer, given while it ran, reaches it.
        task_step(
            "T001",
            &[("calc/__init__.py", "")],
            &format!("ask = {FLOATS:?}\nsleep_ms = 6000"),
        ),
        lead_answer("q1", "answer", "No", "spec.md US1", ""),
        task_step(
            "T001",
            &[("calc/__init__.py", "")],
            "when = \"The engineer answers\"",
        ),
        handover_step(1, ""),
        task_step("T002", &[("calc/__init__.py", CALC_INIT)], ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        // A checker asks; Fallow stops while its question waits; the answer, given after the
        // restart, reaches the checker in its session.
        check_step("US1", true, &format!("ask = {CHECK:?}\nsleep_ms = 30000")),
        lead_answer("q2", "relay", "", "none", ""),
        check_step("US1", true, "when = \"The engineer answers\""),
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let (e, repo, id) = lock_level(&root, &steps, Level::Strict);
    let v = wait_for(&e.tx, id, "q1 relayed", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
    });
    let t001 = steps_of(&v, "build", "T001");
    assert!(t001[0]["end"].is_null(), "the step still runs: {}", t001[0]);
    ask(
        &e.tx,
        Request::QuestionAnswer {
            id,
            q: "q1".into(),
            answer: "Ints only".into(),
        },
    )
    .unwrap();

    let v = wait_for(&e.tx, id, "q2 relayed", |v| {
        v["feature_loop"]["questions"]["q2"]["state"] == "waiting_engineer"
    });
    let t001 = steps_of(&v, "build", "T001");
    assert_eq!(t001.len(), 2, "T001 ran again with the answer");
    assert!(
        t001[0]["verified"][1][0]
            .as_str()
            .unwrap()
            .starts_with("blocked: "),
        "{}",
        t001[0]
    );
    assert_eq!(t001[1]["resume"], t001[0]["session"]);
    assert!(prompt_of(&root, id, t001[1]).contains("Ints only"));
    // The checker saw q1's answer: the build took it in before the check.
    let check = &steps_of(&v, "review", "US1")[0];
    let base = check["base"].as_str().unwrap();
    assert!(
        git(&repo, &["show", &format!("{base}:specs/001-calc/spec.md")])
            .contains("A (engineer): Ints only")
    );
    assert!(
        question(&v, "q2")["why"]
            .as_str()
            .unwrap()
            .contains("strict"),
        "{}",
        question(&v, "q2")
    );

    let e = restart(e, &root);
    let v = wait_for(&e.tx, id, "the loop waits again", |v| !v["needs"].is_null());
    assert_eq!(
        steps_of(&v, "review", "US1").len(),
        1,
        "nothing runs before the answer"
    );
    ask(
        &e.tx,
        Request::QuestionAnswer {
            id,
            q: "q2".into(),
            answer: "Yes, and add(0, 0)".into(),
        },
    )
    .unwrap();
    let v = wait_for(&e.tx, id, "ready to merge", |v| v["stage"] == "ready");
    let checks = steps_of(&v, "review", "US1");
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert_eq!(
        checks[1]["resume"], checks[0]["session"],
        "the checker resumed in its session"
    );
    assert!(prompt_of(&root, id, checks[1]).contains("Yes, and add(0, 0)"));
    let merged = git(
        &repo,
        &[
            "show",
            &format!(
                "{}:specs/001-calc/spec.md",
                v["integrated"].as_str().unwrap()
            ),
        ],
    );
    assert!(
        merged.contains("Ints only") && merged.contains("Yes, and add(0, 0)"),
        "{merged}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_confirmed_decision_keeps_its_commit_and_a_call_cut_short_goes_to_the_engineer() {
    // Free: the lead decides; the engineer confirms; the decision stays the lead's, committed.
    let root = scratch("ask-confirm");
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        asking_step(
            "T002",
            FLOATS,
            None,
            &[("calc/__init__.py", CALC_INIT)],
            "done",
        ),
        lead_answer("q1", "decided", "No floats", "none", ""),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "sleep_ms = 4000",
        ),
    ]
    .concat();
    let (e, _repo, id) = lock_level(&root, &steps, Level::Free);
    let tx = &e.tx;
    let v = wait_for(tx, id, "the decision", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "decided"
    });
    let commit = question(&v, "q1")["commit"].clone();
    assert!(commit.is_string(), "{}", question(&v, "q1"));
    ask(tx, Request::QuestionConfirm { id, q: "q1".into() }).unwrap();
    let v = ask(tx, Request::Get { id }).unwrap();
    let q1 = question(&v, "q1");
    assert_eq!(q1["state"], "confirmed");
    assert_eq!(
        q1["commit"], commit,
        "confirming keeps the answer's commit: {q1}"
    );
    assert_eq!(q1["by"], "lead");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // The lead's call is cut short by a stop of Fallow: after the restart, the question goes
    // to the engineer, saying why.
    let root = scratch("ask-orphan");
    let steps = [
        asking_step("T001", FLOATS, None, &[("calc/__init__.py", "")], "blocked"),
        lead_answer("q1", "answer", "No", "spec.md US1", "sleep_ms = 60000"),
    ]
    .concat();
    let (e, _repo, id) = lock_level(&root, &steps, Level::Normal);
    wait_for(&e.tx, id, "the lead thinking", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "with_lead"
    });
    let e = restart(e, &root);
    let v = wait_for(&e.tx, id, "relayed after the restart", |v| {
        v["feature_loop"]["questions"]["q1"]["state"] == "waiting_engineer"
    });
    let q1 = question(&v, "q1");
    assert!(
        q1["why"].as_str().is_some_and(|w| w.contains("cut short")),
        "{q1}"
    );
    assert!(!v["needs"].is_null());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
