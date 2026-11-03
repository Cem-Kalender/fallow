//! `fallow-fake-host`: a stand-in for GitHub's `gh` in tests (010 contracts/host-adapter.md, "The
//! fake"). It answers the command lines Fallow's GitHub adapter runs, from a JSON state file given
//! as its leading `--state <file>` (the settings' `[hosting.github] args`), and appends every command line it was given (a JSON array per line) to
//! `<state file>.log`, so a test can check what Fallow asked, and that it never asked for a
//! merge, an approval or a token.
//!
//! ```json
//! {
//!   "login": "octo", "signed_in": true,
//!   "rules": { "main": ["pull_request"] },          // per branch (or "owner/repo:branch"); an
//!                                                   // entry may be a full rule object, e.g.
//!                                                   // { "type": "required_status_checks",
//!                                                   //   "parameters": { "strict_required_status_checks_policy": true } }
//!   "protected": { "main": true, "dev": false },    // classic flag; a branch in neither → 404
//!   "strict": { "main": true },                     // classic required_status_checks.strict
//!                                                   // (absent → 404, as for a reader without admin)
//!   "prs": [{ "number": 1, "head": "h", "base": "main", "title": "t", "body": "…",
//!             "state": "open", "merged_at": null,
//!             "mergeable": "MERGEABLE",            // MERGEABLE | CONFLICTING | UNKNOWN (default MERGEABLE)
//!             "merge_state": "CLEAN",              // mergeStateStatus: CLEAN | BEHIND | DIRTY | BLOCKED
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
    // Does the work, then reports a failure all the same (an answer lost on the way back).
    let fail_after = state["fail_after"][&sub].as_str().map(str::to_string);
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
            // The commit the head branch is at on the remote, as GitHub knows it.
            let head = opt("--head").unwrap_or_default();
            let oid = std::process::Command::new("git")
                .args(["ls-remote", "origin", &format!("refs/heads/{head}")])
                .output()
                .ok()
                .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().map(str::to_string));
            let mut prs = prs;
            prs.push(json!({
                "number": number, "head": head, "base": opt("--base"),
                "title": opt("--title"), "body": body, "state": "open", "merged_at": null,
                "checks": [], "head_oid": oid,
            }));
            state["prs"] = Value::from(prs);
            let tmp = format!("{path}.tmp");
            let _ = std::fs::write(&tmp, serde_json::to_string_pretty(&state).unwrap());
            let _ = std::fs::rename(&tmp, &path);
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
                    "mergeable": p["mergeable"].as_str().unwrap_or("MERGEABLE"),
                    "mergeStateStatus": p["merge_state"].as_str().unwrap_or("CLEAN"),
                })
                .to_string()),
                None => Err(format!("no pull requests found for branch \"{n}\"")),
            }
        }
        "pr list" => {
            let head = opt("--head").unwrap_or_default();
            let repo = opt("--repo").unwrap_or_default();
            let owner = repo.split('/').next().unwrap_or("").to_string();
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
                        "headRepositoryOwner": { "login": p["owner"].as_str().unwrap_or(&owner) },
                        "headRefOid": p["head_oid"],
                    })
                })
                .rev()
                .collect();
            Ok(Value::from(found).to_string())
        }
        _ => Err(format!("unknown command: {}", args.join(" "))),
    };
    if let (Ok(_), Some(msg)) = (&out, &fail_after) {
        eprintln!("{msg}");
        std::process::exit(1);
    }
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
        ["repos", o, r, "rules", "branches", b] => {
            let key = format!("{o}/{r}:{b}");
            let pick = if state["rules"][&key].is_null() { *b } else { key.as_str() };
            let rules: Vec<Value> = state["rules"][pick]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|t| if t.is_string() { json!({ "type": t }) } else { t })
                .collect();
            Ok(Value::from(rules).to_string())
        }
        ["repos", o, r, "branches", b, "protection", "required_status_checks"] => {
            match state["strict"][format!("{o}/{r}:{b}")]
                .as_bool()
                .or(state["strict"][*b].as_bool())
            {
                Some(strict) => Ok(json!({ "strict": strict, "contexts": [] }).to_string()),
                None => Err("gh: Not Found (HTTP 404)".into()),
            }
        }
        ["repos", o, r, "branches", b] => match state["protected"][format!("{o}/{r}:{b}")]
            .as_bool()
            .or(state["protected"][*b].as_bool())
        {
            Some(p) => Ok(json!({ "name": b, "protected": p }).to_string()),
            None => Err("gh: Branch not found (HTTP 404)".into()),
        },
        _ => Err(format!("gh: Not Found (HTTP 404) for {endpoint}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_and_rule_objects_are_answered_as_github_does() {
        let state = json!({
            "rules": { "main": ["pull_request", { "type": "required_status_checks",
                "parameters": { "strict_required_status_checks_policy": true } }] },
            "strict": { "main": true },
        });
        let rules: Value =
            serde_json::from_str(&api(&state, "repos/o/r/rules/branches/main").unwrap()).unwrap();
        assert_eq!(rules[0]["type"], "pull_request");
        assert_eq!(rules[1]["parameters"]["strict_required_status_checks_policy"], true);
        let rsc: Value = serde_json::from_str(
            &api(&state, "repos/o/r/branches/main/protection/required_status_checks").unwrap(),
        )
        .unwrap();
        assert_eq!(rsc["strict"], true);
        assert!(api(&state, "repos/o/r/branches/dev/protection/required_status_checks").is_err());
    }
}
