//! 007 US2 (T038; FR-010 to FR-014; research S-9): a two-story feature with the Standard
//! pipeline runs to one merge. Each task gets a commit, a tick and a gate run; each phase a new
//! builder session, resumed within it, and a handover; each story a review answering every
//! `USn/k`; the merge brings exactly the gated commit, with every task ticked.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::Value;

fn steps(v: &Value) -> Vec<Value> {
    v["steps"].as_object().unwrap().values().cloned().collect()
}

#[test]
fn a_locked_feature_runs_task_by_task_to_one_merge() {
    let root = scratch("loop");
    let (e, repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let lp = &v["feature_loop"];
    assert_eq!(
        lp["outcome"],
        serde_json::json!([true, []]),
        "verified: {}",
        lp["outcome"]
    );

    // Every task accepted with its own commit.
    for t in ["T001", "T002", "T003", "T004"] {
        assert_eq!(
            lp["tasks"][t]["state"], "accepted",
            "{t}: {}",
            lp["tasks"][t]
        );
        assert!(lp["tasks"][t]["commit"].is_string());
    }
    // A gate run per task, against the baseline.
    let gates = v["gate_runs"].as_array().unwrap();
    assert_eq!(gates.iter().filter(|g| g["kind"] == "baseline").count(), 1);
    for t in ["T001", "T002", "T003", "T004"] {
        assert!(
            gates.iter().any(|g| g["kind"] == "part" && g["part"] == t),
            "no gate run for {t}"
        );
    }
    // A session per phase: T002 and T003 share one, T001 and T004 have others.
    let st = steps(&v);
    let session = |task: &str| {
        st.iter()
            .find(|s| s["task"] == task && s["kind"] == "build")
            .and_then(|s| s["session"].as_str().map(str::to_string))
            .unwrap()
    };
    assert_eq!(
        session("T002"),
        session("T003"),
        "one session within a phase"
    );
    assert_ne!(session("T001"), session("T002"), "a new session per phase");
    assert_ne!(session("T003"), session("T004"));
    let t3 = st.iter().find(|s| s["task"] == "T003").unwrap();
    assert_eq!(
        t3["resume"],
        session("T002"),
        "T003 resumed the phase's session"
    );
    // The handovers, committed in the feature's folder.
    for n in 1..=3 {
        assert_eq!(lp["phases"][n - 1]["state"], "done");
        let path = format!("specs/001-calc/handover/phase-{n}.md");
        assert_eq!(lp["phases"][n - 1]["handover"], path.as_str());
        assert!(
            git(&repo, &["show", &format!("fallow/001-calc/build:{path}")])
                .contains("## For the next phase")
        );
    }
    // The second phase's first instruction pointed at the first handover.
    // The page shows each story's checkpoint after its last phase only, where the engine
    // reviews it (T086).
    let view = ask(tx, Request::Loop { id }).unwrap();
    let cps: Vec<Value> = view["phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["checkpoints"].clone())
        .collect();
    assert_eq!(
        cps,
        vec![
            serde_json::json!([]),
            serde_json::json!(["US1"]),
            serde_json::json!(["US2"])
        ]
    );
    let t2 = st.iter().find(|s| s["task"] == "T002").unwrap();
    let prompt = std::fs::read_to_string(
        root.join("data/runs")
            .join(id.to_string())
            .join("steps")
            .join(format!("{}.prompt", t2["n"])),
    )
    .unwrap();
    assert!(prompt.contains("handover/phase-1.md"), "{prompt}");
    // A review per story, complete and approved.
    let verdicts = v["verdicts"].as_array().unwrap();
    assert_eq!(verdicts.len(), 2);
    assert!(
        verdicts
            .iter()
            .all(|x| x["complete"] == true && x["approved"] == true)
    );
    // tasks.md ticked in the tasks' own commits.
    let tasks_md = git(
        &repo,
        &["show", "fallow/001-calc/build:specs/001-calc/tasks.md"],
    );
    assert!(
        tasks_md.contains("- [X] T001") && tasks_md.contains("- [X] T004"),
        "{tasks_md}"
    );

    // One merge: exactly the gated commit onto main.
    let gated = v["integrated"].as_str().unwrap().to_string();
    ask(tx, Request::Merge { id }).unwrap();
    let v = wait_for(tx, id, "merged", |v| v["stage"] == "done");
    assert!(v["merged"].is_string());
    let main = git(&repo, &["rev-parse", "main"]);
    assert_eq!(
        git(&repo, &["rev-parse", "main^2"]),
        gated,
        "the gated commit, merged with a merge commit"
    );
    assert!(
        git(&repo, &["show", &format!("{main}:specs/001-calc/tasks.md")]).contains("- [X] T003")
    );
    assert!(git(&repo, &["show", &format!("{main}:calc/__init__.py")]).contains("def sub"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
