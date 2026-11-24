//! 012 US8 (FR-039 to FR-041, research R-18): every project is a git repository. A folder that
//! is not one yet is shown what its first commit would hold and made one on the engineer's
//! click; a project added before at a folder inside a bigger repository moves to that
//! repository's top folder, its features keeping their spec documents where they are.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::Studio;
use crate::conversation::events::Event as CEvent;
use crate::project::{self, Kind};
use crate::verify::git;

/// Folders of dependencies and build output, named in the confirmation and proposed for the
/// ignore file (R-18).
pub(super) const HEAVY: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".gradle",
];

/// What a folder's first commit would hold.
#[derive(Debug, Default)]
struct Scan {
    files: u64,
    bytes: u64,
    heavy: Vec<String>,
    secrets: Vec<String>,
}

/// Walks `root` (not `.git`), counting what would be committed with the proposed ignore file:
/// dependency and build folders and likely secrets are named, not counted.
fn scan(root: &Path) -> Scan {
    let mut s = Scan::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries: Vec<_> = std::fs::read_dir(&d)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        let csproj = entries.iter().any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".csproj")
        });
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let rel = p
                .strip_prefix(root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            let Ok(t) = e.file_type() else {
                continue;
            };
            if t.is_dir() {
                if name == ".git" {
                    continue;
                }
                if HEAVY.contains(&name.as_str())
                    || (csproj && matches!(name.as_str(), "bin" | "obj"))
                {
                    s.heavy.push(format!("{rel}/"));
                    continue;
                }
                stack.push(p);
                continue;
            }
            if super::files::is_secret(&rel) {
                s.secrets.push(rel);
                continue;
            }
            s.files += 1;
            s.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    s.heavy.sort();
    s.secrets.sort();
    s
}

/// The ignore file proposed: what the folder's own `.gitignore` holds, then the dependency and
/// build folders by name and the likely secrets by path.
fn proposed_gitignore(root: &Path, s: &Scan) -> String {
    let had = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    let mut lines: Vec<String> = had.lines().map(str::to_string).collect();
    let add = |l: String, lines: &mut Vec<String>| {
        if !lines.iter().any(|x| x.trim() == l) {
            lines.push(l);
        }
    };
    let mut new = Vec::new();
    for h in &s.heavy {
        let name = h.trim_end_matches('/').rsplit('/').next().unwrap_or(h);
        add(format!("{name}/"), &mut new);
    }
    for f in &s.secrets {
        add(format!("/{f}"), &mut new);
    }
    let new: Vec<String> = new
        .into_iter()
        .filter(|l| !lines.iter().any(|x| x.trim() == l))
        .collect();
    if !new.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("# Proposed by Fallow: dependencies, build output and likely secrets".into());
        lines.extend(new);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// The engineer's git identity, as git reads it for `dir` (R-3): never set by Fallow.
fn identity(dir: &Path) -> Result<(), String> {
    let get = |k: &str| {
        crate::platform::command("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "--get", k])
            .stdin(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if get("user.name").is_some() && get("user.email").is_some() {
        Ok(())
    } else {
        Err("Set your name and e-mail in git first: git config --global user.name \"Your Name\" and git config --global user.email you@example.com".into())
    }
}

impl Studio {
    /// 012 (FR-041, Phase 5 review): a repository whose features or project conversation still
    /// work without a branch and worktree of their own (an init that stopped after its commit,
    /// or a folder made a repository outside Fallow): set up by the same click before it is used.
    pub fn setup_left(&self, p: &project::Project) -> bool {
        p.features.iter().any(|f| f.branch.is_none())
            || p.conversation
                .as_ref()
                .and_then(|c| self.convs.get(c))
                .is_some_and(|c| !super::own_copy(&c.state.workdir))
    }

    /// 012 (FR-039): what "Make it a git repository" would do (read only).
    pub fn git_scan(&self, id: &str) -> Result<Value, String> {
        let p = self.project(id)?;
        match p.kind() {
            Kind::Git if self.setup_left(p) => {
                let left: Vec<&str> = p
                    .features
                    .iter()
                    .filter(|f| f.branch.is_none())
                    .map(|f| f.name())
                    .collect();
                return Ok(json!({ "needs": "branches", "features": left, "identity": true }));
            }
            Kind::Git => return Err(format!("{} is already a git repository", p.name)),
            Kind::NeedsMove => {
                let top = project::top_of(&p.path).unwrap_or_default();
                return Ok(json!({ "inside": top }));
            }
            Kind::Missing => return Err(format!("{} is not there", p.path.display())),
            Kind::Unknown => return Err(project::GIT_NEEDED.to_string()),
            Kind::NeedsGit => {}
        }
        let s = scan(&p.path);
        Ok(json!({
            "needs": if p.path.join(".git").exists() { "first_commit" } else { "init" },
            "files": s.files,
            "bytes": s.bytes,
            "heavy": s.heavy,
            "secrets": s.secrets,
            "gitignore": proposed_gitignore(&p.path, &s),
            "identity": identity(&p.path).is_ok(),
        }))
    }

    /// 012 (FR-039): `git init` where needed, the ignore file the engineer accepted, and the
    /// first commit of what is there, with the engineer's git as it is set up; then Spec Kit's
    /// files, and the features' branches and worktrees.
    pub fn git_init(&mut self, id: &str, gitignore: Option<String>) -> Result<Value, String> {
        let p = self.project(id)?.clone();
        // Already a repository: only what is left, the features' branches and worktrees.
        let finishing = p.kind() == Kind::Git && self.setup_left(&p);
        match p.kind() {
            Kind::Git if finishing => {}
            Kind::Git => return Err(format!("{} is already a git repository", p.name)),
            Kind::NeedsMove => {
                let top = project::top_of(&p.path).unwrap_or_default();
                return Err(format!(
                    "{} is inside the repository {}: use that folder instead",
                    p.path.display(),
                    top.display()
                ));
            }
            Kind::Missing => return Err(format!("{} is not there", p.path.display())),
            Kind::Unknown => return Err(project::GIT_NEEDED.to_string()),
            Kind::NeedsGit => {}
        }
        // A folder inside a repository git would not read (another owner, `safe.directory`)
        // must not become a repository of its own (Phase 5 review, 5).
        if !p.path.join(".git").exists() {
            if let Some(outer) = p.path.ancestors().skip(1).find(|a| a.join(".git").exists()) {
                return Err(format!(
                    "{} is inside the repository {}, which git does not read here: add that folder as the project once git trusts it",
                    p.path.display(),
                    outer.display()
                ));
            }
        }
        // Nothing changes when git cannot commit as the engineer (SC-012).
        if !finishing {
            identity(&p.path)?;
        }
        let path = p.path.clone();
        let run = |args: &[&str]| -> Result<String, String> {
            let out = crate::platform::command("git")
                .current_dir(&path)
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("git: {e}"))?;
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        };
        if !finishing {
            if !path.join(".git").exists() {
                run(&["init", "-q"])?;
            }
            if let Some(text) = gitignore.filter(|t| !t.trim().is_empty()) {
                std::fs::write(path.join(".gitignore"), text).map_err(|e| e.to_string())?;
            }
            run(&["add", "-A"])?;
            run(&["commit", "-q", "-m", "Initial commit (Fallow)"])?;
        }
        let commit = git::head(&path)?;
        // Spec Kit's files, written only now that the folder is a repository (FR-039); never
        // when finishing, which changes nothing in the folder (third review, 3).
        let report = if finishing {
            crate::project::install::Report::default()
        } else {
            crate::project::install::install(&p.root_dir())?
        };
        // The features and conversations move onto branches and worktrees, as in any git
        // project (research S-6), so nothing more is written in the engineer's own checkout.
        let base = git::branch(&path).ok_or_else(|| {
            format!(
                "{} is on a detached HEAD: check out the branch to build on first",
                path.display()
            )
        })?;
        std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
        // A branch the engineer has checked out, or a feature folder git ignores, would stop it
        // halfway: refused before any branch is made (fourth review, 1 and 3). The first commit
        // is made by then; the engineer fixes what is named and clicks again to finish.
        for f in p.features.iter().filter(|f| f.branch.is_none()) {
            let name = f.name();
            if base == name {
                return Err(format!(
                    "{name} is checked out in {}: check out another branch there, then click again",
                    path.display()
                ));
            }
            if path.join(&f.dir).is_dir() {
                match git::ignore_state(&path, &f.dir) {
                    Some(false) => {}
                    Some(true) => {
                        return Err(format!(
                            "{} is ignored in the repository: change the ignore file, commit, then click again",
                            f.dir
                        ));
                    }
                    None => return Err(format!("git could not read {} in the repository", f.dir)),
                }
            }
        }
        let done = self.branches_for(&p, &path, &commit, &base);
        // What was done is kept, even when a later feature stopped it (fourth review, 4).
        self.store.save()?;
        done?;
        let wd = self.worktrees(&p.id).join("_project");
        if let Some(c) = p.conversation.as_ref().and_then(|c| self.convs.get_mut(c)) {
            if !own_or_none(&c.state.workdir) {
                if !super::own_copy(&wd) {
                    git::worktree_prune(&path);
                    git::worktree_add(&path, &wd, None, &commit)?;
                }
                c.append(CEvent::WorkdirSet { workdir: wd })?;
            }
        }
        self.store.save()?;
        Ok(json!({ "kind": "git", "branch": base, "commit": commit, "spec_kit": report }))
    }

    /// The features' branches and worktrees after a project's first commit, each with its
    /// documents.
    fn branches_for(
        &mut self,
        p: &project::Project,
        path: &Path,
        commit: &str,
        base: &str,
    ) -> Result<(), String> {
        let id = p.id.clone();
        let (path, commit, base) = (path.to_path_buf(), commit.to_string(), base.to_string());
        for f in p.features.iter().filter(|f| f.branch.is_none()) {
            let name = f.name().to_string();
            // What an earlier try made is taken up, not made again (Phase 5 review, 3).
            if !git::rev_exists(&path, &format!("refs/heads/{name}")) {
                git::create_branch(&path, &name, &commit)?;
            }
            let wd = self.worktrees(&p.id).join(&name);
            if !super::own_copy(&wd) {
                // A worktree whose folder went is forgotten first (third review, 2).
                git::worktree_prune(&path);
                git::worktree_add(&path, &wd, Some(&name), &commit)?;
            }
            // Its documents as they are in the folder, when the commit does not hold them so
            // (third review, 1): committed on its branch, never in the engineer's checkout.
            let from = path.join(&f.dir);
            if from.is_dir() && !git::committed_as_is(&path, &f.dir) {
                copy_dir(&from, &wd.join(&f.dir))?;
                git::commit_all(&wd, &format!("spec: {name}"), Some(&[f.dir.as_str()]))?;
            }
            if let Some(c) = f.conversation.as_ref().and_then(|c| self.convs.get_mut(c)) {
                c.append(CEvent::WorkdirSet { workdir: wd })?;
            }
            if let Some(fm) = self.project_mut(&id)?.feature_mut(&name) {
                fm.base_branch = Some(base.clone());
                fm.branch = Some(name.clone());
            }
        }
        Ok(())
    }

    /// 012 (FR-039, R-18): a project added before at a folder inside a bigger repository moves
    /// to that repository's top folder. Its features' spec folders are recorded against it
    /// (`<sub>/<dir>`), each gets a branch and a worktree from the top's `HEAD` (with a `spec:`
    /// commit of its documents when they are not committed there), and `.specify/` and
    /// `.fallow/` stay at the subfolder, the project's spec root. `confirmed: false` only says
    /// what would move.
    pub fn move_project(&mut self, id: &str, confirmed: bool) -> Result<Value, String> {
        let p = self.project(id)?.clone();
        if p.kind() != Kind::NeedsMove {
            return Err(format!("{} is not inside a bigger repository", p.name));
        }
        let top =
            project::top_of(&p.path).ok_or("the repository's top folder could not be read")?;
        let sub = sub_path(&top, &p.path)?;
        let moves: Vec<Value> = p
            .features
            .iter()
            .map(
                |f| json!({ "feature": f.name(), "from": f.dir, "to": format!("{sub}/{}", f.dir) }),
            )
            .collect();
        match git::ignore_state(&top, &sub) {
            Some(false) => {}
            Some(true) => {
                return Err(format!(
                    "{sub} is ignored in the repository {}: its documents could not be committed there",
                    top.display()
                ));
            }
            None => {
                return Err(format!(
                    "git could not read the repository {}",
                    top.display()
                ));
            }
        }
        // Gates and instructions are read from the top's commit: what is not committed there yet
        // is named, for the engineer to commit (Phase 5 review, 6).
        let uncommitted: Vec<String> = [".fallow/squad.toml", ".specify/memory/constitution.md"]
            .iter()
            .map(|f| format!("{sub}/{f}"))
            .filter(|f| top.join(f).is_file())
            .filter(|f| !git::committed_as_is(&top, f))
            .collect();
        if !confirmed {
            return Ok(json!({
                "top": top,
                "sub": sub,
                "features": moves,
                "stay": [format!("{sub}/.specify"), format!("{sub}/.fallow")],
                "uncommitted": uncommitted,
            }));
        }
        let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
            (Ok(x), Ok(y)) => x == y,
            _ => a == b,
        };
        if let Some(other) = self
            .store
            .projects
            .iter()
            .find(|q| q.id != p.id && same(&q.path, &top))
        {
            return Err(format!(
                "{} is already the project {}",
                top.display(),
                other.name
            ));
        }
        let head = git::head(&top)
            .map_err(|_| format!("the repository {} has no commit yet", top.display()))?;
        let base = git::branch(&top).ok_or_else(|| {
            format!(
                "{} is on a detached HEAD: check out the branch to build on first",
                top.display()
            )
        })?;
        for f in &p.features {
            let name = f.name().to_string();
            if git::rev_exists(&top, &format!("refs/heads/{name}")) {
                return Err(format!(
                    "the branch {name} exists already in {}",
                    top.display()
                ));
            }
            // Never undo a working copy this move did not make (Phase 5 review, 7).
            if self.worktrees(&p.id).join(&name).exists() {
                return Err(format!(
                    "a working copy of {name} is there already: remove it first"
                ));
            }
        }
        std::fs::create_dir_all(self.worktrees(&p.id)).map_err(|e| e.to_string())?;
        // Every branch and worktree first; on any failure they all go again, and nothing of the
        // project has changed (Phase 5 review, 4).
        let mut made: Vec<(String, PathBuf)> = Vec::new();
        let undo = |made: &[(String, PathBuf)]| {
            for (b, wd) in made {
                let _ = git::worktree_remove(&top, wd);
                git::worktree_prune(&top);
                if !b.is_empty() {
                    let _ = git::delete_branch(&top, b);
                }
            }
        };
        let mut plan: Vec<(String, String, PathBuf)> = Vec::new();
        for f in &p.features {
            let name = f.name().to_string();
            let dir = format!("{sub}/{}", f.dir);
            let wd = self.worktrees(&p.id).join(&name);
            let step = (|| -> Result<(), String> {
                git::create_branch(&top, &name, &head)?;
                made.push((name.clone(), wd.clone()));
                git::worktree_add(&top, &wd, Some(&name), &head)?;
                // Its documents as they are in the engineer's folder, committed on its branch
                // when the top's commit does not hold them so.
                let from = top.join(&dir);
                if from.is_dir() {
                    copy_dir(&from, &wd.join(&dir))?;
                    git::commit_all(&wd, &format!("spec: {name}"), Some(&[dir.as_str()]))?;
                }
                Ok(())
            })();
            if let Err(e) = step {
                undo(&made);
                return Err(format!(
                    "the project could not be moved, nothing changed: {e}"
                ));
            }
            plan.push((name, dir, wd));
        }
        // The project conversation, if there was one, works in a worktree as everywhere: never
        // in the engineer's checkout (Phase 5 review, 1).
        let project_wd = self.worktrees(&p.id).join("_project");
        if p.conversation
            .as_ref()
            .is_some_and(|c| self.convs.contains_key(c))
        {
            if let Err(e) = git::worktree_add(&top, &project_wd, None, &head) {
                made.push((String::new(), project_wd.clone()));
                undo(&made);
                return Err(format!(
                    "the project could not be moved, nothing changed: {e}"
                ));
            }
        }
        for (name, dir, wd) in plan {
            let conv = p.feature(&name).and_then(|f| f.conversation.clone());
            if let Some(c) = conv.as_ref().and_then(|c| self.convs.get_mut(c)) {
                c.append(CEvent::WorkdirSet { workdir: wd })?;
            }
            if let Some(fm) = self.project_mut(id)?.feature_mut(&name) {
                fm.dir = dir;
                fm.base_branch = Some(base.clone());
                fm.branch = Some(name.clone());
            }
        }
        if let Some(c) = p.conversation.as_ref().and_then(|c| self.convs.get_mut(c)) {
            c.append(CEvent::WorkdirSet {
                workdir: project_wd,
            })?;
        }
        let pm = self.project_mut(id)?;
        pm.path = top.clone();
        pm.spec_root = sub.clone();
        self.store.save()?;
        Ok(json!({ "path": top, "spec_root": sub }))
    }
}

/// Whether a conversation already works in a worktree of Fallow's own.
fn own_or_none(wd: &Path) -> bool {
    super::own_copy(wd)
}

/// `inner` relative to `top`, with forward slashes.
fn sub_path(top: &Path, inner: &Path) -> Result<String, String> {
    let (t, i) = (
        top.canonicalize().map_err(|e| e.to_string())?,
        inner.canonicalize().map_err(|e| e.to_string())?,
    );
    let rel = i
        .strip_prefix(&t)
        .map_err(|_| format!("{} is not inside {}", inner.display(), top.display()))?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Copies the files of `from` into `to`, folders included; what is there already is replaced.
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((a, b)) = stack.pop() {
        std::fs::create_dir_all(&b).map_err(|e| format!("{}: {e}", b.display()))?;
        for e in std::fs::read_dir(&a).map_err(|e| e.to_string())?.flatten() {
            let (pa, pb) = (e.path(), b.join(e.file_name()));
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((pa, pb));
            } else if e.file_type().is_ok_and(|t| t.is_file()) {
                std::fs::copy(&pa, &pb).map_err(|e| format!("{}: {e}", pa.display()))?;
            }
        }
    }
    Ok(())
}
