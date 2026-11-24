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
        ["control", "control", "edit", "edit", "target"],
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

fn refusals(v: &Value) -> Vec<Value> {
    v["turns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["refusals"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn refusal_answer(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    conv: &str,
    id: &str,
    answer: &str,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::Refusal {
            conv: conv.into(),
            id: id.into(),
            answer: answer.into(),
        },
    )
}

fn needs(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>) -> Vec<Value> {
    ask(tx, fallow::run::engine::Request::Dashboard).unwrap()["needs_you"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n["kind"] == "refused")
        .collect()
}

/// 015 US2 (FR-009, SC-005; R-4, R-5, R-9): in Manual (the assistant) and in Auto (the lead), a
/// request outside the agent's fixed limits is refused, recorded, told to the agent and listed
/// in "Needs you", whose notification names the chat only. "Allow this once" lets exactly that
/// request through once (a push onto the protected target included) and tells the agent;
/// a different request and a third identical one are refused again. A runtime's own settings
/// file is never allowed. "Dismiss" clears the item. A grant not used before a restart expires.
#[test]
fn refusals_are_never_silent() {
    let root = scratch("us15-never-silent");
    let repo = repo(&root);
    let x = repo.join("x.txt").display().to_string();
    let y = repo.join("y.txt").display().to_string();
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!(
            r#"[[step]]
kind = "assistant_turn"
edit = [{{ path = {x}, text = "one" }}, {{ path = ".claude/settings.json", text = "{{}}" }}]
output = {{ reply = "tried" }}

[[step]]
kind = "assistant_turn"
when = "allowed, once, exactly"
edit = [{{ path = {x}, text = "two" }}, {{ path = {y}, text = "other" }}]
output = {{ reply = "again" }}

[[step]]
kind = "assistant_turn"
edit = [{{ path = {x}, text = "three" }}]
output = {{ reply = "third" }}

[[step]]
kind = "lead_turn"
edit = [{{ path = "src/main.rs", text = "fn main() {{}}" }}]
approve = "git push origin HEAD:main"
output = {{ reply = "$APPROVED", step = "none" }}

[[step]]
kind = "lead_turn"
when = "allowed, once, exactly"
approve = "git push origin HEAD:main"
output = {{ reply = "$APPROVED", step = "none" }}

[[step]]
kind = "lead_turn"
output = {{ reply = "nothing", step = "none" }}
"#,
            x = lit(&x),
            y = lit(&y)
        ),
    )
    .unwrap();
    let lead;
    {
        let e = start_studio(&root, &scenario, None);
        let tx = &e.tx;
        let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
            .as_str()
            .unwrap()
            .to_string();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "spec kit"]);

        // The assistant, in Manual: its folder edit and the runtime's settings file.
        let s = start_session(tx, &project, "edit the folder");
        let started = std::time::Instant::now();
        let v = wait_turn(tx, &s, 1);
        let rs = refusals(&v);
        let limit = |l: &str| rs.iter().find(|r| r["limit"] == l).cloned().unwrap();
        let (edit, control) = (limit("edit"), limit("control"));
        assert!(
            edit["reason"].as_str().unwrap().contains("project folder"),
            "{edit}"
        );
        assert!(v["activity"].to_string().contains("Fallow refused this"));
        let n = needs(tx);
        assert_eq!(n.len(), 2, "{n:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        // The notification names the project and the chat, never the path.
        for item in &n {
            let notice = fallow::service::notify::notice(item, None);
            assert!(
                !notice.body.contains("x.txt") && !notice.body.contains("settings"),
                "{}",
                notice.body
            );
            assert!(notice.url.contains("session="), "{}", notice.url);
        }
        // The runtime's settings file: never allowed once; dismissing clears its item.
        let cid = control["id"].as_str().unwrap();
        assert!(
            refusal_answer(tx, &s, cid, "allow_once")
                .unwrap_err()
                .contains("never editable")
        );
        refusal_answer(tx, &s, cid, "dismiss").unwrap();
        assert!(
            refusal_answer(tx, &s, cid, "dismiss").is_err(),
            "already answered"
        );
        assert_eq!(needs(tx).len(), 1);
        // Allowed once: the agent is told, the same edit goes through once, a different one
        // does not, and a third identical one is refused again.
        let eid = edit["id"].as_str().unwrap();
        let told = refusal_answer(tx, &s, eid, "allow_once").unwrap();
        assert_eq!(told["told"], "sent", "{told}");
        let v = wait_turn(tx, &s, 2);
        assert!(
            v["turns"][1]["said"].as_str().unwrap().contains("x.txt"),
            "{}",
            v["turns"][1]
        );
        assert_eq!(std::fs::read_to_string(&x).unwrap(), "two");
        assert!(!std::path::Path::new(&y).exists());
        let st: Vec<String> = refusals(&v)
            .iter()
            .map(|r| r["state"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(st, ["used", "dismissed", "refused"], "{}", v["turns"]);
        say_solo(tx, &s, "once more");
        let v = wait_turn(tx, &s, 3);
        assert_eq!(std::fs::read_to_string(&x).unwrap(), "two", "refused again");
        assert_eq!(refusals(&v).len(), 4);

        // The lead, in Auto: an edit outside the spec documents and a push onto the target.
        lead = sreq(
            tx,
            Req::NewFeature {
                project: project.clone(),
                title: "Limits".into(),
                lead: None,
            },
        )
        .unwrap()["conversation"]
            .as_str()
            .unwrap()
            .to_string();
        sreq(
            tx,
            Req::SetMode {
                conv: lead.clone(),
                mode: ChatMode::Auto,
            },
        )
        .unwrap();
        sreq(
            tx,
            Req::Say {
                conv: lead.clone(),
                text: "go".into(),
            },
        )
        .unwrap();
        let l = turn_ended(tx, &lead, 1);
        assert_eq!(l["turns"][0]["reply"], "deny", "{}", l["turns"][0]);
        let rs = refusals(&l);
        let mut lim: Vec<&str> = rs.iter().map(|r| r["limit"].as_str().unwrap()).collect();
        lim.sort();
        assert_eq!(lim, ["edit", "target"], "{rs:?}");
        let push = rs.iter().find(|r| r["limit"] == "target").unwrap();
        let edit = rs.iter().find(|r| r["limit"] == "edit").unwrap();
        assert!(
            needs(tx)
                .iter()
                .any(|n| n["conv"] == lead.as_str() && n["solo"] == false)
        );
        // The push allowed once: the lead is told and its retry goes through.
        refusal_answer(tx, &lead, push["id"].as_str().unwrap(), "allow_once").unwrap();
        let l = turn_ended(tx, &lead, 2);
        assert_eq!(l["turns"][1]["reply"], "allow", "{}", l["turns"][1]);
        // The edit allowed once, but the lead never tries again before Fallow stops.
        refusal_answer(tx, &lead, edit["id"].as_str().unwrap(), "allow_once").unwrap();
        turn_ended(tx, &lead, 3);
        stop(e);
    }
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
    let st: Vec<String> = refusals(&l)
        .iter()
        .map(|r| r["state"].as_str().unwrap().to_string())
        .collect();
    assert!(st.contains(&"expired".to_string()), "{st:?}");
    assert!(st.contains(&"used".to_string()), "{st:?}");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 015 US2 (R-5; review of US2): "Allow this once" while the agent still works is told when its
/// turn ends (the assistant through its waiting messages, the lead through Fallow's note) and
/// the retry goes through; a refusal shows within 2 s while the turn works; the lead's allowed
/// edit is kept at its turn's end, not thrown away; a refusal in an answer the engineer
/// cancelled leaves "Needs you" and cannot be allowed.
#[test]
fn allowing_while_the_agent_works_and_a_cancelled_answer() {
    let root = scratch("us15-busy");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r#"[[step]]
kind = "assistant_turn"
exec = ["git push origin HEAD:main || true"]
sleep_ms = 2500
output = { reply = "pushed?" }

[[step]]
kind = "assistant_turn"
when = "allowed, once, exactly"
exec = ["git push origin HEAD:main || true"]
output = { reply = "again" }

[[step]]
kind = "lead_turn"
approve = "git push origin HEAD:main"
edit = [{ path = "src/main.rs", text = "fn main() {}" }]
sleep_ms = 2500
output = { reply = "$APPROVED", step = "none" }

[[step]]
kind = "lead_turn"
when = "allowed, once, exactly"
approve = "git push origin HEAD:main"
output = { reply = "$APPROVED", step = "none" }

[[step]]
kind = "lead_turn"
when = "allowed, once, exactly"
edit = [{ path = "src/main.rs", text = "fn main() {}" }]
output = { reply = "edited", step = "none" }

[[step]]
kind = "lead_turn"
approve = "git push origin HEAD:main"
sleep_ms = 4000
output = { reply = "$APPROVED", step = "none" }
"#,
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
    let refused_while_busy = |conv: &str| {
        wait_conv(tx, conv, "a refusal while the turn works", |v| {
            v["busy"] == true && !refusals(v).is_empty()
        })
    };

    // The assistant: its push onto the protected target, allowed while it still works.
    let t0 = std::time::Instant::now();
    let s = start_session(tx, &project, "push it");
    let v = refused_while_busy(&s);
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        t0.elapsed()
    );
    let r = &refusals(&v)[0];
    assert_eq!(r["limit"], "target", "{r}");
    let told = refusal_answer(tx, &s, r["id"].as_str().unwrap(), "allow_once").unwrap();
    assert_eq!(told["told"], "queued", "{told}");
    let v = wait_turn(tx, &s, 2);
    assert_eq!(
        refusals(&v).len(),
        1,
        "the retry went through: {}",
        v["turns"]
    );
    assert_eq!(refusals(&v)[0]["state"], "used");

    // The lead, in Auto: its push and its edit, allowed while it still works.
    let lead = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Busy".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    sreq(
        tx,
        Req::SetMode {
            conv: lead.clone(),
            mode: ChatMode::Auto,
        },
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "go".into(),
        },
    )
    .unwrap();
    let l = wait_conv(tx, &lead, "both refusals while the lead works", |v| {
        v["busy"] == true && refusals(v).len() == 2
    });
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        t0.elapsed()
    );
    let rs = refusals(&l);
    let push = rs.iter().find(|r| r["limit"] == "target").unwrap().clone();
    let edit = rs.iter().find(|r| r["limit"] == "edit").unwrap().clone();
    let told = refusal_answer(tx, &lead, push["id"].as_str().unwrap(), "allow_once").unwrap();
    assert_eq!(told["told"], "queued");
    // Turn 1 ends; Fallow's note starts turn 2, whose push goes through.
    let l = turn_ended(tx, &lead, 2);
    assert!(
        l["turns"][1]["said"]
            .as_str()
            .unwrap()
            .contains("allowed, once, exactly"),
        "{}",
        l["turns"][1]
    );
    assert_eq!(l["turns"][1]["reply"], "allow", "{}", l["turns"][1]);
    // The edit allowed once: the lead's retry writes it, and it is kept.
    refusal_answer(tx, &lead, edit["id"].as_str().unwrap(), "allow_once").unwrap();
    let l = turn_ended(tx, &lead, 3);
    let t = &l["turns"][2];
    assert!(
        t["discarded"].as_array().is_none_or(|d| d.is_empty()),
        "{t}"
    );
    let wd = std::path::PathBuf::from(l["workdir"].as_str().unwrap());
    assert!(wd.join("src/main.rs").exists(), "kept in the lead's copy");
    assert!(
        git(&wd, &["log", "--oneline", "-1", "--", "src/main.rs"]).contains("spec"),
        "committed with the turn"
    );

    // A cancelled answer: its refusal leaves "Needs you" and cannot be allowed.
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "push again".into(),
        },
    )
    .unwrap();
    let before = refusals(&l).len();
    let l = wait_conv(tx, &lead, "a new refusal while the lead works", |v| {
        v["busy"] == true && refusals(v).len() > before
    });
    let r = refusals(&l).last().cloned().unwrap();
    assert!(needs(tx).iter().any(|n| n["refusal"] == r["id"]));
    sreq(tx, Req::Cancel { conv: lead.clone() }).unwrap();
    wait_conv(tx, &lead, "the answer cancelled", |v| v["busy"] == false);
    assert!(
        !needs(tx).iter().any(|n| n["conv"] == lead.as_str()),
        "{:?}",
        needs(tx)
    );
    assert!(
        refusal_answer(tx, &lead, r["id"].as_str().unwrap(), "allow_once")
            .unwrap_err()
            .contains("cancelled")
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// 015 US2 (review of US2, round 2, 1): while the lead's question waits, an allowed refusal's
/// note waits too: the engineer's next message, a `/` command here, reaches the lead as written,
/// and the note goes as its own message after that turn.
#[test]
fn a_note_behind_the_lead_s_question_waits_for_the_engineer_s_answer() {
    let root = scratch("us15-question");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"[[step]]
kind = "lead_turn"
approve = "git push origin HEAD:main"
output = { reply = "Which way?", step = "none", question = { text = "Push now?", options = [{ label = "A", text = "yes" }, { label = "B", text = "no" }] } }

[[step]]
kind = "lead_turn"
when = "run the Spec Kit step `plan`"
output = { reply = "Planned.", step = "plan" }

[[step]]
kind = "lead_turn"
when = "allowed, once, exactly"
approve = "git push origin HEAD:main"
output = { reply = "$APPROVED", step = "none" }
"##,
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
    let lead = sreq(
        tx,
        Req::NewFeature {
            project,
            title: "Question".into(),
            lead: None,
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    sreq(
        tx,
        Req::SetMode {
            conv: lead.clone(),
            mode: ChatMode::Auto,
        },
    )
    .unwrap();
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "go".into(),
        },
    )
    .unwrap();
    let l = turn_ended(tx, &lead, 1);
    assert!(
        !l["turns"][0]["result"]["question"].is_null(),
        "{}",
        l["turns"][0]
    );
    let r = refusals(&l)[0].clone();
    let told = refusal_answer(tx, &lead, r["id"].as_str().unwrap(), "allow_once").unwrap();
    assert_eq!(told["told"], "queued", "behind the question: {told}");
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let l = sreq(tx, Req::Conversation { id: lead.clone() }).unwrap();
    assert_eq!(
        l["turns"].as_array().unwrap().len(),
        1,
        "no turn of its own yet"
    );
    assert_eq!(refusals(&l)[0]["told"], "waiting");
    // The engineer's `/plan` is a command, as written.
    sreq(
        tx,
        Req::Say {
            conv: lead.clone(),
            text: "/plan".into(),
        },
    )
    .unwrap();
    let l = turn_ended(tx, &lead, 2);
    assert_eq!(l["turns"][1]["said"], "/plan", "{}", l["turns"][1]);
    assert_eq!(l["turns"][1]["reply"], "Planned.", "{}", l["turns"][1]);
    // Then the note goes, and the push is allowed.
    let l = turn_ended(tx, &lead, 3);
    assert_eq!(l["turns"][2]["reply"], "allow", "{}", l["turns"][2]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
