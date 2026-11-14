//! 012 US7 (T039; quickstart Q8; FR-030 to FR-038, SC-011; research R-15): from a discussion to
//! a feature. The assistant's suggestion offers "Make this a feature"; the confirmed brief is
//! committed in the feature's worktree, from the engineer's branch, and its spec session runs
//! specify on it (after the constitution step when the constitution is still the template); the
//! session and the spec session link to each other; refusals say why; `/feature` asks for a
//! suggestion; an earlier project conversation is read-only and its proposal starts a feature.

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::Value;

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

const SCENARIO: &str = r##"
[[step]]
kind = "assistant_turn"
when = "suggest a feature from the discussion"
output = { reply = "Here is one.", proposed_feature = { title = "Week start", brief = "Let the engineer choose the first day." } }

[[step]]
kind = "assistant_turn"
when = "plan the picker"
output = { reply = "Make it a feature?", proposed_feature = { title = "Week picker", brief = "Weeks start on Monday." } }

[[step]]
kind = "lead_turn"
when = "speckit-constitution"
output = { reply = "Constitution set.", step = "constitution" }

[[step]]
kind = "lead_turn"
when = "from the engineer"
files = [{ path = "specs/001-week-picker/spec.md", text = "# Week picker\n" }]
output = { reply = "Specified.", step = "specify" }

[[step]]
kind = "lead_turn"
when = "Set brief"
output = { reply = "Specified.", step = "specify" }
"##;

fn setup(
    name: &str,
    constitution: bool,
) -> (fallow::service::ToolEngine, std::path::PathBuf, String) {
    let root = scratch(name);
    let repo = repo(&root);
    if constitution {
        std::fs::create_dir_all(repo.join(".specify/memory")).unwrap();
        std::fs::write(
            repo.join(".specify/memory/constitution.md"),
            "# Shop constitution\n\nTests first.\n",
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "constitution"]);
    }
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, SCENARIO).unwrap();
    let e = start_studio(&root, &sc, None);
    let added = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    (e, repo, project)
}

fn conv(tx: &Tx, id: &str) -> Value {
    sreq(tx, Req::Conversation { id: id.to_string() }).unwrap()
}

#[test]
fn a_suggestion_becomes_a_feature_from_the_brief_with_the_constitution_first() {
    let (e, repo, project) = setup("us12-feature", false);
    let tx = &e.tx;
    let s = start_session(tx, &project, "plan the picker");
    let v = wait_turn(tx, &s, 1);
    assert_eq!(
        v["session"]["suggestions"][0]["title"], "Week picker",
        "{}",
        v["session"]
    );
    let d = sreq(
        tx,
        Req::FeatureDraft {
            session: s.clone(),
            suggestion: None,
        },
    )
    .unwrap();
    assert_eq!(d["title"], "Week picker");
    assert_eq!(d["brief"], "Weeks start on Monday.");
    // Refusals: an empty brief; the folder on no branch.
    let err = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "Week picker".into(),
            brief: " ".into(),
            lead: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("brief"), "{err}");
    git(&repo, &["checkout", "-q", "--detach"]);
    let err = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "Week picker".into(),
            brief: "x".into(),
            lead: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("no branch"), "{err}");
    git(&repo, &["checkout", "-q", "main"]);
    assert!(
        sreq(
            tx,
            Req::Project {
                id: project.clone()
            }
        )
        .unwrap()["feature_list"]
            .as_array()
            .unwrap()
            .is_empty(),
        "nothing made"
    );

    // Confirmed with an edited brief.
    let brief = "Weeks start on Monday, from the engineer.";
    let r = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "Week picker".into(),
            brief: brief.into(),
            lead: None,
        },
    )
    .unwrap();
    let feature = r["feature"].as_str().unwrap().to_string();
    let spec = r["conversation"].as_str().unwrap().to_string();
    assert_eq!(feature, "001-week-picker");
    let f = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: feature.clone(),
        },
    )
    .unwrap();
    assert_eq!(f["base_branch"], "main", "from the engineer's branch");
    assert_eq!(f["origin"]["kind"], "session", "{f}");
    assert_eq!(f["origin"]["id"], s.as_str());
    let committed = git(
        &repo,
        &["show", &format!("{feature}:specs/{feature}/brief.md")],
    );
    assert!(committed.contains(brief), "{committed}");
    assert!(
        git(&repo, &["log", "-1", "--format=%s", &feature])
            .contains("spec: brief from solo session")
    );
    // The constitution is the template: its step first, then specify on the brief by itself.
    let c = wait_conv(tx, &spec, "the constitution turn", |v| {
        v["turns"]
            .as_array()
            .is_some_and(|t| !t.is_empty() && !t[0]["reply"].is_null())
    });
    assert!(
        c["turns"][0]["said"]
            .as_str()
            .unwrap()
            .starts_with("/constitution"),
        "{}",
        c["turns"][0]
    );
    let c = wait_conv(tx, &spec, "specify on the brief", |v| {
        v["turns"]
            .as_array()
            .is_some_and(|t| t.len() >= 2 && !t[1]["reply"].is_null())
    });
    let said = c["turns"][1]["said"].as_str().unwrap();
    assert!(
        said.starts_with("/specify") && said.contains(brief),
        "{said}"
    );
    // The links go both ways.
    let v = conv(tx, &s);
    assert_eq!(
        v["session"]["features"][0]["feature"],
        feature.as_str(),
        "{}",
        v["session"]
    );
    // A taken title is refused, and nothing more is made.
    let err = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "week  PICKER".into(),
            brief: "again".into(),
            lead: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("exists already"), "{err}");
    let v = conv(tx, &s);
    assert!(
        v["session"]["starting"].is_null(),
        "nothing left starting: {}",
        v["session"]
    );
}

#[test]
fn with_a_constitution_set_specify_runs_first_and_feature_asks_for_a_suggestion() {
    let (e, _repo, project) = setup("us12-feature-set", true);
    let tx = &e.tx;
    let s = start_session(tx, &project, "plan the picker");
    wait_turn(tx, &s, 1);
    // `/feature` is in the session's menu and asks the assistant for a suggestion.
    let menu = sreq(tx, Req::Commands { conv: s.clone() }).unwrap();
    assert!(
        menu["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "feature"),
        "{menu}"
    );
    say_solo(tx, &s, "/feature");
    let v = wait_turn(tx, &s, 2);
    assert_eq!(
        v["session"]["suggestions"][1]["title"], "Week start",
        "{}",
        v["session"]
    );
    let r = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "Week start".into(),
            brief: "Set brief: Monday or Sunday.".into(),
            lead: None,
        },
    )
    .unwrap();
    let spec = r["conversation"].as_str().unwrap().to_string();
    let c = wait_conv(tx, &spec, "specify", |v| {
        v["turns"]
            .as_array()
            .is_some_and(|t| !t.is_empty() && !t[0]["reply"].is_null())
    });
    assert!(
        c["turns"][0]["said"]
            .as_str()
            .unwrap()
            .starts_with("/specify"),
        "{}",
        c["turns"][0]
    );
}

#[test]
fn an_earlier_project_conversation_is_read_only_and_its_proposal_starts_a_feature() {
    let (e, _repo, project) = setup("us12-feature-earlier", true);
    let tx = &e.tx;
    // A project conversation from before 012.
    let pc = sreq(
        tx,
        Req::ProjectConversation {
            project: project.clone(),
        },
    )
    .unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let err = sreq(
        tx,
        Req::Say {
            conv: pc.clone(),
            text: "hello".into(),
        },
    )
    .unwrap_err();
    assert!(err.contains("read-only"), "{err}");
    let r = sreq(
        tx,
        Req::FeatureFromConversation {
            conv: pc.clone(),
            title: "Week start".into(),
            brief: "Set brief: from the old talk.".into(),
            lead: None,
        },
    )
    .unwrap();
    let f = sreq(
        tx,
        Req::Feature {
            project: project.clone(),
            feature: r["feature"].as_str().unwrap().into(),
        },
    )
    .unwrap();
    assert_eq!(f["origin"]["kind"], "conversation", "{f}");
    // The sessions list names earlier conversations that have turns; this one has none (and
    // can get none: it is read-only), so it is not listed.
    let l = sreq(tx, Req::Sessions { project }).unwrap();
    assert!(l["earlier"].as_array().unwrap().is_empty(), "{l}");
}

#[test]
fn with_spec_kit_gone_from_the_branch_specify_still_runs_and_a_second_look_shows_it_started() {
    // Fallow puts Spec Kit's skills in the lead's copy before every turn, so a branch that lost
    // them still specifies (the "Spec Kit missing" refusal cannot be reached from a session).
    let (e, repo, project) = setup("us12-feature-nokit", true);
    let tx = &e.tx;
    git(&repo, &["rm", "-r", "-q", ".claude/skills/speckit-specify"]);
    git(&repo, &["commit", "-q", "-m", "no specify"]);
    let s = start_session(tx, &project, "plan the picker");
    wait_turn(tx, &s, 1);
    let r = sreq(
        tx,
        Req::FeatureFromSession {
            session: s.clone(),
            title: "Week picker".into(),
            brief: "Set brief: Monday.".into(),
            lead: None,
        },
    )
    .unwrap();
    assert!(r["warning"].is_null(), "{r}");
    let spec = r["conversation"].as_str().unwrap().to_string();
    let c = wait_conv(tx, &spec, "specify", |v| {
        v["turns"]
            .as_array()
            .is_some_and(|t| !t.is_empty() && !t[0]["reply"].is_null())
    });
    assert!(
        c["turns"][0]["said"]
            .as_str()
            .unwrap()
            .starts_with("/specify"),
        "{}",
        c["turns"][0]
    );
    // A second look at the same discussion shows it started already (analyze C10).
    let d = sreq(
        tx,
        Req::FeatureDraft {
            session: s.clone(),
            suggestion: Some(1),
        },
    )
    .unwrap();
    assert_eq!(d["started"][0]["feature"], r["feature"], "{d}");
    // The brief's marker is gone once its first turn started.
    let dir = repo
        .parent()
        .unwrap()
        .join("data/conversations")
        .join(&spec);
    assert!(!dir.join("brief_pending.json").exists());
}

#[test]
fn a_restart_before_feature_started_was_written_links_the_feature_again() {
    let root;
    let s;
    let project;
    {
        let (e, repo, p) = setup("us12-feature-restart", true);
        root = repo.parent().unwrap().to_path_buf();
        project = p;
        let tx = &e.tx;
        s = start_session(tx, &project, "plan the picker");
        wait_turn(tx, &s, 1);
        // The feature is made, and Fallow goes down before `feature_started` is written: the
        // intent is put back by hand after the fact, as a restart would find it.
        let r = sreq(
            tx,
            Req::FeatureFromSession {
                session: s.clone(),
                title: "Week picker".into(),
                brief: "Set brief: Monday.".into(),
                lead: None,
            },
        )
        .unwrap();
        assert!(r["feature"].is_string());
        stop(e);
    }
    let log = root
        .join("data/conversations")
        .join(&s)
        .join("events.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| !l.contains("\"feature_started\""))
        .collect();
    std::fs::write(&log, format!("{}\n", kept.join("\n"))).unwrap();
    let sc = root.join("scenario.toml");
    let e = start_studio(&root, &sc, None);
    let v = conv(&e.tx, &s);
    assert!(v["session"]["starting"].is_null(), "{}", v["session"]);
    assert_eq!(
        v["session"]["features"][0]["feature"], "001-week-picker",
        "{}",
        v["session"]
    );
    let _ = project;
}

/// 012 T042: the page for `checks/solo-feature.png`: a session whose assistant suggests a feature,
/// served for a headless browser (`FALLOW_VISUAL_ROOT`, `FALLOW_VISUAL_SECS`).
#[test]
#[ignore]
fn serve_the_012_feature_demo() {
    let root =
        std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    let repo = repo(&root);
    std::fs::create_dir_all(repo.join(".specify/memory")).unwrap();
    std::fs::write(
        repo.join(".specify/memory/constitution.md"),
        "# Shop constitution\n\nTests first.\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "constitution"]);
    let sc = root.join("scenario.toml");
    std::fs::write(
        &sc,
        format!(
            "{SCENARIO}\n[[step]]\nkind = \"lead_turn\"\nwhen = \"Weeks start on Monday.\"\noutput = {{ reply = \"I wrote the spec from your brief.\", step = \"specify\" }}\n"
        ),
    )
    .unwrap();
    let e = start_studio(&root, &sc, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let s = start_session(tx, &project, "plan the picker");
    wait_turn(tx, &s, 1);
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), &project).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}

#[test]
fn a_brief_left_waiting_runs_once_after_a_restart_and_never_over_work_done() {
    // A handover's brief that failed to start (its marker set aside) runs at the next start,
    // exactly once; a marker in a spec session that has turns already is dropped, not sent.
    let (e, repo, project) = setup("us12-feature-waiting", true);
    let root = repo.parent().unwrap().to_path_buf();
    let tx = &e.tx;
    let fresh = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Week start".into(),
            lead: None,
        },
    )
    .unwrap();
    let busy = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Week picker".into(),
            lead: None,
        },
    )
    .unwrap();
    let (fresh_c, busy_c) = (
        fresh["conversation"].as_str().unwrap().to_string(),
        busy["conversation"].as_str().unwrap().to_string(),
    );
    sreq(
        tx,
        Req::Say {
            conv: busy_c.clone(),
            text: "by hand, from the engineer".into(),
        },
    )
    .unwrap();
    turn_ended(tx, &busy_c, 1);
    stop(e);
    let mark = |c: &str, feature: &str, brief: &str| {
        let v = serde_json::json!({
            "project": project, "feature": feature,
            "text": format!("# T\n\n{brief}\n"), "message": "spec: brief from solo session s",
            "brief": brief, "failed": "specify could not start: the lead cannot answer",
        });
        std::fs::write(
            root.join("data/conversations")
                .join(c)
                .join("brief_pending.json"),
            v.to_string(),
        )
        .unwrap();
    };
    mark(
        &fresh_c,
        fresh["feature"].as_str().unwrap(),
        "Set brief: Monday or Sunday.",
    );
    mark(
        &busy_c,
        busy["feature"].as_str().unwrap(),
        "Set brief: again.",
    );
    let e = start_studio(&root, &root.join("scenario.toml"), None);
    let tx = &e.tx;
    let c = wait_conv(tx, &fresh_c, "the waiting brief", |v| {
        v["turns"]
            .as_array()
            .is_some_and(|t| !t.is_empty() && !t[0]["reply"].is_null())
    });
    assert!(
        c["turns"][0]["said"]
            .as_str()
            .unwrap()
            .starts_with("/specify Set brief: Monday or Sunday."),
        "{}",
        c["turns"][0]
    );
    std::thread::sleep(std::time::Duration::from_millis(2500));
    assert_eq!(
        conv(tx, &fresh_c)["turns"].as_array().unwrap().len(),
        1,
        "sent once"
    );
    assert_eq!(
        conv(tx, &busy_c)["turns"].as_array().unwrap().len(),
        1,
        "never over work done"
    );
    assert!(
        !root
            .join("data/conversations")
            .join(&busy_c)
            .join("brief_pending.json")
            .exists()
    );
    let wd = std::path::PathBuf::from(c["workdir"].as_str().unwrap());
    assert!(
        git(&wd, &["log", "--format=%s"]).contains("spec: brief from"),
        "the brief committed"
    );
}
