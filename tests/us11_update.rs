//! 011 US1 (quickstart Q1; FR-001 to FR-008): a loop whose target moved is brought up to date in
//! its own working copy, a conflict goes to a builder, and only a newly verified commit merges.
//! No network, no model: the fake runtime plays every step.

mod common;

use common::*;

/// Phase 1's checkpoint: two features of one project, both locked, each with its own loop.
#[test]
fn two_features_lock_in_one_project() {
    let root = scratch("u-two");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}", lead_writes_calc(), lead_writes_calc2()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let a = lock_calc_in(&e.tx, &repo, None, Some((5.0, 60)), None);
    // A project is named after its folder.
    let b = lock_feature_in(&e.tx, "repo", "Calc2", "002-calc2", None, Some((5.0, 60)), None);
    assert_ne!(a, b);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
