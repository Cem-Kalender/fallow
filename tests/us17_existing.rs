//! 017 US3 (T030, T032, T033; FR-013, FR-017 to FR-020): agent files as Fallow 0.3.1 left them,
//! after the update. An unedited copy of a shipped agent is replaced; an edited copy naming an
//! alias is kept byte for byte, with the new default beside it, and the shipped default runs
//! meanwhile; the engineer's own agent and a project's lead that name aliases cannot be used,
//! and every place that needs them says why; nothing is rewritten; a file is fixed from the page.

mod common;

use common::*;
use fallow::defs::{DEFAULT_AGENTS, DEFAULT_PIPELINES, fingerprint};
use fallow::run::engine::Request;
use fallow::service::answer::{KeptTheme, PageRequest, PageResponse, answer};
use fallow::studio::Req;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The fingerprints `.shipped.json` held for the shipped agents in Fallow 0.3.1 (`git show
/// v0.3.1:assets/agents/<name>.md`, FNV-1a 64 as `defs::fingerprint`).
const V031: [(&str, &str); 7] = [
    ("assistant", "9661c3de2c6d2088"),
    ("builder", "b2bc37acc69a6577"),
    ("docs-writer", "3a31208a60947057"),
    ("lead", "39f4631feb4c8cfa"),
    ("reviewer", "044b6dbbd29ca81e"),
    ("security-reviewer", "b06ebc8de2ceac9d"),
    ("test-writer", "d4b88c6dac0dffe2"),
];

fn shipped(name: &str) -> &'static str {
    DEFAULT_AGENTS.iter().find(|(n, _)| *n == name).unwrap().1
}

/// A shipped agent's text as 0.3.1 shipped it: the model an alias, no `may_run` for the lead
/// and the assistant (017 changed only those lines). Checked against 0.3.1's fingerprint.
fn v031(name: &str) -> String {
    let t = shipped(name)
        .replace("model: claude-opus-5-5\n", "model: opus\n")
        .replace("model: claude-sonnet-5-5\n", "model: sonnet\n")
        .replace("may_run: []\n", "");
    let want = V031.iter().find(|(n, _)| *n == name).unwrap().1;
    assert_eq!(fingerprint(&t), want, "{name} as 0.3.1 shipped it");
    t
}

/// The engineer's own checker, as written for 0.3.1: an alias, no `may_run`.
const UI_CHECKER_031: &str = "---\nname: ui-checker\ndescription: Checks every page in a browser\nruntime: claude\nmodel: haiku\nkind: checker\nreturns: verdict\n---\n\nOpen every page and check it.\n";

/// The same checker, valid under 017.
const UI_CHECKER: &str = "---\nname: ui-checker\ndescription: Checks every page in a browser\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nOpen every page and check it.\n";

/// The standard pipeline with `ui-checker` in the reviewer's place.
fn checked_pipeline() -> String {
    let standard = DEFAULT_PIPELINES
        .iter()
        .find(|(n, _)| *n == "standard")
        .unwrap()
        .1;
    let t = replace_found(standard, "name = \"standard\"", "name = \"checked\"");
    replace_found(&t, "agent = \"reviewer\"", "agent = \"ui-checker\"")
}

fn call(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    method: &str,
    url: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let PageResponse { status, body, .. } = answer(
        PageRequest {
            method: method.into(),
            url: url.into(),
            body: body.map(|b| b.to_string().into_bytes()).unwrap_or_default(),
        },
        tx,
        &KeptTheme::default(),
    );
    let v = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
    (status, v)
}

/// Every agent file under `dirs` (`*.md` and `*.md.new`), with its bytes.
fn agent_files(dirs: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for d in dirs {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            out.insert(e.path(), std::fs::read(e.path()).unwrap());
        }
    }
    out
}

fn add_project(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, repo: &Path) -> String {
    let p = sreq(
        tx,
        Req::AddProject {
            path: repo.to_path_buf(),
        },
    )
    .unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "spec kit and agents"]);
    p
}

#[test]
fn files_from_before_the_update_are_kept_listed_and_fixable() {
    let root = scratch("existing");
    let config = root.join("config");
    let agents = config.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::create_dir_all(config.join("pipelines")).unwrap();

    // The config folder as 0.3.1 left it: every shipped file copied and recorded; the reviewer
    // edited (its instructions, its model still the alias); the engineer's own `ui-checker`.
    let mut record = BTreeMap::new();
    for (name, _) in V031 {
        std::fs::write(agents.join(format!("{name}.md")), v031(name)).unwrap();
        record.insert(
            format!("agents/{name}.md"),
            V031.iter().find(|(n, _)| *n == name).unwrap().1.to_string(),
        );
    }
    for (name, text) in DEFAULT_PIPELINES {
        std::fs::write(config.join(format!("pipelines/{name}.toml")), text).unwrap();
        record.insert(format!("pipelines/{name}.toml"), fingerprint(text));
    }
    std::fs::write(
        config.join(".shipped.json"),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();
    let reviewer_031 = v031("reviewer").replace(
        "Run one command per call",
        "Check the README too. Run one command per call",
    );
    assert_ne!(reviewer_031, v031("reviewer"));
    std::fs::write(agents.join("reviewer.md"), &reviewer_031).unwrap();
    std::fs::write(agents.join("ui-checker.md"), UI_CHECKER_031).unwrap();

    // A project with a pipeline naming `ui-checker` and its own lead, chosen as its lead while
    // 0.3.1 ran (written here in 017's form so this Fallow takes the choice, then put back as
    // 0.3.1 had it: `model: opus`).
    let repo = repo_named(&root, "shop");
    std::fs::create_dir_all(repo.join(".fallow/pipelines")).unwrap();
    std::fs::write(
        repo.join(".fallow/pipelines/checked.toml"),
        checked_pipeline(),
    )
    .unwrap();
    second_lead(
        &repo,
        "careful-lead",
        "claude",
        "claude-opus-5-5",
        "CAREFUL-MARKER",
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        format!("{}{}", lead_writes_calc(), calc_loop_ok()),
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let project = add_project(&e.tx, &repo);
    sreq(
        &e.tx,
        Req::PatchProject {
            id: project.clone(),
            patch: json!({"lead": "careful-lead"}),
        },
    )
    .unwrap();
    stop(e);
    let lead_file = repo.join(".fallow/agents/careful-lead.md");
    let careful_031 = replace_found(
        &std::fs::read_to_string(&lead_file).unwrap(),
        "model: claude-opus-5-5",
        "model: opus",
    );
    std::fs::write(&lead_file, &careful_031).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "the lead as 0.3.1 had it"]);

    // The update: Fallow starts.
    let news = fallow::defs::install_defaults(&config).unwrap();
    assert_eq!(
        news,
        ["agents/reviewer.md"],
        "only the edited copy gets a .new"
    );
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;

    // AS1: the unedited copy is the new default.
    assert_eq!(
        std::fs::read_to_string(agents.join("builder.md")).unwrap(),
        shipped("builder")
    );
    // AS2: the edited copy is kept byte for byte, the new default beside it.
    assert_eq!(
        std::fs::read_to_string(agents.join("reviewer.md")).unwrap(),
        reviewer_031
    );
    assert_eq!(
        std::fs::read_to_string(agents.join("reviewer.md.new")).unwrap(),
        shipped("reviewer")
    );
    assert_eq!(
        std::fs::read_to_string(agents.join("ui-checker.md")).unwrap(),
        UI_CHECKER_031
    );
    let dirs = [agents.clone(), repo.join(".fallow/agents")];
    let before = agent_files(&dirs);

    let (status, v) = call(tx, "GET", &format!("/api/agents?project={project}"), None);
    assert_eq!(status, 200, "{v}");
    let v = &v["value"];
    let unusable = v["unusable"].as_array().unwrap_or_else(|| panic!("{v}"));
    let entry = |name: &str| {
        unusable
            .iter()
            .find(|u| u["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not listed as unusable: {v}"))
            .clone()
    };
    let agent = |name: &str| {
        v["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .cloned()
    };
    let reviewer = entry("reviewer");
    assert_eq!(reviewer["source"], "user", "{reviewer}");
    assert_eq!(reviewer["in_force"], "default", "{reviewer}");
    assert_eq!(reviewer["text"], reviewer_031.as_str());
    assert!(
        reviewer["file"].as_str().unwrap().ends_with("reviewer.md"),
        "{reviewer}"
    );
    let model_line = reviewer_031
        .lines()
        .position(|l| l == "model: sonnet")
        .unwrap()
        + 1;
    let problems = reviewer["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1, "{reviewer}");
    assert_eq!(problems[0]["line"], model_line, "{reviewer}");
    assert!(
        problems[0]["message"]
            .as_str()
            .unwrap()
            .contains("is not a full model name for claude"),
        "{reviewer}"
    );
    assert_eq!(agent("reviewer").unwrap()["source"], "default", "{v}");
    assert_eq!(
        agent("reviewer").unwrap()["model"],
        "claude-sonnet-5-5",
        "{v}"
    );
    assert_eq!(agent("builder").unwrap()["source"], "default", "{v}");

    // AS3: the engineer's own agent: not in force, and the pipeline that names it says why.
    let ui = entry("ui-checker");
    assert_eq!(ui["in_force"], Value::Null, "{ui}");
    assert_eq!(ui["problems"].as_array().unwrap().len(), 2, "{ui}");
    assert!(agent("ui-checker").is_none(), "{v}");
    let pipeline_problem = v["problems"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|p| p.contains("checked.toml"))
        .unwrap_or_else(|| panic!("no problem for the pipeline: {v}"));
    assert!(
        pipeline_problem.contains("ui-checker.md:")
            && pipeline_problem.contains("is not a full model name"),
        "{pipeline_problem}"
    );
    // The project's lead: its file cannot be used, and the project says why.
    let careful = entry("careful-lead");
    assert_eq!(careful["source"], "project", "{careful}");
    assert_eq!(careful["in_force"], Value::Null, "{careful}");
    let p = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    let why = p["lead"]["problem"]
        .as_str()
        .unwrap_or_else(|| panic!("{p}"));
    assert!(
        why.contains("careful-lead.md:") && why.contains("is not a full model name"),
        "{why}"
    );

    // AS7: a loop runs, with the shipped defaults in place of the unusable copies (the project's
    // lead falls back to `lead`), and nothing is written to any agent file.
    let id = lock_feature_in(
        tx,
        &project,
        "Calc",
        "001-calc",
        None,
        Some((5.0, 60)),
        None,
    );
    let l = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    assert_eq!(l["stage"], "ready", "{}", l["needs_engineer"]);
    let view = ask(tx, Request::Loop { id }).unwrap();
    let card = view["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "reviewer")
        .cloned()
        .unwrap_or_else(|| panic!("{view}"));
    assert_eq!(card["model"], "claude-sonnet-5-5", "{card}");
    // A save from the page of another agent.
    let docs = replace_found(
        shipped("docs-writer"),
        "model: claude-sonnet-5-5",
        "model: claude-opus-5-5",
    );
    let (status, s) = call(
        tx,
        "PUT",
        "/api/agents/docs-writer?scope=user",
        Some(json!({ "text": docs, "base": shipped("docs-writer") })),
    );
    assert_eq!(status, 200, "{s}");
    let after = agent_files(&dirs);
    let mut others = after.clone();
    assert_eq!(
        others.remove(&agents.join("docs-writer.md")),
        Some(docs.clone().into_bytes())
    );
    let mut was = before.clone();
    was.remove(&agents.join("docs-writer.md"));
    assert!(others == was, "an agent file changed on its own");

    // AS2 with a save (017 review 1): the page shows the reviewer in force, the shipped text,
    // and saves it for the engineer. Their edited copy, which still names `sonnet`, is not
    // written over: refused, naming the file and saying it holds their own version; nothing
    // changes.
    let reviewer_file = agents.join("reviewer.md");
    let (status, s) = call(
        tx,
        "PUT",
        "/api/agents/reviewer?scope=user",
        Some(json!({ "text": shipped("reviewer"), "base": shipped("reviewer") })),
    );
    assert_eq!(status, 409, "{s}");
    let why = s["error"].as_str().unwrap();
    assert!(
        why.contains(&reviewer_file.display().to_string())
            && why.contains("holds your own reviewer.md, which Fallow could not use")
            && why.contains(fallow::studio::defs::NOT_LOADED),
        "{why}"
    );
    // Nor as a new agent of that name (no text loaded).
    let (status, s) = call(
        tx,
        "PUT",
        "/api/agents/reviewer?scope=user",
        Some(json!({ "text": shipped("reviewer") })),
    );
    assert_eq!(status, 409, "{s}");
    assert!(agent_files(&dirs) == after, "the edited reviewer is kept");

    // AS4 (T033): the reviewer fixed from the page: saved at its own place, in force, and no
    // longer listed. The page loaded that file, so it is saved over it.
    let fixed = replace_found(&reviewer_031, "model: sonnet", "model: claude-sonnet-5-5");
    let (status, s) = call(
        tx,
        "PUT",
        "/api/agents/reviewer?scope=user",
        Some(json!({ "text": fixed, "base": reviewer_031 })),
    );
    assert_eq!(status, 200, "{s}");
    assert_eq!(
        std::fs::read_to_string(agents.join("reviewer.md")).unwrap(),
        fixed
    );
    let (_, v) = call(tx, "GET", &format!("/api/agents?project={project}"), None);
    let v = &v["value"];
    assert!(
        !v["unusable"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["name"] == "reviewer"),
        "{v}"
    );
    let reviewer = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "reviewer")
        .unwrap();
    assert_eq!(reviewer["source"], "user", "{reviewer}");
    stop(e);
}

/// US3 AS6 (T032; FR-020): a loop locked with a pipeline whose checker was valid at the lock;
/// the checker's file is replaced by one naming an alias before its step is due. The step does
/// not start and the loop asks the engineer, naming the file and its problem.
#[test]
fn a_loop_whose_next_agent_became_unusable_asks_the_engineer() {
    let root = scratch("existing-loop");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("agents")).unwrap();
    std::fs::write(config.join("agents/ui-checker.md"), UI_CHECKER).unwrap();
    let repo = repo(&root);
    std::fs::create_dir_all(repo.join(".fallow/pipelines")).unwrap();
    std::fs::write(
        repo.join(".fallow/pipelines/checked.toml"),
        checked_pipeline(),
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    // The first task waits, so the file is replaced well before the checkpoint.
    let steps = calc_loop_ok().replacen(
        "[[step]]\nkind = \"build\"\n",
        "[[step]]\nkind = \"build\"\nsleep_ms = 1500\n",
        1,
    );
    assert_ne!(steps, calc_loop_ok(), "the first task waits");
    std::fs::write(&scenario, format!("{}{steps}", lead_writes_calc())).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo, Some("checked"), Some((5.0, 60)), None);
    std::fs::write(config.join("agents/ui-checker.md"), UI_CHECKER_031).unwrap();

    let v = wait_for(tx, id, "the engineer asked", |v| {
        !v["needs_engineer"].is_null()
    });
    let why = v["needs_engineer"].to_string();
    assert!(
        why.contains("ui-checker.md:") && why.contains("is not a full model name"),
        "{why}"
    );
    // No step of ui-checker started.
    let steps = v["steps"].as_object().unwrap();
    assert!(
        !steps
            .values()
            .any(|s| s["def"] == "ui-checker" && !s["started"].is_null()),
        "{steps:?}"
    );
    // The file is as the engineer left it.
    assert_eq!(
        std::fs::read_to_string(config.join("agents/ui-checker.md")).unwrap(),
        UI_CHECKER_031
    );
    stop(e);
}
