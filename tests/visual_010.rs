//! 010's page checks (T012, T025, T028): demo projects with remotes in every state, and one with a
//! feature ready to lock whose loop is published, on the fake runtime and the fake hosting tool,
//! served on a port of their own, so the checks never touch the engineer's Fallow or the network.
//! Not a test: run it by hand with `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test
//! visual_010 -- --ignored --nocapture`; it writes the page's address to `<root>/url.txt`, the
//! projects' ids to `<root>/projects.txt`, and serves for `FALLOW_VISUAL_SECS` seconds (default
//! 1800). Once the pull request is opened, checks appear on it, then it is merged
//! (`FALLOW_VISUAL_MERGE_AFTER` seconds later, default 40).

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

#[test]
#[ignore]
fn serve_the_010_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    let merge_after: u64 = std::env::var("FALLOW_VISUAL_MERGE_AFTER")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    std::fs::create_dir_all(&root).unwrap();
    // One project per state of the remote; `calc` has a feature to lock and publish.
    let names = ["plain", "guarded", "open", "elsewhere", "unknown", "slow", "calc"];
    let repos: Vec<PathBuf> = names.iter().map(|n| repo_named(&root, n)).collect();
    for (n, r) in names.iter().zip(&repos) {
        let address = match *n {
            "plain" => continue,
            "elsewhere" => "git@gitlab.com:acme/elsewhere.git".to_string(),
            _ => format!("https://github.com/acme/{n}.git"),
        };
        bare_remote_in(&root, r, &address, &format!("{n}.remote.git"));
    }
    let state = fake_host(
        &root,
        json!({
            "login": "octo",
            "rules": { "acme/guarded:main": ["pull_request"], "acme/calc:main": ["pull_request"] },
            "protected": { "acme/guarded:main": false, "acme/open:main": false, "acme/slow:main": true, "acme/calc:main": false },
        }),
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, format!("{}{}", lead_writes_calc(), calc_loop_ok())).unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let mut ids = Vec::new();
    for r in &repos {
        let id = sreq(tx, Req::AddProject { path: r.clone() }).unwrap()["project"]
            .as_str()
            .unwrap()
            .to_string();
        ids.push(id);
    }
    // `calc`: the Calc feature worked out with the lead, ready to lock from the page.
    let calc = repos.last().unwrap();
    git(calc, &["add", "."]);
    git(calc, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        tx,
        Req::NewFeature {
            project: "calc".into(),
            title: "Calc".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    for (n, text) in [(1, "Write it."), (2, "Analyze.")] {
        sreq(
            tx,
            Req::Say {
                conv: conv.clone(),
                text: text.into(),
            },
        )
        .unwrap();
        turn_ended(tx, &conv, n);
    }
    std::fs::write(root.join("projects.txt"), ids.join("\n")).unwrap();
    let web = fallow::service::http::Web::start(tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    // The remote's side: checks on the pull request once it is there, then its merge.
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    let mut opened: Option<std::time::Instant> = None;
    while std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_secs(1));
        let mut s = host_state(&state);
        let Some(pr) = s["prs"].as_array_mut().and_then(|a| a.first_mut()) else {
            continue;
        };
        let at = *opened.get_or_insert_with(std::time::Instant::now);
        if pr["checks"].as_array().is_none_or(|c| c.is_empty()) {
            pr["checks"] = json!([
                { "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": "https://github.com/acme/calc/actions/runs/1" },
                { "name": "lint", "status": "IN_PROGRESS", "conclusion": "", "detailsUrl": "https://github.com/acme/calc/actions/runs/2" }
            ]);
            set_host_state(&state, &s);
        } else if pr["state"] == "open" && at.elapsed() > Duration::from_secs(merge_after) {
            pr["state"] = json!("merged");
            pr["merged_at"] = json!("2026-11-03T08:00:00Z");
            pr["checks"][1]["status"] = json!("COMPLETED");
            pr["checks"][1]["conclusion"] = json!("SUCCESS");
            set_host_state(&state, &s);
        }
    }
}
