//! 012 US4 (FR-016, FR-017; research R-10, R-11): the engine's side of the ref watcher. A
//! thread snapshots every project's refs every 2 s; the engine diffs each snapshot against the
//! last one it kept, names each change's source from the records only, stores it in
//! `<data>/projects/<p>/git.jsonl` (with the last snapshot in `git-seen.json`, so what changed
//! while Fallow was stopped shows after a restart) and tells the page. A session's git command
//! gets an opening snapshot when Fallow allows it and a closing one at its result: only a
//! change in that window, on a ref that command targets, is the session's.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::engine::{Engine, Msg, Reply};
use crate::conversation::events::{Event as CEvent, Kind};
use crate::conversation::solo::{BaseWhy, How, Observed, SoloEvent};
use crate::studio::refwatch::{self, GitChange, Snapshot, Source};
use crate::verify::git;

/// The most changes kept per project (R-10).
const KEEP: usize = 1000;

/// One project's watch: the last snapshot kept, when it was taken, and the pages waiting.
#[derive(Default)]
pub struct Watch {
    pub last: Option<Snapshot>,
    pub at: Option<Instant>,
    pub seq: u64,
    pub waiters: Vec<(Instant, Sender<Reply>)>,
    loaded: bool,
    /// Sessions' commands that ended while another was still open: their claims, kept until
    /// the last one ends and everything is recorded at once (R-10; Phase 7 reviews).
    held: Vec<Claim>,
}

/// One ended command's claim on the refs it targets, with its window's opening.
#[derive(Clone)]
struct Claim {
    conv: String,
    targets: Vec<String>,
    turn: u32,
    opening: Option<Snapshot>,
    opened_at: Instant,
    folder: bool,
}

/// The projects the thread snapshots, kept current by the engine.
pub type WatchList = Arc<Mutex<Vec<(String, PathBuf)>>>;

/// The watcher thread: every 2 s, each project's snapshot to the engine. It ends with the engine.
pub fn spawn(tx: Sender<Msg>, list: WatchList) {
    let _ = std::thread::Builder::new()
        .name("refwatch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let items = list.lock().map(|l| l.clone()).unwrap_or_default();
                for (project, path) in items {
                    let at = Instant::now();
                    if let Some(snap) = refwatch::take(&path) {
                        if tx.send(Msg::Refs { project, snap, at }).is_err() {
                            return;
                        }
                    }
                }
                if tx.send(Msg::RefsRound).is_err() {
                    return;
                }
            }
        });
}

/// Who owns what by the records (R-10): each loop, spec session and session of the project with
/// the commits its folded record holds and the branches that are its own.
struct Owner {
    source: Source,
    commits: BTreeSet<String>,
    branches: Vec<String>,
    /// A loop's or spec session's own branches: their changes go in the quiet group.
    fallows: bool,
}

/// Whether branch `b` is `o`'s own: one of its names, or under a `…/` prefix of them.
fn owns(o: &Owner, b: &str) -> bool {
    o.branches
        .iter()
        .any(|x| x == b || (x.ends_with('/') && b.starts_with(x.as_str())))
}

impl Engine {
    fn watch_dir(&self, project: &str) -> PathBuf {
        self.data.join("projects").join(project)
    }

    /// The project's watch, its last snapshot read from `git-seen.json` the first time.
    pub(crate) fn watch_mut(&mut self, project: &str) -> &mut Watch {
        let dir = self.watch_dir(project);
        let w = self.watch.entry(project.to_string()).or_default();
        if !w.loaded {
            w.loaded = true;
            w.last = std::fs::read_to_string(dir.join("git-seen.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok());
        }
        w
    }

    fn keep_seen(&mut self, project: &str, snap: Snapshot, at: Instant) {
        let dir = self.watch_dir(project);
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(t) = serde_json::to_string(&snap) {
            let tmp = dir.join("git-seen.json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join("git-seen.json"));
            }
        }
        let w = self.watch_mut(project);
        w.last = Some(snap);
        w.at = Some(at);
    }

    /// Keeps the thread's list of projects current: the usable repositories.
    pub(crate) fn refresh_watch_list(&mut self) {
        let items: Vec<(String, PathBuf)> = self
            .studio
            .store
            .projects
            .iter()
            .filter(|p| p.path.join(".git").exists())
            .map(|p| (p.id.clone(), p.path.clone()))
            .collect();
        // A project seen for the first time is snapshot now, so its first change is seen.
        for (id, path) in &items {
            if self.watch_mut(id).last.is_none() {
                if let Some(s) = refwatch::take(path) {
                    self.keep_seen(id, s, Instant::now());
                }
            }
        }
        if let Ok(mut l) = self.watch_list.lock() {
            *l = items;
        }
    }

    /// Whether a session of `project` has a git command open (its window is not closed yet).
    fn call_open(&self, project: &str) -> bool {
        self.studio
            .convs
            .values()
            .any(|c| c.state.project == project && c.open_call.is_some())
    }

    /// A snapshot from the thread: diffed against the last one kept, unless a session's command
    /// is open (its closing snapshot takes them) or the snapshot is older than the last kept.
    pub(crate) fn on_refs(&mut self, project: &str, snap: Snapshot, at: Instant) {
        if !self.git.ok || self.call_open(project) {
            return;
        }
        // Claims whose call's conversation went away: recorded now.
        if !self.watch_mut(project).held.is_empty() {
            self.finish_held(project);
            return;
        }
        let w = self.watch_mut(project);
        if w.at.is_some_and(|t| t > at) {
            return;
        }
        match w.last.clone() {
            None => self.keep_seen(project, snap, at),
            Some(old) if old == snap => {}
            Some(old) => {
                self.record_between(project, &old, &snap, None, None);
                self.keep_seen(project, snap, at);
            }
        }
    }

    fn owners(&self, project: &str) -> Vec<Owner> {
        let mut out = Vec::new();
        for (id, r) in &self.runs {
            let Some(l) = r
                .state
                .feature_loop
                .as_ref()
                .filter(|l| l.project == project)
            else {
                continue;
            };
            // What the loop made: its record's commits, less those it only points at (where it
            // started, its base, its target, the lock it builds).
            let mut v = serde_json::to_value(&r.state).unwrap_or(Value::Null);
            refwatch::strip_keys(
                &mut v,
                &[
                    "start_commit",
                    "base_commit",
                    "target_commit",
                    "lock_commit",
                    "prev_lock_commit",
                    "reconciled_target",
                    "lined_at",
                    "publish_moved",
                    "overtaken_tip",
                    "unchecked",
                    "update_merges",
                    "base",
                    "merge_unchecked",
                    "publish_unchecked",
                    "moved",
                ],
            );
            let mut commits = BTreeSet::new();
            refwatch::commits_in(&v, &mut commits);
            out.push(Owner {
                source: Source::Loop {
                    id: *id,
                    feature: l.feature.clone(),
                },
                commits,
                branches: vec![
                    l.build_branch.clone(),
                    format!("fallow/{}", l.feature),
                    format!("fallow/{}/", l.feature),
                ],
                fallows: true,
            });
        }
        for (id, c) in &self.studio.convs {
            if c.state.project != project {
                continue;
            }
            // What the conversation made, never what it only points at (a session's start, a
            // turn's head before it): its turns' commits, a session's retried and the engineer's
            // edit commits, a spec session's locks.
            let mut commits: BTreeSet<String> = c
                .state
                .turns
                .iter()
                .filter_map(|t| t.commit.clone())
                .collect();
            match (c.state.kind, c.state.solo.as_ref()) {
                (Kind::Solo, Some(s)) => {
                    for t in s.commits.values() {
                        commits.extend(t.commits.iter().cloned());
                        commits.extend(t.commit.iter().cloned());
                    }
                    commits.extend(s.engineer_edits.iter().map(|(_, c, _)| c.clone()));
                    out.push(Owner {
                        source: Source::Session {
                            id: id.clone(),
                            name: s.name.clone().unwrap_or_default(),
                        },
                        commits,
                        branches: vec![s.branch.clone()],
                        fallows: false,
                    });
                }
                (Kind::Spec, _) => {
                    let feature = c.state.feature.clone().unwrap_or_default();
                    if let Some(f) = self
                        .studio
                        .store
                        .get(project)
                        .and_then(|p| p.feature(&feature))
                    {
                        commits.extend(f.locks.iter().map(|l| l.commit.clone()));
                    }
                    out.push(Owner {
                        source: Source::Spec {
                            feature: feature.clone(),
                        },
                        commits,
                        branches: vec![feature],
                        fallows: true,
                    });
                }
                _ => {}
            }
        }
        out
    }

    /// The changes between two snapshots, each with its source; `call`: the session whose
    /// command's window this is, and the refs that command targets (R-10).
    pub(crate) fn record_between(
        &mut self,
        project: &str,
        old: &Snapshot,
        new: &Snapshot,
        call: Option<(&str, &[String], u32)>,
        only: Option<&[String]>,
    ) {
        let Some(repo) = self.studio.store.get(project).map(|p| p.path.clone()) else {
            return;
        };
        let mut changes = refwatch::diff(&repo, old, new);
        if let Some(only) = only {
            changes.retain(|c| refwatch::targeted(only, &c.0));
        }
        if changes.is_empty() {
            return;
        }
        let owners = self.owners(project);
        let at = crate::run::time::rfc3339(crate::run::time::unix_now());
        let mut kept: Vec<GitChange> = Vec::new();
        let mut observed: Vec<(String, Observed)> = Vec::new();
        let mut gone_branches: Vec<String> = Vec::new();
        for (refname, before, after, how, n) in changes {
            let branch = refname.strip_prefix("refs/heads/").map(str::to_string);
            let by_call = call.filter(|(_, targets, _)| refwatch::targeted(targets, &refname));
            let source = if let Some((conv, _, _)) = by_call {
                let name = self
                    .studio
                    .convs
                    .get(conv)
                    .and_then(|c| c.state.solo.as_ref())
                    .and_then(|s| s.name.clone())
                    .unwrap_or_default();
                Source::Session {
                    id: conv.to_string(),
                    name,
                }
            } else {
                let probe = after.clone().or_else(|| before.clone()).unwrap_or_default();
                owners
                    .iter()
                    .find(|o| o.commits.contains(&probe))
                    // A branch's creation or deletion is its owner's, whose records name it.
                    .or_else(|| {
                        matches!(how, How::Created | How::Deleted)
                            .then(|| {
                                owners
                                    .iter()
                                    .find(|o| branch.as_ref().is_some_and(|b| owns(o, b)))
                            })
                            .flatten()
                    })
                    .map(|o| o.source.clone())
                    .unwrap_or(Source::Outside)
            };
            // A session's own branch moved by Fallow's snapshots or its turns: not shown.
            let own_session = owners.iter().any(|o| {
                matches!(o.source, Source::Session { .. })
                    && branch.as_ref().is_some_and(|b| o.branches.contains(b))
            });
            if let (How::Deleted, Some(b)) = (how, &branch) {
                if b.starts_with("fallow/solo/") {
                    gone_branches.push(b.clone());
                }
            }
            if own_session && how != How::Deleted {
                continue;
            }
            let quiet = owners.iter().any(|o| {
                o.fallows
                    && o.source == source
                    && branch
                        .as_ref()
                        .is_some_and(|b| owns(o, b) || b.starts_with("fallow/"))
            });
            if let (Some((id, _, _)), Source::Session { .. }) = (by_call, &source) {
                let id = &id.to_string();
                let commits = match (&before, &after) {
                    (Some(b), Some(a)) => git::first_parents(&repo, b, a),
                    (None, Some(a)) => vec![a.clone()],
                    _ => Vec::new(),
                };
                observed.push((
                    id.clone(),
                    Observed {
                        refname: refname.clone(),
                        before: before.clone(),
                        after: after.clone(),
                        how,
                        n,
                        commits,
                        turn: call.map_or(0, |(_, _, t)| t),
                    },
                ));
            }
            kept.push(GitChange {
                refname,
                before,
                after,
                how,
                n,
                source,
                quiet,
                at: at.clone(),
            });
        }
        for (id, o) in observed {
            self.squash_check(&id, &o);
            if let Some(c) = self.studio.convs.get_mut(&id) {
                let _ = c.append(CEvent::Solo(SoloEvent::GitObserved(o)));
                c.tick();
            }
        }
        // A session whose branch went outside its own close is gone at once (R-19).
        if !gone_branches.is_empty() {
            let ids: Vec<String> = self
                .studio
                .convs
                .iter()
                .filter(|(_, c)| {
                    c.state
                        .solo
                        .as_ref()
                        .is_some_and(|s| gone_branches.contains(&s.branch))
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                self.studio.check_gone_pub(&id);
            }
        }
        if kept.is_empty() {
            return;
        }
        self.store_changes(project, &kept);
    }

    fn store_changes(&mut self, project: &str, kept: &[GitChange]) {
        let dir = self.watch_dir(project);
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("git.jsonl");
        let mut lines: Vec<String> = std::fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        for c in kept {
            if let Ok(l) = serde_json::to_string(c) {
                lines.push(l);
            }
        }
        if lines.len() > KEEP {
            let cut = lines.len() - KEEP;
            lines.drain(..cut);
        }
        let _ = std::fs::write(&file, format!("{}\n", lines.join("\n")));
        let w = self.watch_mut(project);
        w.seq += 1;
        let seq = w.seq;
        for (_, tx) in w.waiters.drain(..) {
            let _ = tx.send(Reply::Ok(json!({ "seq": seq })));
        }
        self.bus
            .publish(json!({ "type": "git", "project": project }));
    }

    /// R-11: a session's command that put one new commit on a local branch holding exactly the
    /// session's changes up to a turn is a squash; on the starting branch the base moves.
    fn squash_check(&mut self, conv: &str, o: &Observed) {
        if o.how != How::Commits || o.n != Some(1) {
            return;
        }
        // A fast-forward to one of the session's own commits is no squash (Phase 7 review, 3).
        if let Some(s) = self
            .studio
            .convs
            .get(conv)
            .and_then(|c| c.state.solo.as_ref())
        {
            let own = o.after.as_deref().is_some_and(|a| {
                s.commits
                    .values()
                    .any(|t| t.commit.as_deref() == Some(a) || t.commits.iter().any(|c| c == a))
                    || s.engineer_edits.iter().any(|(_, c, _)| c == a)
            });
            if own {
                return;
            }
        }
        let (Some(b), Some(c_new)) = (o.refname.strip_prefix("refs/heads/"), o.after.as_deref())
        else {
            return;
        };
        let Some(c) = self.studio.convs.get(conv) else {
            return;
        };
        let Some(solo) = c.state.solo.clone() else {
            return;
        };
        let base = solo.base();
        let wt = solo.worktree.clone();
        let mut turns: Vec<(u32, String)> = solo
            .commits
            .iter()
            .filter(|(n, _)| !solo.undone.contains(n))
            .filter_map(|(n, t)| t.commit.clone().map(|cm| (*n, cm)))
            .collect();
        turns.sort_by_key(|t| std::cmp::Reverse(t.0));
        let Some((j, turn_commit)) = turns
            .into_iter()
            .find(|(_, tc)| refwatch::is_squash_of(&wt, c_new, &base.mark, tc))
        else {
            return;
        };
        let now = c.state.turns.last().map_or(0, |t| t.n);
        let start = solo.start_branch.clone();
        if let Some(c) = self.studio.convs.get_mut(conv) {
            let _ = c.append(CEvent::Solo(SoloEvent::Squashed {
                branch: b.to_string(),
                commit: c_new.to_string(),
                upto_turn: j,
            }));
            if start.as_deref() == Some(b) {
                let _ = c.append(CEvent::Solo(SoloEvent::BaseMoved {
                    turn: now,
                    mark: turn_commit,
                    gates: c_new.to_string(),
                    why: BaseWhy::Squash,
                }));
            }
        }
    }

    /// A session's git command ended (its `tool_result`, or its turn's end). While another
    /// session's command is open, its claim waits; when the last one ends, the held claims are
    /// recorded at once (`finish_held`).
    pub(crate) fn close_call(&mut self, conv: &str) {
        let Some(c) = self.studio.convs.get_mut(conv) else {
            return;
        };
        let Some(call) = c.open_call.take() else {
            return;
        };
        let project = c.state.project.clone();
        self.watch_mut(&project).held.push(Claim {
            conv: conv.to_string(),
            targets: call.targets.clone(),
            turn: call.turn,
            opening: call.opening.clone(),
            opened_at: call.opened_at,
            folder: call.folder,
        });
        if !self.call_open(&project) {
            self.finish_held(&project);
        }
    }

    /// The held claims recorded (R-10): what moved before the earliest window opened is
    /// anyone's; in the windows, each changed ref is the session's that alone claimed it (a
    /// wildcard claim included), the rest its owner's by the records or outside Fallow; the
    /// folder's state for each command that touched it. Also run by the periodic watch for
    /// claims left without an open call (a conversation removed).
    pub(crate) fn finish_held(&mut self, project: &str) {
        let Some(repo) = self.studio.store.get(project).map(|p| p.path.clone()) else {
            self.watch_mut(project).held.clear();
            return;
        };
        let Some(closing) = refwatch::take(&repo) else {
            // Kept for the next try (Phase 7 review, 7).
            return;
        };
        let claims = std::mem::take(&mut self.watch_mut(project).held);
        if claims.is_empty() {
            return;
        }
        let now = Instant::now();
        let w = self.watch_mut(project);
        let (last, last_at) = (w.last.clone(), w.at);
        let first = claims.iter().min_by_key(|c| c.opened_at).cloned();
        let opening = first.as_ref().and_then(|c| c.opening.clone());
        // From the earliest opening, unless the kept snapshot moved on since (never twice).
        let fresh = match (&first, last_at) {
            (Some(f), Some(t)) => t <= f.opened_at,
            _ => true,
        };
        let from = if fresh {
            if let (Some(l), Some(o)) = (&last, &opening) {
                self.record_between(project, l, o, None, None);
            }
            opening.or(last)
        } else {
            last
        };
        if let Some(from) = from {
            let changed: Vec<String> = refwatch::diff(&repo, &from, &closing)
                .into_iter()
                .map(|c| c.0)
                .collect();
            let mut by: Vec<(String, u32, Vec<String>)> = Vec::new();
            let mut rest: Vec<String> = Vec::new();
            for r in changed {
                let mut who: Vec<&Claim> = claims
                    .iter()
                    .filter(|c| refwatch::targeted(&c.targets, &r))
                    .collect();
                who.dedup_by(|a, b| a.conv == b.conv);
                let convs: std::collections::BTreeSet<&str> =
                    who.iter().map(|c| c.conv.as_str()).collect();
                if convs.len() == 1 && self.studio.convs.contains_key(who[0].conv.as_str()) {
                    // The latest of that session's commands naming it.
                    let c = *who.iter().max_by_key(|c| c.turn).unwrap_or(&who[0]);
                    match by
                        .iter_mut()
                        .find(|(id, t, _)| *id == c.conv && *t == c.turn)
                    {
                        Some(e) => e.2.push(r),
                        None => by.push((c.conv.clone(), c.turn, vec![r])),
                    }
                } else {
                    rest.push(r);
                }
            }
            if !rest.is_empty() {
                self.record_between(project, &from, &closing, None, Some(&rest));
            }
            for (conv, turn, refs) in &by {
                self.record_between(
                    project,
                    &from,
                    &closing,
                    Some((conv, refs, *turn)),
                    Some(refs),
                );
            }
        }
        self.keep_seen(project, closing, now);
        if claims.iter().any(|c| c.folder) {
            let state = refwatch::folder_state(&repo);
            for c in claims.iter().filter(|c| c.folder) {
                if let Some(cv) = self.studio.convs.get_mut(&c.conv) {
                    let _ = cv.append(CEvent::Solo(SoloEvent::FolderStateSeen {
                        state,
                        turn: c.turn,
                    }));
                    cv.tick();
                }
            }
        }
    }

    /// `GET /api/projects/<p>/git-changes`: the changes, newest first, the quiet ones apart.
    pub(crate) fn git_changes(&self, project: &str) -> Result<Value, String> {
        self.studio
            .store
            .get(project)
            .ok_or_else(|| format!("there is no project {project}"))?;
        let text =
            std::fs::read_to_string(self.watch_dir(project).join("git.jsonl")).unwrap_or_default();
        let all: Vec<GitChange> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let (quiet, main): (Vec<&GitChange>, Vec<&GitChange>) =
            all.iter().rev().take(300).partition(|c| c.quiet);
        let seq = self.watch.get(project).map_or(0, |w| w.seq);
        Ok(json!({ "changes": main, "quiet": quiet, "seq": seq }))
    }

    /// Long polling on `project:<p>` (R-10): answered at the next stored change.
    pub(crate) fn wait_project(&mut self, project: &str, after: u64, reply: Sender<Reply>) {
        if self.studio.store.get(project).is_none() {
            let _ = reply.send(Reply::Err(format!("there is no project {project}")));
            return;
        }
        let w = self.watch_mut(project);
        if w.seq != after {
            let _ = reply.send(Reply::Ok(json!({ "seq": w.seq, "reset": w.seq < after })));
        } else {
            w.waiters.push((Instant::now(), reply));
        }
    }

    /// Pages waiting longer than 25 s are answered with nothing new.
    pub(crate) fn prune_watch_waiters(&mut self) {
        for w in self.watch.values_mut() {
            let seq = w.seq;
            let (old, keep): (Vec<_>, Vec<_>) = w
                .waiters
                .drain(..)
                .partition(|(since, _)| since.elapsed() > Duration::from_secs(25));
            w.waiters = keep;
            for (_, tx) in old {
                let _ = tx.send(Reply::Ok(json!({ "seq": seq, "timeout": true })));
            }
        }
    }
}
