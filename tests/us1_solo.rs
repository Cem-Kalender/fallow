//! 006 US1 (T038): a solo run from the plan to the merge, with the fake agent. Two steering
//! actions (approve, merge), the gates against the baseline, a complete review, and exactly the
//! verified commit on the target branch.

mod common;

use common::*;
use fallow::run::engine::Request;

#[test]
fn a_solo_run_goes_from_the_plan_to_the_merge() {
    let root = scratch("solo");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            r#"
[[step]]
kind = "plan_turn"
files = [{{ path = ".fallow/work.toml", text = {plan} }}]
commit = "Plan: add"
output = {{ reply = "Here is the plan: one builder.", commit = "$HEAD", ready = true }}

[[step]]
kind = "build"
part = "main"
files = [{{ path = "calc.py", text = {calc} }}, {{ path = "tests/__init__.py", text = "" }}, {{ path = "tests/test_calc.py", text = {test} }}]
commit = "Add add()"
output = {{ status = "done", commit = "$HEAD", summary = "add() with tests", files = ["calc.py", "tests/test_calc.py"] }}
cost = 0.20

[[step]]
kind = "review"
output = {{ answers = [
  {{ criterion = "main/add", ran = "python -c 'from calc import add; print(add(2,3))'", result = "5", passed = true }},
  {{ criterion = "main/bad", ran = "python -c 'from calc import add; add(\"a\",1)'", result = "TypeError", passed = true, failing_case = "add('a', 1) raises TypeError" }},
], summary = "all criteria hold" }}
"#,
            plan = lit(PLAN),
            calc = lit(CALC),
            test = lit(CALC_TEST)
        ),
    )
    .unwrap();
    let data = root.join("data");
    let (tx, _h) = fallow::service::engine_for_tests(data, settings(&scenario)).unwrap();
    let id = create(&tx, &repo, "Add an add() function that only takes ints");

    let v = wait_for(&tx, id, "an approvable plan", |v| {
        v["plan_approvable"] == true
    });
    assert_eq!(v["stage"], "planning");
    assert_eq!(v["conversation"][0]["who"], "planner");
    assert!(
        git(&repo, &["log", "--oneline", "main"]).lines().count() == 1,
        "nothing on main yet"
    );
    // Nothing starts before approval (FR-003).
    assert!(
        v["steps"]
            .as_object()
            .unwrap()
            .values()
            .all(|s| s["kind"] == "plan_turn")
    );
    ask(&tx, Request::Approve { id }).unwrap();

    let v = wait_for(&tx, id, "ready to merge", |v| v["stage"] == "ready");
    let gates = v["gate_runs"].as_array().unwrap();
    assert_eq!(gates[0]["kind"], "baseline");
    assert_eq!(gates[0]["result"]["outcome"]["state"], "no_tests");
    assert_eq!(gates[1]["kind"], "part");
    assert_eq!(gates[1]["class"], "pass");
    let verdict = &v["verdicts"][0];
    assert_eq!(verdict["complete"], true);
    assert_eq!(verdict["approved"], true);
    let integrated = v["integrated"].as_str().unwrap().to_string();
    assert_eq!(
        git(&repo, &["rev-parse", "main"]),
        git(
            &repo,
            &[
                "rev-parse",
                &format!("{}~0", v["start_commit"].as_str().unwrap())
            ]
        ),
        "nothing on the target branch before the merge"
    );

    ask(&tx, Request::Merge { id }).unwrap();
    let v = wait_for(&tx, id, "the merge", |v| v["stage"] == "done");
    assert_eq!(
        git(&repo, &["rev-parse", "main^2"]),
        integrated,
        "exactly the verified commit"
    );
    assert!(git(&repo, &["show", "main:calc.py"]).contains("def add"));
    let kinds: Vec<&str> = v["steering"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["approve_plan", "merge"]);
    assert_eq!(
        v["interventions"], 0,
        "approve and merge are counted apart (FR-020)"
    );
    assert!((v["cost_usd"].as_f64().unwrap() - 0.22).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(&root);
}
