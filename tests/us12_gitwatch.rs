//! 012 US4 (T034; quickstart Q4, Q5; FR-016, FR-017, SC-004, SC-010; research R-10, R-11): git
//! by asking, every change shown. A squash merge into the folder is one approval with its facts,
//! and it shows as the session's, recognized as a squash, moving the base; a commit made in the
//! folder from a terminal shows as "outside Fallow" within 5 s; a push shows the remote-tracking
//! ref created by the session; a command changing the folder carries the folder's branch and
//! uncommitted changes; a conflicting merge leaves the folder `conflicted`; a spec session's own
//! commits are Fallow's, in the quiet group, never "outside".

mod common;

use common::*;
use fallow::conversation::events::ApprovalAnswer;
use fallow::studio::Req;
use serde_json::Value;

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

fn project_with(root: &std::path::Path, scenario: &str) -> (fallow::service::ToolEngine, std::path::PathBuf, String) {
    let repo = repo(root);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "a"]);
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, scenario).unwrap();
    let e = start_studio(root, &sc, None);
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project)
}

fn changes(tx: &Tx, project: &str) -> Value {
    sreq(tx, Req::GitChanges { project: project.to_string() }).unwrap()
}

fn change_of<'a>(list: &'a Value, refname: &str) -> Option<&'a Value> {
    list.as_array().unwrap().iter().find(|c| c["refname"] == refname)
}

/// Waits for a pending approval and answers it once.
fn allow_next(tx: &Tx, s: &str) -> Value {
    let w = wait_conv(tx, s, "a command waiting", |v| !v["pending_approval"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(tx, Req::Approve { conv: s.to_string(), id, answer: ApprovalAnswer::Once, prefix: None }).unwrap();
    w
}

#[test]
fn a_squash_merge_is_one_approval_and_shows_as_the_sessions_squash() {
    let root = scratch("us12-gw-squash");
    let repo_path = root.join("repo").display().to_string().replace('\\', "/");
    let scenario = format!(
        r#"
[[step]]
kind = "assistant_turn"
when = "make b"
files = [{{ path = "b.txt", text = "b\n" }}]
output = {{ reply = "made b" }}

[[step]]
kind = "assistant_turn"
when = "squash it into main"
exec = ["git -C {repo_path} merge --squash fallow/solo/make-b && git -C {repo_path} commit -q -m squash"]
output = {{ reply = "squashed" }}
"#
    );
    let (e, repo, project) = project_with(&root, &scenario);
    let tx = &e.tx;
    let s = start_session(tx, &project, "make b");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "squash it into main");
    // One approval, with what it does to the folder.
    let w = allow_next(tx, &s);
    let f = &w["pending_approval"]["facts"];
    assert_eq!(f["folder_branch"], "main", "{f}");
    assert_eq!(f["merge"]["squash"], true, "{f}");
    assert_eq!(f["merge"]["commits"].as_array().unwrap().len(), 1, "one snapshot commit: {f}");
    assert_eq!(f["merge"]["own_commits"], "unknown");
    let v = wait_turn(tx, &s, 2);
    assert_eq!(v["turns"][1]["approvals"].as_array().unwrap().len(), 1, "one approval");
    let main = git(&repo, &["rev-parse", "main"]);
    // main moved by one commit, the session named as its source.
    let ch = changes(tx, &project);
    let c = change_of(&ch["changes"], "refs/heads/main").unwrap_or_else(|| panic!("{ch}"));
    // Only the squash is the session's: what came before it is not.
    let theirs: Vec<&Value> = ch["changes"].as_array().unwrap().iter().filter(|c| c["source"]["kind"] == "session").collect();
    assert_eq!(theirs.len(), 1, "{ch}");
    assert_eq!(c["after"], main.as_str(), "{ch}");
    assert_eq!(c["how"], "commits");
    assert_eq!(c["n"], 1);
    assert_eq!(c["source"]["kind"], "session", "{c}");
    assert_eq!(c["source"]["id"], s.as_str());
    // Recognized as a squash up to turn 1; the base moved; the folder's state recorded.
    let sess = &v["session"];
    assert_eq!(sess["squashes"][0]["upto_turn"], 1, "{sess}");
    assert_eq!(sess["squashes"][0]["commit"], main.as_str());
    assert_eq!(sess["base"]["gates"], main.as_str(), "{sess}");
    assert!(sess["folder"]["state"].is_string(), "{sess}");
    assert_eq!(sess["observed"][0]["refname"], "refs/heads/main");
}

#[test]
fn a_terminal_commit_shows_as_outside_fallow_within_five_seconds() {
    let root = scratch("us12-gw-outside");
    let (e, repo, project) = project_with(&root, "");
    let tx = &e.tx;
    // The watch has seen the project once.
    std::thread::sleep(std::time::Duration::from_millis(2500));
    std::fs::write(repo.join("t.txt"), "terminal\n").unwrap();
    git(&repo, &["add", "t.txt"]);
    git(&repo, &["commit", "-q", "-m", "from a terminal"]);
    let t0 = std::time::Instant::now();
    let c = loop {
        let ch = changes(tx, &project);
        if let Some(c) = change_of(&ch["changes"], "refs/heads/main") {
            break c.clone();
        }
        assert!(t0.elapsed() < std::time::Duration::from_secs(5), "not seen within 5 s: {ch}");
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    assert_eq!(c["source"]["kind"], "outside", "{c}");
    assert_eq!(c["quiet"], false);
}

#[test]
fn a_push_shows_the_remote_ref_created_by_the_session_and_folder_commands_carry_its_facts() {
    let root = scratch("us12-gw-push");
    let bare = root.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare"]);
    let repo_path = root.join("repo").display().to_string().replace('\\', "/");
    let scenario = format!(
        r#"
[[step]]
kind = "assistant_turn"
when = "make p"
files = [{{ path = "p.txt", text = "p\n" }}]
output = {{ reply = "made p" }}

[[step]]
kind = "assistant_turn"
when = "push it"
exec = ["git push -q origin fallow/solo/make-p"]
output = {{ reply = "pushed" }}

[[step]]
kind = "assistant_turn"
when = "reset the folder"
exec = ["git -C {repo_path} reset --hard"]
output = {{ reply = "asked" }}
"#
    );
    let (e, repo, project) = project_with(&root, &scenario);
    git(&repo, &["remote", "add", "origin", &bare.display().to_string()]);
    let tx = &e.tx;
    let s = start_session(tx, &project, "make p");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "push it");
    allow_next(tx, &s);
    wait_turn(tx, &s, 2);
    let ch = changes(tx, &project);
    let c = change_of(&ch["changes"], "refs/remotes/origin/fallow/solo/make-p").unwrap_or_else(|| panic!("{ch}"));
    assert_eq!(c["how"], "created");
    assert_eq!(c["source"]["kind"], "session", "{c}");
    // A command that changes the folder (not a merge) carries its branch and uncommitted changes.
    std::fs::write(repo.join("dirty.txt"), "mine\n").unwrap();
    say_solo(tx, &s, "reset the folder");
    let w = wait_conv(tx, &s, "the reset asked", |v| !v["pending_approval"].is_null());
    let f = &w["pending_approval"]["facts"];
    assert_eq!(f["folder_branch"], "main", "{f}");
    assert!(f["uncommitted"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("dirty.txt")), "{f}");
    assert!(f["merge"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(tx, Req::Approve { conv: s.clone(), id, answer: ApprovalAnswer::Deny, prefix: None }).unwrap();
    wait_turn(tx, &s, 3);
    assert!(repo.join("dirty.txt").exists(), "denied: nothing changed");
}

#[test]
fn a_conflicting_merge_leaves_the_folder_conflicted_and_a_spec_sessions_commits_stay_fallows() {
    let root = scratch("us12-gw-conflict");
    let repo_path = root.join("repo").display().to_string().replace('\\', "/");
    let scenario = format!(
        r##"
[[step]]
kind = "assistant_turn"
when = "change a"
files = [{{ path = "a.txt", text = "session\n" }}]
output = {{ reply = "changed a" }}

[[step]]
kind = "assistant_turn"
when = "merge it"
exec = ["git -C {repo_path} merge -q fallow/solo/change-a"]
output = {{ reply = "merged?" }}

[[step]]
kind = "lead_turn"
files = [{{ path = "specs/001-charts/spec.md", text = "# Charts\n" }}]
output = {{ reply = "Drafted.", step = "specify" }}
"##
    );
    let (e, repo, project) = project_with(&root, &scenario);
    let tx = &e.tx;
    let s = start_session(tx, &project, "change a");
    wait_turn(tx, &s, 1);
    // The folder's main changes the same line.
    std::fs::write(repo.join("a.txt"), "folder\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "folder edit"]);
    say_solo(tx, &s, "merge it");
    allow_next(tx, &s);
    let v = wait_turn(tx, &s, 2);
    assert_eq!(v["session"]["folder"]["state"], "conflicted", "{}", v["session"]);
    git(&repo, &["merge", "--abort"]);
    // A spec session's commits are Fallow's own, folded, never "outside".
    let f = sreq(tx, Req::NewFeature { project: project.clone(), title: "Charts".into(), lead: None }).unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    sreq(tx, Req::Say { conv: conv.clone(), text: "Draft it.".into() }).unwrap();
    turn_ended(tx, &conv, 1);
    std::thread::sleep(std::time::Duration::from_millis(3000));
    let ch = changes(tx, &project);
    assert!(change_of(&ch["changes"], "refs/heads/001-charts").is_none(), "not in the main list: {ch}");
    let q = change_of(&ch["quiet"], "refs/heads/001-charts").unwrap_or_else(|| panic!("{ch}"));
    assert_eq!(q["source"]["kind"], "spec", "{q}");
}

/// 012 T038: the page's demo. Not a test: run it by hand with `FALLOW_VISUAL_ROOT=<empty folder>
/// cargo test --test us12_gitwatch serve_the_012_git_demo -- --ignored --nocapture`; it writes the
/// page's address to `<root>/url.txt` and `<project>` to `<root>/projects.txt`, and serves for
/// `FALLOW_VISUAL_SECS` seconds (default 1800). The session squashed its work into main, a
/// terminal commit followed, and a command changing the folder waits with its facts.
#[test]
#[ignore]
fn serve_the_012_git_demo() {
    let root = std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let repo_path = root.join("repo").display().to_string().replace('\\', "/");
    let scenario = format!(
        r#"
[[step]]
kind = "assistant_turn"
when = "Fix the week"
files = [{{ path = "picker.js", text = "const firstDay = 1;\n" }}]
output = {{ reply = "Weeks start on Monday now." }}

[[step]]
kind = "assistant_turn"
when = "Squash it into main"
exec = ["git -C {repo_path} merge --squash fallow/solo/fix-the-week && git -C {repo_path} commit -q -m \"Fix the week\""]
output = {{ reply = "Squashed into **main**." }}

[[step]]
kind = "assistant_turn"
when = "Clean the folder"
exec = ["git -C {repo_path} reset --hard"]
output = {{ reply = "asked" }}
"#
    );
    let (e, repo, project) = project_with(&root, &scenario);
    let tx = &e.tx;
    let s = start_session(tx, &project, "Fix the week");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "Squash it into main");
    allow_next(tx, &s);
    wait_turn(tx, &s, 2);
    std::thread::sleep(std::time::Duration::from_millis(2500));
    std::fs::write(repo.join("notes.md"), "from a terminal\n").unwrap();
    git(&repo, &["add", "notes.md"]);
    git(&repo, &["commit", "-q", "-m", "notes"]);
    std::fs::write(repo.join("draft.md"), "mine\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3000));
    say_solo(tx, &s, "Clean the folder");
    wait_conv(tx, &s, "the reset asked", |v| !v["pending_approval"].is_null());
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), &project).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}

#[test]
fn a_fast_forward_to_the_sessions_commit_is_no_squash_and_writes_nothing_in_the_session() {
    let root = scratch("us12-gw-ff");
    let (e, repo, project) = project_with(
        &root,
        r#"
[[step]]
kind = "assistant_turn"
when = "make f"
files = [{ path = "f.txt", text = "f\n" }]
output = { reply = "made f" }
"#,
    );
    let tx = &e.tx;
    let s = start_session(tx, &project, "make f");
    wait_turn(tx, &s, 1);
    std::thread::sleep(std::time::Duration::from_millis(2500));
    // The engineer fast-forwards main to the session's commit, from a terminal.
    git(&repo, &["merge", "-q", "--ff-only", "fallow/solo/make-f"]);
    let t0 = std::time::Instant::now();
    loop {
        let ch = changes(tx, &project);
        if change_of(&ch["changes"], "refs/heads/main").is_some() {
            break;
        }
        assert!(t0.elapsed() < std::time::Duration::from_secs(6), "{ch}");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
    assert!(v["session"]["squashes"].as_array().unwrap().is_empty(), "{}", v["session"]);
    assert!(v["session"]["observed"].as_array().unwrap().is_empty(), "not the session's command");
}
