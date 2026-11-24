//! 008 T038: a spec session can be deleted. Before the lock its draft goes too (the chat, the
//! worktree, the branch, and the feature from the project); while its loop still goes, the
//! delete is refused.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;

#[test]
fn deleting_an_unlocked_session_removes_its_draft() {
    let root = scratch("delete-draft");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
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
            title: "Idea".into(),
            lead: None,
        },
    )
    .unwrap();
    let feature = f["feature"].as_str().unwrap().to_string();
    let conv = f["conversation"].as_str().unwrap().to_string();
    assert!(
        git(&repo, &["branch", "--list", &feature]).contains(&feature),
        "the draft has its branch"
    );
    let wt = root.join("data").join("worktrees");
    assert!(std::fs::read_dir(&wt).is_ok_and(|mut d| d.next().is_some()));
    let r = ask(
        tx,
        Request::DeleteFeature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(r["feature_removed"], true, "{r}");
    assert!(
        !git(&repo, &["branch", "--list", &feature]).contains(&feature),
        "the branch is gone"
    );
    assert!(
        !git(&repo, &["worktree", "list"]).contains(&feature),
        "the worktree is gone"
    );
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(
        !p.to_string().contains(&feature),
        "the feature left the project: {p}"
    );
    assert!(
        sreq(tx, Req::Conversation { id: conv }).is_err(),
        "the chat is gone"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_session_whose_loop_still_goes_is_kept() {
    let root = scratch("delete-running");
    // The loop waits in its first build step while the delete is asked.
    let steps = task_step("T001", &[("calc/__init__.py", "")], "sleep_ms = 8000");
    let (e, repo, id) = locked_calc(&root, &steps, None);
    let tx = &e.tx;
    wait_for(tx, id, "the build", |v| v["busy"] != "");
    let project = repo.file_name().unwrap().to_string_lossy().to_lowercase();
    let err = ask(
        tx,
        Request::DeleteFeature {
            project,
            feature: "001-calc".into(),
        },
    )
    .unwrap_err();
    assert!(err.contains("still going"), "{err}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
