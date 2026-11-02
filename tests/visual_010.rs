//! 010's page checks (T012, T025, T028): demo projects with remotes in every state, on the fake
//! runtime and the fake hosting tool, served on a port of their own, so the checks never touch
//! the engineer's Fallow or the network. Not a test: run it by hand with
//! `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_010 -- --ignored --nocapture`; it
//! writes the page's address to `<root>/url.txt`, the projects' ids to `<root>/projects.txt`, and
//! serves for `FALLOW_VISUAL_SECS` seconds (default 1800).

mod common;

use common::*;
use fallow::studio::Req;
use serde_json::json;
use std::path::PathBuf;

#[test]
#[ignore]
fn serve_the_010_demo() {
    let root = PathBuf::from(std::env::var("FALLOW_VISUAL_ROOT").expect("FALLOW_VISUAL_ROOT"));
    let secs: u64 = std::env::var("FALLOW_VISUAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800);
    std::fs::create_dir_all(&root).unwrap();
    // One project per state of the remote.
    let names = ["plain", "guarded", "open", "elsewhere", "unknown", "slow"];
    let repos: Vec<PathBuf> = names.iter().map(|n| repo_named(&root, n)).collect();
    for (n, r) in names.iter().zip(&repos) {
        let address = match *n {
            "plain" => continue,
            "elsewhere" => "git@gitlab.com:acme/elsewhere.git".to_string(),
            _ => format!("https://github.com/acme/{n}.git"),
        };
        bare_remote_in(&root, r, &address, &format!("{n}.remote.git"));
    }
    fake_host(
        &root,
        json!({
            "login": "octo",
            "rules": { "acme/guarded:main": ["pull_request"] },
            "protected": { "acme/guarded:main": false, "acme/open:main": false, "acme/slow:main": true },
            "sleep_ms": { "api": 0 },
        }),
    );
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
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
    std::fs::write(root.join("projects.txt"), ids.join("\n")).unwrap();
    let web = fallow::service::http::Web::start(tx.clone(), "a".repeat(64)).unwrap();
    let url = format!("http://127.0.0.1:{}/?k={}", web.port, "a".repeat(64));
    std::fs::write(root.join("url.txt"), &url).unwrap();
    println!("serving {url} for {secs} s");
    std::thread::sleep(std::time::Duration::from_secs(secs));
}
