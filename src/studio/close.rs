//! 012 US6 (FR-021 to FR-025, SC-007, SC-010; research R-11, R-14): closing a session. Closing
//! first says what it will do, changing nothing: stop the turn first; or what is not merged,
//! uncommitted, ignored or undone. A merged and clean session closes at once. The engineer's
//! choice (keep or discard) is written before anything is removed, so a restart finishes it.
//! Every git command runs without hooks.

use std::path::Path;

use serde_json::{Value, json};

use super::Studio;
use crate::conversation::events::Event as CEvent;
use crate::conversation::solo::{CloseChoice, Closed, Solo, SoloEvent};
use crate::verify::git;

/// What closing a session would lose, read without changing anything (R-14).
#[derive(Debug, Default)]
struct Facts {
    merged: bool,
    commits: Vec<(String, String)>,
    files: Vec<String>,
    edits: Vec<String>,
    ignored: Vec<String>,
    counts: std::collections::BTreeMap<String, u64>,
    undone: Vec<(u32, String, String)>,
    /// The copy is there but git cannot read it (its link gone): its files cannot be named.
    unreadable: Option<String>,
    /// The session's branch is gone: what it held cannot be judged merged.
    branch_gone: bool,
    /// The copy's HEAD is not its branch's tip (detached, or moved): its commits are named.
    head_off: Vec<(String, String)>,
}

impl Facts {
    /// Nothing to lose but undone turns.
    fn clean(&self) -> bool {
        self.merged && self.files.is_empty() && self.ignored.is_empty() && self.unreadable.is_none() && self.head_off.is_empty()
    }

    /// What the engineer is shown, fingerprinted: a choice is done only while it still holds.
    fn digest(&self) -> String {
        crate::studio::extras::sha256_hex(self.shown().to_string().as_bytes())
    }

    fn ask(&self) -> Value {
        let mut v = json!({ "ask": self.shown() });
        v["ask"]["digest"] = json!(self.digest());
        v
    }

    fn shown(&self) -> Value {
        json!({
            "commits": self.commits.iter().map(|(c, s)| json!({ "commit": c, "subject": s })).collect::<Vec<_>>(),
            "files": self.files,
            "edits": self.edits,
            "ignored": { "files": self.ignored, "counts": self.counts },
            "undone": self.undone.iter().map(|(t, c, _)| json!({ "turn": t, "commit": c })).collect::<Vec<_>>(),
            "merged": self.merged,
            "unreadable": self.unreadable,
            "branch_gone": self.branch_gone,
            "head_off": self.head_off.iter().map(|(c, s)| json!({ "commit": c, "subject": s })).collect::<Vec<_>>(),
        })
    }
}

/// The dependency or build folder a path lies in, counted rather than named (R-14, R-18).
fn heavy_of(path: &str) -> Option<String> {
    let folders = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    folders.split('/').find(|c| super::repo::HEAVY.contains(c)).map(str::to_string)
}

/// R-11: every commit not undone is in another local or remote-tracking ref, or the last squash
/// Fallow saw covers the last turn with changes, with nothing committed after it.
fn merged(repo: &Path, s: &Solo, head: &str) -> bool {
    if head == s.start_commit || git::contained_elsewhere(repo, head, "refs/heads/fallow/") {
        return true;
    }
    let last = s
        .commits
        .iter()
        .rev()
        .find(|(n, t)| t.commit.is_some() && !s.undone.contains(n))
        .map(|(n, t)| (*n, t.commit.clone().unwrap_or_default()));
    match (s.squashes.last(), last) {
        (Some((_, _, upto)), Some((n, commit))) => *upto >= n && commit == head,
        _ => false,
    }
}

impl Studio {
    fn close_facts(&self, conv: &str) -> Result<Facts, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.as_ref().ok_or("not a session")?;
        let p = self.project(&c.state.project)?;
        let repo = p.path.clone();
        let linked = s.worktree.join(".git").is_file();
        let head = git::refs_under(&repo, &format!("refs/heads/{}", s.branch))
            .into_iter()
            .find(|(r, _)| *r == format!("refs/heads/{}", s.branch))
            .map(|(_, h)| h);
        let mut f = Facts::default();
        match &head {
            Some(h) => f.merged = merged(&repo, s, h),
            None => f.branch_gone = true,
        }
        if !f.merged {
            f.commits = match &head {
                Some(h) => git::log_subjects(&repo, &format!("{}..{h}", s.start_commit)),
                None if linked => git::log_subjects(&s.worktree, &format!("{}..HEAD", s.start_commit)),
                None => Vec::new(),
            };
            let mine: Vec<&String> = f.commits.iter().map(|(c, _)| c).collect();
            f.edits = s
                .engineer_edits
                .iter()
                .filter(|(_, commit, _)| mine.contains(&commit))
                .flat_map(|(_, _, files)| files.clone())
                .collect();
        }
        // Without its link git would read a repository around the folder: the copy is named as
        // unreadable instead, never closed as clean (Phase 10 review).
        if !linked && s.worktree.exists() {
            f.unreadable = Some(s.worktree.display().to_string());
        }
        // Commits the copy holds that its branch does not (a detached HEAD): named, never lost
        // with the copy (Phase 10 review).
        if let (true, Some(tip)) = (linked, &head) {
            if git::head(&s.worktree).is_ok_and(|h| h != *tip) {
                f.head_off = git::log_subjects(&s.worktree, &format!("{tip}..HEAD"));
                if f.head_off.is_empty() {
                    f.head_off.push((git::head(&s.worktree).unwrap_or_default(), "the copy is not on its branch".into()));
                }
            }
        }
        if linked {
            let (changed, ignored) = git::status_with_ignored(&s.worktree)?;
            f.files = changed;
            for i in ignored {
                match heavy_of(&i) {
                    Some(h) => *f.counts.entry(h).or_default() += 1,
                    None => f.ignored.push(i),
                }
            }
        }
        let prefix = format!("refs/fallow/solo/{conv}/undone/");
        f.undone = git::refs_under(&repo, &prefix)
            .into_iter()
            .filter_map(|(r, c)| Some((r.strip_prefix(&prefix)?.parse().ok()?, c, r)))
            .collect();
        f.undone.sort();
        Ok(f)
    }

    /// 012 (R-14): `POST /api/sessions/<s>/close`. Without a choice: closes a merged and clean
    /// session (its undone turns, if any, confirmed), or says what closing would lose. With
    /// `keep` or `discard`: the engineer's choice, done.
    ///
    /// A choice (and a confirmation of undone turns) carries the `digest` of the question it
    /// answers; when the session changed since, the question comes back instead (II).
    pub fn close_session(&mut self, conv: &str, choice: Option<&str>, confirmed: bool, digest: Option<&str>) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.as_ref().ok_or("not a session")?;
        if s.closed.is_some() {
            return Err("the session is closed already".into());
        }
        if c.state.busy {
            let n = c.state.turns.last().map(|t| t.n).unwrap_or(1);
            return Ok(json!({ "stop_first": n }));
        }
        let choice = match choice {
            None => None,
            Some("keep") => Some(CloseChoice::Keep),
            Some("discard") => Some(CloseChoice::Discard),
            Some(o) => return Err(format!("closing is keep or discard, not {o}")),
        };
        let f = self.close_facts(conv)?;
        let now = f.digest();
        let choice = match choice {
            Some(ch) => {
                if digest != Some(now.as_str()) {
                    return Ok(f.ask());
                }
                ch
            }
            None => {
                if !f.clean() || (!f.undone.is_empty() && (!confirmed || digest != Some(now.as_str()))) {
                    return Ok(f.ask());
                }
                CloseChoice::Merged
            }
        };
        self.conv_mut(conv)?.append(CEvent::Solo(SoloEvent::CloseStarted { choice, digest: Some(now) }))?;
        self.finish_close(conv, choice, false)
    }

    /// The close after its intent: also what a restart runs, skipping the steps already done.
    /// `restart`: finished after a restart, when the copy may have changed since the engineer
    /// was shown what closing would do.
    fn finish_close(&mut self, conv: &str, choice: CloseChoice, restart: bool) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.clone().ok_or("not a session")?;
        let p = self.project(&c.state.project)?.clone();
        let repo = p.path.clone();
        let copy = s.worktree.join(".git").is_file();
        // A merged close is checked again as it is done: what appeared since (files, commits)
        // makes it a keep, the copy kept, never removed unnamed (Phase 10 review).
        let mut choice = choice;
        let mut keep_copy = false;
        if choice == CloseChoice::Merged && !self.close_facts(conv).is_ok_and(|f| f.clean()) {
            choice = CloseChoice::Keep;
            keep_copy = true;
        }
        // After a restart: when the session no longer holds what the engineer was shown, the copy
        // and the branch stay (a discard becomes a keep), never removed unnamed.
        if restart && choice != CloseChoice::Merged {
            let shown = s.closing_digest.clone();
            if shown.is_none() || self.close_facts(conv).map(|f| f.digest()).ok() != shown {
                choice = CloseChoice::Keep;
                keep_copy = true;
            }
        }
        let mut how = match choice {
            CloseChoice::Merged => Closed::Merged,
            CloseChoice::Keep => Closed::Kept,
            CloseChoice::Discard => Closed::Discarded,
        };
        if choice == CloseChoice::Keep && s.worktree.exists() {
            // What is uncommitted goes on the branch first; when it cannot, or the copy's state
            // cannot be read, the copy stays (II).
            let committed = if !copy {
                Err("git cannot read the working copy".to_string())
            } else {
                git::status_with_ignored(&s.worktree).map(|(c, _)| c).and_then(|left| {
                    if left.is_empty() {
                        return Ok(());
                    }
                    self.settle(conv)?;
                    let still = git::status_with_ignored(&s.worktree)?.0;
                    if still.is_empty() { Ok(()) } else { Err(format!("still uncommitted: {}", still.join(", "))) }
                })
            };
            // The copy's own commits must be on the branch kept: a detached or diverged HEAD
            // keeps the copy (review).
            let on_branch = || {
                let tip = git::refs_under(&repo, &format!("refs/heads/{}", s.branch))
                    .into_iter()
                    .find(|(r, _)| *r == format!("refs/heads/{}", s.branch))
                    .map(|(_, h)| h);
                match (tip, git::head(&s.worktree).ok()) {
                    (Some(t), Some(h)) if t == h => Ok(()),
                    _ => Err("the working copy's HEAD is not the branch kept".to_string()),
                }
            };
            if let Err(e) = committed.and_then(|()| on_branch()) {
                self.console.push(format!("session {conv}: keeping its working copy: {e}"));
                how = Closed::KeptWithCopy;
            }
        }
        if keep_copy && s.worktree.exists() {
            how = Closed::KeptWithCopy;
        }
        if how != Closed::KeptWithCopy && (copy || s.worktree.exists()) {
            git::worktree_remove_nohooks(&repo, &s.worktree);
        }
        if matches!(how, Closed::Merged | Closed::Discarded) {
            let _ = git::branch_delete_nohooks(&repo, &s.branch);
        }
        // In every case: the undone refs (named before), and the gate copies.
        for (r, _) in git::refs_under(&repo, &format!("refs/fallow/solo/{conv}/undone/")) {
            let _ = git::delete_ref_nohooks(&repo, &r);
        }
        self.forget_gate_copies(conv);
        let branch = matches!(how, Closed::Kept | Closed::KeptWithCopy).then(|| s.branch.clone());
        let worktree = (how == Closed::KeptWithCopy).then(|| s.worktree.clone());
        let c = self.conv_mut(conv)?;
        c.append(CEvent::Solo(SoloEvent::SessionClosed { how, branch: branch.clone(), worktree: worktree.clone() }))?;
        c.tick();
        if let Ok(pm) = self.project_mut(&p.id) {
            if pm.last_session.as_deref() == Some(conv) {
                pm.last_session = None;
            }
        }
        let _ = self.store.save();
        let name = serde_json::to_value(how).unwrap_or(Value::Null);
        Ok(json!({ "closed": name, "branch": branch, "worktree": worktree }))
    }

    /// 012 (R-14, SC-007): a close Fallow stopped in the middle of is finished.
    pub(super) fn finish_close_on_load(&mut self, id: &str) {
        let Some(choice) = self
            .convs
            .get(id)
            .and_then(|c| c.state.solo.as_ref())
            .filter(|s| s.closed.is_none())
            .and_then(|s| s.closing)
        else {
            return;
        };
        if let Err(e) = self.finish_close(id, choice, true) {
            self.console.push(format!("session {id}: the close could not be finished: {e}"));
        }
    }

    /// 012 (R-14): `POST /api/sessions/<s>/remove-copy`: a session kept with its copy loses it,
    /// confirmed, naming the files lost.
    pub fn remove_copy(&mut self, conv: &str, confirmed: bool) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.clone().ok_or("not a session")?;
        if !matches!(s.closed, Some((Closed::KeptWithCopy, _, _))) || s.copy_removed {
            return Err("only a session closed with its working copy kept has one to remove".into());
        }
        let lost = if s.worktree.join(".git").is_file() {
            let (c, i) = git::status_with_ignored(&s.worktree)
                .map_err(|e| format!("Fallow cannot read the working copy, so it cannot name what would be lost: {e}"))?;
            [c, i].concat()
        } else if s.worktree.exists() {
            vec!["(git cannot read this copy: its files are not listed)".to_string()]
        } else {
            Vec::new()
        };
        if !confirmed {
            return Err(format!(
                "this needs the engineer's confirmation: it removes the working copy{}",
                if lost.is_empty() { String::new() } else { format!(" and loses {}", lost.join(", ")) }
            ));
        }
        let repo = self.project(&c.state.project)?.path.clone();
        git::worktree_remove_nohooks(&repo, &s.worktree);
        let c = self.conv_mut(conv)?;
        c.append(CEvent::Solo(SoloEvent::CopyRemoved { files: lost.clone() }))?;
        c.tick();
        Ok(json!({ "files_lost": lost }))
    }

    /// 012 (FR-025): `DELETE /api/sessions/<s>`: a closed session's record, confirmed.
    pub fn delete_session(&mut self, conv: &str, confirmed: bool) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let s = c.state.solo.as_ref().ok_or("not a session")?;
        if s.closed.is_none() {
            return Err("close the session first".into());
        }
        if matches!(s.closed, Some((Closed::KeptWithCopy, _, _))) && !s.copy_removed {
            return Err("remove its working copy first: the record is what names it".into());
        }
        if !confirmed {
            return Err("this needs the engineer's confirmation: the session's record and its images are deleted".into());
        }
        let project = c.state.project.clone();
        let dir = c.dir.clone();
        self.convs.remove(conv);
        let _ = std::fs::remove_dir_all(&dir);
        if let Ok(pm) = self.project_mut(&project) {
            pm.sessions.retain(|x| x != conv);
            if pm.last_session.as_deref() == Some(conv) {
                pm.last_session = None;
            }
        }
        self.store.save()?;
        Ok(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_folders_are_counted_not_named() {
        assert_eq!(heavy_of("node_modules/x/index.js").as_deref(), Some("node_modules"));
        assert_eq!(heavy_of("web/node_modules/a.js").as_deref(), Some("node_modules"));
        assert_eq!(heavy_of(".env"), None);
        assert_eq!(heavy_of("config/.env.local"), None);
        assert_eq!(heavy_of("build"), None, "a file named build is named");
        assert_eq!(heavy_of("dist/app.js").as_deref(), Some("dist"));
    }
}
