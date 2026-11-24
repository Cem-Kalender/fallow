//! 010 US1 (quickstart Q1; FR-001 to FR-004): a project shows whether it has a remote, which one,
//! the target branch there and whether it is protected, never a guess. No network: the remote is
//! a bare repository behind a github.com address, and `gh` is `fallow-fake-host`.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ADDRESS: &str = "https://github.com/acme/shop.git";

/// A project at `<root>/repo`, prepared by `prepare`, with Fallow started; its id.
fn project(
    root: &Path,
    prepare: impl FnOnce(&Path, &Path),
) -> (fallow::service::ToolEngine, String, PathBuf) {
    let repo = repo(root);
    prepare(root, &repo);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(root, &scenario, None);
    let id = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    (e, id, repo)
}

fn view(e: &fallow::service::ToolEngine, id: &str) -> Value {
    sreq(&e.tx, Req::Project { id: id.into() }).unwrap()
}

/// The project's remote once its protection is no longer being checked.
fn settled(e: &fallow::service::ToolEngine, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let r = view(e, id)["remote"].clone();
        if r["checking"] != true && r["protected"] != "checking" {
            return r;
        }
        assert!(Instant::now() < end, "still checking: {r}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn hosted(state: Value) -> impl FnOnce(&Path, &Path) {
    move |root: &Path, repo: &Path| {
        bare_remote(root, repo, ADDRESS);
        fake_host(root, state);
    }
}

#[test]
fn a_project_without_a_remote_says_so() {
    let root = scratch("r-none");
    let (e, id, _) = project(&root, |_, _| {});
    let r = settled(&e, &id);
    assert_eq!(r["state"], "none", "{r}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn protection_is_read_from_the_rules_or_the_branch_flag() {
    for (name, state, want) in [
        (
            "r-rule",
            json!({ "login": "octo", "rules": { "main": ["pull_request"] }, "protected": { "main": false } }),
            "yes",
        ),
        (
            "r-flag",
            json!({ "login": "octo", "protected": { "main": true } }),
            "yes",
        ),
        (
            "r-open",
            json!({ "login": "octo", "rules": { "main": ["deletion"] }, "protected": { "main": false } }),
            "no",
        ),
    ] {
        let root = scratch(name);
        let (e, id, _) = project(&root, hosted(state));
        let r = settled(&e, &id);
        assert_eq!(r["state"], "known", "{name}: {r}");
        assert_eq!(r["service"], "github", "{r}");
        assert_eq!(r["repo"], json!({ "owner": "acme", "name": "shop" }), "{r}");
        assert_eq!(r["remote"], "origin", "{r}");
        assert_eq!(r["target"], "main", "{r}");
        assert_eq!(r["protected"], want, "{name}: {r}");
        assert_eq!(r["account"], "octo", "{r}");
        assert!(r["checked_at"].is_string(), "{r}");
        drop(e);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn what_cannot_be_read_is_unknown_with_the_reason_never_a_guess() {
    // Not signed in.
    let root = scratch("r-signed-out");
    let (e, id, _) = project(
        &root,
        hosted(json!({ "signed_in": false, "protected": { "main": true } })),
    );
    let r = settled(&e, &id);
    assert_eq!(r["protected"], "unknown", "{r}");
    assert!(r["why"].as_str().unwrap().contains("gh auth login"), "{r}");
    let log = fake_host_log(&root.join("host.json"));
    assert!(
        log.iter()
            .flatten()
            .all(|a| a != "token" && a != "--show-token"),
        "never a token: {log:?}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);

    // The target branch is not on the remote (the fake answers 404).
    let root = scratch("r-404");
    let (e, id, _) = project(&root, hosted(json!({ "login": "octo" })));
    let r = settled(&e, &id);
    assert_eq!(r["protected"], "unknown", "{r}");
    assert!(
        r["why"]
            .as_str()
            .unwrap()
            .contains("main is not on acme/shop yet"),
        "{r}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn another_service_is_named_and_its_tool_never_run() {
    let root = scratch("r-gitlab");
    let (e, id, _) = project(&root, |root: &Path, repo: &Path| {
        bare_remote(root, repo, "git@gitlab.com:acme/shop.git");
        fake_host(root, json!({ "login": "octo" }));
    });
    let r = settled(&e, &id);
    assert_eq!(r["state"], "unsupported", "{r}");
    assert_eq!(r["service"], "gitlab.com", "{r}");
    assert!(fake_host_log(&root.join("host.json")).is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_remote_is_the_one_the_target_tracks() {
    let root = scratch("r-upstream");
    let (e, id, _) = project(&root, |root: &Path, repo: &Path| {
        bare_remote(root, repo, ADDRESS);
        // A second remote, `upstream`, which `main` now tracks.
        let up = root.join("upstream.git");
        std::fs::create_dir_all(&up).unwrap();
        git(&up, &["init", "-q", "--bare", "-b", "main"]);
        let address = "https://github.com/acme-org/shop.git";
        git(repo, &["remote", "add", "upstream", address]);
        let local = up.display().to_string().replace('\\', "/");
        git(
            repo,
            &["config", &format!("url.{local}.insteadOf"), address],
        );
        git(repo, &["push", "-q", "-u", "upstream", "main"]);
        fake_host(
            root,
            json!({ "login": "octo", "protected": { "main": true } }),
        );
    });
    let r = settled(&e, &id);
    assert_eq!(r["remote"], "upstream", "{r}");
    assert_eq!(r["repo"]["owner"], "acme-org", "{r}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_project_answers_at_once_and_keeps_the_last_value_when_it_cannot_check() {
    let root = scratch("r-slow");
    let (e, id, _) = project(
        &root,
        hosted(
            json!({ "login": "octo", "protected": { "main": true }, "sleep_ms": { "api": 3000 } }),
        ),
    );
    let t = Instant::now();
    let r = view(&e, &id)["remote"].clone();
    // Not held by the hosting tool (which takes 3 s here): it answers while the read runs.
    assert!(
        t.elapsed() < Duration::from_millis(2800),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(r["protected"], "checking", "{r}");
    assert!(r["checking"] == true || r["state"] == "checking", "{r}");
    let r = settled(&e, &id);
    assert_eq!(r["protected"], "yes", "{r}");
    let checked = r["checked_at"].clone();
    // Opened again while it cannot check: read again at once, the last value kept, with its time.
    let state = root.join("host.json");
    let mut s = host_state(&state);
    s["fail"] = json!({ "api": "gh: HTTP 502 Bad Gateway" });
    s["sleep_ms"] = json!({});
    set_host_state(&state, &s);
    // (Not within 5 s of the last read: opening a project never reads twice in a row.)
    std::thread::sleep(Duration::from_secs(5));
    sreq(&e.tx, Req::CheckRemote { id: id.clone() }).unwrap();
    let r = settled(&e, &id);
    assert_eq!(
        (r["protected"].as_str(), r["stale"].as_bool()),
        (Some("yes"), Some(true)),
        "{r}"
    );
    assert_eq!(r["checked_at"], checked, "{r}");
    assert!(r["why"].as_str().unwrap().contains("502"), "{r}");
    s["fail"] = json!({});
    set_host_state(&state, &s);
    // Fallow starts again and cannot check now: the last value, with its time.
    let ToolEngine { tx, handle, .. } = e;
    tx.send(fallow::run::engine::Msg::Shutdown).unwrap();
    handle.join().unwrap();
    let mut s = host_state(&state);
    s["fail"] = json!({ "api": "gh: could not resolve host api.github.com" });
    s["sleep_ms"] = json!({});
    set_host_state(&state, &s);
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let r = settled(&e, &id);
    assert_eq!(r["protected"], "yes", "the last value: {r}");
    assert_eq!(r["stale"], true, "{r}");
    assert_eq!(r["checked_at"], checked, "{r}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

use fallow::service::ToolEngine;
