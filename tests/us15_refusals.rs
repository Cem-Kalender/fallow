//! 015 (FR-003, FR-009, FR-010; research R-3, R-4, R-8, R-10): in Auto the assistant is not
//! asked, its fixed limits refuse visibly (a record, and the reason in the agent's reply), a
//! refusal the runtime made by its own rule is recorded too and never allowed, output that may
//! hold secret values is hidden from the activity and Fallow's records, and switching to Auto
//! answers the request waiting for the engineer as Auto would.

mod common;

use common::*;
use fallow::studio::Req;
use fallow::studio::mode::ChatMode;
use serde_json::Value;

fn limits(v: &Value) -> Vec<String> {
    v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["refusals"].as_array().cloned().unwrap_or_default())
        .map(|r| r["limit"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Every file under `dir`, read as text where it is.
fn all_text(dir: &std::path::Path) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        // The working copies hold the project's files, the `.env` included: not Fallow's
        // records.
        if p.is_dir() && p.file_name().is_some_and(|n| n == "worktrees") {
            continue;
        }
        if p.is_dir() {
            out.push_str(&all_text(&p));
        } else if let Ok(t) = std::fs::read_to_string(&p) {
            out.push_str(&t);
        }
    }
    out
}

#[test]
fn auto_asks_nothing_refuses_at_the_limits_visibly_and_hides_secrets() {
    let root = scratch("us15-auto");
    let repo = repo(&root);
    std::fs::write(repo.join(".env"), "TOKEN=dotenv-secret-7731\n").unwrap();
    git(&repo, &["add", "-f", ".env"]);
    git(&repo, &["commit", "-q", "-m", "env"]);
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let in_folder = repo.join("from-the-session.txt").display().to_string();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            r#"[[step]]
kind = "assistant_turn"
edit = [{{ path = {in_folder}, text = "x" }}, {{ path = ".claude/settings.json", text = "{{}}" }}, {{ path = "notes.txt", text = "fine" }}]
read = [".env"]
exec = ["FALLOW_FLAG=1 env", "echo plain-output", "git push origin HEAD:main"]
output = {{ reply = "done" }}
"#,
            in_folder = lit(&in_folder)
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
    let s = start_session(tx, &project, "do it all");
    let v = wait_turn(tx, &s, 1);
    assert_eq!(v["session"]["mode"], "auto", "{}", v["session"]);
    assert_eq!(v["session"]["default_mode"], "auto");
    // Nothing was asked.
    assert!(
        v["turns"][0]["approvals"].as_array().unwrap().is_empty(),
        "{}",
        v["turns"][0]
    );
    // Three refusals: the engineer's folder, the runtime's settings file, the protected target
    // (no remote read yet: the checked-out branch counts as protected).
    let mut l = limits(&v);
    l.sort();
    assert_eq!(
        l,
        ["control", "edit", "target"],
        "{}",
        v["turns"][0]["refusals"]
    );
    for r in v["turns"][0]["refusals"].as_array().unwrap() {
        assert_eq!(r["state"], "refused", "{r}");
        assert!(!r["reason"].as_str().unwrap_or_default().is_empty(), "{r}");
    }
    assert!(
        !std::path::Path::new(&in_folder).exists(),
        "the folder is untouched"
    );
    // The agent was told Fallow's reason (the activity shows the refused calls).
    let activity = v["activity"].to_string();
    assert!(activity.contains("Fallow refused this"), "{activity}");
    // Within its copy it worked: the edit and the plain command.
    let wt = v["session"]["worktree"].as_str().unwrap().to_string();
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&wt).join("notes.txt")).unwrap(),
        "fine"
    );
    assert!(activity.contains("plain-output"), "{activity}");
    // Secret values never reach the page or Fallow's data folder.
    assert!(activity.contains("Output hidden"), "{activity}");
    // The environment the agent saw (it inherits this test's): its values are not shown.
    let path = std::env::var("PATH").unwrap();
    assert!(!activity.contains(&path), "{activity}");
    assert!(!activity.contains("dotenv-secret-7731"), "{activity}");
    let data = all_text(&root.join("data"));
    assert!(!data.contains(&path));
    assert!(!data.contains("dotenv-secret-7731"));
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn switching_to_auto_answers_the_waiting_request_as_auto_would() {
    let root = scratch("us15-switch");
    let repo = repo(&root);
    let folder = repo.display().to_string();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            r#"[[step]]
kind = "assistant_turn"
exec = ["echo waited"]
output = {{ reply = "one" }}

[[step]]
kind = "assistant_turn"
exec = [{folder_cmd}]
output = {{ reply = "two" }}
"#,
            folder_cmd = lit(&format!("git -C {folder} branch from-the-session"))
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
    // 1: asked in Manual; switching to Auto lets it run.
    let s = start_session(tx, &project, "first");
    wait_conv(tx, &s, "a command waiting", |v| {
        !v["pending_approval"].is_null()
    });
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let v = wait_turn(tx, &s, 1);
    let a = &v["turns"][0]["approvals"][0];
    assert_eq!(a["answer"], "once", "{a}");
    assert!(
        v["activity"].to_string().contains("waited"),
        "{}",
        v["activity"]
    );
    assert_eq!(v["mode_changes"][0]["mode"], "auto");
    // 2: back in Manual, a command that changes the engineer's folder is asked with its
    // facts; switching to Auto refuses it at the limit instead.
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "manual".into(),
        },
    )
    .unwrap();
    say_solo(tx, &s, "second");
    let w = wait_conv(tx, &s, "a folder command waiting", |v| {
        !v["pending_approval"].is_null()
    });
    assert!(
        !w["pending_approval"]["facts"].is_null(),
        "{}",
        w["pending_approval"]
    );
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let v = wait_turn(tx, &s, 2);
    assert_eq!(
        v["turns"][1]["approvals"][0]["answer"], "deny",
        "{}",
        v["turns"][1]
    );
    assert_eq!(limits(&v), ["edit"]);
    assert!(
        !git(&repo, &["branch", "--list", "from-the-session"]).contains("from-the-session"),
        "the folder is untouched"
    );
    // The old names are not modes any more.
    assert!(
        sreq(
            tx,
            Req::SessionMode {
                session: s.clone(),
                mode: "until_switch".into(),
            },
        )
        .is_err()
    );
    let _ = ChatMode::Auto;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
