//! Run records on disk (005 research R-15, contracts/run-record.md).
//!
//! Constitution Principle VII (v5.0.0) allows exactly these fields for runs the engineer chose to
//! record: mode and components switched off, fingerprints, agent, model and version identifiers,
//! commit identifiers, start time and durations, counts, fixed-value outcomes and reported token
//! and cost figures. Parts and roles are numbers; no text written by the engineer or an agent, no
//! names, paths, test names, command lines or session output. Records live in Fallow's cache,
//! readable by the user only, until the engineer deletes them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The directory below the cache folder; the name carries the format version.
pub const DIR: &str = "runs/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprints {
    pub task: String,
    pub hidden: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRow {
    /// 1 is the first session (the lead, or the plain session).
    pub n: u32,
    pub role: u32,
    /// A built-in profile id, or `custom`.
    pub agent: String,
    pub model: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartRow {
    pub n: u32,
    pub working_s: u64,
    pub blocked_s: u64,
    pub signals: u32,
    pub repair_rounds: u32,
    /// `pass`, `baseline-only`, `regression`, `timed-out`, `start-failed`.
    pub gates: Vec<String>,
    /// `approved`, `changes-requested`, `incomplete`.
    pub reviews: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRow {
    pub role: u32,
    pub working_s: u64,
    pub blocked_s: u64,
    pub interventions: u32,
    pub cost_usd: Option<f64>,
    pub tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hidden {
    pub passed: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub mode: String,
    pub switched_off: Vec<String>,
    pub fingerprints: Fingerprints,
    pub sessions: Vec<SessionRow>,
    pub start_commit: String,
    pub final_commit: Option<String>,
    pub started: String,
    pub duration_s: u64,
    pub time_limit_s: u64,
    pub idle_end_s: u64,
    pub parts: Vec<PartRow>,
    pub per_role: Vec<RoleRow>,
    pub hidden: Hidden,
}

/// Fixed values a record may hold where text could go (contracts/run-record.md).
pub const GATE_VALUES: &[&str] = &[
    "pass",
    "baseline-only",
    "regression",
    "timed-out",
    "start-failed",
    "failed",
];
pub const REVIEW_VALUES: &[&str] = &["approved", "changes-requested", "incomplete"];
pub const MODE_VALUES: &[&str] = &["plain", "solo", "squad"];

/// A model id as the agent reported it, kept only when it looks like a provider's model id: a
/// custom alias the engineer chose could carry their own words (constitution VII marker test).
pub fn model_id(s: &str) -> Option<String> {
    const FAMILIES: &[&str] = &["claude-", "gpt-", "o1", "o3", "o4", "gemini-", "codex-"];
    identifier(s).filter(|m| FAMILIES.iter().any(|f| m.starts_with(f)))
}

/// An agent's version as it reported it: digits first (`2.1.330`), never words.
pub fn version_id(s: &str) -> Option<String> {
    identifier(s).filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
}

/// A model or version as the agent reported it: an identifier, never free text (R-15). Anything
/// else is dropped.
pub fn identifier(s: &str) -> Option<String> {
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':' | '/' | '@' | '[' | ']')
        });
    ok.then(|| s.to_string())
}

impl RunRecord {
    /// Checks that only fixed values sit where text could go; a record that fails is not
    /// written (the constitution's boundary, enforced in code as well as by the CI test).
    pub fn check(&self) -> Result<(), String> {
        if !MODE_VALUES.contains(&self.mode.as_str()) {
            return Err(format!("mode {:?}", self.mode));
        }
        for c in &self.switched_off {
            if !super::burst::COMPONENTS.contains(&c.as_str()) {
                return Err(format!("component {c:?}"));
            }
        }
        let hex =
            |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_hexdigit());
        if !hex(&self.fingerprints.task) || !hex(&self.fingerprints.hidden) {
            return Err("fingerprints must be hashes".into());
        }
        if !hex(&self.start_commit) || self.final_commit.as_deref().is_some_and(|c| !hex(c)) {
            return Err("commits must be identifiers".into());
        }
        for s in &self.sessions {
            if crate::session::profiles::builtins()
                .iter()
                .all(|p| p.id != s.agent)
                && s.agent != "custom"
            {
                return Err(format!("agent {:?}", s.agent));
            }
            if s.model.as_deref().is_some_and(|m| model_id(m).is_none())
                || s.version
                    .as_deref()
                    .is_some_and(|v| version_id(v).is_none())
            {
                return Err("model and version must be identifiers".into());
            }
        }
        for p in &self.parts {
            if p.gates.iter().any(|g| !GATE_VALUES.contains(&g.as_str()))
                || p.reviews
                    .iter()
                    .any(|r| !REVIEW_VALUES.contains(&r.as_str()))
            {
                return Err("gate and review outcomes must be fixed values".into());
            }
        }
        if !self
            .started
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | 'Z'))
        {
            return Err("started must be a time".into());
        }
        Ok(())
    }
}

/// The records' directory under the cache folder.
pub fn dir(cache: &Path) -> PathBuf {
    cache.join(DIR)
}

/// Writes a record as the next number; returns that number.
pub fn write(cache: &Path, r: &RunRecord) -> Result<u32, String> {
    r.check()?;
    let d = dir(cache);
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    let n = list(cache).iter().map(|(n, _)| *n).max().unwrap_or(0) + 1;
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    let path = d.join(format!("{n}.json"));
    super::inject::write_private(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(n)
}

/// Every readable record, by number.
pub fn list(cache: &Path) -> Vec<(u32, RunRecord)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir(cache)) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        let n = p
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok());
        if let (Some(n), Some("json")) = (n, p.extension().and_then(|x| x.to_str())) {
            if let Ok(r) = std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<RunRecord>(&t).map_err(|e| e.to_string()))
            {
                out.push((n, r));
            }
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

/// Deletes record `n`.
pub fn delete(cache: &Path, n: u32) -> Result<(), String> {
    let p = dir(cache).join(format!("{n}.json"));
    std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// How many records there are and their total size in bytes.
pub fn usage(cache: &Path) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(dir(cache)) else {
        return (0, 0);
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .fold((0, 0), |(n, size), e| {
            (n + 1, size + e.metadata().map_or(0, |m| m.len()))
        })
}

/// One group of the comparison (FR-019): a task (by fingerprints and settings) in a mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub task_fp: String,
    pub hidden_fp: String,
    pub time_limit_s: u64,
    pub idle_end_s: u64,
    pub mode: String,
    pub runs: usize,
    pub passing: usize,
    pub median_s: u64,
    pub min_s: u64,
    pub max_s: u64,
    pub interventions: u32,
    /// Total reported cost over passing runs, when every session of every run reported it.
    pub cost_per_pass: Option<f64>,
}

/// A run passes when it ran hidden tests and none failed.
pub fn passed(r: &RunRecord) -> bool {
    r.hidden.failed == 0 && r.hidden.passed > 0
}

/// The runs grouped by task fingerprint, settings and mode (FR-019).
pub fn groups(records: &[(u32, RunRecord)]) -> Vec<Group> {
    type Key = (String, String, u64, u64, String);
    let mut map: std::collections::BTreeMap<Key, Vec<&RunRecord>> = Default::default();
    for (_, r) in records {
        map.entry((
            r.fingerprints.task.clone(),
            r.fingerprints.hidden.clone(),
            r.time_limit_s,
            r.idle_end_s,
            r.mode.clone(),
        ))
        .or_default()
        .push(r);
    }
    map.into_iter()
        .map(
            |((task_fp, hidden_fp, time_limit_s, idle_end_s, mode), rs)| {
                let mut d: Vec<u64> = rs.iter().map(|r| r.duration_s).collect();
                d.sort_unstable();
                let passing = rs.iter().filter(|r| passed(r)).count();
                let known = rs
                    .iter()
                    .all(|r| r.per_role.iter().all(|x| x.cost_usd.is_some()));
                let cost: f64 = rs
                    .iter()
                    .flat_map(|r| r.per_role.iter().filter_map(|x| x.cost_usd))
                    .sum();
                Group {
                    task_fp,
                    hidden_fp,
                    time_limit_s,
                    idle_end_s,
                    mode,
                    runs: rs.len(),
                    passing,
                    median_s: d[d.len() / 2],
                    min_s: d[0],
                    max_s: d[d.len() - 1],
                    interventions: rs
                        .iter()
                        .flat_map(|r| r.per_role.iter().map(|x| x.interventions))
                        .sum(),
                    cost_per_pass: (known && passing > 0).then(|| cost / passing as f64),
                }
            },
        )
        .collect()
}
