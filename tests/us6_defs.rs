//! 007 US6 (T074; FR-024 to FR-028): the definitions and pipelines API (create, edit and
//! delete, with problems at their lines; the override order; an update of an edited default
//! beside it as `.md.new`), a custom checker that runs only when its paths change, whose
//! findings need what was run and whose failure becomes a repair, and a pipeline the lead
//! proposes, which is never used before it is saved.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;
use serde_json::{Value, json};

const CHECKER: &str = "---\nname: api-checker\ndescription: Calls the API the story changed\nruntime: claude\nmodel: haiku\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nCall every endpoint the changes touch.\n";

fn agents(tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>, project: Option<&str>) -> Value {
    sreq(
        tx,
        Req::Agents {
            project: project.map(str::to_string),
        },
    )
    .unwrap()
}

fn agent<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or(&Value::Null)
}

fn pipelines(
    tx: &std::sync::mpsc::Sender<fallow::run::engine::Msg>,
    project: Option<&str>,
) -> Value {
    sreq(
        tx,
        Req::Pipelines {
            project: project.map(str::to_string),
        },
    )
    .unwrap()
}

fn pipeline<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["pipelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap_or(&Value::Null)
}

fn user() -> String {
    "user".into()
}

fn steps_of<'a>(v: &'a Value, kind: &str, part: &str) -> Vec<&'a Value> {
    // In step order: the map's keys are strings.
    let mut out: Vec<&Value> = v["steps"]
        .as_object()
        .unwrap()
        .values()
        .filter(|s| s["kind"] == kind && s["part"] == part)
        .collect();
    out.sort_by_key(|s| s["n"].as_u64());
    out
}

#[test]
fn definitions_and_pipelines_are_created_edited_and_deleted() {
    let root = scratch("defs-api");
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    // The shipped defaults are in the engineer's folder, and an update changed the builder
    // after the engineer edited it.
    fallow::defs::install_defaults(&config).unwrap();
    let builder = config.join("agents/builder.md");
    let edited = std::fs::read_to_string(&builder)
        .unwrap()
        .replace("model: sonnet", "model: opus");
    std::fs::write(&builder, &edited).unwrap();
    let rec = config.join(".shipped.json");
    let mut r: Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    r["agents/builder.md"] = json!("0000000000000000");
    std::fs::write(&rec, r.to_string()).unwrap();
    assert_eq!(
        fallow::defs::install_defaults(&config).unwrap(),
        ["agents/builder.md"]
    );

    let repo = repo_named(&root, "shop");
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let p = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();

    // The library: the defaults with their kind and source; the edited builder is the
    // engineer's, with the update beside it.
    let v = agents(tx, Some(&p));
    assert_eq!(v["agents"].as_array().unwrap().len(), 6, "{v}");
    assert_eq!(agent(&v, "reviewer")["source"], "default");
    assert_eq!(agent(&v, "reviewer")["kind"], "checker");
    let b = agent(&v, "builder");
    assert_eq!(b["source"], "user");
    assert_eq!(b["model"], "opus");
    assert_eq!(b["text"], edited);
    assert!(
        b["update"]
            .as_str()
            .is_some_and(|u| u.contains("model: sonnet")),
        "the new default beside it: {b}"
    );
    assert!(
        b["used_by"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "standard")
    );

    // A new agent: problems at their lines, then saved; a project override wins by name.
    let bad = CHECKER.replace("kind: checker", "kind: judge");
    let why = sreq(
        tx,
        Req::PutAgent {
            name: "api-checker".into(),
            scope: user(),
            text: bad,
        },
    )
    .unwrap_err();
    assert!(
        why.contains("api-checker.md:6:") && why.contains("kind \"judge\""),
        "{why}"
    );
    let why = sreq(
        tx,
        Req::PutAgent {
            name: "../x".into(),
            scope: user(),
            text: CHECKER.into(),
        },
    )
    .unwrap_err();
    assert!(why.contains("a-z, 0-9 and -"), "{why}");
    sreq(
        tx,
        Req::PutAgent {
            name: "api-checker".into(),
            scope: user(),
            text: CHECKER.into(),
        },
    )
    .unwrap();
    assert!(config.join("agents/api-checker.md").is_file());
    let over = CHECKER.replace("model: haiku", "model: opus");
    sreq(
        tx,
        Req::PutAgent {
            name: "api-checker".into(),
            scope: format!("project:{p}"),
            text: over,
        },
    )
    .unwrap();
    assert!(repo.join(".fallow/agents/api-checker.md").is_file());
    let v = agents(tx, Some(&p));
    assert_eq!(agent(&v, "api-checker")["source"], "project");
    assert_eq!(agent(&v, "api-checker")["model"], "opus");
    assert_eq!(
        agent(&agents(tx, None), "api-checker")["source"],
        "user",
        "without the project"
    );
    // A broken file is listed with its line.
    std::fs::write(
        repo.join(".fallow/agents/broken.md"),
        "---\nname: broken\nfoo: 1\n---\nx\n",
    )
    .unwrap();
    let v = agents(tx, Some(&p));
    assert!(
        v["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x.as_str().unwrap().contains("broken.md:3:")),
        "{v}"
    );

    // A pipeline: a copy of Standard with the checker on api/ changes.
    let std_toml = pipeline(&pipelines(tx, Some(&p)), "standard")["text"]
        .as_str()
        .unwrap()
        .to_string();
    let copy = std_toml.replace("name = \"standard\"", "name = \"standard-api\"")
        + "\n[[step]]\nagent = \"api-checker\"\nwhen = \"checkpoint\"\npaths = [\"api/**\"]\non_fail = \"repair\"\n";
    let why = sreq(
        tx,
        Req::PutPipeline {
            name: "standard-api".into(),
            scope: user(),
            toml: copy.replace("api-checker", "ghost"),
        },
    )
    .unwrap_err();
    assert!(
        why.contains("step 4") && why.contains("no agent named \"ghost\""),
        "{why}"
    );
    sreq(
        tx,
        Req::PutPipeline {
            name: "standard-api".into(),
            scope: user(),
            toml: copy.clone(),
        },
    )
    .unwrap();
    let pl = pipelines(tx, Some(&p));
    let sa = pipeline(&pl, "standard-api");
    assert_eq!(sa["source"], "user");
    assert_eq!(sa["verifying"], true);
    assert_eq!(sa["steps"].as_array().unwrap().len(), 4);
    // Removing the gates is allowed, and says what is not verified.
    let lean = "name = \"lean\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"repair\"\n";
    sreq(
        tx,
        Req::PutPipeline {
            name: "lean".into(),
            scope: format!("project:{p}"),
            toml: lean.into(),
        },
    )
    .unwrap();
    let l = pipeline(&pipelines(tx, Some(&p)), "lean").clone();
    assert_eq!(l["verifying"], false);
    assert_eq!(l["missing"], json!(["no gates after each task"]));
    assert!(
        pipeline(&pipelines(tx, None), "lean").is_null(),
        "a project's own"
    );
    // The engineer's change that would break a project's own pipeline is refused, and so is
    // one under the pipelines Fallow ships.
    let shipped_reviewer = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "reviewer")
        .unwrap()
        .1;
    let lead = shipped_reviewer
        .replace("kind: checker", "kind: lead")
        .replace("returns: verdict", "returns: reply");
    let why = sreq(
        tx,
        Req::PutAgent {
            name: "reviewer".into(),
            scope: user(),
            text: lead,
        },
    )
    .unwrap_err();
    assert!(
        why.contains("the pipeline lean,") && why.contains("the pipeline standard,"),
        "{why}"
    );
    // A change that keeps a pipeline working but stops it verifying is refused too.
    let maker = shipped_reviewer
        .replace("kind: checker", "kind: maker")
        .replace("returns: verdict", "returns: work");
    let why = sreq(
        tx,
        Req::PutAgent {
            name: "reviewer".into(),
            scope: user(),
            text: maker,
        },
    )
    .unwrap_err();
    assert!(
        why.contains("standard (it would no longer verify)"),
        "{why}"
    );
    assert_eq!(
        agent(&agents(tx, Some(&p)), "reviewer")["kind"],
        "checker",
        "nothing was written"
    );

    // The project's pipeline must exist.
    let why = sreq(
        tx,
        Req::PatchProject {
            id: p.clone(),
            patch: json!({ "pipeline": "nope" }),
        },
    )
    .unwrap_err();
    assert!(why.contains("no pipeline"), "{why}");
    sreq(
        tx,
        Req::PatchProject {
            id: p.clone(),
            patch: json!({ "pipeline": "lean" }),
        },
    )
    .unwrap();
    // ...and so it cannot go while the project uses it.
    let why = sreq(
        tx,
        Req::DeletePipeline {
            name: "lean".into(),
            scope: format!("project:{p}"),
        },
    )
    .unwrap_err();
    assert!(why.contains("shop uses it"), "{why}");
    sreq(
        tx,
        Req::PatchProject {
            id: p.clone(),
            patch: json!({ "pipeline": "standard" }),
        },
    )
    .unwrap();
    sreq(
        tx,
        Req::DeletePipeline {
            name: "lean".into(),
            scope: format!("project:{p}"),
        },
    )
    .unwrap();
    assert!(pipeline(&pipelines(tx, Some(&p)), "lean").is_null());

    // An agent a pipeline uses cannot go; the project's override can, and the user's shows.
    let why = sreq(
        tx,
        Req::DeleteAgent {
            name: "api-checker".into(),
            scope: user(),
        },
    )
    .unwrap_err();
    assert!(why.contains("standard-api"), "{why}");
    sreq(
        tx,
        Req::DeleteAgent {
            name: "api-checker".into(),
            scope: format!("project:{p}"),
        },
    )
    .unwrap();
    assert_eq!(
        agent(&agents(tx, Some(&p)), "api-checker")["source"],
        "user"
    );
    // A default is reset, not deleted.
    sreq(
        tx,
        Req::DeleteAgent {
            name: "builder".into(),
            scope: user(),
        },
    )
    .unwrap();
    let b = agent(&agents(tx, Some(&p)), "builder").clone();
    assert_eq!(b["source"], "default");
    assert_eq!(b["model"], "sonnet");
    let why = sreq(
        tx,
        Req::DeleteAgent {
            name: "reviewer".into(),
            scope: format!("project:{p}"),
        },
    )
    .unwrap_err();
    assert!(why.contains("no definition"), "{why}");

    // The runtimes Fallow knows.
    let rt = sreq(tx, Req::Runtimes).unwrap();
    let names: Vec<&str> = rt
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["claude", "fake"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

/// A checker's verdict on a story: a high finding at `api/__init__.py`, with or without what
/// was run, or none.
fn api_check(story: &str, finding: Option<bool>) -> String {
    let findings = match finding {
        None => String::new(),
        Some(ran) => format!(
            "{{ severity = \"high\", where = \"api/__init__.py\", ran = \"{}\", result = \"add() answers floats\" }}",
            if ran {
                "python -c 'import api; print(api.add(1.5, 1))'"
            } else {
                ""
            }
        ),
    };
    format!(
        "[[step]]\nkind = \"review\"\npart = {story:?}\noutput = {{ answers = [], findings = [{findings}], summary = \"api checked\" }}\n\n"
    )
}

fn repair_step(part: &str, files: &[(&str, &str)]) -> String {
    task_step(part, files, "").replacen("kind = \"build\"", "kind = \"repair\"", 1)
}

#[test]
fn a_custom_checker_runs_on_its_paths_and_its_failure_is_repaired() {
    let root = scratch("defs-checker");
    let api = "from calc import add\n";
    let steps = [
        task_step("T001", &[("calc/__init__.py", "")], ""),
        handover_step(1, ""),
        task_step(
            "T002",
            &[("calc/__init__.py", CALC_INIT), ("api/__init__.py", api)],
            "",
        ),
        task_step(
            "T003",
            &[("tests/__init__.py", ""), ("tests/test_add.py", TEST_ADD)],
            "",
        ),
        handover_step(2, ""),
        check_step("US1", true, ""),
        // A finding without what was run does not count: asked again.
        api_check("US1", Some(false)),
        api_check("US1", Some(true)),
        repair_step(
            "US1",
            &[("api/__init__.py", "from calc import add  # ints only\n")],
        ),
        check_step("US1", true, ""),
        api_check("US1", None),
        // US2 changes nothing under api/: the checker does not run.
        task_step(
            "T004",
            &[
                ("calc/__init__.py", CALC_INIT_SUB),
                ("tests/test_sub.py", TEST_SUB),
            ],
            "",
        ),
        handover_step(3, ""),
        check_step("US2", true, ""),
    ]
    .concat();
    let repo = repo(&root);
    std::fs::create_dir_all(repo.join(".fallow/agents")).unwrap();
    std::fs::write(repo.join(".fallow/agents/api-checker.md"), CHECKER).unwrap();
    std::fs::create_dir_all(repo.join(".fallow/pipelines")).unwrap();
    let standard = fallow::defs::DEFAULT_PIPELINES[0].1;
    std::fs::write(
        repo.join(".fallow/pipelines/standard-api.toml"),
        standard.replace("name = \"standard\"", "name = \"standard-api\"")
            + "\n[[step]]\nagent = \"api-checker\"\nwhen = \"checkpoint\"\npaths = [\"api/**\"]\non_fail = \"repair\"\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{steps}", lead_writes_calc())).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo, Some("standard-api"), Some((5.0, 60)), None);
    let v = wait_for(tx, id, "ready to merge", |v| {
        v["stage"] == "ready" || !v["needs_engineer"].is_null()
    });
    assert_eq!(v["stage"], "ready", "{}", v["needs_engineer"]);

    let by = |part: &str, def: &str| -> Vec<Value> {
        steps_of(&v, "review", part)
            .into_iter()
            .filter(|s| s["def"] == def)
            .cloned()
            .collect()
    };
    let api1 = by("US1", "api-checker");
    assert_eq!(
        api1.len(),
        3,
        "asked again, failed, then passed after the repair: {api1:?}"
    );
    assert_eq!(api1[0]["verified"][0], false);
    let r0 = api1[0]["verified"][1].to_string();
    assert!(r0.contains("without what was run"), "{r0}");
    let r1 = api1[1]["verified"][1].to_string();
    assert!(r1.contains("high finding at api/__init__.py"), "{r1}");
    assert_eq!(api1[2]["verified"][0], true);
    assert_eq!(
        steps_of(&v, "repair", "US1").len(),
        1,
        "its failure became a repair"
    );
    assert!(
        by("US2", "api-checker").is_empty(),
        "nothing under api/ changed in US2"
    );
    assert_eq!(by("US2", "reviewer").len(), 1);

    assert!(
        r1.contains("ran python -c 'import api"),
        "the verdict says what it ran: {r1}"
    );
    // The loop screen shows the checker's outcome on its story, and not on US2.
    let view = ask(tx, Request::Loop { id }).unwrap();
    let story = |id: &str| {
        view["checks"]["stories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()
            .clone()
    };
    let us1 = story("US1");
    let c = us1["checkers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["agent"] == "api-checker")
        .unwrap()
        .clone();
    assert_eq!(c["passed"], true, "{us1}");
    assert!(
        !story("US2")["checkers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["agent"] == "api-checker")
    );
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pipeline_the_lead_proposes_is_used_only_once_saved() {
    let root = scratch("defs-proposed");
    let proposed = "name = \"careful\"\n[[step]]\nagent = \"builder\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"fallow:gates\"\nwhen = \"task\"\non_fail = \"repair\"\n[[step]]\nagent = \"reviewer\"\nwhen = \"checkpoint\"\non_fail = \"engineer\"\n";
    let steps = format!(
        "[[step]]\nkind = \"lead_turn\"\noutput = {{ reply = \"Here is one.\", step = \"none\", proposed_pipeline = {} }}\n\n{}",
        lit(proposed),
        lead_writes_calc()
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, steps).unwrap();
    let repo = repo(&root);
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let p = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    let c = sreq(tx, Req::ProjectConversation { project: p.clone() }).unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    sreq(
        tx,
        Req::Say {
            conv: c.clone(),
            text: "Design a pipeline whose reviews go to me.".into(),
        },
    )
    .unwrap();
    let v = turn_ended(tx, &c, 1);
    assert_eq!(v["turns"][0]["result"]["proposed_pipeline"], proposed);
    // Proposed, not saved: not in the library, and a lock cannot use it.
    assert!(pipeline(&pipelines(tx, Some(&p)), "careful").is_null());
    assert!(!repo.join(".fallow/pipelines/careful.toml").exists());
    let f = sreq(
        tx,
        Req::NewFeature {
            project: p.clone(),
            title: "Calc".into(),
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    for (i, text) in ["Write it.", "Analyze."].iter().enumerate() {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: (*text).into(),
            },
        )
        .unwrap();
        turn_ended(tx, &conv, i + 1);
    }
    let lock = |pipeline: &str| {
        ask(
            tx,
            Request::Lock {
                project: p.clone(),
                feature: "001-calc".into(),
                pipeline: Some(pipeline.into()),
                level: None,
                budget_usd: Some(5.0),
                budget_minutes: Some(60),
            },
        )
    };
    let why = lock("careful").unwrap_err();
    assert!(why.contains("there is no pipeline \"careful\""), "{why}");
    // Saved by the engineer, it is.
    sreq(
        tx,
        Req::PutPipeline {
            name: "careful".into(),
            scope: format!("project:{p}"),
            toml: proposed.into(),
        },
    )
    .unwrap();
    let id = lock("careful").unwrap()["loop"].as_u64().unwrap();
    let v = ask(tx, Request::Loop { id }).unwrap();
    assert_eq!(v["pipeline"]["name"], "careful", "{}", v["pipeline"]);
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}
