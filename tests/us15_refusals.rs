//! 015 (FR-003, FR-009, FR-010; research R-3, R-4, R-8, R-10; 2026-11-24 amendment): in Auto the
//! assistant runs everything but its fixed limits; at the folder and target limits it waits for
//! the engineer's answer (Allow once, Always allow, Deny) on the same card as Manual's; a
//! control file is refused with no allow at all; output that may hold secret values is hidden
//! from the activity and Fallow's records; switching to Auto answers the waiting request as Auto
//! would.

mod common;

use common::*;
use fallow::conversation::events::ApprovalAnswer;
use fallow::studio::Req;
use fallow::studio::mode::ChatMode;
use serde_json::Value;

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

fn limits(v: &Value) -> Vec<String> {
    v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["refusals"].as_array().cloned().unwrap_or_default())
        .map(|r| r["limit"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn refusals(v: &Value) -> Vec<Value> {
    v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["refusals"].as_array().cloned().unwrap_or_default())
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

/// The engineer's answer to the waiting card.
fn answer(
    tx: &Tx,
    conv: &str,
    id: &str,
    a: ApprovalAnswer,
    prefix: Option<&str>,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::Approve {
            conv: conv.into(),
            id: id.into(),
            answer: a,
            prefix: prefix.map(str::to_string),
        },
    )
}

/// The card waiting in `conv`, once there is one.
fn waiting(tx: &Tx, conv: &str) -> Value {
    wait_conv(tx, conv, "a request waiting at a limit", |v| {
        !v["pending_approval"].is_null()
    })["pending_approval"]
        .clone()
}

/// Denies every card until turn `n` ends: the limits they waited at, in order, and the view.
fn deny_all(tx: &Tx, conv: &str, n: usize) -> (Vec<String>, Value) {
    let mut held = Vec::new();
    loop {
        let v = wait_conv(tx, conv, "a card or the turn's end", |v| {
            !v["pending_approval"].is_null()
                || (v["busy"] == false
                    && v["turns"].as_array().is_some_and(|t| t.len() >= n)
                    && !v["turns"][n - 1]["reply"].is_null())
        });
        let a = &v["pending_approval"];
        if a.is_null() {
            return (held, v);
        }
        held.push(a["limit"].as_str().unwrap_or("none").to_string());
        answer(
            tx,
            conv,
            a["id"].as_str().unwrap(),
            ApprovalAnswer::Deny,
            None,
        )
        .unwrap();
    }
}

fn needs(tx: &Tx, kind: &str) -> Vec<Value> {
    ask(tx, fallow::run::engine::Request::Dashboard).unwrap()["needs_you"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n["kind"] == kind)
        .collect()
}

#[test]
fn auto_waits_at_the_limits_refuses_control_files_and_hides_secrets() {
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
exec = ["FALLOW_FLAG=1 env", "echo plain-output", "git push origin HEAD:main", {shell_folder}, "echo '{{}}' > .claude/settings.local.json"]
output = {{ reply = "done" }}
"#,
            in_folder = lit(&in_folder),
            shell_folder = lit(&format!("echo from-auto > {}", repo.join("shell.txt").display()))
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
    // The folder edit, the push onto the protected target (no remote read yet: the checked-out
    // branch counts) and the shell write in the folder wait for the engineer, in that order:
    // each denied, the agent goes on.
    let (held, v) = deny_all(tx, &s, 1);
    assert_eq!(held, ["edit", "target", "edit"], "{}", v["turns"][0]);
    assert_eq!(v["session"]["mode"], "auto", "{}", v["session"]);
    assert_eq!(v["session"]["default_mode"], "auto");
    for a in v["turns"][0]["approvals"].as_array().unwrap() {
        assert_eq!(a["answer"], "deny", "{a}");
        assert!(a["limit"].is_string(), "{a}");
    }
    // The two runtime settings files: refused at once, never held, never allowed.
    assert_eq!(
        limits(&v),
        ["control", "control"],
        "{}",
        v["turns"][0]["refusals"]
    );
    for r in v["turns"][0]["refusals"].as_array().unwrap() {
        assert_eq!(r["state"], "refused", "{r}");
        assert!(!r["reason"].as_str().unwrap_or_default().is_empty(), "{r}");
    }
    assert!(
        !repo.join("shell.txt").exists(),
        "the shell write did not happen"
    );
    assert!(
        !std::path::Path::new(&in_folder).exists(),
        "the folder is untouched"
    );
    // The agent was told Fallow's reason (the activity shows the denied and refused calls).
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
    let a = waiting(tx, &s);
    assert!(a["limit"].is_null(), "an ordinary Manual card: {a}");
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
    // 2: back in Manual, a command that changes the engineer's folder waits at the folder
    // limit with its facts; switching to Auto keeps it waiting there (a new card).
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "manual".into(),
        },
    )
    .unwrap();
    say_solo(tx, &s, "second");
    let a = waiting(tx, &s);
    assert_eq!(a["limit"], "edit", "{a}");
    assert!(!a["facts"].is_null(), "{a}");
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let w = wait_conv(tx, &s, "the card asked again under Auto", |v| {
        v["pending_approval"]["id"].is_string() && v["pending_approval"]["id"] != a["id"]
    });
    assert_eq!(w["pending_approval"]["limit"], "edit");
    answer(
        tx,
        &s,
        w["pending_approval"]["id"].as_str().unwrap(),
        ApprovalAnswer::Deny,
        None,
    )
    .unwrap();
    let v = wait_turn(tx, &s, 2);
    let ap = v["turns"][1]["approvals"].as_array().unwrap();
    assert_eq!(ap.len(), 2, "{}", v["turns"][1]);
    assert!(ap.iter().all(|a| a["answer"] == "deny"), "{ap:?}");
    assert!(limits(&v).is_empty());
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
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 2026-11-24 amendment: in Auto, a command writing the engineer's folder holds the agent's
/// request open and shows a card ("Needs you" too, whose notification names the chat only).
/// Allow once runs it right then; Deny tells the agent R-4's reason, and nothing is written.
/// Nothing is queued: no turn follows on its own.
#[test]
fn a_folder_write_waits_allow_once_runs_it_and_deny_denies_it() {
    let root = scratch("us15-wait");
    let repo = repo(&root);
    let a = repo.join("a.txt");
    let b = repo.join("b.txt");
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            "{}{}",
            assistant_step(
                "wrote a",
                &format!("exec = [{}]", lit(&format!("echo one > {}", a.display())))
            ),
            assistant_step(
                "wrote b",
                &format!("exec = [{}]", lit(&format!("echo two > {}", b.display())))
            )
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
    let s = start_session(tx, &project, "write a");
    let card = waiting(tx, &s);
    assert_eq!(card["limit"], "edit", "{card}");
    assert_eq!(card["kind"], "run", "{card}");
    assert!(
        card["always"].is_string(),
        "Always allow is offered: {card}"
    );
    assert!(
        card["reason"].as_str().unwrap().contains("project folder"),
        "{card}"
    );
    assert!(!a.exists(), "it waits: nothing ran yet");
    let n = needs(tx, "limit");
    assert_eq!(n.len(), 1, "{n:?}");
    let notice = fallow::service::notify::notice(&n[0], None);
    assert!(!notice.body.contains("a.txt"), "{}", notice.body);
    assert!(notice.url.contains("session="), "{}", notice.url);
    // Allow once: it runs now, in the same turn.
    answer(
        tx,
        &s,
        card["id"].as_str().unwrap(),
        ApprovalAnswer::Once,
        None,
    )
    .unwrap();
    let v = wait_turn(tx, &s, 1);
    assert_eq!(std::fs::read_to_string(&a).unwrap().trim(), "one");
    assert!(needs(tx, "limit").is_empty());
    assert!(refusals(&v).is_empty());
    // Deny: the agent is told why, and nothing is written.
    say_solo(tx, &s, "write b");
    let card = waiting(tx, &s);
    answer(
        tx,
        &s,
        card["id"].as_str().unwrap(),
        ApprovalAnswer::Deny,
        None,
    )
    .unwrap();
    let v = wait_turn(tx, &s, 2);
    assert!(!b.exists());
    assert!(
        v["activity"].to_string().contains("Fallow refused this"),
        "{}",
        v["activity"]
    );
    // Nothing waits behind the turns.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let v = sreq(tx, Req::Conversation { id: s.clone() }).unwrap();
    assert_eq!(v["turns"].as_array().unwrap().len(), 2);
    assert!(v["session"]["queue"].as_array().unwrap().is_empty());
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 2026-11-24 amendment: "Always allow" on a push onto the protected target records an entry
/// beside the approved commands; the next matching push then runs without a card, in Auto and in
/// Manual. A prefix that is not the command's start is refused; the entry can be removed.
#[test]
fn always_allow_past_the_target_limit_in_both_modes() {
    let root = scratch("us15-always");
    let repo = repo(&root);
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let push = "git push origin HEAD:main";
    let step = |r: &str| assistant_step(r, &format!("exec = [{}]", lit(push)));
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}{}", step("one"), step("two"), step("three")),
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
    // A remote whose protection Fallow cannot read: its target counts as protected.
    bare_remote(&root, &repo, "https://example.invalid/team/calc.git");
    let s = start_session(tx, &project, "push");
    let card = waiting(tx, &s);
    assert_eq!(card["limit"], "target", "{card}");
    let id = card["id"].as_str().unwrap();
    assert!(
        answer(tx, &s, id, ApprovalAnswer::Always, Some("git pull")).is_err(),
        "not the command's start"
    );
    answer(
        tx,
        &s,
        id,
        ApprovalAnswer::Always,
        Some("git push origin HEAD:main"),
    )
    .unwrap();
    wait_turn(tx, &s, 1);
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    let entries = p["allowed_past_limits"].as_array().cloned().unwrap();
    assert_eq!(entries.len(), 1, "{p}");
    assert_eq!(entries[0]["limit"], "target");
    assert_eq!(entries[0]["prefix"], "git push origin HEAD:main");
    assert!(
        p["approved_commands"].as_array().unwrap().is_empty(),
        "not an approved command"
    );
    // Auto: the same push runs with no card.
    say_solo(tx, &s, "push again");
    let v = wait_turn(tx, &s, 2);
    assert!(
        v["turns"][1]["approvals"].as_array().unwrap().is_empty(),
        "{}",
        v["turns"][1]
    );
    // Manual: too.
    sreq(
        tx,
        Req::SessionMode {
            session: s.clone(),
            mode: "manual".into(),
        },
    )
    .unwrap();
    say_solo(tx, &s, "and again");
    let v = wait_turn(tx, &s, 3);
    assert!(
        v["turns"][2]["approvals"].as_array().unwrap().is_empty(),
        "{}",
        v["turns"][2]
    );
    assert!(refusals(&v).is_empty());
    // Removed on the project's page; one never given cannot be added there.
    let bogus = serde_json::json!({ "allowed_past_limits": [{ "limit": "edit", "kind": "run", "prefix": "rm" }] });
    assert!(
        sreq(
            tx,
            Req::PatchProject {
                id: project.clone(),
                patch: bogus
            }
        )
        .is_err()
    );
    sreq(
        tx,
        Req::PatchProject {
            id: project.clone(),
            patch: serde_json::json!({ "allowed_past_limits": [] }),
        },
    )
    .unwrap();
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(
        p["allowed_past_limits"].as_array().unwrap().is_empty(),
        "{p}"
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// Constitution VI: a control-file write is refused at once, in Auto, with no card: no Allow
/// once, no Always allow; only Dismiss, which clears its "Needs you" item.
#[test]
fn a_control_file_write_is_refused_with_no_allow() {
    let root = scratch("us15-control");
    let repo = repo(&root);
    std::fs::write(
        root.join("settings.extra.toml"),
        "[defaults]\nchat_mode = \"auto\"\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        assistant_step(
            "tried",
            "exec = [\"echo '{}' > .claude/settings.local.json\"]",
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
    let s = start_session(tx, &project, "change the settings");
    let v = wait_turn(tx, &s, 1);
    assert!(
        v["turns"][0]["approvals"].as_array().unwrap().is_empty(),
        "no card"
    );
    let rs = refusals(&v);
    assert_eq!(rs.len(), 1, "{rs:?}");
    assert_eq!(rs[0]["limit"], "control");
    let id = rs[0]["id"].as_str().unwrap();
    assert_eq!(needs(tx, "refused").len(), 1);
    let once = sreq(
        tx,
        Req::Refusal {
            conv: s.clone(),
            id: id.into(),
            answer: "allow_once".into(),
        },
    );
    assert!(once.unwrap_err().contains("never editable"));
    sreq(
        tx,
        Req::Refusal {
            conv: s.clone(),
            id: id.into(),
            answer: "dismiss".into(),
        },
    )
    .unwrap();
    assert!(needs(tx, "refused").is_empty());
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert!(p["allowed_past_limits"].as_array().unwrap().is_empty());
    let wt = std::path::PathBuf::from(v["session"]["worktree"].as_str().unwrap());
    assert!(!wt.join(".claude/settings.local.json").exists());
    let _ = ChatMode::Auto;
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
