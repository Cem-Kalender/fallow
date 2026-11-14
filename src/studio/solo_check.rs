//! 012 US3 (FR-013, FR-014, FR-019 to FR-022; research R-4, R-5, R-12): seeing and checking what
//! a session changed. The working copy is settled before a turn, an undo and a gate run; a turn
//! can be undone back to before it, its commits kept under a ref; the session's changes are
//! listed against its base; the project's gates run on demand on the session's head.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use super::{Conv, Note, Studio};
use crate::conversation::events::Event as CEvent;
use crate::conversation::solo::{BaseWhy, GateOutcome, SoloEvent};
use crate::run::engine::{Ctx, Msg};
use crate::verify::gates::{self, GateResult};
use crate::verify::git;

/// Said on the Changes panel and in every undo confirmation (analyze C1).
pub const ONLY_THE_COPY: &str = "Undo and Changes cover only this session's working copy: what a command wrote elsewhere is not seen or undone";

/// What an undo of turn `k` would do (R-5).
struct UndoPlan {
    from: u32,
    to: u32,
    head_after: String,
    turns: Vec<u32>,
    shared: Vec<(u32, u32)>,
    edits: Vec<(String, Vec<String>)>,
    /// What is not committed yet in the working copy: committed as the engineer's (or the
    /// pending turn's) first, then undone with the rest, kept under the ref.
    uncommitted: Vec<String>,
    elsewhere: Vec<Value>,
    base_moves: bool,
}

impl UndoPlan {
    /// More than the last turn, past the engineer's edits, or over a turn that changed
    /// something elsewhere (review 18).
    fn needs_confirm(&self) -> bool {
        self.turns.len() > 1
            || !self.edits.is_empty()
            || !self.uncommitted.is_empty()
            || !self.elsewhere.is_empty()
    }

    fn view(&self) -> Value {
        json!({
            "from": self.from,
            "turns": self.turns,
            "shared": self.shared.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
            "edits": self.edits.iter().map(|(c, f)| json!({ "commit": c, "files": f })).collect::<Vec<_>>(),
            "uncommitted": self.uncommitted,
            "elsewhere": self.elsewhere,
            "base_moves": self.base_moves,
            "head": self.head_after,
            "needs_confirm": self.needs_confirm(),
            "notice": ONLY_THE_COPY,
        })
    }
}

impl Studio {
    /// The number the session's next turn will have.
    fn next_turn(c: &Conv) -> u32 {
        c.state.turns.len() as u32 + 1
    }

    /// 012 (R-4): the working copy settled before a turn, an undo, a gate run and a close. A
    /// failed turn commit is tried again, what the turn left committed as the turn's and what
    /// changed since as the engineer's; otherwise the engineer's edits since the last commit are
    /// committed as theirs. Answers the files committed as the engineer's. With "fix it in the
    /// next turn" chosen, nothing is done: the next turn's commit holds it.
    pub(super) fn settle(&mut self, conv: &str) -> Result<Vec<String>, String> {
        self.settle_with(conv, false)
    }

    /// `settle`; `retry_folded`: a failed commit is tried again even when "fix it in the next
    /// turn" was chosen (before an undo, which would otherwise lose it: Phase 6 review, 2).
    fn settle_with(&mut self, conv: &str, retry_folded: bool) -> Result<Vec<String>, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        let wt = solo.worktree.clone();
        let before = format!("turn {}", Self::next_turn(c));
        if solo.pending_commit.is_some() && solo.fold_next && !retry_folded {
            return Ok(Vec::new());
        }
        let mut engineer: Vec<String> = Vec::new();
        if let Some(n) = solo.pending_commit {
            // What the turn left, by content; whatever differs since is the engineer's.
            let recs = solo
                .commits
                .get(&n)
                .map(|t| t.changed.clone())
                .unwrap_or_default();
            let mut turn_paths = Vec::new();
            for r in &recs {
                if git::blob_of(&wt, &r.path) == r.blob {
                    turn_paths.push(r.path.clone());
                } else {
                    engineer.push(r.path.clone());
                }
            }
            let msg = format!("Fallow solo {conv}, turn {n}: committed again");
            match git::snapshot_paths(&wt, &solo.branch, &msg, &turn_paths) {
                Ok(Some(commit)) => {
                    self.conv_mut(conv)?
                        .append(CEvent::Solo(SoloEvent::TurnCommitted { n, commit }))?;
                }
                Ok(None) => {
                    // Nothing of the turn's is left as it left it: no commit of its own (the
                    // engineer's edits go in theirs). Its pending state ends.
                    self.conv_mut(conv)?
                        .append(CEvent::Solo(SoloEvent::UncommittedDiscarded {
                            n,
                            files: Vec::new(),
                        }))?;
                }
                Err(e) => {
                    return Err(format!(
                        "Fallow could not commit turn {n}: {e}. Fix it in the next turn, or undo those changes"
                    ));
                }
            }
        }
        // The engineer's edits (and what was not the turn's in a retried commit).
        let dirty: Vec<String> = git::changes_all(&wt)
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.trim_matches('"').to_string())
            .collect();
        if dirty.is_empty() {
            return Ok(Vec::new());
        }
        for d in dirty {
            if !engineer.contains(&d) {
                engineer.push(d);
            }
        }
        let msg = format!("Fallow solo {conv}: the engineer's edits before {before}");
        match git::snapshot(&wt, &solo.branch, &msg) {
            Ok(Some(commit)) => {
                self.conv_mut(conv)?
                    .append(CEvent::Solo(SoloEvent::EngineerEdits {
                        before,
                        commit,
                        files: engineer.clone(),
                    }))?;
                Ok(engineer)
            }
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(format!(
                "Fallow could not commit your edits in the session's working copy: {e}"
            )),
        }
    }

    /// 012 (R-4, contracts/web-api.md): the engineer's way on from a failed turn commit: "Fix it
    /// in the next turn", or "Undo these changes" (with `confirmed`), which discards the working
    /// copy's uncommitted changes and answers the files lost.
    pub fn solo_pending(
        &mut self,
        conv: &str,
        choice: &str,
        confirmed: bool,
    ) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        if c.state.busy {
            return Err("Wait until the turn ends".into());
        }
        let solo = c.state.solo.clone().ok_or("not a session")?;
        let n = solo.pending_commit.ok_or("no turn's commit is waiting")?;
        match choice {
            "fold_next" => {
                self.conv_mut(conv)?
                    .append(CEvent::Solo(SoloEvent::FoldNext { n }))?;
                Ok(json!({}))
            }
            "discard_preview" => {
                Ok(json!({ "files": git::changes_all(&solo.worktree).unwrap_or_default() }))
            }
            "discard" => {
                let files: Vec<String> = git::changes_all(&solo.worktree)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| f.trim_matches('"').to_string())
                    .collect();
                if !confirmed {
                    return Err(format!(
                        "this needs the engineer's confirmation: it discards {}",
                        if files.is_empty() {
                            "nothing".to_string()
                        } else {
                            files.join(", ")
                        }
                    ));
                }
                git::discard_nohooks(&solo.worktree)?;
                self.conv_mut(conv)?
                    .append(CEvent::Solo(SoloEvent::UncommittedDiscarded {
                        n,
                        files: files.clone(),
                    }))?;
                Ok(json!({ "files": files }))
            }
            other => Err(format!("no choice {other:?}: fold_next or discard")),
        }
    }

    /// What undoing turn `k` takes back (R-5): the turns from it to the last (a turn whose
    /// commit holds an earlier one's changes takes that one with it), the engineer's edits among
    /// them, what they changed elsewhere, and whether the base moves back.
    fn undo_plan(&self, conv: &str, k: u32) -> Result<UndoPlan, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        let last = c.state.turns.len() as u32;
        let live = |n: u32| {
            !solo.undone.contains(&n) && c.state.turns.iter().any(|t| t.n == n && !t.withdrawn)
        };
        if !live(k) {
            return Err(format!("there is no turn {k} to undo"));
        }
        // Turns sharing one commit are undone together.
        let mut from = k;
        loop {
            let pulled = (from..=last)
                .filter_map(|t| solo.commits.get(&t).and_then(|c| c.folded_into.map(|_| t)))
                .filter_map(|t| {
                    // `folded_into` on turn t names the earlier turn whose changes its commit holds.
                    solo.commits.get(&t).and_then(|c| c.folded_into)
                })
                .filter(|j| *j < from)
                .min();
            match pulled {
                Some(j) => from = j,
                None => break,
            }
        }
        let turns: Vec<u32> = (from..=last).filter(|n| live(*n)).collect();
        let shared: Vec<(u32, u32)> = turns
            .iter()
            .filter_map(|t| {
                solo.commits
                    .get(t)
                    .and_then(|c| c.folded_into)
                    .map(|j| (j, *t))
            })
            .collect();
        // The engineer's edits made after turn `from` started: before turns from+1 … last+1.
        let head_now = git::head(&solo.worktree).unwrap_or_default();
        let head_after_ = c
            .state
            .turns
            .iter()
            .find(|t| t.n == from)
            .and_then(|t| t.head.clone());
        let edits: Vec<(String, Vec<String>)> = solo
            .engineer_edits
            .iter()
            .filter(|(_, commit, _)| {
                git::is_ancestor(&solo.worktree, commit, &head_now)
                    && head_after_
                        .as_deref()
                        .is_none_or(|h| !git::is_ancestor(&solo.worktree, commit, h))
            })
            .map(|(_, commit, files)| (commit.clone(), files.clone()))
            .collect();
        let uncommitted: Vec<String> = git::changes_all(&solo.worktree).unwrap_or_default();
        let elsewhere: Vec<Value> = solo
            .observed
            .iter()
            .filter(|o| turns.contains(&o.turn))
            .map(|o| json!(o))
            .collect();
        let base_moves = solo.base_moves.iter().any(|(t, _, _)| turns.contains(t));
        let head_after = c
            .state
            .turns
            .iter()
            .find(|t| t.n == from)
            .and_then(|t| t.head.clone())
            .ok_or_else(|| {
                format!("Fallow does not know where the branch stood before turn {from}")
            })?;
        Ok(UndoPlan {
            from,
            to: last,
            head_after,
            turns,
            shared,
            edits,
            uncommitted,
            elsewhere,
            base_moves,
        })
    }

    /// 012 (R-5): `GET …/undo?turn=k`, what the confirmation names.
    pub fn undo_preview(&self, conv: &str, k: u32) -> Result<Value, String> {
        Ok(self.undo_plan(conv, k)?.view())
    }

    /// 012 (FR-014, R-5): undoes turn `k` and every turn after it: the working copy is settled,
    /// `UndoStarted` written, the old head kept under `refs/fallow/solo/<id>/undone/<k>`, the
    /// branch and working copy reset without hooks, and `TurnsUndone` written.
    pub fn undo_turns(&mut self, conv: &str, k: u32, confirmed: bool) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        if c.state.busy {
            return Err("Wait until the turn ends".into());
        }
        let solo = c.state.solo.clone().ok_or("not a session")?;
        if solo.closed.is_some() {
            return Err("the session is closed".into());
        }
        // Confirmed before anything is done: what is not committed yet is named too (Phase 6
        // review, 1).
        let first = self.undo_plan(conv, k)?;
        if first.needs_confirm() && !confirmed {
            return Err(format!(
                "this needs the engineer's confirmation: it undoes turns {}",
                first
                    .turns
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // A failed commit is tried again first, a fold chosen or not; when it still fails the
        // undo waits (R-4).
        self.settle_with(conv, true)?;
        let plan = self.undo_plan(conv, k)?;
        let wt = solo.worktree.clone();
        let head_before = git::head(&wt)?;
        let keep_ref = format!("refs/fallow/solo/{conv}/undone/{}", plan.from);
        self.conv_mut(conv)?
            .append(CEvent::Solo(SoloEvent::UndoStarted {
                from: plan.from,
                head_before: head_before.clone(),
                head_after: plan.head_after.clone(),
            }))?;
        git::update_ref_nohooks(&wt, &keep_ref, &head_before)?;
        git::reset_hard_nohooks(&wt, &plan.head_after)?;
        // Exactly the commit before (SC-003): nothing untracked is left either.
        git::discard_nohooks(&wt)?;
        let c = self.conv_mut(conv)?;
        // A fold chosen for a turn that is undone now has nothing left to fold.
        if let Some(n) = solo.pending_commit.filter(|n| *n >= plan.from) {
            c.append(CEvent::Solo(SoloEvent::UncommittedDiscarded {
                n,
                files: Vec::new(),
            }))?;
        }
        c.append(CEvent::Solo(SoloEvent::TurnsUndone {
            from: plan.from,
            to: plan.to,
            head_before,
            head_after: plan.head_after.clone(),
            keep_ref,
        }))?;
        c.tick();
        Ok(json!({ "head": plan.head_after, "turns": plan.turns }))
    }

    /// 012 (R-5, R-19): an undo Fallow stopped in the middle of is finished from git on load.
    pub(super) fn finish_undo_on_load(&mut self, id: &str) {
        let Some(c) = self.convs.get(id) else {
            return;
        };
        let Some(solo) = c.state.solo.clone() else {
            return;
        };
        let Some((from, head_before, head_after)) = solo.undoing.clone() else {
            return;
        };
        let wt = solo.worktree.clone();
        let to = c.state.turns.len() as u32;
        let keep_ref = format!("refs/fallow/solo/{id}/undone/{from}");
        if !git::rev_exists(&wt, &keep_ref) {
            let _ = git::update_ref_nohooks(&wt, &keep_ref, &head_before);
        }
        if git::head(&wt).ok().as_deref() == Some(head_before.as_str()) {
            let _ = git::reset_hard_nohooks(&wt, &head_after);
        }
        if let Some(c) = self.convs.get_mut(id) {
            let _ = c.append(CEvent::Solo(SoloEvent::TurnsUndone {
                from,
                to,
                head_before,
                head_after,
                keep_ref,
            }));
        }
    }

    /// 012 (R-12): gate copies of a session that a restart left behind.
    pub(super) fn forget_gate_copies(&self, id: &str) {
        let Some(c) = self.convs.get(id) else {
            return;
        };
        let Some(solo) = c.state.solo.as_ref() else {
            return;
        };
        let Some(dir) = solo.worktree.parent() else {
            return;
        };
        let repo = self.store.get(&c.state.project).map(|p| p.path.clone());
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{id}-gates-")) {
                match &repo {
                    Some(r) => git::worktree_remove_nohooks(r, &e.path()),
                    None => {
                        let _ = std::fs::remove_dir_all(e.path());
                    }
                }
            }
        }
    }

    /// The turns the engineer undid since the last turn the assistant knows of, for the next
    /// turn's prompt (FR-014).
    pub(super) fn undone_to_tell(c: &Conv) -> Vec<(u32, String)> {
        let Some(solo) = c.state.solo.as_ref() else {
            return Vec::new();
        };
        let kept = c
            .state
            .turns
            .iter()
            .filter(|t| !solo.undone.contains(&t.n) && !t.withdrawn)
            .map(|t| t.n)
            .max()
            .unwrap_or(0);
        c.state
            .turns
            .iter()
            .filter(|t| solo.undone.contains(&t.n) && t.n > kept)
            .map(|t| (t.n, t.said.clone()))
            .collect()
    }

    /// 012 (R-5): at the end of a turn, whether it brought the branch up to date with the
    /// starting branch: a merge of a commit that is on the starting branch and was not on the
    /// session's branch before the turn, or a rebase or fast-forward onto it.
    pub(super) fn base_after_turn(&mut self, conv: &str, n: u32, before: &str, after: &str) {
        let Some(c) = self.convs.get_mut(conv) else {
            return;
        };
        let mark = c.dir.join("start_before");
        let s_before = c
            .start_before
            .take()
            .or_else(|| {
                std::fs::read_to_string(&mark)
                    .ok()
                    .map(|s| s.trim().to_string())
            })
            .filter(|s| !s.is_empty());
        let _ = std::fs::remove_file(&mark);
        let c = &*c;
        let Some(solo) = c.state.solo.clone() else {
            return;
        };
        let Some(start) = solo.start_branch.clone() else {
            return;
        };
        let wt = solo.worktree.clone();
        // The starting branch as it was when the turn began: a merge of the session's own
        // commits into it moves nothing (R-5; Phase 6 review, 3).
        let Some(s) = s_before.or_else(|| git::rev(&wt, &format!("refs/heads/{start}")).ok())
        else {
            return;
        };
        if before == after {
            return;
        }
        let base = solo.base();
        let on_s = |x: &str| git::is_ancestor(&wt, x, &s);
        let had = |x: &str| git::is_ancestor(&wt, x, before);
        let mut moved: Option<(String, String)> = None;
        for (commit, parents) in git::parents_between(&wt, before, after) {
            if parents.len() < 2 {
                continue;
            }
            if let Some(p) = parents.iter().find(|p| on_s(p) && !had(p)) {
                moved = Some((commit.clone(), p.clone()));
                break;
            }
        }
        if moved.is_none() {
            if let Ok(b) = git::merge_base(&wt, after, &s) {
                let b = b.trim().to_string();
                if b != base.gates && git::is_ancestor(&wt, &base.gates, &b) && !had(&b) {
                    moved = Some((b.clone(), b));
                }
            }
        }
        if let (Some((mark, gates)), Some(c)) = (moved, self.convs.get_mut(conv)) {
            let _ = c.append(CEvent::Solo(SoloEvent::BaseMoved {
                turn: n,
                mark,
                gates,
                why: BaseWhy::UpToDate,
            }));
        }
    }

    /// 012 (FR-019): `GET /api/sessions/<s>/changes`, the session's files against its base: each
    /// with its mark, the turns that changed it and who (the assistant, or the engineer).
    pub fn solo_changes(&self, conv: &str) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.as_ref().ok_or("not a session")?;
        let wt = &solo.worktree;
        let base = solo.base();
        // Who made each commit: a turn's, or the engineer's edits.
        let mut whose: BTreeMap<String, (Option<u32>, &'static str)> = BTreeMap::new();
        for (_, commit, _) in &solo.engineer_edits {
            whose.insert(commit.clone(), (None, "you"));
        }
        for (n, t) in &solo.commits {
            for commit in t.commits.iter().chain(t.commit.iter()) {
                whose
                    .entry(commit.clone())
                    .or_insert((Some(*n), "assistant"));
            }
        }
        let mut per: BTreeMap<String, (BTreeSet<u32>, BTreeSet<&'static str>)> = BTreeMap::new();
        let head = git::head(wt).unwrap_or_default();
        for commit in git::commits_oldest_first(wt, &base.mark, &head) {
            let (turn, by) = whose.get(&commit).cloned().unwrap_or((None, "assistant"));
            for f in git::files_of(wt, &commit) {
                let e = per.entry(f).or_default();
                if let Some(t) = turn {
                    e.0.insert(t);
                }
                e.1.insert(by);
            }
        }
        let uncommitted: BTreeSet<String> = git::changes_all(wt)
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.trim_matches('"').to_string())
            .collect();
        let files: Vec<Value> = git::name_status(wt, &base.mark, None)?
            .into_iter()
            .map(|(mark, path)| {
                let (turns, by) = per.get(&path).cloned().unwrap_or_default();
                json!({
                    "path": path,
                    "mark": mark.to_string(),
                    "turns": turns,
                    "by": by,
                    "uncommitted": uncommitted.contains(&path),
                })
            })
            .collect();
        Ok(json!({
            "base": base,
            "files": files,
            "notice": ONLY_THE_COPY,
        }))
    }

    /// 012 (FR-021, FR-022, R-12): the project's gates on the session's head, on demand. Refused
    /// while a turn works; the working copy is settled first so they run on a commit. They run
    /// on a thread in a worktree of their own at the head, read from the project folder's
    /// committed squad file, compared with a baseline at `base.gates` (cached per commit).
    pub fn run_solo_gates(&mut self, ctx: &Ctx, conv: &str) -> Result<Value, String> {
        let c = self.convs.get(conv).ok_or("there is no session")?;
        if c.state.busy {
            return Err("Wait until the turn ends".into());
        }
        if c.gates_running {
            return Ok(json!({ "running": true }));
        }
        let project = c.state.project.clone();
        if let Some(n) = c
            .state
            .solo
            .as_ref()
            .and_then(|s| s.pending_commit.filter(|_| s.fold_next))
        {
            return Err(format!(
                "Turn {n}'s changes are not committed yet: they go with the next turn. Send it, or undo them, first"
            ));
        }
        self.settle(conv)?;
        let c = self.convs.get(conv).ok_or("there is no session")?;
        let solo = c.state.solo.clone().ok_or("not a session")?;
        let p = self.project(&project)?.clone();
        let head = git::head(&solo.worktree)?;
        let base = solo.base().gates;
        let squad = super::solo::squad_at_head(&p.root_dir());
        if squad.gates.is_empty() {
            self.conv_mut(conv)?
                .append(CEvent::Solo(SoloEvent::GatesRun {
                    commit: head,
                    base,
                    results: Vec::new(),
                }))?;
            return Ok(json!({ "running": false, "gates": 0 }));
        }
        // A baseline is kept per commit and per gate definitions: a changed squad file runs it
        // again (Phase 6 review, 12).
        let defs_key = format!(
            "{base}:{}",
            squad
                .gates
                .iter()
                .map(|g| format!("{}={}", g.name, g.command))
                .collect::<Vec<_>>()
                .join(";")
        );
        let baseline = (base != head)
            .then(|| c.gate_baselines.get(&defs_key).cloned())
            .flatten();
        let need_baseline = base != head && baseline.is_none();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let dir: PathBuf = self
            .worktrees(&p.id)
            .join("_solo")
            .join(format!("{conv}-gates-{stamp}"));
        let base_dir: PathBuf = self
            .worktrees(&p.id)
            .join("_solo")
            .join(format!("{conv}-gates-{stamp}-base"));
        let repo = p.path.clone();
        let tx = ctx.tx.clone();
        let id = conv.to_string();
        let defs = squad.gates.clone();
        self.conv_mut(conv)?.gates_running = true;
        let spawned = std::thread::Builder::new()
            .name(format!("gates-{conv}"))
            .spawn(move || {
                let stop = AtomicBool::new(false);
                // Each gate with the end of its output (FR-021).
                let run_at = |commit: &str,
                              d: &std::path::Path|
                 -> Result<Vec<(GateResult, String)>, String> {
                    if let Err(e) = git::worktree_add_detached_nohooks(&repo, d, commit) {
                        git::worktree_remove_nohooks(&repo, d);
                        return Err(e);
                    }
                    let out = defs
                        .iter()
                        .map(|g| {
                            let mut text = String::new();
                            let r = gates::run(
                                g,
                                d,
                                commit,
                                &mut |l| {
                                    text.push_str(l);
                                    text.push('\n');
                                    if text.len() > 8192 {
                                        let cut = text.len() - 4096;
                                        let cut = (cut..text.len())
                                            .find(|i| text.is_char_boundary(*i))
                                            .unwrap_or(cut);
                                        text.drain(..cut);
                                    }
                                },
                                &stop,
                            );
                            (r, text)
                        })
                        .collect();
                    git::worktree_remove_nohooks(&repo, d);
                    Ok(out)
                };
                // A gate that panics still ends the run, said so (Phase 6 review, 12).
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let runs = run_at(&head, &dir)?;
                    let base_runs = if need_baseline {
                        let bd = base_dir.clone();
                        Some(run_at(&base, &bd)?.into_iter().map(|(r, _)| r).collect())
                    } else {
                        None
                    };
                    Ok((runs, base_runs))
                }))
                .unwrap_or_else(|_| {
                    git::worktree_remove_nohooks(&repo, &dir);
                    git::worktree_remove_nohooks(&repo, &base_dir);
                    Err("the gate run stopped unexpectedly".to_string())
                });
                let _ = tx.send(Msg::Studio(Note::GatesDone {
                    conv: id,
                    commit: head,
                    base: defs_key,
                    outcome: done,
                    baseline,
                }));
            });
        if let Err(e) = spawned {
            self.conv_mut(conv)?.gates_running = false;
            return Err(format!("cannot start the gates: {e}"));
        }
        Ok(json!({ "running": true }))
    }

    /// The gate run's end: classified against the baseline and written as `GatesRun`.
    #[allow(clippy::type_complexity)]
    pub(super) fn solo_gates_done(
        &mut self,
        conv: &str,
        commit: String,
        key: String,
        outcome: Result<(Vec<(GateResult, String)>, Option<Vec<GateResult>>), String>,
        baseline: Option<Vec<GateResult>>,
    ) -> Result<(), String> {
        // The key is `<base>:<gate definitions>`.
        let base = key.split(':').next().unwrap_or_default().to_string();
        let c = self.conv_mut(conv)?;
        c.gates_running = false;
        let (runs, fresh) = match outcome {
            Ok(x) => x,
            Err(e) => {
                c.gates_error = Some(e);
                c.tick();
                return Ok(());
            }
        };
        c.gates_error = None;
        if let Some(b) = &fresh {
            c.gate_baselines.insert(key.clone(), b.clone());
        }
        let base_runs = fresh.or(baseline).unwrap_or_default();
        let results = runs
            .iter()
            .map(|(r, text)| {
                let b = base_runs.iter().find(|b| b.gate == r.gate);
                let class = gates::classify(r, b);
                GateOutcome {
                    gate: r.gate.clone(),
                    outcome: r.outcome.label(),
                    class: match class {
                        gates::Class::Pass => "pass",
                        gates::Class::BaselineOnly => "baseline_only",
                        gates::Class::Regression => "regression",
                        gates::Class::Fail => "fail",
                    }
                    .into(),
                    output: text.clone(),
                }
            })
            .collect();
        c.append(CEvent::Solo(SoloEvent::GatesRun {
            commit,
            base,
            results,
        }))?;
        c.tick();
        Ok(())
    }
}
