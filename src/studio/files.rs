//! 009 US5: the project's files, read-only (research R-8, R-10, R-12; contracts/web-api.md
//! "Files").
//!
//! A version is the project folder as it is, or a feature's live working copy: the loop's build
//! worktree, the spec session's worktree, or its branch when neither is left. git says what the
//! project ignores, what changed against the feature's base, and which commit (so which task and
//! which agent) last changed a file. Nothing here writes to a project. A path never leads outside
//! the project and Fallow's copies of it, and a file that may hold secrets is listed but never
//! read (constitution V).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::verify::git;

/// A text file is shown up to this many bytes.
pub const MAX_TEXT: usize = 512 * 1024;
/// A file the engineer refers to with `@` goes to the lead up to this many bytes.
pub const REF_MAX: usize = 64 * 1024;
/// A file with a NUL byte in its first bytes is binary.
const SNIFF: usize = 8 * 1024;

pub const OUTSIDE: &str = "That path is outside the project";
pub const GONE: &str = "This version is gone";
pub const SECRET: &str = "Fallow does not read files that may hold secrets";

#[derive(Debug, Clone, Serialize)]
pub struct Version {
    pub id: String,
    pub label: String,
    /// `folder`, `loop`, `spec` or `branch`.
    pub kind: String,
    /// The feature's loop is running.
    pub running: bool,
    /// The commit the feature started from (none for the folder).
    pub base: Option<String>,
    /// The working copy read, or the repository for a branch.
    #[serde(skip)]
    pub dir: PathBuf,
    /// The branch read, for a `branch` version.
    #[serde(skip)]
    pub rev: Option<String>,
    /// The feature's folder (`specs/001-cart`): its files are tagged `spec`.
    #[serde(skip)]
    pub feature_dir: Option<String>,
    /// The version is a git working copy or branch (a plain folder is not).
    #[serde(skip)]
    pub git: bool,
    /// The spec session's lead, for its commits.
    #[serde(skip)]
    pub lead: String,
    /// The agent at work in a running loop, for changes not committed yet.
    #[serde(skip)]
    pub at_work: Option<String>,
}

/// What a feature offers as a version, worked out by the caller (the engine knows the loops).
pub struct FeatureCopy {
    pub name: String,
    pub dir: String,
    pub base_branch: Option<String>,
    pub branch: Option<String>,
    /// The loop's build worktree, and whether the loop runs, with the agent at work.
    pub loop_wt: Option<(PathBuf, bool, Option<String>)>,
    /// The spec session's worktree.
    pub spec_wt: Option<PathBuf>,
    pub lead: String,
}

/// The versions of a project: the folder first, then each feature that still has a copy or a
/// branch (FR-028).
pub fn versions(project: &Path, features: Vec<FeatureCopy>) -> Vec<Version> {
    let mut out = vec![Version {
        id: "folder".into(),
        label: "Project folder".into(),
        kind: "folder".into(),
        running: false,
        base: None,
        dir: project.to_path_buf(),
        rev: None,
        feature_dir: None,
        // 012 (FR-041): every project is a git repository.
        git: true,
        lead: "lead".into(),
        at_work: None,
    }];
    for f in features {
        let (kind, dir, rev, running, at_work) = match (&f.loop_wt, &f.spec_wt, &f.branch) {
            (Some((wt, running, who)), _, _) if wt.is_dir() => {
                ("loop", wt.clone(), None, *running, who.clone())
            }
            (_, Some(wt), _) if wt.is_dir() && wt.as_path() != project => {
                ("spec", wt.clone(), None, false, None)
            }
            (_, _, Some(b)) if git::rev_exists(project, b) => (
                "branch",
                project.to_path_buf(),
                Some(b.clone()),
                false,
                None,
            ),
            _ => continue,
        };
        let tip = rev.clone().unwrap_or_else(|| "HEAD".into());
        let base = f
            .base_branch
            .as_ref()
            .and_then(|b| git::merge_base(&dir, &tip, b).ok());
        out.push(Version {
            id: format!("feature:{}", f.name),
            label: f.name.clone(),
            kind: kind.into(),
            running,
            base,
            dir,
            rev,
            feature_dir: Some(f.dir.clone()),
            git: true,
            lead: f.lead.clone(),
            at_work,
        });
    }
    out
}

/// A relative path that stays inside a version: `/`-separated, no `..`, no drive, no `\`, and
/// no other spelling of a name Windows would also accept (a trailing dot or space, an 8.3 short
/// name with `~`), so a secret cannot be reached under another name. git's own folder is not a
/// project file (its config can hold credentials).
pub fn safe_rel(rel: &str) -> Result<String, String> {
    let bad = rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.contains(':')
        || rel.contains('\0')
        || rel.split('/').any(|c| {
            c.is_empty()
                || c == "."
                || c == ".."
                || c.ends_with('.')
                || c.ends_with(' ')
                || c.eq_ignore_ascii_case(".git")
                || short_name(c)
        });
    if bad {
        Err(OUTSIDE.into())
    } else {
        Ok(rel.to_string())
    }
}

/// An 8.3 short name (`ENV~1`, `CREDEN~1.JSO`): `~` followed by digits.
fn short_name(c: &str) -> bool {
    c.match_indices('~').any(|(i, _)| {
        c[i + 1..]
            .chars()
            .next()
            .is_some_and(|d| d.is_ascii_digit())
    })
}

/// The path of `p` relative to `root` as the file system names it (its real, long name), to
/// check it against the secrets list too.
fn real_rel(root: &Path, p: &Path) -> Option<String> {
    let (r, c) = (root.canonicalize().ok()?, p.canonicalize().ok()?);
    let rel = c.strip_prefix(&r).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// `rel` under `root` on disk, refused when any part of it is a link or it leads outside.
pub fn safe_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = safe_rel(rel)?;
    let mut p = root.to_path_buf();
    for c in rel.split('/') {
        p.push(c);
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if m.file_type().is_symlink() {
                return Err(OUTSIDE.into());
            }
        }
    }
    if p.exists() {
        let (Ok(cr), Ok(cp)) = (root.canonicalize(), p.canonicalize()) else {
            return Err(OUTSIDE.into());
        };
        if !cp.starts_with(&cr) {
            return Err(OUTSIDE.into());
        }
    }
    Ok(p)
}

/// Whether a file may hold secrets (constitution V): listed, never read.
pub fn is_secret(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    if parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|d| matches!(*d, ".aws" | ".ssh" | ".gnupg"))
    {
        return true;
    }
    if parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|d| matches!(*d, ".kube" | ".docker"))
    {
        return true;
    }
    let name = parts.last().copied().unwrap_or_default();
    if name == ".env" || name == ".envrc" || name == ".pgpass" {
        return true;
    }
    if let Some(rest) = name.strip_prefix(".env.") {
        return !matches!(rest, "example" | "sample" | "template" | "dist");
    }
    if name.ends_with(".env")
        || matches!(
            name,
            "token" | ".token" | "token.txt" | "token.json" | "tokens.json"
        )
        || name.ends_with(".token")
    {
        return true;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    matches!(
        ext,
        "pem" | "key" | "p12" | "pfx" | "keystore" | "jks" | "ppk" | "tfvars"
    ) || ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"]
        .iter()
        .any(|k| name.starts_with(k))
        || matches!(name, ".npmrc" | ".pypirc" | ".netrc" | ".git-credentials")
        || name.starts_with("credentials")
        || name.starts_with("secrets.")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<&'static str>,
    pub spec: bool,
    pub secret: bool,
    /// Who last changed a changed file, and in which task (FR-033).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

fn mark_of(c: char) -> &'static str {
    match c {
        'A' => "added",
        'D' => "deleted",
        _ => "changed",
    }
}

/// The files of a version with their marks, and a fingerprint of the list and the marks.
pub fn entries(
    v: &Version,
    step_agent: &dyn Fn(u64, u32) -> Option<String>,
) -> Result<(Vec<Entry>, String, usize), String> {
    let (files, marks): (Vec<String>, Vec<(char, String)>) = if !v.git {
        (walk(&v.dir), Vec::new())
    } else if let Some(rev) = &v.rev {
        let marks = match &v.base {
            Some(b) => git::name_status(&v.dir, b, Some(rev))?,
            None => Vec::new(),
        };
        (git::files_at(&v.dir, rev)?, marks)
    } else {
        let marks = match &v.base {
            Some(b) => git::name_status(&v.dir, b, None)?,
            None => Vec::new(),
        };
        (git::listed(&v.dir)?, marks)
    };
    let marks: BTreeMap<String, char> = marks.into_iter().map(|(c, p)| (p, c)).collect();
    let mut paths: Vec<String> = files;
    // Deleted files are not in the working copy, but are listed with their mark.
    for (p, c) in &marks {
        if *c == 'D' {
            paths.push(p.clone());
        }
    }
    paths.sort();
    paths.dedup();
    let spec_prefix = v.feature_dir.as_ref().map(|d| format!("{d}/"));
    // Who changed each changed file: one log for all of them.
    let commits = match (&v.base, v.git, marks.is_empty()) {
        (Some(b), true, false) => {
            git::last_changes(&v.dir, b, v.rev.as_deref()).unwrap_or_default()
        }
        _ => BTreeMap::new(),
    };
    let list: Vec<Entry> = paths
        .into_iter()
        .map(|p| {
            let mark = marks.get(&p).copied().map(mark_of);
            let (who, task) = match (mark, commits.get(&p)) {
                (Some(_), Some((subject, body))) => {
                    let w = who_of(v, subject, body, step_agent);
                    (w.0, w.1)
                }
                _ => (None, None),
            };
            Entry {
                mark,
                spec: spec_prefix.as_ref().is_some_and(|d| p.starts_with(d)),
                secret: is_secret(&p),
                path: p,
                who,
                task,
            }
        })
        .collect();
    let changed = list.iter().filter(|e| e.mark.is_some()).count();
    let mut h = Fnv::new();
    for e in &list {
        h.add(e.path.as_bytes());
        h.add(e.mark.unwrap_or("").as_bytes());
    }
    // A file written again keeps its path and mark: its size and time count too.
    if v.rev.is_none() {
        for e in list.iter().filter(|e| e.mark.is_some()) {
            if let Ok(m) = std::fs::metadata(v.dir.join(&e.path)) {
                h.add(&m.len().to_le_bytes());
                if let Ok(t) = m.modified().map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                }) {
                    h.add(&t.to_le_bytes());
                }
            }
        }
    }
    Ok((list, format!("{:016x}", h.0), changed))
}

struct Fnv(u64);
impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn add(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= u64::from(*x);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// The files of a plain folder, leaving out `.git` and the caches git also skips; links are not
/// followed.
fn walk(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if t.is_dir() {
                if name == ".git" || git::CACHES.contains(&name.as_str()) {
                    continue;
                }
                stack.push((e.path(), r));
            } else {
                out.push(r);
            }
            if out.len() > 50_000 {
                return out;
            }
        }
    }
    out
}

/// A file of a version (FR-030, FR-036a, FR-037). `step_agent` names the agent definition of a
/// loop's step (run, step), for "who changed it".
pub fn file(
    v: &Version,
    rel: &str,
    changes: bool,
    step_agent: &dyn Fn(u64, u32) -> Option<String>,
) -> Result<Value, String> {
    let rel = safe_rel(rel)?;
    let on_disk = if v.rev.is_none() {
        Some(safe_path(&v.dir, &rel)?)
    } else {
        None
    };
    let mark = if v.git {
        v.base
            .as_ref()
            .map(|b| git::name_status(&v.dir, b, v.rev.as_deref()))
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .find(|(_, p)| *p == rel)
            .map(|(c, _)| mark_of(c))
    } else {
        None
    };
    let mut out = json!({ "path": rel, "version": v.id, "mark": mark });
    // The name asked for, and the name on disk: either one on the secrets list keeps it unread.
    let real = on_disk.as_ref().and_then(|p| real_rel(&v.dir, p));
    if is_secret(&rel) || real.as_deref().is_some_and(is_secret) {
        out["kind"] = json!("secret");
        out["note"] = json!(SECRET);
        if let Some(m) = on_disk.as_ref().and_then(|p| std::fs::metadata(p).ok()) {
            out["size"] = json!(m.len());
        }
        return Ok(out);
    }
    // The bytes: from disk, from the branch, or, for a deleted file, as they were at the base.
    let (bytes, deleted) = match (&on_disk, &v.rev) {
        (Some(p), _) if p.is_file() => (std::fs::read(p).map_err(|e| e.to_string())?, false),
        (None, Some(rev)) => match git::show_bytes(&v.dir, rev, &rel)? {
            Some(b) => (b, false),
            None => (deleted_text(v, &rel)?, true),
        },
        _ => (deleted_text(v, &rel)?, true),
    };
    out["size"] = json!(bytes.len());
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        out["kind"] = json!("binary");
        return Ok(out);
    }
    let cut = bytes.len() > MAX_TEXT;
    let mut end = bytes.len().min(MAX_TEXT);
    while end > 0 && end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    out["kind"] = json!(if deleted { "deleted" } else { "text" });
    out["cut"] = json!(cut);
    // The lines added against the base: all of an added file's.
    let added: Vec<(u32, u32)> = match (mark, &v.base) {
        (Some("added"), _) => vec![(1, text.lines().count().max(1) as u32)],
        (Some("changed"), Some(b)) => git::added_ranges(&v.dir, b, v.rev.as_deref(), &rel)?,
        _ => Vec::new(),
    };
    out["added"] = json!(added);
    if changes {
        out["changes"] = json!(match (mark, &v.base) {
            (Some("added"), _) => text
                .lines()
                .map(|l| format!("+{l}"))
                .collect::<Vec<_>>()
                .join("\n"),
            (Some(_), Some(b)) => git::diff_path(&v.dir, b, v.rev.as_deref(), &rel, 3)?,
            _ => String::new(),
        });
    }
    out["text"] = json!(text);
    if let Some(b) = v.base.as_ref().filter(|_| v.git) {
        out["last"] = last(v, b, &rel, mark.is_some(), step_agent)?;
    }
    Ok(out)
}

fn deleted_text(v: &Version, rel: &str) -> Result<Vec<u8>, String> {
    let b = v
        .base
        .as_ref()
        .filter(|_| v.git)
        .ok_or_else(|| format!("{rel} is not there"))?;
    git::show_bytes(&v.dir, b, rel)?.ok_or_else(|| format!("{rel} is not there"))
}

/// Who last changed a file on this version, from the commit messages Fallow writes (research
/// R-8): `<kind> <task>: …` with "Fallow loop r<id>, step <n>" for a loop's step, `spec: …` for
/// the spec session's lead.
fn last(
    v: &Version,
    base: &str,
    rel: &str,
    marked: bool,
    step_agent: &dyn Fn(u64, u32) -> Option<String>,
) -> Result<Value, String> {
    // A change in a running loop's worktree, not committed yet: the agent at work.
    if v.kind == "loop" && v.running && marked {
        let committed = git::show_bytes(&v.dir, "HEAD", rel)?;
        let now = std::fs::read(v.dir.join(rel)).ok();
        if committed != now {
            return Ok(
                json!({ "who": v.at_work.clone().unwrap_or_else(|| "the agent at work".into()), "live": true }),
            );
        }
    }
    let Some((commit, subject, body, when)) =
        git::last_change(&v.dir, base, v.rev.as_deref(), rel)?
    else {
        return Ok(Value::Null);
    };
    let (who, task, step) = who_of(v, &subject, &body, step_agent);
    Ok(
        json!({ "who": who, "task": task, "step": step, "commit": commit, "when": when, "live": false }),
    )
}

/// Who made a commit, in which task and step, from the message Fallow wrote for it.
fn who_of(
    v: &Version,
    subject: &str,
    body: &str,
    step_agent: &dyn Fn(u64, u32) -> Option<String>,
) -> (Option<String>, Option<String>, Option<u32>) {
    let mut who: Option<String> = None;
    let mut task: Option<String> = None;
    let mut step: Option<u32> = None;
    if let Some(rest) = body.split("Fallow loop r").nth(1) {
        let run: Option<u64> = rest.split(',').next().and_then(|s| s.trim().parse().ok());
        step = rest
            .split("step ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse().ok());
        if let (Some(r), Some(n)) = (run, step) {
            who = step_agent(r, n);
        }
        // "build T009: …": the task.
        if let Some((head, _)) = subject.split_once(':') {
            let mut w = head.split_whitespace();
            let kind = w.next().unwrap_or_default();
            task = w.next().map(str::to_string);
            if who.is_none() && !kind.is_empty() {
                who = Some(kind.to_string());
            }
        }
    } else if let Some(rest) = subject.strip_prefix("Fallow solo ") {
        // 012 (FR-019): a session's commits: the engineer's edits, or the assistant's turn.
        if rest.contains("the engineer's edits") {
            who = Some("you".into());
        } else if let Some(t) = rest.split(", turn").nth(1) {
            let n = t.trim_start_matches('s').split(':').next().unwrap_or_default().trim();
            who = Some(format!("assistant, turn {n}"));
        } else {
            who = Some("assistant".into());
        }
    } else if subject.starts_with("spec: ") {
        who = Some(v.lead.clone());
    } else if subject.starts_with("lock ") {
        who = Some("you".into());
    }
    (who, task, step)
}

// ---- "Open in my editor" (research R-10) -------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Editor {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<Editor>,
}

fn prefs_file(data: &Path) -> PathBuf {
    data.join("prefs.json")
}

fn read_prefs(data: &Path) -> Prefs {
    std::fs::read_to_string(prefs_file(data))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_prefs(data: &Path, p: &Prefs) -> Result<(), String> {
    let f = prefs_file(data);
    let tmp = f.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &f).map_err(|e| format!("{}: {e}", f.display()))
}

pub fn editor(data: &Path) -> Option<Editor> {
    read_prefs(data).editor
}

/// Splits a typed command into the program and its fixed arguments: on spaces, double quotes
/// grouping. No shell reads it.
pub fn parse_command(text: &str) -> Result<Editor, String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for ch in text.trim().chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    parts.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if quoted {
        return Err("a quote is not closed".into());
    }
    if any {
        parts.push(cur);
    }
    let mut it = parts.into_iter();
    let program = it.next().ok_or("say which program opens files")?;
    Ok(Editor {
        program,
        args: it.collect(),
    })
}

/// Saves the editor once its program is there; `None` clears it.
pub fn set_editor(data: &Path, e: Option<Editor>) -> Result<Option<Editor>, String> {
    if let Some(ed) = &e {
        if crate::platform::find_program(&ed.program).is_none() {
            return Err(format!(
                "there is no program {} on this computer",
                ed.program
            ));
        }
    }
    let mut p = read_prefs(data);
    p.editor = e.clone();
    write_prefs(data, &p)?;
    Ok(e)
}

/// The code editors found on this computer, to propose the first time.
pub fn found_editors() -> Vec<Value> {
    let mut out = Vec::new();
    let names: &[(&str, &str)] = &[
        ("Visual Studio Code", "code"),
        ("Visual Studio Code Insiders", "code-insiders"),
        ("Cursor", "cursor"),
        ("Windsurf", "windsurf"),
        ("Zed", "zed"),
        ("Sublime Text", "subl"),
        ("IntelliJ IDEA", "idea"),
        ("PyCharm", "pycharm"),
        ("Notepad++", "notepad++"),
    ];
    for (label, name) in names {
        if crate::platform::find_program(name).is_some() {
            out.push(json!({ "name": label, "program": name, "args": [] }));
        }
    }
    #[cfg(windows)]
    {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let places: Vec<(&str, Option<PathBuf>)> = vec![
            (
                "Visual Studio Code",
                env("LOCALAPPDATA").map(|d| d.join("Programs/Microsoft VS Code/bin/code.cmd")),
            ),
            (
                "Visual Studio Code",
                env("ProgramFiles").map(|d| d.join("Microsoft VS Code/bin/code.cmd")),
            ),
            (
                "Cursor",
                env("LOCALAPPDATA").map(|d| d.join("Programs/cursor/resources/app/bin/cursor.cmd")),
            ),
            (
                "Notepad++",
                env("ProgramFiles").map(|d| d.join("Notepad++/notepad++.exe")),
            ),
        ];
        for (label, p) in places {
            if let Some(p) = p.filter(|p| p.is_file()) {
                if !out.iter().any(|e| e["name"] == label) {
                    out.push(
                        json!({ "name": label, "program": p.display().to_string(), "args": [] }),
                    );
                }
            }
        }
    }
    out
}

/// Starts the editor on one file: the program itself with its fixed arguments and the file's
/// path last, never through a shell, and never "open with the program for this type" (FR-032).
/// A Windows `.cmd` or `.bat` launcher needs `cmd /c`: there every argument is quoted, and one
/// holding a character `cmd` would read is refused.
pub fn open(path: &Path, e: &Editor) -> Result<(), String> {
    let program = crate::platform::find_program(&e.program)
        .ok_or_else(|| format!("there is no program {} on this computer", e.program))?;
    let file = path.display().to_string();
    let launcher = program
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case("cmd") || x.eq_ignore_ascii_case("bat"));
    let mut cmd = if launcher {
        let mut all = vec![program.display().to_string()];
        all.extend(e.args.iter().cloned());
        all.push(file.clone());
        if all
            .iter()
            .any(|a| a.contains(['"', '%', '^', '&', '|', '<', '>', '!']))
        {
            return Err(format!(
                "{} is a script launcher, and this path or one of its arguments holds a character it would read (\" % ^ & | < > !)",
                e.program
            ));
        }
        let mut c = std::process::Command::new("cmd");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
            let line = all
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(" ");
            c.raw_arg("/d /s /v:off /c").raw_arg(format!("\"{line}\""));
        }
        #[cfg(not(windows))]
        {
            c.arg("/c").args(&all);
        }
        c
    } else {
        let mut c = std::process::Command::new(&program);
        c.args(&e.args).arg(&file);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("{} could not be started: {err}", e.program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        for bad in [
            "",
            "../x",
            "/etc/x",
            "C:x",
            "a\\b",
            "a//b",
            "a/./b",
            "a/../b",
            ".env ",
            "key.pem.",
            "ENV~1",
            "a/CREDEN~1.JSO",
            ".git/config",
            ".GIT/HEAD",
            "vendor/x/.git/config",
        ] {
            assert!(safe_rel(bad).is_err(), "{bad}");
        }
        assert_eq!(safe_rel("shop/cart.py").unwrap(), "shop/cart.py");
        assert!(safe_rel("notes~").is_ok() && safe_rel("~draft.md").is_ok());
    }

    #[test]
    fn secrets_are_named_not_read() {
        for s in [
            ".env",
            "app/.env.local",
            "key.pem",
            "a/b.key",
            "c.p12",
            "id_rsa",
            "id_ed25519.pub",
            ".npmrc",
            ".netrc",
            ".git-credentials",
            "credentials.json",
            "secrets.yaml",
            ".aws/config",
            "home/.ssh/known_hosts",
            "production.env",
            ".envrc",
            "x.tfvars",
            ".pgpass",
            "k.ppk",
            ".kube/config",
            ".docker/config.json",
            "token.json",
        ] {
            assert!(is_secret(s), "{s}");
        }
        for s in [
            ".env.example",
            ".env.sample",
            "shop/cart.py",
            "keys.md",
            "README.md",
            "tokenizer.py",
            "tokens.css",
            "token.go",
        ] {
            assert!(!is_secret(s), "{s}");
        }
    }

    #[test]
    fn a_typed_command_splits_without_a_shell() {
        let e = parse_command("\"C:\\Program Files\\x\\x.exe\" -r --wait").unwrap();
        assert_eq!(e.program, "C:\\Program Files\\x\\x.exe");
        assert_eq!(e.args, ["-r", "--wait"]);
        let e = parse_command("code & del x").unwrap();
        assert_eq!(e.program, "code");
        assert_eq!(e.args, ["&", "del", "x"]);
        assert!(parse_command("\"open").is_err());
        assert!(parse_command("  ").is_err());
    }
}
