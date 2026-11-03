# Research: Merge conflicts between loops

Decisions for [plan.md](./plan.md), from the code as it is after 010 (`src/run/feature.rs`,
`src/run/machine.rs`, `src/run/engine.rs`, `src/publish/`, `src/verify/git.rs`).

## R-1 Knowing that the target moved, and whether it would conflict, without touching anything

**Decision**: the target *moved* when its tip is not an ancestor of the loop's verified commit
(`!git::is_ancestor(target_tip, integrated)`, FR-001). Whether a merge *would conflict* is read with
`git merge-tree --write-tree --name-only <target_tip> <integrated>` (git 2.38+), which merges in
memory and lists the conflicting files, without a working copy, an index or a ref changing. When the
installed git is older, the same is tried in a detached scratch worktree of Fallow's own
(`merge --no-commit --no-ff`, read the conflicting files, `merge --abort`, remove the worktree).

The check runs on a thread (as 010's remote reads), never on the engine thread: when a loop becomes
ready, when its page is read (at most every 10 s), every 30 s for each ready loop (local git only, no
network), and again inside `machine::merge` right before merging (FR-003's recheck).

**Rationale**: the target and the engineer's folder must never be touched to find out (FR-001,
SC-003); `merge-tree` is exactly an in-memory trial merge. Ancestry, not "a different tip", is the
meaning FR-001 gives to "moved".

**Alternatives**: trying the real merge and aborting (today's `merge_into`) touches the engineer's
folder when the target is checked out there; comparing tips flags a target that is only behind.

## R-2 Bringing a loop up to date: an update in the loop's own working copy

**Decision**: an update merges the target into the loop's branch in the loop's build worktree
(`run.worktrees()/build`), with `git merge --no-ff <target_tip>` and Fallow's commit message
"Fallow: <feature> brought up to date with <target> at <short>". It is recorded as a new event
`Updated { n, target, target_commit, conflicts, docs_changed }` (data-model.md).

- **Without a conflict**: the merge commit is made at once.
- **With a conflict**: the merge is left in progress in the build worktree (conflict markers in the
  files, `MERGE_HEAD` set) and a repair step is queued for the builder on the part `update-<n>`
  (FR-005), its prompt naming each file and what each side changed (the target's commits and the
  loop's, per file: `git log --oneline <merge-base>..<side> -- <file>`). The step ends verified only
  when no unmerged path is left (`diff --name-only --diff-filter=U` empty), no conflict marker line
  (`<<<<<<< `, `=======`, `>>>>>>> `) is in the files that were in conflict, and the merge is
  committed (`MERGE_HEAD` gone; Fallow commits it if the builder left it staged).
- **The feature's own locked documents** (`specs/<feature>/spec.md`, `plan.md`, `tasks.md`, and any
  file the lock's checksums cover) changed on the target since the loop's merge-base: the update is
  not made at all (`docs_changed` is recorded, nothing merged) and the loop waits for the engineer
  with "Lock again" and "Stop" (FR-005). "Lock again" opens the feature's spec session with the
  target's documents (the existing relock path, `engine.relock`, R-3 of 007), whose relock brings
  the target in through `take_lock`.

**Rationale**: the closest precedent is `take_lock` (feature.rs:1025), which already merges into the
build worktree and commits; an update is the same with a real merge (no `-X theirs`) and a builder
for what conflicts. A merge in progress survives a restart on disk (VII).

**Alternatives**: rebasing the loop's branch (rewrites commits already verified and published; a
published branch would need a force: ruled out by II); resolving in a scratch worktree then copying
(two copies of the truth).

## R-3 Verifying again: gates on the new starting point, every story checked again

**Decision**: after the update's merge commit, the loop goes back to building (from Ready), and the
fold of `Updated` does what `Relocked` does for affected stories, for *every* story: each story's
last phase is set back from Done with `reopened = Some(seq)`, which `round_bounds` (feature.rs:531)
already reads as the start of a new checking round, so each checker step runs again at its
checkpoint. The gate baseline moves to the target commit merged in: a `Baseline` gate run is made on
`target_commit` (the `baseline` lookup keyed by commit, state.rs:1095, finds it), and the gates of
the merged result run against it. `finish()` then records the new `Integrated` commit and Ready, as
today.

Counting (FR-007): `Updated` starts a fresh count as `Relocked` does (`fresh_since`, feature.rs:503):
the update's own repairs count on its part `update-<n>` against `caps.task_repairs`, the review rounds
after it against `caps.review_rounds`, both from zero; spending counts against the loop's budget as
any step.

**Rationale**: reuses the relock machinery that already reopens stories and resets counts; no new
scheduling path.

## R-4 Who starts an update

**Decision**: when R-1 finds the target moved for a ready loop:
- level **Free**: the engine queues the update itself (if the loop is not over its budget and its
  overtaken count is below its limit);
- **Normal** and **Strict**: `NeedsEngineer` with the options "bring up to date" and "stop", shown
  under "Needs you" as `update` (dashboard), with what it will do.

A published loop is checked the same way against its remote target (R-7).

## R-5 The merge line

**Decision**: a line per `(repository, target branch)`, kept in the engine and rebuilt on load from
two new events in each loop's log: `Lined { at }` (joined, with the time) and `Unlined { why }`.
Order is by `at`. A loop *holds its turn* when it is first in its line. A loop joins when an update
for its merge starts or its merge is requested, and leaves when it is merged, stopped, deleted, or
waits for anything (the engine appends `Unlined` whenever the loop enters `needs`, a cap, its budget,
Paused, Ready waiting for the engineer's Merge, or a wait for another loop: FR-009a). A loop that
wants its turn while another holds it is shown "in line to merge after <feature>" (its `waiting`
reason, as a slot wait is shown today, machine.rs:166).

A publication's update (R-7) does not join this line: its turn is the time from the start of its
update to its push (FR-007).

**Rationale**: an event per join and leave keeps the line in the loops' own logs (VII) without a new
file; the engine already rebuilds cross-loop state (slots) on each drive.

## R-6 Overtaken updates and their limit

**Decision**: a new lock setting `update_limit` (default 3, at least 1), carried like
`when_verified`: lock form → `Request::Lock` → `LoopCreated.update_limit` → `LoopState` and
`LockRec`. `LoopState.overtaken` counts updates whose result was outrun: at the end of an update (the
loop ready again), if R-1 finds the target moved again while the loop held its turn, `Overtaken`
is appended (local: the line kept other loops out, so the move came from outside Fallow; a
publication: any move of the remote target between the update's start and the push). The count is
reset by `Merged`, `Published`, or the engineer's "Verify again on the latest <target>" (a
`Steering` of kind `Update`). At the limit the loop asks whatever its level, with "Verify again on the
latest <target>", "Merge without the latest check" (only when R-1 finds no conflict) or, for a
publication, "Publish without the latest check" / "Leave the pull request as it is", and "Stop"
(except FR-016b).

"Merge/Publish without the latest check" records `Merged { how, unchecked: [commits] }` /
`Published { unchecked }`: the target commits the result was not verified with. The loop's page, its
history, the notification and (for a publication) the pull request's text say so, never as verified
(constitution I).

## R-7 Pull requests: reading behind/conflict, updating, pushing forward

**Decision**:
- **Reading**: `gh pr view --json state,mergedAt,url,statusCheckRollup,mergeable,mergeStateStatus`
  (one call, as today). `mergeable: CONFLICTING` → conflicts; `mergeStateStatus: BEHIND` → behind.
  Whether the target requires branches to be up to date is read with the protection (010 R-2): the
  classic protection's `required_status_checks.strict`, or a ruleset rule `required_status_checks`
  with `strict_required_status_checks_policy`. Unreadable → "behind" without urging (spec).
- **Updating**: `git fetch <remote> <target>` (the engineer's git, 010 R-11's runner, network allowed
  by V for publishing), then the update of R-2 against the fetched commit (`FETCH_HEAD` resolved to a
  sha), in the build worktree.
- **Pushing forward** (FR-012): a new `push::push_forward(repo, remote, branch, from, to)`: refuses
  unless `ls-remote` shows `branch` at exactly `from` (the last commit Fallow pushed, recorded in
  `Publication.commit`) and `to` descends from `from`; then a plain `git push <remote>
  <to>:refs/heads/<branch>` (never `+`, never a force flag: the remote itself refuses a non-forward
  move if someone pushed meanwhile). Never a fallback to `-2`; the existing `push` keeps its rule for
  first publications.
- **Before any push** (FR-011a): `git fetch <remote> <target>` and R-1's ancestry check against it;
  moved → no push, the loop goes to R-4.
- **Stage**: a published loop's update runs while the stage stays `Publishing`; `machine::drive`
  lets `feature::drive` run in Publishing when an update is in progress (`l.update.is_some()`), and
  the pull request keeps being followed; a merge or close seen meanwhile stops the update's running
  steps (as `stop` does for steps) and drops it (`UpdateDropped`).

## R-8 Overlap warnings

**Decision**:
- **What a loop touches**: `LoopState.touched`, the set of files changed by its verified steps
  (each step's verification already lists its files), plus, at the lock, the paths its tasks name.
  Paths in task text are read with the helper `relock.rs::paths_in` (moved to `src/speckit/tasks.rs`
  and made public).
- **At the lock**: a new read `GET /api/projects/<id>/features/<f>/overlap` returns, for the tasks of
  the feature as it would be locked, the unfinished loops (FR-016a: every stage but done and stopped)
  whose `touched` or task paths overlap, with the files. The lock form shows it before "Lock", with
  "Run after <feature>" (the lock then carries `run_after: <run id>`) and "Run now anyway".
- **While running**: after each verified step, the engine intersects the loop's `touched` with every
  other unfinished loop's of the same project; a new overlap appends `Overlap { with, files }` to both
  logs (once per pair and file set) and shows on both pages with "Wait for <feature>" (FR-015;
  not for a loop whose pull request is open, FR-016b).
- **Waiting**: `LoopState.waits_for: Option<run id>` (from `run_after` or "Wait for"); the loop is
  driven as today but starts no step while the other loop is unfinished (shown "waiting for
  <feature>"); "Wait for" lets the current step finish first. The wait ends when the other loop is
  merged, stopped or deleted (`WaitEnded`). A wait that would close a cycle (following `waits_for`
  from the other loop reaches this one) is refused (FR-016).
- **A "Run after" loop that built nothing**: when its wait ends and the target moved since its lock,
  the target is merged into its branch without gates or checker (R-2's merge, no re-verification),
  locked documents excepted (R-2).

## R-9 Restart

**Decision**: every new state is folded from events (`Updated`, `UpdateDropped`, `Lined`,
`Unlined`, `Overtaken`, `Overlap`, `WaitsFor`, `WaitEnded`); a merge in progress in the build
worktree is found on load (`MERGE_HEAD` present) and its repair step re-queued by the existing
`requeue_cut_short`. The line is rebuilt from `Lined`/`Unlined` (VII).

## R-10 Tests without a network or a model

**Decision**: the fake runtime plays the builder's conflict repair (a scenario step that writes the
resolved file and commits); two features in one project need `lock_calc_in` generalized to a feature
name and a second set of Spec Kit documents (`CALC2_*`). The fake host gains `mergeable` and
`mergeStateStatus` in its state file, and protection with `strict`. A bare remote (010) lets tests move
the remote target. New test files `us11_update.rs`, `us11_line.rs`, `us11_publish_update.rs`,
`us11_overlap.rs`; the CI gate (`no_egress_no_push.rs`) gains the second push site
(`push_forward`) as allowed and still forbids `+` and force flags.
