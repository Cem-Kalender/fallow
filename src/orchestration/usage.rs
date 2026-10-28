//! Cost as the agent reports it (005 research R-10, FR-015).
//!
//! In a recorded run, a Claude Code session gets a status line on its command line: `fallow
//! usage-report`, which Claude Code runs after each response with its session JSON on standard
//! input. It prints one short marker line with the session's reported cost, model and version,
//! which Claude Code shows at the bottom of its screen. Fallow reads that line from the session's
//! screen, in memory, as it reads permission prompts: no connection, no file, no transcript.
//! Cost is Claude Code's own estimate at list price; cumulative tokens are not in its JSON, so
//! they stay unknown. An agent could print a fake marker; the figure is the agent's report.

use serde_json::Value;

/// The marker's start; the rest is `cost=<usd> model=<id> version=<v>`.
pub const MARKER: &str = "fallow-usage";

/// What a session reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub cost_usd: Option<f64>,
    pub model: Option<String>,
    pub version: Option<String>,
}

/// The marker line for Claude Code's status line JSON (`fallow usage-report`).
pub fn marker(json: &str) -> String {
    let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
    let cost = v["cost"]["total_cost_usd"].as_f64();
    let model = v["model"]["id"].as_str().and_then(super::record::model_id);
    let version = v["version"].as_str().and_then(super::record::version_id);
    format!(
        "{MARKER} cost={} model={} version={}",
        cost.map_or("-".to_string(), |c| format!("{c:.4}")),
        model.as_deref().unwrap_or("-"),
        version.as_deref().unwrap_or("-")
    )
}

/// The latest marker among a screen's lines, bottom first.
pub fn read(lines: &[String]) -> Option<Report> {
    lines.iter().rev().find_map(|l| {
        let at = l.find(MARKER)?;
        let mut r = Report {
            cost_usd: None,
            model: None,
            version: None,
        };
        for kv in l[at + MARKER.len()..].split_whitespace() {
            match kv.split_once('=') {
                Some(("cost", c)) => r.cost_usd = c.parse::<f64>().ok().filter(|c| c.is_finite()),
                Some(("model", m)) if m != "-" => r.model = super::record::model_id(m),
                Some(("version", v)) if v != "-" => r.version = super::record::version_id(v),
                _ => {}
            }
        }
        Some(r)
    })
}

/// The command-line additions that give a Claude Code session the status line (research R-10):
/// `--settings` with a `statusLine` command running `fallow usage-report`. `None` when the
/// profile passes its own `--settings`: cost is then unknown for that session. The program path
/// uses forward slashes, since Claude Code runs status line commands through Git Bash on Windows.
pub fn status_line_args(exe: &std::path::Path, profile_args: &[String]) -> Option<Vec<String>> {
    if profile_args
        .iter()
        .any(|a| a == "--settings" || a.starts_with("--settings="))
    {
        return None;
    }
    let program = exe.to_string_lossy().replace('\\', "/");
    let command = format!("\"{program}\" usage-report");
    let settings = serde_json::json!({
        "statusLine": { "type": "command", "command": command }
    });
    Some(vec!["--settings".into(), settings.to_string()])
}

/// `fallow usage-report`: reads the status line JSON on standard input and prints the marker.
pub fn run_report() -> std::process::ExitCode {
    use std::io::Read;
    let mut s = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut s);
    println!("{}", marker(&s));
    std::process::ExitCode::SUCCESS
}
