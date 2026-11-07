//! 012 US6 (T048; quickstart Q7; FR-021 to FR-025, SC-007, SC-010; research R-11, R-14): closing
//! a session never loses work that is not merged without the engineer's choice. A merged and
//! clean session closes without a question; undone turns, ignored files (a `.env`) and work not
//! merged are named first; "keep" commits and keeps the branch, "discard" names what goes; a
//! failed keep keeps the copy, and "Remove working copy" follows; a closed session's record can
//! be deleted; a restart in the middle of a close finishes it.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;
use std::path::{Path, PathBuf};

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

fn scenario() -> String {
    format!(
        "{}{}{}{}{}",
        assistant_step("Fixed.", "when = \"Fix the week\"\nfiles = [{ path = \"picker.js\", text = \"const firstDay = 1;\\n\" }]"),
        assistant_step("Added a test.", "when = \"add a test\"\nfiles = [{ path = \"picker.test.js\", text = \"test()\\n\" }]"),
        assistant_step("Working.", "when = \"slow one\"\nsleep_ms = 5000"),
        assistant_step("Bye is just text to me.", "when = \"/exit\""),
        assistant_step("Fixed again.", "when = \"Fix it again\"\nfiles = [{ path = \"picker.js\", text = \"const firstDay = 0;\\n\" }]"),
    )
}

fn setup(name: &str) -> (fallow::service::ToolEngine, PathBuf, String, PathBuf) {
    let root = scratch(name);
    let repo = repo(&root);
    std::fs::write(repo.join(".gitignore"), ".env\nnode_modules/\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ignore"]);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario()).unwrap();
    let e = start_studio(&root, &sc, None);
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project, root)
}

/// The page's close: the question first, then the choice (or the confirmation) with its digest.
fn close(tx: &Tx, s: &str, choice: Option<&str>, confirmed: bool) -> Result<Value, String> {
    let req = |choice: Option<&str>, confirmed: bool, digest: Option<String>| {
        sreq(tx, Req::CloseSession { session: s.to_string(), choice: choice.map(str::to_string), confirmed, digest })
    };
    if choice.is_none() && !confirmed {
        return req(None, false, None);
    }
    let first = req(None, false, None)?;
    if first.get("ask").is_none() {
        return Ok(first);
    }
    req(choice, confirmed, first["ask"]["digest"].as_str().map(str::to_string))
}

fn view(tx: &Tx, s: &str) -> Value {
    sreq(tx, Req::Conversation { id: s.to_string() }).unwrap()
}

/// A session with one turn that changed `picker.js`: its id, branch and working copy.
fn one_turn(tx: &Tx, project: &str) -> (String, String, PathBuf) {
    let s = start_session(tx, project, "Fix the week");
    let v = wait_turn(tx, &s, 1);
    let branch = v["session"]["branch"].as_str().unwrap().to_string();
    let wt = PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    (s, branch, wt)
}

fn has_branch(repo: &Path, b: &str) -> bool {
    !git(repo, &["branch", "--list", b]).trim().is_empty()
}

#[test]
fn a_merged_and_clean_session_closes_without_a_question() {
    let (e, repo, project, _root) = setup("us12-close-merged");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    let r = close(tx, &s, None, false).unwrap();
    assert_eq!(r["closed"], "merged", "{r}");
    assert!(!wt.exists(), "its working copy is gone");
    assert!(!has_branch(&repo, &branch), "its merged branch is gone");
    let v = view(tx, &s);
    assert_eq!(v["session"]["closed"]["how"], "merged", "{}", v["session"]);
    assert!(sreq(tx, Req::Say { conv: s.clone(), text: "more".into() }).is_err(), "closed: no more messages");
    let l = sreq(tx, Req::Sessions { project: project.clone() }).unwrap();
    assert_eq!(l["closed"][0]["id"], s.as_str(), "{l}");
    assert!(l["open"].as_array().unwrap().is_empty(), "{l}");
    // Its record can be deleted, confirmed.
    assert!(sreq(tx, Req::DeleteSession { session: s.clone(), confirmed: false }).is_err());
    sreq(tx, Req::DeleteSession { session: s.clone(), confirmed: true }).unwrap();
    let l = sreq(tx, Req::Sessions { project }).unwrap();
    assert!(l["closed"].as_array().unwrap().is_empty(), "{l}");
    assert!(sreq(tx, Req::Conversation { id: s }).is_err(), "the record is gone");
}

#[test]
fn undone_turns_are_named_once_and_their_refs_go() {
    let (e, repo, project, _root) = setup("us12-close-undone");
    let tx = &e.tx;
    let (s, branch, _wt) = one_turn(tx, &project);
    say_solo(tx, &s, "add a test");
    wait_turn(tx, &s, 2);
    sreq(tx, Req::Undo { session: s.clone(), turn: 2, confirmed: true }).unwrap();
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    let keep = format!("refs/fallow/solo/{s}/undone/2");
    assert!(!git(&repo, &["for-each-ref", &keep]).trim().is_empty(), "the undone turn is kept");
    let r = close(tx, &s, None, false).unwrap();
    let ask = &r["ask"];
    assert_eq!(ask["undone"][0]["turn"], 2, "{r}");
    assert!(ask["undone"][0]["commit"].is_string(), "{r}");
    assert!(ask["commits"].as_array().unwrap().is_empty() && ask["files"].as_array().unwrap().is_empty(), "{r}");
    assert!(git(&repo, &["for-each-ref", &keep]).trim().len() > 0, "nothing removed before the answer");
    let r = close(tx, &s, None, true).unwrap();
    assert_eq!(r["closed"], "merged", "{r}");
    assert!(git(&repo, &["for-each-ref", &keep]).trim().is_empty(), "the undone refs are gone");
}

#[test]
fn an_ignored_env_is_named_and_dependency_folders_are_counted() {
    let (e, repo, project, _root) = setup("us12-close-env");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    std::fs::write(wt.join(".env"), "TOKEN=x\n").unwrap();
    std::fs::create_dir_all(wt.join("node_modules/left")).unwrap();
    std::fs::write(wt.join("node_modules/left/index.js"), "").unwrap();
    std::fs::write(wt.join("node_modules/left/package.json"), "{}").unwrap();
    let r = close(tx, &s, None, false).unwrap();
    let ig = &r["ask"]["ignored"];
    assert_eq!(ig["files"], serde_json::json!([".env"]), "{r}");
    assert_eq!(ig["counts"]["node_modules"], 2, "{r}");
    assert!(wt.join(".env").exists(), "nothing removed before the answer");
}

#[test]
fn keep_commits_what_is_left_and_keeps_the_branch_discard_names_what_goes() {
    let (e, repo, project, _root) = setup("us12-close-keep");
    let tx = &e.tx;
    // Keep: an uncommitted change, not merged.
    let (s, branch, wt) = one_turn(tx, &project);
    std::fs::write(wt.join("notes.txt"), "mine\n").unwrap();
    let r = close(tx, &s, None, false).unwrap();
    let ask = &r["ask"];
    assert!(ask["files"].as_array().unwrap().iter().any(|f| f == "notes.txt"), "{r}");
    assert!(!ask["commits"].as_array().unwrap().is_empty(), "{r}");
    let r = close(tx, &s, Some("keep"), false).unwrap();
    assert_eq!(r["closed"], "kept", "{r}");
    assert_eq!(r["branch"], branch.as_str(), "{r}");
    assert!(!wt.exists());
    assert!(has_branch(&repo, &branch));
    assert_eq!(git(&repo, &["show", &format!("{branch}:notes.txt")]).trim(), "mine", "the change was committed");

    // Discard: the commits and files named, then the branch is gone.
    let s2 = start_session(tx, &project, "Fix it again");
    let v = wait_turn(tx, &s2, 1);
    let b2 = v["session"]["branch"].as_str().unwrap().to_string();
    let wt2 = PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    std::fs::write(wt2.join("scratch.txt"), "x\n").unwrap();
    let r = close(tx, &s2, None, false).unwrap();
    assert_eq!(r["ask"]["commits"].as_array().unwrap().len(), 1, "{r}");
    assert!(r["ask"]["commits"][0]["subject"].is_string(), "{r}");
    let r = close(tx, &s2, Some("discard"), false).unwrap();
    assert_eq!(r["closed"], "discarded", "{r}");
    assert!(!has_branch(&repo, &b2) && !wt2.exists());
}

#[test]
fn a_failed_keep_keeps_the_copy_and_remove_working_copy_follows() {
    let (e, repo, project, _root) = setup("us12-close-keepfail");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    std::fs::write(wt.join("notes.txt"), "mine\n").unwrap();
    let gitdir = PathBuf::from(git(&wt, &["rev-parse", "--absolute-git-dir"]).trim());
    std::fs::write(gitdir.join("index.lock"), "").unwrap();
    let r = close(tx, &s, Some("keep"), false).unwrap();
    assert_eq!(r["closed"], "kept_with_copy", "{r}");
    assert!(wt.join("notes.txt").exists(), "the copy is kept");
    assert!(has_branch(&repo, &branch));
    let _ = std::fs::remove_file(gitdir.join("index.lock"));
    assert!(sreq(tx, Req::RemoveCopy { session: s.clone(), confirmed: false }).is_err());
    let r = sreq(tx, Req::RemoveCopy { session: s.clone(), confirmed: true }).unwrap();
    assert!(r["files_lost"].as_array().unwrap().iter().any(|f| f == "notes.txt"), "{r}");
    assert!(!wt.exists());
    assert!(has_branch(&repo, &branch), "the branch stays");
}

#[test]
fn a_copy_git_cannot_read_is_named_and_keep_keeps_it() {
    let (e, repo, project, _root) = setup("us12-close-unreadable");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    // The copy's link to the repository is gone, its files still there.
    std::fs::write(wt.join("mine.txt"), "not listed by git
").unwrap();
    std::fs::remove_file(wt.join(".git")).unwrap();
    let r = close(tx, &s, None, true).unwrap();
    assert!(r["ask"]["unreadable"].is_string(), "never closed as clean: {r}");
    let r = close(tx, &s, Some("keep"), false).unwrap();
    assert_eq!(r["closed"], "kept_with_copy", "{r}");
    assert!(wt.join("mine.txt").exists(), "nothing deleted unread");
}

#[test]
fn a_restart_finishing_a_merged_close_keeps_what_appeared_since() {
    let (e, repo, project, root) = setup("us12-close-restart-merged");
    let (s, branch, wt);
    {
        let t = one_turn(&e.tx, &project);
        s = t.0;
        branch = t.1;
        wt = t.2;
    }
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    stop(e);
    // The intent was written; while Fallow was down the engineer added a file to the copy.
    std::fs::write(wt.join("later.txt"), "written while Fallow was down
").unwrap();
    let log = root.join("data/conversations").join(&s).join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let seq = text.lines().filter(|l| !l.trim().is_empty()).count() + 1;
    std::fs::write(&log, format!("{text}{{\"seq\":{seq},\"at\":\"2026-11-07T00:00:00Z\",\"type\":\"close_started\",\"choice\":\"merged\"}}
")).unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let v = view(&e.tx, &s);
    assert_ne!(v["session"]["closed"]["how"], "merged", "{}", v["session"]);
    assert!(has_branch(&repo, &branch), "the branch is kept");
    assert!(git(&repo, &["show", &format!("{branch}:later.txt")]).contains("written while") || wt.join("later.txt").exists(), "the file is kept");
}

#[test]
fn commits_on_a_detached_head_in_the_copy_are_named_and_kept() {
    let (e, repo, project, _root) = setup("us12-close-detached");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    git(&repo, &["merge", "-q", "--no-edit", &branch]);
    git(&wt, &["checkout", "-q", "--detach"]);
    std::fs::write(wt.join("loose.txt"), "on no branch
").unwrap();
    git(&wt, &["add", "loose.txt"]);
    git(&wt, &["commit", "-q", "-m", "loose work"]);
    let r = close(tx, &s, None, true).unwrap();
    assert_eq!(r["ask"]["head_off"][0]["subject"], "loose work", "{r}");
    let r = close(tx, &s, Some("keep"), false).unwrap();
    assert_eq!(r["closed"], "kept_with_copy", "{r}");
    assert!(wt.join("loose.txt").exists());
}

#[test]
fn a_choice_made_on_an_old_question_is_asked_again() {
    let (e, repo, project, _root) = setup("us12-close-stale");
    let tx = &e.tx;
    let (s, branch, wt) = one_turn(tx, &project);
    let first = sreq(tx, Req::CloseSession { session: s.clone(), choice: None, confirmed: false, digest: None }).unwrap();
    let shown = first["ask"]["digest"].as_str().unwrap().to_string();
    // Something appears after the question was shown: the discard is not done.
    std::fs::write(wt.join("late.txt"), "late\n").unwrap();
    let r = sreq(tx, Req::CloseSession { session: s.clone(), choice: Some("discard".into()), confirmed: false, digest: Some(shown) }).unwrap();
    assert!(r["ask"]["files"].as_array().unwrap().iter().any(|f| f == "late.txt"), "asked again: {r}");
    assert!(wt.join("late.txt").exists() && has_branch(&repo, &branch));
    // A choice without the question's digest is not done either.
    let r = sreq(tx, Req::CloseSession { session: s.clone(), choice: Some("discard".into()), confirmed: false, digest: None }).unwrap();
    assert!(r.get("ask").is_some(), "{r}");
}

#[test]
fn closing_during_a_turn_asks_to_stop_and_exit_is_a_message() {
    let (e, _repo, project, _root) = setup("us12-close-busy");
    let tx = &e.tx;
    let s = start_session(tx, &project, "slow one");
    wait_conv(tx, &s, "the turn at work", |v| v["busy"] == true);
    let r = close(tx, &s, Some("discard"), false).unwrap();
    assert_eq!(r["stop_first"], 1, "{r}");
    assert!(view(tx, &s)["session"]["closed"].is_null());
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "/exit");
    let v = wait_turn(tx, &s, 2);
    assert_eq!(v["turns"][1]["reply"], "Bye is just text to me.", "{}", v["turns"][1]);
    assert!(v["session"]["closed"].is_null(), "/exit closes nothing");
}

#[test]
fn a_restart_in_the_middle_of_a_close_finishes_it() {
    let (e, repo, project, root) = setup("us12-close-restart");
    let (s, branch, wt, digest);
    {
        let tx = &e.tx;
        let t = one_turn(tx, &project);
        s = t.0;
        branch = t.1;
        wt = t.2;
        digest = close(tx, &s, None, false).unwrap()["ask"]["digest"].as_str().unwrap().to_string();
    }
    stop(e);
    // As if Fallow went down right after writing the intent.
    let log = root.join("data/conversations").join(&s).join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let seq = text.lines().filter(|l| !l.trim().is_empty()).count() + 1;
    std::fs::write(&log, format!("{text}{{\"seq\":{seq},\"at\":\"2026-11-07T00:00:00Z\",\"type\":\"close_started\",\"choice\":\"discard\",\"digest\":\"{digest}\"}}\n")).unwrap();
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let v = view(&e.tx, &s);
    assert_eq!(v["session"]["closed"]["how"], "discarded", "{}", v["session"]);
    assert!(!wt.exists() && !has_branch(&repo, &branch));
}

/// 012 T050: the page for `checks/solo-close.png`: a session not merged, with an uncommitted
/// change, an ignored `.env` and a dependency folder, and a closed one in the list; served for a
/// headless browser (`FALLOW_VISUAL_ROOT`, `FALLOW_VISUAL_SECS`).
#[test]
#[ignore]
fn serve_the_012_close_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let repo = repo(&root);
    std::fs::write(repo.join(".gitignore"), ".env\nnode_modules/\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ignore"]);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario()).unwrap();
    let e = start_studio(&root, &sc, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    // A session closed earlier, its branch kept.
    let old = start_session(tx, &project, "Fix it again");
    wait_turn(tx, &old, 1);
    close(tx, &old, Some("keep"), false).unwrap();
    let (s, _branch, wt) = one_turn(tx, &project);
    say_solo(tx, &s, "add a test");
    wait_turn(tx, &s, 2);
    std::fs::write(wt.join("notes.txt"), "mine\n").unwrap();
    std::fs::write(wt.join(".env"), "TOKEN=x\n").unwrap();
    std::fs::create_dir_all(wt.join("node_modules/left")).unwrap();
    std::fs::write(wt.join("node_modules/left/index.js"), "").unwrap();
    sreq(tx, Req::OpenSession { session: s.clone() }).unwrap();
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), &project).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
