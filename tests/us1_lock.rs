//! 007 US1 (T025; FR-008, FR-016a; contracts/spec-kit-files.md): the lock is refused, naming each
//! problem, while analyze has not run or found something critical, a story has no must-fail
//! scenario, a marker is open, or tasks.md cannot be read; a lock commits the documents and
//! creates the loop with its phases and the suggested budget; after it, the documents cannot be
//! changed through the page.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;

const SPEC_BAD: &str = "# Cart\n\n## User Scenarios & Testing\n\n### User Story 1 - Add to cart (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** an empty cart, **When** adding apple 2, **Then** it holds apple 2.\n2. **Given** a cart, **When** adding 0, **Then** [NEEDS CLARIFICATION: refuse or ignore?]\n";
const SPEC_GOOD: &str = "# Cart\n\n## User Scenarios & Testing\n\n### User Story 1 - Add to cart (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** an empty cart, **When** adding apple 2, **Then** it holds apple 2.\n2. **Given** 5 in stock, **When** adding 9, **Then** it is refused (must fail)\n";
const PLAN: &str = "# Plan\n\n## Commands\n\n- `python -m shop`\n";
const TASKS_BAD: &str = "# Tasks\n\n- [ ] T001 before any phase\n";
const TASKS_GOOD: &str = "# Tasks\n\n## Phase 1: Setup\n\n- [ ] T001 Create shop/cart.py\n\n## Phase 2: User Story 1 - Add to cart\n\n- [ ] T002 [US1] Cart model\n- [ ] T003 [US1] Refuse beyond stock\n";

#[test]
fn the_lock_names_every_problem_then_commits_and_creates_the_loop() {
    let root = scratch("lock");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let lit = |s: &str| format!("'''\n{s}'''");
    std::fs::write(
        &scenario,
        format!(
            "[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/001-cart/spec.md\", text = {} }}, {{ path = \"specs/001-cart/plan.md\", text = {} }}, {{ path = \"specs/001-cart/tasks.md\", text = {} }}]\noutput = {{ reply = \"Spec, plan and tasks.\", step = \"tasks\" }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed.\", step = \"analyze\", findings = [{{ severity = \"CRITICAL\", text = \"tasks miss the stock rule\" }}, {{ severity = \"LOW\", text = \"wording\" }}] }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed again.\", step = \"analyze\", findings = [{{ severity = \"CRITICAL\", text = \"tasks miss the stock rule\" }}, {{ severity = \"LOW\", text = \"wording\" }}] }}\n",
            lit(SPEC_BAD),
            lit(PLAN),
            lit(TASKS_BAD)
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    let feature = "001-cart".to_string();
    let lock = |tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>| {
        ask(
            tx,
            Request::Lock {
                project: project.clone(),
                feature: feature.clone(),
                pipeline: None,
                level: None,
                budget_usd: None,
                budget_minutes: None,
                when_verified: None,
                update_limit: None,
                run_after: None,
            },
        )
    };
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Write it all.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 1);

    // Before analyze: refused, with every problem named.
    let err = lock(tx).unwrap_err();
    for want in [
        "analyze step has not run",
        "open clarification",
        "US1 has no scenario marked (must fail)",
        "not under a `## Phase` heading",
    ] {
        assert!(err.contains(want), "missing {want:?} in:\n{err}");
    }
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(fv["lock_check"]["ok"], false);
    assert_eq!(fv["markers"].as_array().unwrap().len(), 1);

    // Analyze ran and found something critical: refused until it is dismissed.
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Analyze.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 2);
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(fv["step"], "done");
    assert_eq!(fv["findings"].as_array().unwrap().len(), 2);
    let put = |path: &str, text: &str| {
        let d = sreq(
            tx,
            Req::Doc {
                project: project.clone(),
                feature: feature.clone(),
                path: path.into(),
            },
        )
        .unwrap();
        sreq(
            tx,
            Req::PutDoc {
                project: project.clone(),
                feature: feature.clone(),
                path: path.into(),
                text: text.into(),
                version: d["version"].as_str().unwrap().into(),
            },
        )
        .unwrap();
    };
    put("specs/001-cart/spec.md", SPEC_GOOD);
    put("specs/001-cart/tasks.md", TASKS_GOOD);
    // T086: the documents changed after analyze, so analyze runs again before the lock.
    let err = lock(tx).unwrap_err();
    // 008 T042: the message says why analyze must run again.
    assert!(err.contains("the documents changed after analyze"), "{err}");
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Analyze again.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 3);
    let err = lock(tx).unwrap_err();
    assert!(
        err.contains("CRITICAL problem: tasks miss the stock rule"),
        "{err}"
    );
    assert!(
        !err.contains("must fail") && !err.contains("open clarification"),
        "{err}"
    );
    sreq(
        tx,
        Req::Dismiss {
            project: project.clone(),
            feature: feature.clone(),
            finding: 0,
        },
    )
    .unwrap();

    // The lock: the documents committed on the feature branch, the loop with its phases and the
    // budget suggested from three tasks at $0.50 and 10 minutes each.
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(fv["lock_check"]["ok"], true, "{}", fv["lock_check"]);
    assert_eq!(fv["budget_suggestion"]["tasks"], 3);
    let l = lock(tx).unwrap();
    let id = l["loop"].as_u64().unwrap();
    assert_eq!(l["lock"], 1);
    let commit = l["commit"].as_str().unwrap();
    assert_eq!(git(&repo, &["rev-parse", "001-cart"]), commit);
    assert!(git(&repo, &["show", "001-cart:specs/001-cart/tasks.md"]).contains("T003"));
    assert_eq!(git(&repo, &["rev-parse", "fallow/001-cart/build"]), commit);
    let run = ask(tx, Request::Get { id }).unwrap();
    let lp = &run["feature_loop"];
    assert_eq!(lp["feature"], "001-cart");
    assert_eq!(lp["phases"].as_array().unwrap().len(), 2);
    assert_eq!(lp["phases"][1]["spec"]["story"], "US1");
    assert!((lp["budget_usd"].as_f64().unwrap() - 1.5).abs() < 1e-9);
    assert_eq!(lp["budget_minutes"], 30);
    assert_eq!(lp["base_branch"], "main");
    assert_eq!(run["stage"], "building");
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(fv["stage"], "building");
    assert_eq!(fv["locks"].as_array().unwrap().len(), 1);

    // Locked: the page cannot change the documents.
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: feature.clone(),
            path: "specs/001-cart/spec.md".into(),
        },
    )
    .unwrap();
    let after = sreq(
        tx,
        Req::PutDoc {
            project: project.clone(),
            feature: feature.clone(),
            path: "specs/001-cart/spec.md".into(),
            text: "changed".into(),
            version: d["version"].as_str().unwrap().into(),
        },
    );
    assert!(after.unwrap_err().contains("locked"));
    // Nor can the lead, the stepper or a second lock.
    let said = sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "One more change.".into(),
        },
    );
    assert!(said.unwrap_err().contains("locked"));
    let step = sreq(
        tx,
        Req::SetStep {
            project: project.clone(),
            feature: feature.clone(),
            to: "plan".into(),
        },
    );
    assert!(step.unwrap_err().contains("locked"));
    let again = lock(tx).unwrap_err();
    assert!(again.contains("already locked"), "{again}");
    let runs = ask(tx, Request::List).unwrap();
    assert_eq!(runs.as_array().unwrap().len(), 1, "one loop");

    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_plain_folder_gets_git_and_its_features_their_branches() {
    let root = scratch("lock-plain");
    let folder = root.join("thesis");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plot.py"), "print(1)\n").unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        "[[step]]\nkind = \"lead_turn\"\nfiles = [{ path = \"specs/001-plots/spec.md\", text = \"# Plots\\n\" }]\noutput = { reply = \"Drafted.\", step = \"specify\" }\n",
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(
        tx,
        Req::AddProject {
            path: folder.clone(),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Plots".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    sreq(
        tx,
        Req::Say {
            conv: conv.clone(),
            text: "Draft it.".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &conv, 1);
    // In a plain folder the lead wrote in the folder itself.
    assert!(folder.join("specs/001-plots/spec.md").exists());
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: "001-plots".into(),
        },
    )
    .unwrap();
    assert!(
        fv["lock_check"]["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().contains("plain folder"))
    );
    // A folder inside another repository is refused.
    let inner = root.join("repo-around");
    std::fs::create_dir_all(inner.join("sub")).unwrap();
    git(&inner, &["init", "-q"]);
    let sub = sreq(
        tx,
        Req::AddProject {
            path: inner.join("sub"),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        sreq(tx, Req::GitInit { id: sub })
            .unwrap_err()
            .contains("inside the repository")
    );
    // Git for the thesis: the feature gets its branch and worktree, the session moves there.
    sreq(
        tx,
        Req::GitInit {
            id: project.clone(),
        },
    )
    .unwrap();
    let fv = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: "001-plots".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["branch"], "001-plots");
    assert!(fv["base_branch"].is_string());
    let c = sreq(tx, Req::Conversation { id: conv.clone() }).unwrap();
    let wd = c["workdir"].as_str().unwrap();
    assert!(wd.contains("worktrees"), "{wd}");
    let d = sreq(
        tx,
        Req::Doc {
            project: project.clone(),
            feature: "001-plots".into(),
            path: "specs/001-plots/spec.md".into(),
        },
    )
    .unwrap();
    assert_eq!(d["text"].as_str().unwrap().trim_end(), "# Plots");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// T086: a lead turn that changes a document after analyze (here a clarify) means analyze must
/// run again: the lock is refused, and the stepper is back at analyze for the lead's next turn.
#[test]
fn a_document_changed_after_analyze_sends_the_session_back_to_analyze() {
    let root = scratch("lock-reanalyze");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    let lit = |s: &str| format!("'''\n{s}'''");
    std::fs::write(
        &scenario,
        format!(
            "[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/001-cart/spec.md\", text = {} }}, {{ path = \"specs/001-cart/plan.md\", text = {} }}, {{ path = \"specs/001-cart/tasks.md\", text = {} }}]\noutput = {{ reply = \"Written.\", step = \"tasks\" }}\n\n[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Analyzed.\", step = \"analyze\", findings = [] }}\n\n[[step]]\nkind = \"lead_turn\"\nfiles = [{{ path = \"specs/001-cart/spec.md\", text = {} }}]\noutput = {{ reply = \"Clarified.\", step = \"clarify\" }}\n",
            lit(SPEC_GOOD),
            lit(PLAN),
            lit(TASKS_GOOD),
            lit(&format!("{SPEC_GOOD}\n## Clarifications\n\n- Q: x? → A: y.\n"))
        ),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    let view = || {
        sreq(
            tx,
            Req::Feature {
                project: project.clone(),
                feature: "001-cart".into(),
            },
        )
        .unwrap()
    };
    for (n, text) in [
        (1, "Write it all."),
        (2, "Analyze."),
        (3, "One more thing."),
    ] {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: text.into(),
            },
        )
        .unwrap();
        turn_ended(tx, &conv, n);
        if n == 2 {
            assert_eq!(view()["lock_check"]["ok"], true, "{}", view()["lock_check"]);
        }
    }
    let v = view();
    assert_eq!(v["analyzed"], false, "{v}");
    assert_eq!(v["step"], "analyze", "the lead's next turn runs analyze");
    assert!(
        v["lock_check"]["problems"]
            .to_string()
            .contains("the documents changed after analyze"),
        "{}",
        v["lock_check"]
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
