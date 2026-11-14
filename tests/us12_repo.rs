//! 012 US8 (T023; quickstart Q9; FR-039, FR-041; research R-18): every project is a git
//! repository. A folder without git is shown what its first commit would hold and changes only
//! on the engineer's click; a subfolder of a repository is refused, naming the top folder; a
//! project added before at such a subfolder moves to the top folder with its features; a
//! plain-folder project from before gets the same question.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;
use serde_json::json;
use std::path::Path;

/// Git's global settings for this test process: an identity, nothing from the machine.
fn identity(root: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let cfg = root.parent().unwrap().join("us12-repo-gitconfig");
    ONCE.call_once(|| {
        std::fs::write(&cfg, "[user]\n\tname = Engineer\n\temail = engineer@example.invalid\n[init]\n\tdefaultBranch = main\n[core]\n\tautocrlf = false\n").unwrap();
        // SAFETY: set once, before any engine of this test process starts.
        unsafe {
            std::env::set_var("GIT_CONFIG_GLOBAL", &cfg);
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        }
    });
}

fn listing(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            out.push(p.strip_prefix(dir).unwrap().display().to_string());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_folder_without_git_is_scanned_and_becomes_a_repository_on_the_click() {
    let root = scratch("us12-repo-init");
    identity(&root);
    let shop = root.join("shop");
    std::fs::create_dir_all(shop.join("node_modules/lib")).unwrap();
    std::fs::write(shop.join("app.js"), "console.log(1)\n").unwrap();
    std::fs::write(shop.join("node_modules/lib/index.js"), "x\n").unwrap();
    std::fs::write(shop.join(".env"), "TOKEN=not-a-real-one\n").unwrap();
    let before = listing(&shop);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let added = sreq(tx, Req::AddProject { path: shop.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    let v = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(v["kind"], "needs_git", "{v}");
    // Nothing else opens, and nothing in the folder changed.
    let refused = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap_err();
    assert!(refused.contains("not a git repository yet"), "{refused}");
    // Nor is an agent saved into its folder (Phase 5 review, 2).
    let refused = sreq(
        tx,
        Req::PutAgent {
            name: "reviewer".into(),
            scope: format!("project:{project}"),
            text: "---\nkind: checker\n---\nReview.\n".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("not a git repository yet"), "{refused}");
    let scan = sreq(
        tx,
        Req::GitScan {
            id: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(scan["needs"], "init", "{scan}");
    assert_eq!(scan["files"], 1, "only app.js would be committed: {scan}");
    assert_eq!(scan["heavy"], json!(["node_modules/"]), "{scan}");
    assert_eq!(scan["secrets"], json!([".env"]), "{scan}");
    assert_eq!(scan["identity"], true);
    let ignore = scan["gitignore"].as_str().unwrap().to_string();
    assert!(
        ignore.contains("node_modules/") && ignore.contains("/.env"),
        "{ignore}"
    );
    assert_eq!(listing(&shop), before, "the scan changes nothing");

    // The click: one commit, without the dependencies and the secret.
    let r = sreq(
        tx,
        Req::GitInit {
            id: project.clone(),
            gitignore: Some(ignore),
        },
    )
    .unwrap();
    assert!(r["commit"].is_string(), "{r}");
    assert_eq!(git(&shop, &["rev-list", "--count", "HEAD"]), "1");
    let files = git(&shop, &["ls-files"]);
    assert!(
        files.contains("app.js") && files.contains(".gitignore"),
        "{files}"
    );
    assert!(
        !files.contains(".env") && !files.contains("node_modules"),
        "{files}"
    );
    let v = sreq(
        tx,
        Req::Project {
            id: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(v["kind"], "git", "{v}");
    assert!(
        shop.join(".specify").is_dir(),
        "Spec Kit comes once it is a repository"
    );
    sreq(
        tx,
        Req::NewFeature {
            project,
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
}

#[test]
fn a_subfolder_of_a_repository_is_refused_naming_its_top_folder() {
    let root = scratch("us12-repo-sub");
    identity(&root);
    let mono = repo_named(&root, "mono");
    std::fs::create_dir_all(mono.join("app")).unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let err = sreq(
        &e.tx,
        Req::AddProject {
            path: mono.join("app"),
        },
    )
    .unwrap_err();
    assert!(err.contains("inside"), "{err}");
    assert!(err.contains("mono"), "names the top folder: {err}");
    assert!(!mono.join("app/.specify").exists(), "nothing written");
}

/// A project from before 012, written as Fallow kept it.
fn legacy(root: &Path, path: &Path, features: serde_json::Value) {
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let p = json!([{
        "id": "app", "path": path, "name": "app", "added": "2026-10-01T00:00:00Z",
        "budget_version": 1, "features": features,
    }]);
    std::fs::write(data.join("projects.json"), p.to_string()).unwrap();
}

const SPEC: &str = "# Plots\n\n## User Scenarios & Testing\n\n### User Story 1 - Plot (Priority: P1)\n\n**Acceptance Scenarios**:\n\n1. **Given** data, **When** plotting, **Then** a chart.\n2. **Given** no data, **When** plotting, **Then** it is refused (must fail)\n";
const PLAN: &str = "# Plan\n\n## Commands\n\n- `python -m plots`\n";
const TASKS: &str = "# Tasks\n\n## Phase 1: Setup\n\n- [ ] T001 Create plots.py\n\n## Phase 2: User Story 1 - Plot\n\n- [ ] T002 [US1] Plot\n";

#[test]
fn a_subfolder_project_from_before_moves_to_the_top_folder_and_its_feature_can_be_locked() {
    let root = scratch("us12-repo-move");
    identity(&root);
    let mono = repo_named(&root, "mono");
    let app = mono.join("app");
    let docs = app.join("specs/001-plots");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("spec.md"), SPEC).unwrap();
    std::fs::write(docs.join("plan.md"), PLAN).unwrap();
    std::fs::write(docs.join("tasks.md"), TASKS).unwrap();
    std::fs::create_dir_all(app.join(".fallow")).unwrap();
    std::fs::write(
        app.join(".fallow/squad.toml"),
        "[[gate]]\nname = \"tests\"\ncommand = \"python -m unittest\"\nbaseline = true\n",
    )
    .unwrap();
    legacy(
        &root,
        &app,
        json!([{ "dir": "specs/001-plots", "title": "Plots", "created": "2026-10-01T00:00:00Z",
                 "step": "done", "analyzed": true }]),
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Project { id: "app".into() }).unwrap();
    assert_eq!(v["kind"], "needs_move", "{v}");
    let refused = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "001-plots".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("top folder"), "{refused}");
    // What would move, and nothing changes before the click.
    let m = sreq(
        tx,
        Req::Move {
            id: "app".into(),
            confirmed: false,
        },
    )
    .unwrap();
    assert_eq!(m["features"][0]["to"], "app/specs/001-plots", "{m}");
    assert!(m["top"].as_str().unwrap().contains("mono"), "{m}");
    assert_eq!(git(&mono, &["branch", "--list", "001-plots"]), "");

    let r = sreq(
        tx,
        Req::Move {
            id: "app".into(),
            confirmed: true,
        },
    )
    .unwrap();
    assert_eq!(r["spec_root"], "app", "{r}");
    let v = sreq(tx, Req::Project { id: "app".into() }).unwrap();
    assert_eq!(v["kind"], "git", "{v}");
    // Its feature has a branch holding its documents; they are still where they were.
    assert_eq!(
        git(&mono, &["show", "001-plots:app/specs/001-plots/spec.md"]),
        SPEC.trim_end()
    );
    assert!(docs.join("spec.md").exists());
    let fv = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "001-plots".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["branch"], "001-plots", "{fv}");
    let locked = ask(
        tx,
        Request::Lock {
            project: "app".into(),
            feature: "001-plots".into(),
            pipeline: None,
            level: None,
            budget_usd: None,
            budget_minutes: None,
            when_verified: None,
            update_limit: None,
            run_after: None,
        },
    );
    assert!(locked.is_ok(), "it can be locked: {locked:?}");
    // An agent saved for the project goes to its `.fallow/`, at the spec root (Phase 5 review, 3).
    let reviewer = fallow::defs::DEFAULT_AGENTS
        .iter()
        .find(|(n, _)| *n == "reviewer")
        .unwrap()
        .1;
    sreq(
        tx,
        Req::PutAgent {
            name: "reviewer".into(),
            scope: "project:app".into(),
            text: reviewer.into(),
        },
    )
    .unwrap();
    assert!(app.join(".fallow/agents/reviewer.md").is_file());
    assert!(!mono.join(".fallow/agents/reviewer.md").exists());
    // A new feature goes under the spec root too, numbered after the moved one.
    let f = sreq(
        tx,
        Req::NewFeature {
            project: "app".into(),
            title: "Charts".into(),
            lead: None,
        },
    )
    .unwrap();
    let fv = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "002-charts".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["dir"], "app/specs/002-charts", "{f} {fv}");
}

#[test]
fn a_plain_folder_project_from_before_gets_the_question_and_keeps_its_documents() {
    let root = scratch("us12-repo-plain");
    identity(&root);
    let thesis = root.join("thesis");
    std::fs::create_dir_all(thesis.join("specs/001-plots")).unwrap();
    std::fs::write(thesis.join("plot.py"), "print(1)\n").unwrap();
    std::fs::write(thesis.join("specs/001-plots/spec.md"), "# Plots\n").unwrap();
    legacy(
        &root,
        &thesis,
        json!([{ "dir": "specs/001-plots", "title": "Plots", "created": "2026-10-01T00:00:00Z" }]),
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Project { id: "app".into() }).unwrap();
    assert_eq!(v["kind"], "needs_git", "{v}");
    let scan = sreq(tx, Req::GitScan { id: "app".into() }).unwrap();
    assert_eq!(scan["files"], 2, "{scan}");
    sreq(
        tx,
        Req::GitInit {
            id: "app".into(),
            gitignore: None,
        },
    )
    .unwrap();
    // The feature's documents are in the first commit; the feature has its branch.
    assert_eq!(
        git(&thesis, &["show", "HEAD:specs/001-plots/spec.md"]),
        "# Plots"
    );
    let fv = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "001-plots".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["branch"], "001-plots", "{fv}");
}

// 014 (T005): the HTTP transport is in the `automation` build only on Windows.
#[cfg(any(not(windows), feature = "automation"))]
/// 012 T027: the pages' demo. Not a test: run it by hand with `FALLOW_VISUAL_ROOT=<empty folder>
/// cargo test --test us12_repo serve_the_012_repo_demo -- --ignored --nocapture`; it writes the
/// page's address to `<root>/url.txt` and the projects to `<root>/projects.txt` (a folder that
/// needs git, then a project inside a bigger repository), and serves for `FALLOW_VISUAL_SECS`
/// seconds (default 1800).
#[test]
#[ignore]
fn serve_the_012_repo_demo() {
    let root =
        std::path::PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    identity(&root);
    let shop = root.join("shop");
    std::fs::create_dir_all(shop.join("node_modules/lib")).unwrap();
    std::fs::create_dir_all(shop.join("src")).unwrap();
    std::fs::write(shop.join("src/app.js"), "console.log(1)\n").unwrap();
    std::fs::write(shop.join("package.json"), "{}\n").unwrap();
    std::fs::write(shop.join("node_modules/lib/index.js"), "x\n").unwrap();
    std::fs::write(shop.join(".env"), "TOKEN=not-a-real-one\n").unwrap();
    let mono = repo_named(&root, "mono");
    let app = mono.join("app");
    std::fs::create_dir_all(app.join("specs/001-plots")).unwrap();
    std::fs::write(app.join("specs/001-plots/spec.md"), SPEC).unwrap();
    legacy(
        &root,
        &app,
        json!([{ "dir": "specs/001-plots", "title": "Plots", "created": "2026-10-01T00:00:00Z" }]),
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    // With `FALLOW_GIT_CHECK` naming a missing program, nothing is added: the start page says why.
    let shop_id = sreq(&e.tx, Req::AddProject { path: shop.clone() })
        .map(|v| v["project"].as_str().unwrap().to_string())
        .unwrap_or_default();
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    std::fs::write(root.join("projects.txt"), format!("{shop_id}\napp")).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}

#[test]
fn a_project_made_a_repository_outside_fallow_gives_its_features_their_branches_first() {
    let root = scratch("us12-repo-outside");
    identity(&root);
    let thesis = root.join("thesis");
    std::fs::create_dir_all(thesis.join("specs/001-plots")).unwrap();
    std::fs::write(thesis.join("specs/001-plots/spec.md"), "# Plots\n").unwrap();
    legacy(
        &root,
        &thesis,
        json!([{ "dir": "specs/001-plots", "title": "Plots", "created": "2026-10-01T00:00:00Z" }]),
    );
    // The engineer made it a repository by hand, its spec documents left out.
    std::fs::write(
        thesis.join("README.md"),
        "thesis
",
    )
    .unwrap();
    git(&thesis, &["init", "-q"]);
    git(&thesis, &["add", "README.md"]);
    git(&thesis, &["commit", "-q", "-m", "mine"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let v = sreq(tx, Req::Project { id: "app".into() }).unwrap();
    assert_eq!(v["git"]["setup_left"], true, "{v}");
    let refused = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "001-plots".into(),
        },
    )
    .unwrap_err();
    assert!(refused.contains("not set up yet"), "{refused}");
    let scan = sreq(tx, Req::GitScan { id: "app".into() }).unwrap();
    assert_eq!(scan["needs"], "branches", "{scan}");
    sreq(
        tx,
        Req::GitInit {
            id: "app".into(),
            gitignore: None,
        },
    )
    .unwrap();
    assert_eq!(
        git(&thesis, &["rev-list", "--count", "HEAD"]),
        "1",
        "no second commit"
    );
    let fv = sreq(
        tx,
        Req::Feature {
            project: "app".into(),
            feature: "001-plots".into(),
        },
    )
    .unwrap();
    assert_eq!(fv["branch"], "001-plots", "{fv}");
    // Its documents are on its branch; the engineer's checkout is as it was.
    assert_eq!(
        git(&thesis, &["show", "001-plots:specs/001-plots/spec.md"]),
        "# Plots"
    );
    assert_eq!(git(&thesis, &["rev-list", "--count", "main"]), "1");
    assert!(
        !thesis.join(".specify").exists(),
        "nothing written in the folder"
    );
}
