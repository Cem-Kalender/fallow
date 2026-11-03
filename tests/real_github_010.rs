//! 010 T032 (quickstart Q7, SC-003, SC-005): one publication on a real GitHub repository whose
//! `main` requires pull requests, with the engineer's own `git` and `gh`. Not a test: it needs the
//! network and a throwaway repository of the engineer's, so it is ignored. Run it by hand with
//! `FALLOW_REAL_REPO=<owner>/<name> cargo test --test real_github_010 -- --ignored --nocapture`.
//! It replaces that repository's `main` with the test project, protects `main` (pull requests
//! required, admins included), publishes a loop run on the fake runtime, merges the pull request on
//! GitHub as the engineer would, and times both. The repository is the engineer's to delete.

mod common;

use common::*;
use fallow::run::engine::Request;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn gh(args: &[&str], input: Option<&str>) -> String {
    use std::io::Write;
    let mut c = Command::new("gh");
    c.args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped());
    let mut p = c.spawn().expect("gh");
    if let Some(i) = input {
        p.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
    } else {
        drop(p.stdin.take());
    }
    let out = p.wait_with_output().unwrap();
    assert!(out.status.success(), "gh {args:?} failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn remote_main(slug: &str) -> String {
    gh(&["api", &format!("repos/{slug}/commits/main"), "--jq", ".sha"], None)
}

#[test]
#[ignore]
fn publish_to_a_real_protected_github_repository() {
    let slug = std::env::var("FALLOW_REAL_REPO").expect("FALLOW_REAL_REPO=<owner>/<name>");
    let root = scratch("real-github");
    let address = format!("https://github.com/{slug}.git");
    let prepare = |_root: &Path, repo: &Path| {
        git(repo, &["remote", "add", "origin", &address]);
        // A protection left by an earlier run would refuse the reset of main.
        let _ = Command::new("gh")
            .args(["api", "-X", "DELETE", &format!("repos/{slug}/branches/main/protection")])
            .output();
        // The test project becomes the throwaway repository's main, before it is protected.
        git(repo, &["push", "-q", "-f", "-u", "origin", "main"]);
        let rule = json!({
            "required_status_checks": null,
            "enforce_admins": true,
            "required_pull_request_reviews": { "required_approving_review_count": 0 },
            "restrictions": null,
        });
        gh(
            &["api", "-X", "PUT", &format!("repos/{slug}/branches/main/protection"), "--input", "-"],
            Some(&rule.to_string()),
        );
    };
    let (e, _repo, id) = locked_calc_prepared(&root, &calc_loop_ok(), None, None, prepare);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready", |v| v["stage"] == "ready");
    let commit = v["integrated"].as_str().unwrap().to_string();
    let main_before = remote_main(&slug);

    // The remote as the loop page shows it, once read.
    let end = Instant::now() + Duration::from_secs(60);
    let l = loop {
        let l = ask(tx, Request::Loop { id }).unwrap();
        let r = &l["publish"]["remote"];
        if r["checking"] != true && r["state"] != "checking" {
            break l;
        }
        assert!(Instant::now() < end, "the remote was never read: {r}");
        std::thread::sleep(Duration::from_millis(200));
    };
    println!("remote: {}", l["publish"]["remote"]);
    println!("offer: {} ({})", l["publish"]["offer"], l["publish"]["why"]);
    assert_eq!(l["publish"]["remote"]["protected"], "yes", "{}", l["publish"]["remote"]);
    assert_eq!(l["publish"]["offer"], json!(["pull_request"]));
    println!("account: {}", l["publish"]["account"]);

    // SC-003: the publication, timed from the engineer's confirmed action.
    let t0 = Instant::now();
    ask(tx, Request::Publish { id }).unwrap();
    let l = loop {
        let l = ask(tx, Request::Loop { id }).unwrap();
        if !l["publish"]["publication"]["pr_url"].is_null() {
            break l;
        }
        assert!(l["publish"]["failed"].is_null(), "failed: {}", l["publish"]["failed"]);
        assert!(t0.elapsed() < Duration::from_secs(120), "no pull request: {}", l["publish"]);
        std::thread::sleep(Duration::from_millis(200));
    };
    let published = t0.elapsed();
    let pb = l["publish"]["publication"].clone();
    println!("SC-003: published in {:.1} s: {pb}", published.as_secs_f64());
    assert_eq!(pb["commit"], commit.as_str());
    let number = pb["pr_number"].as_u64().or_else(|| {
        pb["pr_url"].as_str().and_then(|u| u.rsplit('/').next()).and_then(|n| n.parse().ok())
    });
    let number = number.expect("pull request number").to_string();

    // On GitHub: from the loop's branch, at exactly the verified commit, into main, with the summary.
    let pr: Value = serde_json::from_str(&gh(
        &["pr", "view", &number, "-R", &slug, "--json", "headRefName,headRefOid,baseRefName,body,state"],
        None,
    ))
    .unwrap();
    println!("on GitHub: head {} at {}, base {}, {}", pr["headRefName"], pr["headRefOid"], pr["baseRefName"], pr["state"]);
    assert_eq!(pr["headRefOid"], commit.as_str());
    assert_eq!(pr["baseRefName"], "main");
    assert_eq!(pr["state"], "OPEN");
    let body = pr["body"].as_str().unwrap();
    assert!(body.contains("Verified by Fallow. The merge is the engineer's."), "{body}");
    assert_eq!(remote_main(&slug), main_before, "the target never moved");

    // The engineer merges on GitHub; SC-005: the loop follows within 2 minutes, without its page.
    gh(&["pr", "merge", &number, "-R", &slug, "--merge"], None);
    let t1 = Instant::now();
    let v = wait_up_to(tx, id, "merged, by the background read", Duration::from_secs(150), |v| {
        v["stage"] == "done"
    });
    println!("SC-005: seen merged {:.1} s after the merge; stage {}", t1.elapsed().as_secs_f64(), v["stage"]);
    let l = ask(tx, Request::Loop { id }).unwrap();
    println!("loop: stage {}, publish {}", l["stage"], l["publish"]["publication"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
