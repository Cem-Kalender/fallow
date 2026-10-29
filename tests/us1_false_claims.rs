//! 006 US1 (T039, constitution I, SC-004): false claims are never accepted. Each becomes a
//! repair, and the engineer is not asked until the repair cap is reached.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

fn reasons(v: &Value) -> Vec<String> {
    v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|s| s["verified"].as_array())
        .filter(|a| a[0] == false)
        .flat_map(|a| a[1].as_array().unwrap().clone())
        .map(|r| r.as_str().unwrap().to_string())
        .collect()
}

const OWNED_PLAN: &str = r#"owns = ["calc.py", "tests/**"]
criteria = [
  { id = "add", text = "add(2, 3) is 5" },
  { id = "bad", text = "add('a', 1) raises TypeError", must_fail = true },
]
"#;

#[test]
fn a_missing_commit_and_a_file_outside_the_part_are_refused() {
    let root = scratch("claims");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(OWNED_PLAN);
    // A commit that does not exist.
    sc.push_str("[[step]]\nkind = \"build\"\noutput = { status = \"done\", commit = \"deadbeefdeadbeef\", summary = \"done, trust me\" }\n");
    // A file outside what the part owns.
    let mut bad = good_files();
    bad.push(("other.txt", "not mine\n"));
    sc.push_str(&build_step("repair", &bad, ""));
    sc.push_str(&build_step("repair", &[("other.txt", "")], ""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    // Two refusals use both repair rounds; the third is the engineer's call.
    let v = wait_for(&tx, id, "the repair cap", |v| v["needs"].is_object());
    let r = reasons(&v);
    assert!(
        r.iter()
            .any(|x| x.contains("deadbeefdeadbeef does not exist")),
        "{r:?}"
    );
    assert!(
        r.iter()
            .any(|x| x.contains("outside what part main may change: other.txt")),
        "{r:?}"
    );
    assert_eq!(v["parts"]["main"]["repairs"], 2);
    assert!(
        v["needs"]["options"]
            .to_string()
            .contains("allow one more repair")
    );
    assert_eq!(
        v["interventions"], 0,
        "the engineer was not asked before the cap"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failing_gate_and_an_incomplete_verdict_are_repaired_without_the_engineer() {
    let root = scratch("gate-verdict");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    let broken = CALC.replace("return a + b", "return a - b");
    sc.push_str(&build_step(
        "build",
        &[
            ("calc.py", &broken),
            ("tests/__init__.py", ""),
            ("tests/test_calc.py", CALC_TEST),
        ],
        "",
    ));
    sc.push_str(&build_step("repair", &[("calc.py", CALC)], ""));
    // An incomplete verdict: no answer for main/bad.
    sc.push_str("[[step]]\nkind = \"review\"\noutput = { answers = [{ criterion = \"main/add\", ran = \"python\", result = \"5\", passed = true }] }\n");
    sc.push_str(&review_ok("when = \"did not count yet\""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    // The failing gate is a gate run that does not pass; the repair follows it.
    let part_runs: Vec<&Value> = v["gate_runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g["kind"] == "part")
        .collect();
    assert_eq!(part_runs.len(), 2, "{part_runs:?}");
    assert_ne!(part_runs[0]["class"], "pass");
    assert_eq!(part_runs[1]["class"], "pass");
    let verdicts = v["verdicts"].as_array().unwrap();
    assert_eq!(verdicts[0]["complete"], false);
    assert!(verdicts[0]["missing"].to_string().contains("main/bad"));
    assert_eq!(verdicts[1]["approved"], true);
    assert_eq!(v["parts"]["main"]["repairs"], 1);
    assert_eq!(v["interventions"], 0);
    // The repaired commit is what is integrated.
    assert!(
        git(
            &repo,
            &[
                "show",
                &format!("{}:calc.py", v["integrated"].as_str().unwrap())
            ]
        )
        .contains("a + b")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_engineer_can_allow_one_more_repair() {
    let root = scratch("cap");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    for kind in ["build", "repair", "repair"] {
        sc.push_str(&format!(
            "[[step]]\nkind = {kind:?}\noutput = {{ status = \"done\", commit = \"nope\", summary = \"x\" }}\n"
        ));
    }
    sc.push_str(&build_step("repair", &good_files(), ""));
    sc.push_str(&review_ok(""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    wait_for(&tx, id, "the repair cap", |v| v["needs"].is_object());
    ask(
        &tx,
        Request::Limits {
            id,
            usd: None,
            minutes: None,
            repair_cap: Some(3),
            once: false,
        },
    )
    .unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(v["parts"]["main"]["repairs"], 3);
    assert_eq!(
        v["interventions"], 1,
        "allowing the repair is one intervention"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_review_whose_commands_were_refused_does_not_count() {
    // Found on a real run: a reviewer whose test commands were refused approved by reading the
    // code. That verdict must not count; the reviewer is resumed to run them one at a time.
    let root = scratch("refused-review");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let mut sc = plan_step(PLAN);
    sc.push_str(&build_step("build", &good_files(), ""));
    sc.push_str(&review_ok(
        "denied = [\"python -m unittest -v; echo done\"]",
    ));
    sc.push_str(&review_ok("when = \"were refused\""));
    std::fs::write(&scenario, &sc).unwrap();
    let (tx, _h) = start_engine(&root, &scenario);
    let id = create(&tx, &repo, "add()");
    wait_for(&tx, id, "plan", |v| v["plan_approvable"] == true);
    ask(&tx, Request::Approve { id }).unwrap();
    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    let verdicts = v["verdicts"].as_array().unwrap();
    assert_eq!(verdicts.len(), 2);
    assert_eq!(verdicts[0]["approved"], false);
    assert!(verdicts[0]["missing"].to_string().contains("were refused"));
    assert_eq!(verdicts[1]["approved"], true);
    let _ = std::fs::remove_dir_all(&root);
}
