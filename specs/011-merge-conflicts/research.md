# Research: Merge conflicts between loops

Decisions for [plan.md](./plan.md), from the code as it is after 010 (`src/run/feature.rs`,
`src/run/state.rs`, `src/run/machine.rs`, `src/run/engine.rs`, `src/publish/`, `src/verify/git.rs`).
Revised after the plan review of 2026-11-03.

## R-1 Knowing that the target moved, and whether it would conflict, without touching anything

**Decision**: the target *moved* when its tip is not an ancestor of the loop's verified commit
(`!git::is_ancestor(target_tip, integrated)`, FR-001). Whether a merge *would conflict* is read with
`git merge-tree --write-tree --name-only --no-messages <integrated> <target_tip>` (git 2.38+): exit 0
means a clean merge, exit 1 a conflict (not an error: the helper reads the exit code itself); stdout
is the resulting tree's id on the first line, then the conflicting paths, one per line. Nothing in a
working copy, an index or a ref changes. With an older git, the *check* is tried in a detached scratch
worktree of Fallow's own (`merge --no-commit --no-ff`, read the unmerged paths, `merge --abort`,
remove it); an *update* (R-2) needs git 2.38 and is refused with an older one, the loop page naming
the version needed (this machine has 2.45). Results are cached per `(target_tip, integrated)`, so the 30 s watch costs nothing while
neither moves.

The check runs on a thread (as 010's remote reads), never on the engine thread: when a loop becomes
ready, when its page is read (at most every 10 s), every 30 s for each ready loop (local git only),
and again inside `machine::merge` right before merging (FR-003).

**Where the target's new commits came from** (FR-002): each commit in `integrated..target_tip` is
matched against the commits other loops of the project recorded in `Merged`/`Published` (and their
`Integrated` commits): a match names that feature; anything else is "outside Fallow".

## R-2 An update: the merge computed by git, committed by Fallow

**Decision**: an update never runs `git merge` in a working copy. Fallow computes it with
`git merge-tree --write-tree <loop_head> <target_commit>`, which yields a tree; when there are
conflicts, that tree holds the conflicted files with conflict markers in them.

- **Without a conflict**: Fallow makes the merge commit itself:
  `git commit-tree <tree> -p <loop_head> -p <target_commit> -m "Fallow: <feature> brought up to date
  with <target> at <short>"`, and moves the loop's branch to it in its build worktree (a fast-forward
  of the loop's own branch: `git merge --ff-only <commit>` there). Recorded as `UpdateMerged`.
- **With a conflict**: Fallow reads the conflict entries with their stages and the informational
  messages (a second `merge-tree --write-tree` without `--name-only`), so it knows each path's kind:
  *text* (markers in the file) or *non-text* (modify/delete, binary, rename/rename, file/directory,
  where git keeps one side or adds `<path>~<side>` files). It commits the conflicted tree as a
  *working commit*, a single-parent commit on the loop's head kept under a Fallow-private ref
  (`refs/fallow/update/<run>-<n>`), never on the loop's branch, and records `UpdateConflicted`.
  The repair step for the builder runs in the build worktree **detached at the working commit**
  (builder steps run in `build_wt`, whose `base` is its head), on the part `update-<n>`, with a
  plain-text prompt naming each path, its kind, and what each side changed
  (`git log --oneline <merge-base>..<side> -- <path>`), and the instruction of FR-005. For this part,
  `work_done`'s "on the build branch" check is replaced by "descends from the working commit";
  `take_answers` does not run for it (answers are merged after the update, as usual). The result must
  report, for every non-text path, how it was resolved (a `resolutions` list in the step's structured
  result, part of the runtime contract, IX); the loop page shows them. At the step's
  end the existing tidy runs against that base (so the target's incoming changes, protected paths
  included, are the base, not the builder's edits: `restore_paths` and `locked_changes` judge only
  what the builder changed). Fallow then checks the result:
  - no line starting with `<<<<<<< ` or `>>>>>>> ` in the text paths that were in conflict (the
    separator `=======` alone is not checked: it is also a Markdown underline);
  - no `<path>~<side>` file left, and every non-text path reported in `resolutions`;
  - other files may change too, as in any repair, under the same protected-path rules;
  - then Fallow makes the real merge commit with the resolved tree:
    `commit-tree <resolved_tree> -p <loop_head> -p <target_commit>`, checks the build worktree out on
    the loop's branch again (still at `loop_head`) and fast-forwards it to the merge commit, and
    records `UpdateMerged`. Whatever the builder did with commits (its own, none, a resolution equal to
    one side) does not matter: Fallow takes the tree.
- **The feature's own locked documents** (its `spec.md`, `plan.md`, `tasks.md` and the other files
  of its spec directory): a path counts as changed when the target changed it since the point last
  reconciled (`merge-base(loop_head, target_commit)`, or, after a "Lock again", the target commit that
  lock was reconciled against, recorded on the lock as `reconciled_target`) **and** its content
  there differs from the lock's. A target that never had the feature's directory, or gained the same
  documents, does not count. If any path counts, the update is not made (`UpdateDocsChanged`, nothing
  merged) and the loop waits for the engineer with "Lock again" and "Stop" (FR-005). Other protected paths (the
  constitution, `.fallow/`) are merged in like any change: they are not this feature's locked
  documents, the builder never edits them (the tidy restores the base's version), and the loop page
  notes that they changed.
- **"Lock again"** opens the feature's spec session with a message from Fallow showing the target's
  version of the changed documents against the locked ones; the engineer and the lead bring them in
  and lock again (the existing relock, `engine.relock`). The new lock records the target commit it was
  reconciled against (`reconciled_target`), so a change the engineer chose not to take is not flagged
  again; only later target changes are.

**Rationale**: the plan review showed that a merge left in progress in the build worktree is undone
by the step's tidy (`own_history` resets to the base, losing `MERGE_HEAD`), that protected paths
among the incoming changes would be reverted, and that a resolution equal to one side commits
nothing. Computing the merge with `merge-tree` and committing with `commit-tree` avoids all three:
there is never a merge in progress, the builder works on an ordinary commit, and Fallow alone makes
the two-parent commit. Rebasing is ruled out (it rewrites verified, possibly published commits).

## R-3 Verifying again: gates on the new starting point, every story checked again

**Decision**: the fold of `UpdateMerged` (state.rs) does, for every story, only this: its phases go
back to `Pending` with `reopened = Some(seq)`, and `outcome` is cleared. It does **not** touch
`affected` (which would queue the relock's builder repairs, feature.rs ~1366). A new
`LoopState.fresh_seq` replaces the direct reads of `relocked_seq` in `repairs_of`, `allowed_more`
and `fresh_since`; both `Relocked` and `UpdateMerged` set it, so the update's repairs and the review
rounds after it count from zero (FR-007). `round_bounds` (feature.rs ~531) already reads `reopened`
as the start of a new round, so each checker runs again at its checkpoint; tasks already accepted stay
accepted, so no builder step runs without a conflict (US2).

Gates: `LoopState.base_commit` (the target commit last merged in; the lock commit before any update)
replaces `lock_commit` where `step()` decides whether a Baseline run is needed (feature.rs ~330), and
`RunState::baseline` is looked up by commit (`base_commit`), not "the newest Baseline per gate".
Accepted tasks are not gated again, so the merged result gets its own gate run: right after
`UpdateMerged`, `step()` runs the Part gates on the merge commit for the part `update-<n>` (as
`gated()` does for a task), against the baseline at `base_commit`; a failure becomes a builder repair
on `update-<n>` (counted as R-3 says), and only once they pass are the stories checked again. The
merge commit is then a gated commit, so `finish()`'s "changed after the last gate run" check finds
nothing ungated.

## R-4 Who starts an update

**Decision**: when R-1 finds the target moved for a ready loop: level **Free** queues the update
itself (unless over budget or at its limit, R-6); **Normal** and **Strict** append `NeedsEngineer`
with "bring up to date" and "stop" (never "stop" while a pull request is open, FR-016b), shown under
"Needs you" as `update`.

## R-5 The merge line

**Decision**: a line per `(repository, target branch)`, kept in the engine and rebuilt on load from
`Lined { at }` and `Unlined { why }` in each loop's log; ordered by `at`, then run id. `at` is the time
the loop became ready with its current result. A loop joins when it becomes ready and wants its turn
(an update for its merge is starting, or the engineer asked to merge), and leaves when merged,
stopped, deleted, or waiting for anything (needs, a cap, its budget, Paused, Ready waiting for the
engineer's Merge, a wait for another loop: FR-009a). It *holds its turn* when it is first.

A merge requested while another loop holds the turn is accepted and queued: the reply and the page
say "in line to merge after <feature>", and the merge runs (with FR-003's recheck) when the turn comes
(the click was the engineer's decision; it stays recorded as a steering action).

A publication's update (R-7) does not join this line: its turn is the time from the start of its
update to its push (FR-007).

## R-6 Overtaken updates and their limit

**Decision**: `update_limit` (default 3, 1 to 20) is a lock setting, carried like `when_verified`:
lock form → `Request::Lock` → `LoopCreated.update_limit` → `LoopState` and `LockRec`.
`LoopState.overtaken` counts updates outrun: when the loop is ready again after an update and R-1
finds the target moved again *while it held its turn*, `Overtaken` is appended (locally the line kept
other loops out, so the move came from outside Fallow; for a publication, any move of the remote
target between the update's start and the push). Reset by `Merged`, `Published`,
`PushedForward` and the engineer's "Verify again on the latest <target>" (`Steering::Update`).

At the limit the loop asks whatever its level, with "Verify again on the latest <target>", "Merge
without the latest check" (only when R-1 finds no conflict), for a publication "Publish without the
latest check" (first publication) or "Leave the pull request as it is" (open one), and "Stop"
(except FR-016b). The chosen way is recorded with the commits not verified with:
`Merged { unchecked }`, `Published { unchecked }`, `LeftPullRequest { unchecked }`; the loop page, the
history, the notification and the pull request's text say so, never as verified (I).

## R-7 Pull requests: reading, fetching, updating, pushing forward

**Decision**:
- **Reading**: `gh pr view --json state,mergedAt,url,statusCheckRollup,mergeable,mergeStateStatus`
  (still one call). `mergeable: CONFLICTING` → conflicts; `mergeStateStatus: BEHIND` → behind. The
  target requiring branches to be up to date comes with the protection read (010 R-2): classic
  protection's `required_status_checks.strict`, or a ruleset `required_status_checks` rule with
  `strict_required_status_checks_policy`; unreadable → shown as behind, not urged.
- **Fetching the target** without writing the engineer's refs: `git ls-remote <remote>
  refs/heads/<target>` gives the sha; `git fetch --no-write-fetch-head <remote> <sha>` brings its
  objects (fetching by sha updates no remote-tracking ref; only objects land in the repository's
  shared object store, as every worktree's commits already do). Concurrent loops never share a
  `FETCH_HEAD`. 010's runner and its no-prompt environment are used.
- **Updating**: R-2 against that sha, in the loop's own worktree, as for a local target.
- **Pushing forward** (FR-012): `push::push_forward(repo, remote, branch, from, to, target)`, built on
  the existing `refspec()` (so the target refusal holds): if `ls-remote` shows the branch already at
  `to`, it is done; it refuses unless the branch is at exactly `from` (the last commit Fallow pushed)
  and `to` descends from `from`; then a plain `git push <remote> <to>:refs/heads/<branch>` (no `+`,
  no force flag; the remote itself refuses a non-forward move if someone pushed meanwhile). Recorded
  as `PushedForward { from, to }`, which updates `Publication.commit`. Never a fallback to `-2`.
- **Before any push** (FR-011a), including 010's first publication: R-1 against the fetched remote
  target; moved → no push, the loop goes to R-4. 010's tests are audited for remotes whose target is
  ahead of the local one.
- **The stage**: a published loop's update runs while the stage stays `Publishing`:
  - `finish()` in an update of a published loop does not append `Stage Ready`: it appends
    `Integrated` and hands over to `push_forward` (a `Done::PushForward` like `Done::Publish`);
  - `feature::drive` keeps driving while an update is in progress in `Publishing`, as in `Building`;
  - limits and caps in that update become `NeedsEngineer` without a stage change (no
    `pause_for_limit`), and their options never include "stop" (FR-016b);
  - the pull request keeps being polled; a merge or close seen meanwhile stops the update's running
    steps and drops it (`UpdateDropped`), deleting its working ref; the loop then follows 010.

**Constitution V**: V lets the engineer's git and remote tools run "to publish a run (push its
branch, open its pull request, read branch rules and check results)". Fetching the target commit is
part of publishing the run: the pull request cannot be updated, nor the push checked (FR-011a),
without the target's commit; it reads only what 010's `ls-remote` already reads, plus its objects.
This is argued here; if the owner prefers it named, it is a PATCH clarification of V.

## R-8 Overlap warnings

**Decision**:
- **What a loop touches**: when a step's result is accepted, `work_done` already has the list of
  files it changed (from the step's base); processing appends `Touched { files }` from that list, with
  no extra git call, folded into `LoopState.touched`. For an update's repair, the base is the working
  commit, so the target's files are not counted as the loop's. At the lock,
  the paths its tasks name come from `paths_in` (moved from `run/relock.rs` to `src/speckit/tasks.rs`,
  public) into `LoopState.task_paths`.
- **At the lock**: `GET /api/projects/<id>/features/<f>/overlap` compares the feature's task paths
  with each unfinished loop's (FR-016a) `touched` and `task_paths`; the lock form shows the overlaps
  with "Run after <feature>" (the lock carries `run_after`) and "Run now anyway".
- **While running**: after each `Touched`, the engine intersects it with every other unfinished loop
  of the project; a new overlap appends `Overlap { with, files }` to both logs, shown on both pages
  with "Wait for <feature>" (FR-015; not for a loop whose pull request is open, FR-016b).
- **Waiting**: `WaitsFor { run, from }` / `WaitEnded`; a waiting loop starts no new step ("waiting for
  <feature>"); "Wait for" lets the current step finish. A wait that would close a cycle (following
  `waits_for` from the other loop reaches this one) is refused, naming the loop already waiting.
- **A "Run after" loop that built nothing**, when its wait ends and the target moved since its lock:
  R-2's merge without a conflict is committed straight onto its branch (no gates or checker: nothing
  is built); a conflict there can only be in files the lock brought, so it is handled as
  `docs_changed` (the engineer locks again).

## R-9 Restart

**Decision**: every new state is folded from events. `Updated` (the target commit only) is written
before the merge is computed; `UpdateConflicted` or `UpdateDocsChanged` after. On load, an update in
progress is resumed from what exists, keyed on the working ref, not on commit ids (commits made again
get new timestamps): `Updated` alone → computed again (the same inputs give the same tree);
`UpdateConflicted` and its ref, a repair step cut short → the build worktree detached at the ref
again and the step re-queued by `requeue_cut_short`; a repair accepted but no `UpdateMerged` → Fallow
makes the merge commit from the accepted tree; `UpdateMerged` with the build worktree still detached →
checked out on the loop's branch again. A forward push whose `PushedForward` was not
written is found by `push_forward` itself (the remote already at `to` means done). The line is
rebuilt from `Lined`/`Unlined`.

## R-10 Tests without a network or a model

**Decision**: the fake runtime plays the conflict repair (a scenario step that writes the resolved
file; with and without its own commit; one resolving to one side). Two features in one project need
`lock_calc_in` generalized to a feature name and a second calc feature's documents. The fake host
gains `mergeable`, `mergeStateStatus` and protection `strict`; the bare remote of 010 lets tests move
the remote target and push a foreign commit to a loop's branch. New `tests/us11_update.rs`,
`us11_line.rs`, `us11_publish_update.rs`, `us11_overlap.rs`; unit tests for `merge-tree` parsing
(exit codes, paths), the marker check, `push_forward`'s refusals, the cycle check, `paths_in`.
`no_egress_no_push.rs`: `push_forward` in `publish/push.rs` is the second allowed push site; `+` and
force flags stay forbidden.
