//! `fallow-fake-host`: a stand-in for GitHub's `gh` in tests (010 contracts/host-adapter.md, "The
//! fake"). It answers the command lines Fallow's GitHub adapter runs, from a JSON state file given
//! as its leading `--state <file>` (the settings' `[hosting.github] args`), and appends every command line it was given (a JSON array per line) to
//! `<state file>.log`, so a test can check what Fallow asked, and that it never asked for a
//! merge, an approval or a token.
//!
//! ```json
//! {
//!   "login": "octo", "signed_in": true,
//!   "rules": { "main": ["pull_request"] },          // per branch: rule types
//!   "protected": { "main": true, "dev": false },    // classic flag; a branch in neither → 404
//!   "prs": [{ "number": 1, "head": "h", "base": "main", "title": "t", "body": "…",
//!             "state": "open", "merged_at": null,
//!             "checks": [{ "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS",
//!                          "detailsUrl": "https://…" }] }],
//!   "fail": { "pr create": "message" },             // auth, api, pr create, pr view, pr list
//!   "sleep_ms": { "api": 3000 }
//! }
//! ```
//!
//! A test may change the file between calls (merge a pull request, close it, set checks).

use serde_json::{Value, json};
use std::io::Write;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("--state") || args.len() < 2 {
        eprintln!("fallow-fake-host: the first arguments must be --state <file>");
        std::process::exit(2);
    }
    let path = args[1].clone();
    args.drain(..2);
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{path}.log"))
    {
        let _ = writeln!(log, "{}", Value::from(args.clone()));
    }
    let mut state: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    let sub = match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("auth"), _) => "auth".to_string(),
        (Some("api"), _) => "api".to_string(),
        (Some("pr"), Some(s)) => format!("pr {s}"),
        _ => String::new(),
    };
    if let Some(ms) = state["sleep_ms"][&sub].as_u64() {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Some(msg) = state["fail"][&sub].as_str() {
        eprintln!("{msg}");
        std::process::exit(1);
    }
    let opt = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let out = match sub.as_str() {
        "auth" => {
            if state["signed_in"].as_bool().unwrap_or(true) {
                let login = state["login"].as_str().unwrap_or("octo");
                Ok(format!(
                    "github.com\n  ✓ Logged in to github.com account {login} (keyring)\n  - Active account: true\n"
                ))
            } else {
                Err("You are not logged into any GitHub hosts. To log in, run: gh auth login".into())
            }
        }
        "api" => api(&state, args.get(1).map(String::as_str).unwrap_or("")),
        "pr create" => {
            let repo = opt("--repo").unwrap_or_default();
            let prs = state["prs"].as_array().cloned().unwrap_or_default();
            let number = prs.iter().filter_map(|p| p["number"].as_u64()).max().unwrap_or(0) + 1;
            let body = opt("--body-file")
                .and_then(|f| std::fs::read_to_string(f).ok())
                .unwrap_or_default();
            let mut prs = prs;
            prs.push(json!({
                "number": number, "head": opt("--head"), "base": opt("--base"),
                "title": opt("--title"), "body": body, "state": "open", "merged_at": null,
                "checks": [],
            }));
            state["prs"] = Value::from(prs);
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&state).unwrap());
            Ok(format!("https://github.com/{repo}/pull/{number}\n"))
        }
        "pr view" => {
            let n: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let repo = opt("--repo").unwrap_or_default();
            match pr(&state, |p| p["number"].as_u64() == Some(n)) {
                Some(p) => Ok(json!({
                    "state": p["state"].as_str().unwrap_or("open").to_uppercase(),
                    "mergedAt": p["merged_at"],
                    "url": format!("https://github.com/{repo}/pull/{n}"),
                    "statusCheckRollup": p["checks"],
                })
                .to_string()),
                None => Err(format!("no pull requests found for branch \"{n}\"")),
            }
        }
        "pr list" => {
            let head = opt("--head").unwrap_or_default();
            let repo = opt("--repo").unwrap_or_default();
            let found: Vec<Value> = state["prs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p["head"].as_str() == Some(head.as_str()))
                .map(|p| {
                    let n = p["number"].as_u64().unwrap_or(0);
                    json!({
                        "number": n,
                        "url": format!("https://github.com/{repo}/pull/{n}"),
                        "state": p["state"].as_str().unwrap_or("open").to_uppercase(),
                    })
                })
                .rev()
                .take(1)
                .collect();
            Ok(Value::from(found).to_string())
        }
        _ => Err(format!("unknown command: {}", args.join(" "))),
    };
    match out {
        Ok(text) => print!("{text}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn pr(state: &Value, pick: impl Fn(&Value) -> bool) -> Option<Value> {
    state["prs"].as_array()?.iter().find(|p| pick(p)).cloned()
}

/// `repos/{o}/{r}/rules/branches/{b}` and `repos/{o}/{r}/branches/{b}`.
fn api(state: &Value, endpoint: &str) -> Result<String, String> {
    let parts: Vec<&str> = endpoint.split('/').collect();
    match parts.as_slice() {
        ["repos", _, _, "rules", "branches", b] => {
            let rules: Vec<Value> = state["rules"][*b]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|t| json!({ "type": t }))
                .collect();
            Ok(Value::from(rules).to_string())
        }
        ["repos", _, _, "branches", b] => match state["protected"][*b].as_bool() {
            Some(p) => Ok(json!({ "name": b, "protected": p }).to_string()),
            None => Err("gh: Branch not found (HTTP 404)".into()),
        },
        _ => Err(format!("gh: Not Found (HTTP 404) for {endpoint}")),
    }
}
