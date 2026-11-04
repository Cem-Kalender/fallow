# Tasks: Merge conflicts between loops

**Input**: Design documents from `specs/011-merge-conflicts/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/web-api.md, quickstart.md

**Tests**: requested, as in 007 to 010. Everything is tested without network or a model: the fake
runtime plays the builders and checkers (including conflict repairs), a local bare repository behind
a `github.com` address stands in for the remote, and `fallow-fake-host` for `gh`. The page is
checked with headless Chrome over CDP on a demo service.

**Organization**: one phase per user story, in the spec's order:
- US1, a conflicting loop is brought up to date and verified again (P1)
- US2, a target that moved without conflict, the merge line and the limit (P1)
- US3, the autonomy level decides who starts it (P2)
- US4, a pull request that fell behind or conflicts (P2)
- US5, a warning before two loops collide (P2)

After every phase, an independent fresh-context reviewer verifies it, and the implementer fixes
the confirmed findings before the next phase. That is done outside this list.

**Build rules for this machine**:
- Debug builds only.
- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, `-j 2`.
- Tests with `--test-threads=2` and `CARGO_TARGET_DIR=target/wip`, so the engineer's running
  Fallow is not locked. Run test files one at a time when memory is short.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1–US5)

---

## Phase 1: Setup

**Purpose**: the test doubles and fixtures every story needs.

- [X] T001 In `tests/common/mod.rs`: add `lock_feature_in(tx, project, repo, feature, pipeline, budget, level)` that locks a feature of a project already added (no `AddProject`; `lock_calc_in` keeps adding the project and calls it, so 007–010 tests are unchanged); add a second Spec Kit feature `002-calc2` (`CALC2_SPEC/PLAN/TASKS`, one story, its tasks naming `calc/__init__.py`, the file `001-calc`'s tasks name) and `lead_writes_calc2()`; add `commit_on(repo, branch, path, text, msg)` to make a commit on the target from "outside Fallow".
- [X] T002 In `src/bin/fallow-fake-agent.rs`: a scenario step may carry `delete = [path]` (files removed before the step's commit), beside the existing `files = [{ path, text }]`; document it in the file's header (a step's `resolutions` need no new key: they go in its free-form `output`, the structured result). Then, in `tests/common/mod.rs` (after T001), scenario helpers: `conflict_repair_step(file, text, commit: bool)` (part `update-1`), `nontext_repair_step(file, delete: bool, resolutions)` (writing `resolutions` into `output`), `leave_side_file_step(path)`.
- [X] T003 [P] In `src/bin/fallow-fake-host.rs`: the PR's `mergeable` (`MERGEABLE|CONFLICTING|UNKNOWN`) and `mergeStateStatus` (`CLEAN|BEHIND|DIRTY|BLOCKED`) in the state file, returned by `pr view --json …,mergeable,mergeStateStatus`; the protection's `required_status_checks.strict` and a ruleset `required_status_checks` rule with `strict_required_status_checks_policy`. Unit test of the new fields.

**Checkpoint**: two features can be locked in one project in a test; the fake agent returns `resolutions`; the fake host answers the new fields.

---

## Phase 2: Foundational

**Purpose**: git helpers, records and the runtime contract. Blocks all stories.

- [X] T004 In `src/verify/git.rs` (research R-1, R-2): `merge_tree(dir, ours, theirs) -> Result<MergeTree { tree, clean: bool, conflicts: Vec<Conflict { path, kind: Text|NonText }> }, String>` running `merge-tree --write-tree` (exit 0 clean, exit 1 conflicts, anything else Err; first stdout line the tree; a second run without `--name-only`/`--no-messages` to classify paths by their stages and messages: markers in the file → `Text`, modify/delete, binary, rename, file/directory → `NonText`); `git_version(dir) -> (u32, u32)` and `merge_tree` refusing below 2.38 with "updates need git 2.38 or newer (found x.y)"; `would_conflict(dir, integrated, target_tip)` using `merge_tree` with the scratch-worktree fallback for older git (`merge --no-commit --no-ff` in a detached worktree under the run's worktrees, unmerged paths, `merge --abort`, remove); `commit_tree(dir, tree, parents: &[&str], msg) -> sha`; `tree_with_paths_from(dir, tree, from_commit, paths) -> tree` (temporary index via `GIT_INDEX_FILE`: `read-tree <tree>`, `read-tree --prefix` / `checkout-index` of those paths from `from_commit`, `write-tree`); `marker_lines(dir, commit, paths) -> Vec<(path, line)>` (only lines starting `<<<<<<< ` or `>>>>>>> `); `side_files(dir, commit, paths) -> Vec<path>` (`<path>~<anything>`); add `create_new_branch(dir, name, at)` beside the existing `git::create_branch` (`src/verify/git.rs` ~260, which already takes a commit and returns Ok when the name is taken: its callers rely on that, so it stays as it is), refusing a taken name so the caller tries `-2`, `-3`…; reuse `git::delete_branch` (~290) to remove it; `log_paths(dir, from, to, path) -> Vec<String>` (`log --oneline from..to -- path`, for the repair prompt). Unit tests in a temporary repository: a clean merge, a text conflict, a modify/delete conflict, the tree holding markers usable by `commit_tree`, a Markdown `=======` underline not flagged, the version gate.
- [X] T005 Records in `src/run/events.rs` and `src/run/state.rs`, exactly as data-model.md (all `#[serde(default)]`, so old logs load): events `Updated { n, target, target_commit, loop_head, remote }`, `UpdateConflicted { n, working, branch, conflicts: [{ path, kind }] }` (`branch` the working branch's actual name), `UpdateDocsChanged { n, paths }`, `UpdateMerged { n, commit }`, `UpdateDropped { n, why }`, `PushedForward { from, to }`, `LeftPullRequest { unchecked }`, `Touched { files }`, `Overtaken { n, target_commit }`, `Lined { at }`, `Unlined { why }`, `Overlap { with, files }`, `WaitsFor { run, from: lock|engineer }`, `WaitEnded { run, why }`; `Merged.unchecked`, `Published.unchecked`; `LoopCreated.update_limit` ("default 3, at least 1", 1 to 20) and `run_after`; `Steering` kinds `Update`, `MergeUnchecked`, `PublishUnchecked`, `LeavePullRequest`, `WaitFor`, `StopWaiting`. `LoopState`: `update: Option<UpdateState { n, target_commit, loop_head, conflicts, remote, working, branch, merged, started_seq }>`, `fresh_seq`, `updates`, `update_limit`, `overtaken` (reset by `Merged`, `Published`, `PushedForward`, `Steering::Update`), `base_commit` (the lock commit until an `UpdateMerged` sets the target commit), `lined_at`, `touched`, `task_paths`, `overlaps`, `waits_for`. `Relocked` gains `reconciled_target` (optional), folded into `LoopState.reconciled_target`; `LoopState.task_paths` is filled from the locked tasks at `LoopCreated` and `Relocked` (with `paths_in`, T007). Folds: `Updated` sets `fresh_seq`, the update in progress, and clears `outcome` (so `step()` drives the update: it returns early while `outcome` is set, feature.rs ~219); `UpdateMerged` sets every story's phases to `Pending` with `reopened = Some(seq)`, clears `outcome`, leaves `affected` untouched, sets `merged`, `base_commit`; `Relocked` also sets `fresh_seq`. Each new event gets its history label (`events.rs` label, `src/run/activity.rs`): which update, what conflicted, who resolved it, how it ended (FR-008). Unit tests for each fold, and that a log from 010 folds unchanged.
- [X] T006 In `src/run/feature.rs`: `repairs_of`, `allowed_more` and `fresh_since` read `fresh_seq` instead of `relocked_seq` (research R-3); `step()`'s Baseline decision uses `base_commit` instead of `lock_commit`; in `src/run/state.rs` `RunState::baseline(gate)` takes the commit (`base_commit`) and returns the Baseline run on it. Every 007–010 test stays green (relock behaviour unchanged).
- [X] T007 [P] Move `paths_in` from `src/run/relock.rs` to `src/speckit/tasks.rs` (public), with its unit tests; `relock.rs` uses it from there.
- [X] T008 [P] In `src/agent/schemas.rs`: the step result's optional `resolutions: [{ path, how }]` (plain JSON, ignored when absent); document it in `specs/007-fallow-studio/contracts/runtime.md` as part of the contract every runtime implements (constitution IX). Unit test: parsed with and without it.
- [X] T009 Lock fields (after T005): `src/project/mod.rs` `LockRec` gains `update_limit`, `run_after`, `reconciled_target` (optional); `Request::Lock` in `src/run/engine.rs` gains `update_limit` (validated 1 to 20, default 3) and `run_after`; `src/service/http.rs` maps them; `engine.lock` writes them to `LoopCreated` and `LockRec`. Every `Request::Lock` in tests and src gets `update_limit: None, run_after: None`; `lock_feature_in` (T001) gains an `extra` argument setting them.

- [X] T010 Switches (research R-8b, FR-016c): in `src/config/squad.rs`, add `updates` to `KNOWN` and an `[updates]` table to `Raw`/`Squad` with `auto`, `line`, `overlap` (booleans, default `true`), with a parse test; `merge::parse` warns only on unknown top-level keys, so `Squad::parse` checks the `[updates]` keys by hand and warns "unknown key updates.<k>" (without `deny_unknown_fields`, which would drop the whole squad file); `LoopCreated` gains `switches { auto, line, overlap }` (`#[serde(default)]`, all true), written by `engine.lock` from the project's squad file, so a recorded run says which parts were off. Their behaviour is built and tested with each part: `auto` in T025 (and its test in T024), `line` in T021 (test in T020), `overlap` in T033 (test in T032).

**Checkpoint**: the helpers pass their unit tests; old logs load; the relock tests still pass with `fresh_seq`.

---

## Phase 3: User Story 1 - A conflicting loop is brought up to date and verified again (Priority: P1) 🎯 MVP

**Goal**: a ready loop whose target conflicts offers "Bring up to date"; the conflict goes to a builder; gates and checker run again; the new verified commit merges cleanly.

**Independent Test**: quickstart Q1.

- [X] T011 [US1] Write `tests/us11_update.rs` (failing first): two features changing the same line of `calc/__init__.py`; merge the first; the second's loop view shows `target.moved`, `target.commits` with `from: "001-calc"` for the first loop's merge and `from: null` ("outside Fallow") for a commit made with `commit_on` (FR-002), `conflicts`, `offer` without `merge`; `Request::Update`; a repair step on part `update-1` whose prompt names the file; the build worktree detached at the working commit meanwhile; after it, every gate the project declares runs on the merge commit, both stories checked again (a review step per story after the update), Ready with a new `integrated` whose parents are the old head and the target commit; merge succeeds; the target never had markers; the engineer's folder unchanged (FR-004, SC-003). Variants: the builder commits itself; resolves to one side; a modify/delete conflict with `resolutions` shown on the loop view; a leftover `~side` file refused; a marker left refused; the repair cap reached → needs-you, nothing merged (FR-007); a change on the target to `specs/002-calc2/spec.md` → `UpdateDocsChanged`, offers `lock_again` and `stop`, nothing merged; a target change to `.specify/memory/constitution.md` merges in and is noted; the constitution changed both by the feature's lock and on the target so that they conflict → `UpdateDocsChanged` naming it, no builder step; while the update runs, the loop is not ready (stage building) and `Request::Merge` and `Request::Publish` are refused (FR-003a); the target branch deleted → nothing merged, the loop view says the target is gone and offers `stop`, and says that moving the loop to another branch is not possible in this version (stop it and start the feature again from that branch); for a published loop whose remote target is gone, it says the pull request must be closed on the remote first (the hosting service usually closes it when its base branch is deleted), after which 010's closed handling offers `stop`; a drive while the update's merge is still being computed, and a drive after `UpdateDocsChanged`, append no `Integrated` and no `Ready` (the loop stays not ready).
- [X] T012 [US1] Create `src/run/update.rs` (research R-2): `start(run, ctx, target_commit, remote)` appends `Updated` (with `loop_head`) and, for a local loop, `Stage { to: Building }` (FR-003a: the old verified commit is no longer offered; a published loop stays in Publishing, T030), then on the run's thread: the docs check first (the feature's spec directory except `handover/`, and other protected paths (constitution, `.fallow/`) that the loop's branch also changed since the merge-base and that would conflict: changed on the target since `merge-base(loop_head, target)` or `reconciled_target`, and different from `loop_head`'s) → `UpdateDocsChanged`; else `merge_tree`; the spec directory except `handover/` taken from `loop_head` (`tree_with_paths_from`); clean → `commit_tree` with parents `[loop_head, target_commit]`, fast-forward the build branch in `build_wt` (`merge --ff-only`), `UpdateMerged`; conflicts → working commit (`commit_tree` with parent `[loop_head]`) on the loop's own branch `fallow/<feature>/update-<n>`, `UpdateConflicted`, and queue the repair: `queue(run, StepKind::Repair, Some("update-<n>"), None, Some(last phase), builder, None (new session), "update <n>: conflicts", prompts::update_conflicts(..))`. `finish_repair(run)`: after the repair is accepted, check `marker_lines` and `side_files` on the text/non-text paths and that every non-text path has a `resolutions` entry (refusals become a repair round with the reason, FR-005), then `commit_tree(resolved_tree, [loop_head, target_commit])`, check out `build_wt` on the build branch (still at `loop_head`) and fast-forward it, delete the working branch, `UpdateMerged`. Results come back as `Msg::Done { done: Done::Update(..) }`.
- [X] T013 [US1] In `src/agent/prompts.rs`: `update_conflicts(feature, target, conflicts with kinds, per-path "what each side changed" (git log lines))`, plain text: "resolve these conflicts: this feature's criteria and the work already on <target> must both keep working"; for non-text paths, ask for `resolutions`. Unit test of the text.
- [X] T014 [US1] In `src/run/feature.rs`: while `l.update` is set and not merged (including after `UpdateDocsChanged`, a repair cap reached, or a gone target, until the engineer decides or the update is dropped), `step()` only runs the queued update repair or returns: it never reaches the phases or `finish()`, so no `Integrated` or `Ready` is appended on the old commit (FR-003a, constitution I); meanwhile `start()` runs the update repair in `build_wt` detached at the working commit (`git checkout --detach <working>`), `take_answers` is skipped while detached; `work_done`'s `on_branch` check for part `update-<n>` is "descends from the working commit" until `merged`, the usual check after; when the update repair is accepted, call `update::finish_repair`. After `UpdateMerged`, `step()` runs every gate the project declares (as `gated()` runs them) on the merge commit for part `update-<n>` (as `gated()` does for a task) against the baseline at `base_commit`, a failure → `repair_or_ask` on `update-<n>` (same cap), and only when they pass do the reopened stories' checkpoints run. Append `Touched { files }` from `work_done`'s changed list for every accepted step (research R-8).
- [X] T015 [US1] In `src/run/machine.rs`: `merge()` rechecks the target first (a target branch that no longer exists → no merge, said so) (`would_conflict` and ancestry, on the merge thread before merging): moved → no merge, the loop records the result and the page offers per FR-003 (FR-003's recheck); `Done::Merge` errors stay as today only for other failures. `drive` handles `Done::Update`. `resume` (research R-9): an update in progress resumes from what exists (`Updated` alone → if the branch head is a merge of `loop_head` and `target_commit`, record `UpdateMerged`; else compute again from `loop_head`; `UpdateConflicted` with its working branch (the name it records) and the step cut short → detach again and re-queue; repair accepted without `UpdateMerged` → `finish_repair`; `UpdateMerged` with `build_wt` detached → check out the branch; a leftover working branch → delete; `UpdateDocsChanged` → needs-you again).
- [X] T016 [US1] In `src/run/engine.rs`: the target watch (research R-1): a thread per check, for each ready loop every 30 s and on `Request::Loop` (at most every 10 s), computing `moved { target_commit, commits with from (R-1: other loops' Merged/Published/Integrated commits, else outside Fallow), conflicts, docs_changed }`, cached per `(target_tip, integrated)`, back as `Msg::Done`; `Request::Update { id }` (refused when not offered) → `update::start`; `Request::Loop`'s view gains `target` (with `gone: true` when the target branch was deleted or renamed), `update` (its `step` derived from the log: `merging` until `UpdateMerged` or `UpdateConflicted`, `resolving the conflicts` while the repair runs, `gates` while every gate the project declares (as `gated()` runs them) on the merge commit run, `checking stories` while the reopened checkpoints run, `pushing` for a published loop's forward push), `updates`, `update_limit`, `overtaken`, `offer` per contracts/web-api.md ("merge" never offered while it would conflict, FR-003).
- [X] T017 [US1] In `src/service/http.rs`: `POST /api/loops/<id>/update`. In `src/service/web/screens/loop.js`: the target area (moved, the commits and where they came from, conflicts, docs changed, the update's step, `resolutions` shown as the builder's account, not as evidence), the offers with the contract's labels ("Bring up to date", "Lock again", "Stop"); "Lock again" opens the feature's spec session with Fallow's message showing the target's version of the changed documents.
- [X] T018 [US1] "Lock again" (research R-2): `Request::LockAgain { id }` in `src/run/engine.rs` (and `POST /api/loops/<id>/lock-again` in `http.rs`) unlocks the feature and returns Fallow's own message, which the page puts in the feature's spec session's box (the engineer sends it, so the lead's conversation stays the engineer's), with the target's version of the changed documents against the loop's (`git diff loop_head target_commit -- specs/<feature>/`, the handover directory left out); `engine.relock` records on `Relocked` and `LockRec` the target commit that update compared against (`reconciled_target`), so the docs check of T012 starts from it next time; when the relock lands while an update is held, `engine.relock` first appends `UpdateDropped { why: "locked again" }` (clearing `update`, deleting any working branch: Fallow's own scratch, never the engineer's work, so no confirmation), so `step()` takes the new lock and the loop goes on. Test in `tests/us11_update.rs`: after "Lock again" keeping the loop's own version, the loop goes on (takes the new lock, builds what the relock affects), and its next update is not stopped by the same change.
- [X] T019 [US1] Headless check (CDP, as 009/010): a demo with a conflicting second loop; screenshots of the loop page before, during (resolving the conflicts) and after the update, saved in `specs/011-merge-conflicts/checks/`.

**Checkpoint**: Q1 passes; the MVP is usable: conflicts no longer dead-end.

---

## Phase 4: User Story 2 - A moved target, the merge line and the limit (Priority: P1)

**Goal**: a target that moved without conflict is verified again too; merges into one target take turns; overtaken updates count to the lock's limit, then the engineer decides.

**Independent Test**: quickstart Q2 and Q4.

- [X] T020 [US2] Write `tests/us11_line.rs` (failing first): with `[updates] line = false` no line forms, the recheck still refuses a conflicting merge, and every move found at the recheck counts as overtaken; a second loop whose target moved without conflict → no builder step, gates on the merge commit and both stories checked again before `merge` is offered (FR-009); two loops locked Free with `when_verified: wait` (so they merge locally) ready for one target (both merges asked for; a Free loop's own update runs outside the line, FR-009a) → the second shows `line.after` and is verified again only once (SC-007); a merge requested while another holds the turn → reply `in_line_after`, merged after (rechecked); a loop waiting for the engineer leaves the line and the one behind goes on (FR-009a); with `update_limit = 1`, a commit on the target from outside Fallow while the loop holds its turn → `Overtaken`, offers `verify_again`, `merge_unchecked` (only without conflict), `stop`; "Merge without the latest check" → `Merged.unchecked` lists the commit, the loop view and notification label it, never as verified; "Verify again on the latest <target>" resets the count; a restart keeps the line's order and the count (FR-017).
- [X] T021 [US2] In `src/run/engine.rs`: the merge line (research R-5): `lines` rebuilt from `lined_at` on load and on each drive, ordered by `at` then run id; `Lined { at }` only for a merge the engineer asked for (with the update made right before it when the target moved; a Free loop's own update runs outside the line, research R-5), `Unlined { why }` when merged, stopped, deleted, or waiting for anything (needs, cap, budget, Paused, Ready awaiting the engineer's Merge, a wait for another loop); a loop not first in its line waits ("in line to merge after <feature>", via `run.waiting`). `machine::merge` while another holds the turn: accepted, recorded, queued.
- [X] T022 [US2] In `src/run/machine.rs`, `src/run/engine.rs` and `src/run/update.rs`: overtaken updates (research R-6): at the merge's recheck (FR-003), while the loop holds its turn, if the target's tip is no longer the `target_commit` its last update merged in, append `Overtaken` and start another update (the 30 s watch counts nothing); at `overtaken == update_limit`, `NeedsEngineer` with `verify again on the latest <target>`, `merge without the latest check` (only when no conflict), `stop` (except FR-016b). `Request::Merge { id, without_latest_check }` records `Merged { unchecked }` with the target commits the result was not verified with; `Request::Publish` gains the same field here (used in T030); every existing `Request::Merge` and `Request::Publish` in tests and src gets `without_latest_check: false`.
- [X] T023 [US2] Page: `loop.js` shows the line ("in line to merge after <feature>"), the overtaken count, the choices "Verify again on the latest <target>" and "Merge without the latest check" with "not checked with: <n> commits" and the list; a merged-unchecked loop never in the colors or words of a verified one. `src/service/notify.rs`: "<feature>: updated <n> times and overtaken; your decision" and "<feature> merged without the latest check (<n> commits)" (project and feature only, V).

**Checkpoint**: Q2 and Q4 pass.

---

## Phase 5: User Story 3 - The autonomy level decides who starts it (Priority: P2)

**Goal**: Free updates on its own within caps and budget; Normal and Strict ask; limits ask whatever the level.

**Independent Test**: quickstart Q3.

- [X] T024 [US3] Extend `tests/us11_update.rs`: with `[updates] auto = false` a Free loop asks as Normal does; the conflict of T011 with the second loop locked Free → no request needed until the merge; with Normal → "Needs you" shows `update` and nothing starts; with Strict → the same; a Free loop over its budget → asks, naming the limit (FR-010); the budget used up during an update (a repair) → the update stops there and the loop asks, nothing merged (FR-007).
- [X] T025 [US3] In `src/run/engine.rs` (research R-4): when the watch finds the target moved for a ready loop: Free → `update::start` unless over budget or at the limit; Normal/Strict → `NeedsEngineer` with "bring up to date" and "stop" (never "stop" while a pull request is open). The same at the merge line's turn (`take_turn`, FR-009a): a Normal or Strict loop whose target moved before its turn asks before the update and leaves the line while it waits (its merge stays asked for, `merge_pending`); a Free loop goes on. In `src/run/dashboard.rs`: needs-you items `update`, `update_limit`, `docs_changed`. `src/service/notify.rs`: "<feature>: <target> moved; bring it up to date".
- [X] T026 [US3] Page: `dashboard.js` shows the new needs-you items with their words.

**Checkpoint**: Q3 passes.

---

## Phase 6: User Story 4 - A pull request that fell behind or conflicts (Priority: P2)

**Goal**: a published loop is updated against the remote target and pushed forward on its own branch, never forced, never to a second pull request.

**Independent Test**: quickstart Q5.

- [X] T027 [US4] Write `tests/us11_publish_update.rs` (failing first), with a bare remote and the fake host: publish a loop; push a conflicting commit to the bare remote's target and set `mergeable: CONFLICTING` → the loop view shows `target.remote`, `pr.conflicting`, offers `bring_up_to_date` (no `stop`); the update fetches the target by sha (no ref of the repository written: `refs/remotes/*` and `FETCH_HEAD` unchanged), resolves, verifies again, and pushes forward: the bare remote's branch moves from the published commit to the new one, `PushedForward` recorded, the PR number unchanged, no `-2` branch, the target unchanged; with a foreign commit pushed to the loop's branch first → nothing pushed, said so, no fallback; behind but not required (`strict: false`) → shown, not urged; required → offered; the PR merged on the remote during an update → the update's steps stopped, `UpdateDropped`, nothing pushed, the loop ends as merged (010); at the limit → `publish_unchecked` for a first publication (blocked by FR-011a) and `leave_pr` for an open one, each recorded with `unchecked`; a first publication when the remote target moved → no push (FR-011a), the update first; the remote target changing the feature's documents while the pull request is open → `lock_again` and `stop` not offered, the close-first message shown; after the pull request is closed on the remote, `lock_again` offered.
- [X] T028 [P] [US4] In `src/publish/host/mod.rs` and `github.rs`: `PrState` gains `mergeable` and `behind` from `pr view --json …,mergeable,mergeStateStatus`; the protection read gains `strict` (classic `required_status_checks.strict`, ruleset `strict_required_status_checks_policy`). Unit tests with sample outputs.
- [X] T029 [P] [US4] In `src/publish/push.rs`: `push_forward(repo, remote, branch, from, to, target)` built on `refspec()` (the target refusal holds): `ls-remote` at `to` → done; not at exactly `from`, or `to` not descending from `from` → Err with the reason, nothing pushed; else plain `git push <remote> <to>:refs/heads/<branch>` (no `+`, no force). In `src/publish/mod.rs`: `fetch_target(repo, remote, target) -> sha` (`ls-remote`, then `fetch --no-write-fetch-head <remote> <sha>`, through `run.rs`). Unit tests of the refusals with a bare repository. Update `tests/no_egress_no_push.rs`: two allowed push sites, both in `publish/push.rs`; `+` refspecs and force flags still forbidden.
- [X] T030 [US4] In `src/run/machine.rs` and `src/run/feature.rs` (research R-7): `publish()` and any push recheck the remote target first (`fetch_target`, ancestry; FR-011a); a published loop's update keeps the stage `Publishing`: `feature::drive` drives it as in `Building`, `finish()` appends `Integrated` and hands over to `push_forward` (`Done::PushForward` → `PushedForward`) instead of `Stage Ready`, limits and caps become `NeedsEngineer` without a stage change and never offer "stop"; the PR poll continues, and a merge or close seen during an update stops the update's running steps, deletes the working branch and appends `UpdateDropped`. Overtaken for a publication: when the recheck before the push finds the remote target moved since the update started, `Overtaken` is appended (any move counts, FR-007), and at the limit the choices of T022 for a publication apply. `Request::Publish`'s `without_latest_check` (the field added in T022) and `Request::LeavePr` (records `LeftPullRequest { unchecked }`); `POST /api/loops/<id>/leave-pr` in `http.rs`.
- [X] T031 [US4] For a published loop, `lock_again` is not offered while its pull request is open (unlock is refused in Publishing, and Stop by FR-016b): after `UpdateDocsChanged` the loop page says to close the pull request on the remote first, after which 010's closed handling brings the loop back to ready; the update dropped with the close, the target watch (T016) reads the target again for the ready loop, finds the documents' change again (`target.docs_changed`), and "Lock again" is offered. Page: `loop.js` shows the pull request's conflict/behind state, "Bring up to date", "Publish without the latest check", "Leave the pull request as it is" with their "not checked with" lines; the pull request's text (`src/publish/body.rs`) says which target commits a result published without the latest check was not verified with. Audit `tests/us10_*` for FR-011a (a remote target ahead of the local one) and adjust only fixtures, never assertions about safety.

**Checkpoint**: Q5 passes; 010's tests still pass.

---

## Phase 7: User Story 5 - A warning before two loops collide (Priority: P2)

**Goal**: overlaps between unfinished loops are named at the lock and while running, with "Run after" and "Wait for".

**Independent Test**: quickstart Q6.

- [X] T032 [US5] Write `tests/us11_overlap.rs` (failing first): with `[updates] overlap = false` no overlap is computed or shown; with `001-calc` running and having changed `calc/__init__.py`, the overlap read for `002-calc2` names the loop and the file; locking with `run_after` → `WaitsFor { from: lock }`, the loop starts no step ("waiting for 001-calc") and starts by itself within 10 s of the other being merged (SC-006; this timing assertion is part of the CI suite and fails the build, constitution VIII), its starting point brought up to the target first without gates when it moved; locking anyway works; no overlap → nothing; while running, two loops changing the same file → `Overlap` on both, `offer` contains `wait_for`; "Wait for" pauses after the current step and resumes when the other is stopped; a wait that would close a cycle is refused naming the waiting loop; `wait_for` not offered while a pull request is open; a restart keeps the waits; the other loop deleted → the wait ends (`WaitEnded`) and the loop starts, saying why.
- [X] T033 [US5] In `src/run/engine.rs` (research R-8): `GET` overlap read (`Request::Overlap { project, feature }`): the feature's task paths (`paths_in` over its tasks) against each unfinished loop's (every stage but done and stopped) `touched` and `task_paths`; after each `Touched`, intersect with the project's other unfinished loops and append `Overlap` once per pair and file set; `Request::Wait { id, for }` with the cycle check and FR-016b, `Request::StopWaiting`; `waits_for` blocks starting steps in `src/run/feature.rs` (shown "waiting for <feature>"); `WaitEnded` when the other loop is merged, stopped or deleted; a "Run after" loop that built nothing, when its wait ends with the target moved: the docs check, then the merge committed straight onto its branch (research R-8). Routes in `http.rs`: `GET /api/projects/<id>/features/<f>/overlap`, `POST`/`DELETE /api/loops/<id>/wait`. `src/run/dashboard.rs`: the `overlap` needs-you item for a new overlap while running.
- [X] T034 [US5] Page: `spec.js`'s lock form reads the overlap before "Lock" and shows the loops and files with "Run after <feature>" and "Run now anyway", and "Updates overtaken before I decide" (default 3) under "When verified"; `loop.js` shows overlaps with "Wait for <feature>", a wait with "Stop waiting"; `dashboard.js` the `overlap` needs-you item. Headless check with screenshots in `specs/011-merge-conflicts/checks/`.

**Checkpoint**: Q6 passes.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T035 [P] Docs: `specs/007-fallow-studio/contracts/storage.md` gains the new events and fields (data-model.md); `specs/004-agent-squads/contracts/squad-file.md` gains `[updates]` (`auto`, `line`, `overlap`); `specs/006-headless-orchestrator/spec.md` notes that FR-026 (bringing a branch up to date) is delivered by 011; `specs/010-publish-remote/spec.md` notes that its FR-012 is amended by 011 FR-012 for moving the loop's own branch forward.
- [X] T036 Restart tests (constitution VII) in `tests/us11_update.rs`: the service stopped and restarted (as 007's restart tests) at each point of an update: after `Updated`, after `UpdateConflicted` with the repair running, after the repair accepted but before the merge commit, after the fast-forward before `UpdateMerged`, during a forward push before `PushedForward`; each resumes without a second merge, a second pull request or a lost resolution; and a restart with a loop waiting for another and two loops in a merge line keeps the wait and the order (FR-017). Done: restarts after `Updated` (the computation made again, one merge), during the update's repair (the repair resumed and accepted, one merge; the restart lands before the builder has written anything, so a half-written resolution is not tested), and with two loops in a merge line (the order kept); the wait across a restart is in `tests/us11_overlap.rs`, the overtaken count in `tests/us11_line.rs`. Not driven by a test: a restart after the repair was accepted but before the merge commit (the repair's commit is kept on the working branch and `update::resolved` makes the merge commit again); a restart between the fast-forward and `UpdateMerged` (handled by `update::already_merged`) and during a push forward (the push forward is checked against the remote before it, and done when the branch is already there).
- [X] T037 Run `cargo clippy --all-targets -j 2 -- -D warnings` and the whole suite (`cargo test -j 2 -- --test-threads=2`, file by file if memory is short; every test runs on the fake runtime, FR-018); fix what fails; re-run a known flaky test alone before calling it a failure, and say so. Done 2026-11-04: clippy with `-D warnings` clean; every test file passes, run file by file (the 011 files split to stay under the time limit). `us8_parallel`'s `checkers_alongside_each_other_overlap_and_are_judged_together` (SC-004's timing) failed once beside another test under load and passed alone: flaky under load, not a regression.
- [ ] T038 Quickstart Q7 by hand with the engineer, on the Studio page: the wording of every new button and notification.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: none. T001 → T002 (both edit `tests/common/mod.rs`); T003 beside them.
- **Foundational (Phase 2)**: T007 first (T005 uses its `paths_in`), then T005; T004 ∥ T007 ∥ T008; T006, T009 and T010 after T005. Blocks every story.
- **US1 (Phase 3)**: needs Phase 2. T011 first (failing), then T012 → T013 → T014 → T015 → T016 → T017 → T018 → T019.
- **US2 (Phase 4)**: needs US1's update (T012, T014, T016). T020 first, then T021 → T022 → T023.
- **US3 (Phase 5)**: needs US1 (T016's watch) and, for `engine.rs` and `notify.rs`, comes after US2 (shared files). T024, then T025 → T026.
- **US4 (Phase 6)**: needs US1 and US2's limit (T022). T027, T028 ∥ T029, then T030 → T031.
- **US5 (Phase 7)**: needs Phase 2 (T007, T009) and T014's `Touched`; its behaviour is independent of US2–US4, but it edits the same files (`engine.rs`, `feature.rs`, `http.rs`), so it runs after them. T032, then T033 → T034.
- **Polish (Phase 8)**: T035 any time after the contracts; T036 after US4; T037 after all; T038 with the engineer, last.

### Within each story

The test file comes first and fails, then the module, then requests and routes, then the page, then the headless check.

### Parallel opportunities

- T003 beside T001 → T002.
- T004 ∥ T007 ∥ T008; T005 after T007; T010 after T005.
- The stories run one after the other (they share `engine.rs`, `feature.rs`, `http.rs`, `notify.rs`); each story's failing test file can be written ahead.
- T028 ∥ T029 (US4: the host adapter and the push are separate files).
- T035 at any time after the contracts are final.

## Parallel Example: User Story 4

```text
Task: "PrState mergeable/behind and protection strict in src/publish/host (T028)"
Task: "push_forward and fetch_target in src/publish/push.rs and mod.rs (T029)"
```

## Implementation Strategy

### MVP first

1. Phases 1 and 2.
2. US1: a conflict is brought up to date by a builder and verified again. Already the main gain: no
   more dead end and no unverified hand-merge.
3. Stop and check Q1; review.

### Then

4. US2: no unchecked combination merged, the merge line, the limit.
5. US3: the levels.
6. US4: pull requests.
7. US5: the warnings.
8. Polish: docs, restart tests, the whole suite, the wording with the engineer.
