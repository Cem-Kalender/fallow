//! 017 T038 (US4, quickstart Q5): the Agents page with many agents, for
//! `specs/017-agent-file-fields/checks/agents.js`. Not a test: run it by hand with
//! `FALLOW_VISUAL_ROOT=<empty folder> FALLOW_VISUAL_AGENTS=20 cargo test --test visual_017
//! --features automation -- --ignored --nocapture`. It writes `<n>` valid checker agents
//! (`agent-01` … `agent-<n>`, default 20) and one file Fallow cannot use (`old-checker.md`, a
//! model alias as before 017) to the engineer's `agents/` folder, adds a project, then serves the
//! page at a fixed address (`url.txt`) for `FALLOW_VISUAL_SECS` seconds (default 1800).

mod common;

use common::*;
use fallow::studio::Req;
use std::path::PathBuf;

/// `n` valid checker agents, `agent-01` … `agent-<n>`, in `dir` (as `visual_fixture`'s).
fn many_agents(dir: &std::path::Path, n: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for i in 1..=n {
        let name = format!("agent-{i:02}");
        std::fs::write(
            dir.join(format!("{name}.md")),
            format!(
                "---\nname: {name}\ndescription: Checks one part of the story ({i} of {n})\nruntime: claude\nmodel: claude-sonnet-5-5\nkind: checker\nmay_run: [gates]\nreturns: verdict\n---\n\nCheck the part numbered {i}.\n"
            ),
        )
        .unwrap();
    }
}

#[test]
#[ignore]
fn serve_the_017_agents() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    let n = std::env::var("FALLOW_VISUAL_AGENTS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    fallow::defs::install_defaults(&config).unwrap();
    let agents = config.join("agents");
    many_agents(&agents, n);
    std::fs::write(
        agents.join("old-checker.md"),
        "---\nname: old-checker\ndescription: Checks the pages, as written before 017\nruntime: claude\nmodel: haiku\nkind: checker\nreturns: verdict\n---\n\nOpen every page.\n",
    )
    .unwrap();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let shop = repo_named(&root, "shop");
    sreq(&e.tx, Req::AddProject { path: shop }).unwrap();
    let web = fallow::service::http::Web::start(e.tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    drop(e);
}
