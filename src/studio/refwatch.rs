//! 012 US4 (FR-016, FR-017; research R-10, R-11): every change to a project's branches and its
//! remote-tracking refs, seen and shown with a proven source. A snapshot is the refs, the
//! folder's checked-out branch and its status, read without a lock; two snapshots give the
//! changes; a session is named the source only of a change its own git command made on the ref
//! that command targets; Fallow's other work by the commits its records hold; anything else is
//! "outside Fallow". Fallow never guesses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::conversation::solo::How;
use crate::verify::git;

/// What a project's git looked like at one moment (R-10).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub refs: BTreeMap<String, String>,
    /// The branch checked out in the engineer's folder (`refs/heads/main`), if any.
    pub head: Option<String>,
}

/// Reads `repo`'s refs and checked-out branch, taking no lock (R-2).
pub fn take(repo: &Path) -> Option<Snapshot> {
    let refs = git::ref_snapshot(repo).ok()?;
    let head = crate::platform::command("git")
        .arg("-C")
        .arg(repo)
        .args(["--no-optional-locks", "symbolic-ref", "-q", "HEAD"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    Some(Snapshot { refs, head })
}

/// A session's git command Fallow allowed, open until its result (R-10): its id, its text, the
/// snapshot taken when it was allowed, and the turn it belongs to.
#[derive(Debug, Clone)]
pub struct OpenCall {
    /// The tool calls' ids (a second command allowed before the first's result joins it).
    pub ids: Vec<String>,
    pub commands: Vec<String>,
    pub opening: Option<Snapshot>,
    pub turn: u32,
    /// The refs the commands target, `…/*` for all of a remote's.
    pub targets: Vec<String>,
    /// A command ran git in the engineer's folder.
    pub folder: bool,
    pub opened_at: std::time::Instant,
}

/// Whether `refname` is among `targets` (`refs/remotes/origin/*` names all of `origin`'s).
pub fn targeted(targets: &[String], refname: &str) -> bool {
    targets.iter().any(|t| match t.strip_suffix('*') {
        Some(prefix) => refname.starts_with(prefix),
        None => t == refname,
    })
}

/// Who made a change (R-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Session { id: String, name: String },
    Loop { id: u64, feature: String },
    Spec { feature: String },
    Outside,
}

/// One change to one ref (R-10), as stored in `git.jsonl` and shown on the project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub refname: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub how: How,
    /// For `commits`: how many.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    pub source: Source,
    /// Fallow's own work on its own branches: shown folded.
    #[serde(default)]
    pub quiet: bool,
    pub at: String,
}

/// The changed refs between two snapshots, each with how it moved.
/// One changed ref: its name, before and after, how it moved, and how many commits.
pub type RefMove = (String, Option<String>, Option<String>, How, Option<u32>);

pub fn diff(repo: &Path, old: &Snapshot, new: &Snapshot) -> Vec<RefMove> {
    let mut out = Vec::new();
    let names: BTreeSet<&String> = old.refs.keys().chain(new.refs.keys()).collect();
    for r in names {
        // A remote's symbolic HEAD follows its branch: not a change of its own.
        if r.ends_with("/HEAD") {
            continue;
        }
        let (b, a) = (old.refs.get(r), new.refs.get(r));
        if b == a {
            continue;
        }
        let (how, n) = match (b, a) {
            (None, Some(_)) => (How::Created, None),
            (Some(_), None) => (How::Deleted, None),
            (Some(b), Some(a)) if git::is_ancestor(repo, b, a) => {
                (How::Commits, git::count_between(repo, b, a))
            }
            (Some(b), Some(a)) if git::is_ancestor(repo, a, b) => (How::Rewound, None),
            _ => (How::Rewritten, None),
        };
        out.push((r.clone(), b.cloned(), a.cloned(), how, n));
    }
    out
}

/// One git command of a command line: whether it runs in the engineer's folder, its
/// subcommand and its words after it.
struct GitCmd {
    in_folder: bool,
    sub: String,
    args: Vec<String>,
}

/// Reads each git command of `command`, following `cd` and `-C` (relative to `cwd`, the
/// command's working directory), `--git-dir`/`--work-tree`, an `env` prefix and `git.exe`.
fn git_cmds(command: &str, folder: &Path, cwd: &Path) -> Vec<GitCmd> {
    let Ok(parts) = super::judge::split(command) else {
        return Vec::new();
    };
    let canon = |p: &Path| p.canonicalize().ok();
    let folder_c = canon(folder);
    let same = |p: &Path| folder_c.is_some() && canon(p) == folder_c;
    let mut here = cwd.to_path_buf();
    let mut out = Vec::new();
    for part in parts {
        let mut w: Vec<String> = part.words.clone();
        // `env A=b git …`
        if w.first().is_some_and(|x| x == "env") {
            w.remove(0);
            while w.first().is_some_and(|x| x.contains('=') && !x.starts_with('-')) {
                w.remove(0);
            }
        }
        let Some(first) = w.first().map(|x| x.to_ascii_lowercase()) else {
            continue;
        };
        if matches!(first.as_str(), "cd" | "pushd" | "set-location" | "sl" | "chdir") {
            if let Some(d) = w.iter().skip(1).find(|x| !x.starts_with('-')) {
                here = here.join(d);
            }
            continue;
        }
        let prog = first.rsplit(['/', '\\']).next().unwrap_or(&first).trim_end_matches(".exe").to_string();
        if prog != "git" {
            continue;
        }
        let mut dir = here.clone();
        let mut i = 1;
        let mut in_folder_flag = false;
        while i < w.len() && w[i].starts_with('-') {
            let a = w[i].as_str();
            if a == "-C" {
                if let Some(d) = w.get(i + 1) {
                    dir = dir.join(d);
                }
                i += 2;
                continue;
            }
            if a == "-c" || a == "--git-dir" || a == "--work-tree" || a == "--namespace" {
                if matches!(a, "--git-dir" | "--work-tree") {
                    if let Some(d) = w.get(i + 1) {
                        let p = dir.join(d);
                        in_folder_flag |= same(&p) || p.parent().is_some_and(same);
                    }
                }
                i += 2;
                continue;
            }
            for k in ["--git-dir=", "--work-tree="] {
                if let Some(d) = a.strip_prefix(k) {
                    let p = dir.join(d);
                    in_folder_flag |= same(&p) || p.parent().is_some_and(same);
                }
            }
            i += 1;
        }
        let Some(sub) = w.get(i).cloned() else {
            continue;
        };
        out.push(GitCmd {
            in_folder: in_folder_flag || same(&dir),
            sub,
            args: w[i + 1..].to_vec(),
        });
    }
    out
}

/// The refs a git command line targets (R-10, "that call's parsed command targets that ref"):
/// a push's refspecs as `refs/remotes/<remote>/<b>` (the current branch when none is named), a
/// `-C <folder>` merge, commit, pull, cherry-pick, reset or rebase moves the folder's checked-out
/// branch, and `branch`, `update-ref`, `checkout -B` and `switch -C` name theirs. `folder` is the
/// engineer's folder; `folder_head` its branch at the opening snapshot; `own` the branch the
/// command runs on when it names no folder.
pub fn targets(command: &str, folder: &Path, folder_head: Option<&str>, own: &str, cwd: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for g in git_cmds(command, folder, cwd) {
        let in_folder = g.in_folder;
        let sub = g.sub.as_str();
        let args: Vec<&str> = g.args.iter().map(String::as_str).collect();
        // Words that are values of an option, not names (`push -o ci.skip origin main`).
        let mut valued = vec!["-o", "--push-option", "--repo", "--receive-pack", "--exec"];
        if matches!(sub, "commit" | "merge" | "tag" | "revert" | "notes") {
            valued.extend(["-m", "-F", "--message", "--file"]);
        }
        let mut plain: Vec<&str> = Vec::new();
        let mut skip = false;
        for a in &args {
            if skip {
                skip = false;
                continue;
            }
            if valued.contains(a) {
                skip = true;
                continue;
            }
            if !a.starts_with('-') {
                plain.push(a);
            }
        }
        let current = if in_folder {
            folder_head.map(|h| h.trim_start_matches("refs/heads/").to_string())
        } else {
            Some(own.to_string())
        };
        match sub {
            "push" => {
                let remote = plain.first().copied().unwrap_or("origin");
                let specs = &plain[plain.len().min(1)..];
                if args.iter().any(|a| matches!(*a, "--all" | "--mirror" | "--branches")) {
                    out.push(format!("refs/remotes/{remote}/*"));
                    continue;
                }
                if specs.is_empty() {
                    if let Some(c) = &current {
                        out.push(format!("refs/remotes/{remote}/{c}"));
                    }
                }
                for s in specs {
                    let s = s.trim_start_matches('+');
                    let dest = s.rsplit_once(':').map_or(s, |(_, d)| d);
                    let dest = dest.trim_start_matches("refs/heads/");
                    let dest = if dest == "HEAD" { current.as_deref().unwrap_or(dest) } else { dest };
                    if !dest.is_empty() {
                        out.push(format!("refs/remotes/{remote}/{dest}"));
                    }
                }
            }
            "merge" | "commit" | "pull" | "cherry-pick" | "reset" | "rebase" | "revert" | "am" => {
                if in_folder {
                    if let Some(h) = folder_head {
                        out.push(h.to_string());
                    }
                }
            }
            "branch" => {
                // Short flags may come together (`-dr`, `-df`).
                let short: String = args
                    .iter()
                    .filter(|a| a.starts_with('-') && !a.starts_with("--"))
                    .flat_map(|a| a.chars().skip(1))
                    .collect();
                let delete = short.contains(['d', 'D']) || args.contains(&"--delete");
                let remote = short.contains('r') || args.contains(&"--remotes");
                if delete {
                    // `git branch -d -r origin/x` deletes a remote-tracking ref.
                    for n in &plain {
                        out.push(if remote { format!("refs/remotes/{n}") } else { format!("refs/heads/{n}") });
                    }
                    continue;
                }
                // Only what moves a branch: `-f`, `-m`/`-M`, `-c`/`-C`, or a bare creation; never
                // a listing (`--contains main`, `-v`, `--list`).
                let listing = ["-l", "--list", "-a", "--all", "-r", "--remotes", "-v", "-vv", "--verbose", "--contains", "--no-contains", "--merged", "--no-merged", "--points-at", "--show-current", "--format", "--sort", "--column"];
                if args.iter().any(|a| listing.contains(a) || a.starts_with("--format=") || a.starts_with("--sort=")) {
                    continue;
                }
                // Two names only for a move or a copy; `git branch x main` names x alone.
                let two = args.iter().any(|a| matches!(*a, "-m" | "-M" | "-c" | "-C" | "--move" | "--copy"));
                let all = args.iter().any(|a| matches!(*a, "-d" | "-D" | "--delete"));
                for n in plain.iter().take(if all { usize::MAX } else if two { 2 } else { 1 }) {
                    out.push(format!("refs/heads/{n}"));
                }
            }
            "update-ref" => {
                if let Some(r) = plain.first() {
                    out.push(r.to_string());
                }
            }
            "checkout" | "switch" => {
                if let Some(k) = args.iter().position(|a| *a == "-B" || *a == "-C" || *a == "-b" || *a == "-c") {
                    if let Some(n) = args.get(k + 1) {
                        out.push(format!("refs/heads/{n}"));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether `command` runs git in the engineer's folder (`git -C <folder> …`, `cd <folder> &&
/// git …`), `cwd` being where it starts.
pub fn in_folder(command: &str, folder: &Path, cwd: &Path) -> bool {
    git_cmds(command, folder, cwd).iter().any(|g| g.in_folder)
}

/// 012 (FR-016, analyze C2; T037): what a request that changes the engineer's folder must show:
/// the folder's checked-out branch and its uncommitted changes, and for a merge the session's
/// commits it would bring (by subject), that the assistant's own commits are not known apart,
/// whether it squashes, and the `.fallow/` paths it would change.
pub fn folder_facts(folder: &Path, command: &str, session_branch: &str, root: &str, cwd: &Path) -> serde_json::Value {
    let branch = crate::platform::command("git")
        .arg("-C")
        .arg(folder)
        .args(["--no-optional-locks", "symbolic-ref", "-q", "--short", "HEAD"])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let uncommitted = git::folder_status(folder).unwrap_or_default();
    let mut v = serde_json::json!({ "folder_branch": branch, "uncommitted": uncommitted });
    // A merge run in the folder (never `git log --grep merge`, nor one in the session's copy),
    // of the branch it names.
    let merge = git_cmds(command, folder, cwd)
        .into_iter()
        .find(|g| g.in_folder && g.sub == "merge" && !g.args.iter().any(|a| a == "--abort" || a == "--continue"));
    if let (Some(m), Some(target)) = (merge, branch.as_deref()) {
        // The first name, past the values of `-m`, `-F`, `-s`, `-X` and the like.
        let valued = ["-m", "-F", "--message", "--file", "-s", "--strategy", "-X", "--strategy-option", "--cleanup", "--into-name"];
        let mut skip = false;
        let mut source = None;
        for a in &m.args {
            if skip {
                skip = false;
                continue;
            }
            if valued.contains(&a.as_str()) {
                skip = true;
                continue;
            }
            if !a.starts_with('-') {
                source = Some(a.clone());
                break;
            }
        }
        let source = source.unwrap_or_else(|| session_branch.to_string());
        let session_branch = source.as_str();
        let squash = m.args.iter().any(|a| a == "--squash");
        let range = format!("{target}..{session_branch}");
        let subjects: Vec<String> = git::subjects(folder, &range);
        // What the session's side changes since they forked (three dots), not main's own.
        let fork = git::merge_base(folder, target, session_branch).unwrap_or_default();
        let fallow: Vec<String> = git::files_between_z(folder, fork.trim(), session_branch)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.starts_with(&format!("{root}.fallow/")))
            .collect();
        v["merge"] = serde_json::json!({
            "into": target,
            "commits": subjects,
            "own_commits": "unknown",
            "squash": squash,
            "fallow_changes": fallow,
        });
    }
    v
}

/// The engineer's folder's state after a command that touched it (R-6, analyze C12).
pub fn folder_state(folder: &Path) -> crate::conversation::solo::FolderState {
    use crate::conversation::solo::FolderState;
    // Tracked files only (R-10: `status -uno`): an untracked file is no change of a command's.
    let lines: Vec<String> = git::folder_status(folder)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !l.starts_with("??"))
        .collect();
    if lines.iter().any(|l| {
        let b = l.as_bytes();
        b.len() > 1 && (b[0] == b'U' || b[1] == b'U' || (b[0] == b'A' && b[1] == b'A') || (b[0] == b'D' && b[1] == b'D'))
    }) {
        return FolderState::Conflicted;
    }
    if lines.is_empty() {
        return FolderState::Unchanged;
    }
    if lines.iter().all(|l| l.as_bytes().get(1) == Some(&b' ')) {
        return FolderState::StagedOnly;
    }
    FolderState::Changed
}

/// Removes `keys` everywhere in `v` (commits a record only points at).
pub fn strip_keys(v: &mut serde_json::Value, keys: &[&str]) {
    match v {
        serde_json::Value::Object(m) => {
            for k in keys {
                m.remove(*k);
            }
            m.values_mut().for_each(|x| strip_keys(x, keys));
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| strip_keys(x, keys)),
        _ => {}
    }
}

/// Every full commit id a record holds (a loop's or a conversation's folded state): what its
/// log proves it made (R-10).
pub fn commits_in(v: &serde_json::Value, out: &mut BTreeSet<String>) {
    match v {
        serde_json::Value::String(s) if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) => {
            out.insert(s.to_ascii_lowercase());
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| commits_in(x, out)),
        serde_json::Value::Object(m) => m.values().for_each(|x| commits_in(x, out)),
        _ => {}
    }
}

/// R-11: whether commit `c` (one new commit on a branch) holds exactly the session's changes from
/// `base` up to `turn_commit`: every path the session changed has its blob from `turn_commit` in
/// `c`, and no other path differs between `c^` and `c`.
pub fn is_squash_of(dir: &Path, c: &str, base: &str, turn_commit: &str) -> bool {
    let session: BTreeSet<String> = git::files_between_z(dir, base, turn_commit)
        .unwrap_or_default()
        .into_iter()
        .collect();
    if session.is_empty() {
        return false;
    }
    let made: BTreeSet<String> = git::files_between_z(dir, &format!("{c}^"), c)
        .unwrap_or_default()
        .into_iter()
        .collect();
    if made != session {
        return false;
    }
    session.iter().all(|p| git::blob_at(dir, c, p) == git::blob_at(dir, turn_commit, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn repo(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fallow-refwatch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        run(&d, &["init", "-q", "-b", "main"]);
        run(&d, &["config", "user.email", "t@example.invalid"]);
        run(&d, &["config", "user.name", "t"]);
        std::fs::write(d.join("a.txt"), "a\n").unwrap();
        run(&d, &["add", "."]);
        run(&d, &["commit", "-q", "-m", "a"]);
        d
    }

    #[test]
    fn changes_are_told_apart() {
        let d = repo("diff");
        let s0 = take(&d).unwrap();
        assert_eq!(s0.head.as_deref(), Some("refs/heads/main"));
        run(&d, &["branch", "x"]);
        std::fs::write(d.join("a.txt"), "b\n").unwrap();
        run(&d, &["commit", "-q", "-am", "b"]);
        let s1 = take(&d).unwrap();
        let ch = diff(&d, &s0, &s1);
        let get = |r: &str| ch.iter().find(|c| c.0 == r).cloned().unwrap();
        assert_eq!(get("refs/heads/x").3, How::Created);
        assert_eq!((get("refs/heads/main").3, get("refs/heads/main").4), (How::Commits, Some(1)));
        run(&d, &["reset", "-q", "--hard", "HEAD~1"]);
        run(&d, &["branch", "-D", "x"]);
        let s2 = take(&d).unwrap();
        let ch = diff(&d, &s1, &s2);
        assert_eq!(ch.iter().find(|c| c.0 == "refs/heads/main").unwrap().3, How::Rewound);
        assert_eq!(ch.iter().find(|c| c.0 == "refs/heads/x").unwrap().3, How::Deleted);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_command_targets_the_refs_it_names() {
        let d = repo("targets");
        let f = d.display().to_string();
        let t = |c: &str| targets(c, &d, Some("refs/heads/main"), "fallow/solo/x", &std::env::temp_dir());
        assert_eq!(t("git push origin fallow/solo/x"), ["refs/remotes/origin/fallow/solo/x"]);
        assert_eq!(t("git push"), ["refs/remotes/origin/fallow/solo/x"]);
        assert_eq!(t("git push origin HEAD:main"), ["refs/remotes/origin/main"]);
        assert_eq!(t(&format!("git -C {f} merge --squash fallow/solo/x && git -C {f} commit -m s")), ["refs/heads/main", "refs/heads/main"]);
        assert!(t("git merge main").is_empty(), "the session's own branch is not watched as a change");
        assert_eq!(t("git branch -f main HEAD"), ["refs/heads/main"]);
        assert_eq!(t("git branch x main"), ["refs/heads/x"]);
        assert_eq!(t("git branch -m old new"), ["refs/heads/old", "refs/heads/new"]);
        assert_eq!(t("git branch -D a b c"), ["refs/heads/a", "refs/heads/b", "refs/heads/c"]);
        assert_eq!(t("git branch -dr origin/x"), ["refs/remotes/origin/x"]);
        assert!(t("npm test").is_empty());
        assert!(t("git branch --contains main").is_empty(), "a listing moves nothing");
        assert!(t("git branch -v").is_empty());
        assert_eq!(t("git push -o ci.skip origin main"), ["refs/remotes/origin/main"]);
        assert_eq!(t("git push --all origin"), ["refs/remotes/origin/*"]);
        assert!(targeted(&t("git push --all origin"), "refs/remotes/origin/x"));
        // `cd <folder>` first, or `git.exe`, or an `env` prefix: still the folder.
        assert_eq!(t(&format!("cd {f} && git reset --hard HEAD~1")), ["refs/heads/main"]);
        assert_eq!(t(&format!("env GIT_X=1 git.exe -C {f} merge x")), ["refs/heads/main"]);
        assert!(in_folder(&format!("cd {f} && git status"), &d, &std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_squash_is_recognized_by_its_tree() {
        let d = repo("squash");
        let base = run(&d, &["rev-parse", "HEAD"]);
        run(&d, &["checkout", "-q", "-b", "s"]);
        std::fs::write(d.join("b.txt"), "b\n").unwrap();
        run(&d, &["add", "."]);
        run(&d, &["commit", "-q", "-m", "t1"]);
        std::fs::write(d.join("a.txt"), "changed\n").unwrap();
        run(&d, &["commit", "-q", "-am", "t2"]);
        let t2 = run(&d, &["rev-parse", "HEAD"]);
        run(&d, &["checkout", "-q", "main"]);
        run(&d, &["merge", "-q", "--squash", "s"]);
        run(&d, &["commit", "-q", "-m", "squash"]);
        let c = run(&d, &["rev-parse", "HEAD"]);
        assert!(is_squash_of(&d, &c, &base, &t2));
        // Up to turn 1 only: not this squash.
        let t1 = run(&d, &["rev-parse", "s~1"]);
        assert!(!is_squash_of(&d, &c, &base, &t1));
        let _ = std::fs::remove_dir_all(&d);
    }
}
