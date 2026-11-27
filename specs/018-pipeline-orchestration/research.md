# Research: Pipelines as an orchestration system (018)

Read for this plan: `src/defs/pipeline.rs`, `src/defs/mod.rs`, `src/run/feature.rs` (the loop: `step`, `extra`, `phase`, `checkpoint`, `parallel_units`, `start`, `work_done`, `finish`, `repair_or_ask`, `phase_session`), `src/run/machine.rs` (`slot`, `queued`, `limit_hit`, `run_gates`), `src/run/engine.rs` (`Slots`, `refresh_slots`, the feature view's `pipelines` and `budget_suggestion`), `src/run/state.rs` (`Step`, `LoopState`), `src/run/events.rs`, `src/run/update.rs` (011's `compute`, `resolved`), `src/verify/git.rs` (`worktree_add`, `merge_into`, `merge_tree`, `commit_tree`, `marker_lines`, `files_between`), `src/studio/defs.rs`, `src/run/activity.rs` (the loop view's pipeline), `src/measure/record.rs`, `src/speckit/tasks.rs` (`[P]` is already parsed as `TaskSpec.parallel`), `src/service/web/screens/pipelines.js` (`layout`, shared with the loop page as `Fallow.pipelineLayout`), `spec.js` (the lock form), `assets/pipelines/*.toml`, and the tests that use pipelines (`tests/us2_pipeline_steps.rs`, `us8_parallel.rs`, `us6_defs.rs`, `us2_*`, `visual_fixture.rs`, the 25 test files that write `[[step]]`).

## R-1 The file format: what is added, and nothing taken away

**Decision**:
- `at` becomes allowed on `when = "task"` for every step except the builder and `fallow:gates`. A `when = "task"` maker **without** `at` is the builder; there must be exactly one (as today). A `when = "task"` checker without `at` means `at = "end"` (it had no meaning before: it was refused).
- `[caps] builders = N`, an integer 1..=8, default 1. `Caps` gets the field with `#[serde(default = "one", skip_serializing_if = "is_one")]`, so `Pipeline::to_toml` of a pipeline with one builder writes exactly what 0.4.0 wrote (the loop records that TOML; the designer's `toToml` writes `builders` only when it is not 1).
- `parallel_with` keeps its meaning (checkers at the same point) and now holds at every point.
- Checks removed: "a checker runs at phases and checkpoints, not per task"; "a checker at the start has nothing to check yet" (replaced by: a checker at a start needs `on_fail = "engineer"`). Checks added: `builders` out of range; `at` on the builder or the gates; a maker with `parallel_with` (already refused: "only checkers at the same point run together" stays).

**Rationale**: every 0.4.0 file keeps its meaning without a version field; only refusals are lifted, none added for files 0.4.0 accepted (a maker with `parallel_with` and `at` on a task step were refused before). Keeping `builders` in `[caps]` puts every maximum of a pipeline in one place, the one the designer already shows as "caps".

**Alternatives considered**: a new `points` table or a version field (rejected: every existing file would need converting, and 017 shows the cost of that); `up_to = N` on the builder step (rejected: a step key that only one step may carry needs its own check, and caps already holds maxima); a free graph of nodes and edges (out of scope by the engineer's decision).

## R-2 The points and their order in the runner

**Decision**: one notion of a *point* in `feature.rs`: `OnceStart`, `PhaseStart(n)`, `TaskStart(id)`, `TaskEnd(id)`, `PhaseEnd(n)`, `CheckpointStart(story)` (at the story's first phase's start, as today), `CheckpointEnd(story)`, `OnceEnd`. A new `point()` function runs the steps of one point: it walks them in pipeline order, grouped into units (R-3); a maker is a unit of one. It returns `Ok(true)` when every unit passed or was skipped by its `paths`, otherwise what it queued or asked. `extra()` becomes the body for a unit of one, unchanged; the checkpoint's end keeps its own function (it answers the scenarios) but shares the unit logic. Order per phase, as FR-004: `PhaseStart`, then per task `TaskStart`, the builder, the gates per task, `TaskEnd`; then the gates per phase, the handover, `PhaseEnd`, the checkpoints.

Parts (the key a step's verdict and repairs are counted under):
- a unit of one keeps 0.4.0's part names (`<agent>@once-start`, `<agent>@phase-N-start|end`, the story for checkpoint ends), so a loop recorded in 0.4.0 that stopped in the middle of such a step resumes on the same records (VII);
- task points: `<agent>@<task>-start|end`; a task-end checker's failure is a repair of the task itself (part = task id, `task = Some`), so it counts against the task's repairs and the task is gated again before its checkers look again;
- a unit of two or more at a phase or once point: its members keep their own parts; their joined findings go to one repair whose part is the point (`phase-N-end`, `once-end`, …); each member's round starts at the latest repair of that point.

**Rationale**: the order is the one the runner already follows; making it one function removes `phase()`'s three nearly identical loops and lets units work everywhere.

**Alternatives considered**: a generic step graph interpreter (rejected: a free graph is out of scope, and the fixed order is what `tasks.md` gives).

## R-3 Units of checkers alongside each other at every point

**Decision**: `parallel_units()` is applied to the steps of any point, not only the checkpoint's end. A unit's members are queued together (all at once, after any repair of that point is settled), judged when all have finished, asked again each on its own when incomplete (FR-012 of 007), and their findings go to one repair. This is 008 R-7's logic, lifted out of `checkpoint()` into a helper both use. In `start()`, the "round begun" rule (checkers alongside each other check the same commit) is generalized from "same story" to "same point". `alongside_of()` takes the point.

**Rationale**: one rule, already proven at the checkpoint (`tests/us8_parallel.rs`).

## R-4 Deciding while steps run

**Decision**: today `step()` decides nothing while any step runs (`if !run.running.is_empty() { return Ok(()) }`). With builders over a group, finishing one task must lead to the next task of the group starting while the others still run. The rule becomes: while steps run, `step()` decides only within the **active group** (R-5): starting its next tasks, processing a finished task's verdict, gates, task points and merges of its tasks. Everything outside the group (the next task, the handover, phase points, checkpoints, `finish`) still waits until nothing runs. Answers are still taken into the build branch only when nothing runs (`take_answers`). Units of checkers need no change: all members are queued at once.

The engineer's attention: a question from a group task (FR-018) records `NeedsEngineer` as today, but `machine::drive`'s early return on `needs` lets group decisions through when the waiting question belongs to a group task; the waiting task is not processed or requeued until the answer, and nothing after the group starts. Any other `NeedsEngineer` (a cap, a limit, a failed start) holds the whole loop as today: running steps finish, no new step starts.

**Rationale**: the smallest change that lets builders overlap while keeping every other decision as serial as today, so 0.4.0's behaviour for loops without groups is unchanged.

**Alternatives considered**: deciding everything while steps run (rejected: steps outside a group depend on the build branch, which the group changes).

## R-5 Groups

**Decision**:
- A group is a run of consecutive `TaskSpec`s with `parallel == true` within one phase of the locked `tasks.md`, of at least two tasks, in a pipeline with `builders > 1`. Computed by a pure function `groups(phase, builders) -> Vec<Vec<TaskId>>` in `src/run/groups.rs`, used by the runner, the lock statement (R-11) and the loop view. Anything else runs as today.
- When the runner reaches a group's first unaccepted task, it appends `GroupStarted { phase, tasks, base }` with `base` = the build branch head. From then on each task of the group runs in its own worktree `worktrees/task-<id>` on its own branch `<build_branch>-<id>` created at `base` (`git::worktree_add` with a new branch, as `build`).
- The scheduler keeps at most `builders` tasks of the group between "first step queued" and "accepted"; the global slot limit and memory room (`Slots::take`) still hold queued steps back (FR-013). Each task goes through its own `TaskStart` point, builder, gates (on the task's commit, in a gate worktree as today), `TaskEnd` point, all in its worktree.
- Fallow does not tick `tasks.md` in a group task's commit (R-6); `locked_changes` then expects `tasks.md` unchanged for that task; `work_done` checks the commit is on the task's branch instead of the build branch.
- A builder of a group task starts a new session (never the phase's), with `loop_task(..., fresh = true)` and one added line naming the other tasks of the group as built alongside in other working copies, not to be touched. Its repairs resume that session. `phase_session()` ignores the steps of group tasks.

**Rationale**: a working copy per builder is the only way two makers edit at once without seeing each other's half-done work (constitution VI, III). Starting from one base makes the overlap check exact (R-6). Deciding group membership from `tasks.md` and the pipeline alone, not from free slots, makes the lock statement true and runs reproducible.

**Alternatives considered**: one shared worktree with file locks (rejected: agents do not respect locks; Fallow could not check what each touched); grouping `[P]` tasks across phases (rejected: a phase boundary is a dependency in Spec Kit).

## R-6 Merging a group, overlaps, and the repair on top

**Decision**:
- Merge order is `tasks.md` order. A task is merged when it is accepted (gates passed, `TaskEnd` passed) and every task before it in the group is merged.
- Before merging task T: `mine = files_between(T.base, T.head)`, `theirs = files_between(T.base, build_head)`, both without `<feature>/tasks.md`. If they share no file: in the build worktree, `merge_into(build, T.head, "Fallow: merge T (built alongside)")`, then Fallow ticks T in `tasks.md` and commits the tick (`commit_all` with only `tasks.md`), and appends `TaskMerged { task, commit }`.
- If they share files: `TaskOverlap { task, files, with }` (`with` = the group's merged tasks that changed those files, from their `TaskMerged` ranges). Fallow then builds the working commit as 011 does: `merge_tree(T.head, build_head)`; with conflicts, the tree holds the markers; `commit_tree(tree, [T.head, build_head])`; the task's worktree is checked out at it (`checkout_detach`, then its branch moved there) and its base becomes `build_head`. A repair step of T (part = T, task = T, in T's session) gets a prompt built from 011's `update_conflicts` shape: each shared file, the log of each side, and "both tasks' work must keep working". Its result is checked like 011's update repair: no conflict markers in the shared files (`git::marker_lines`), no side files (`git::side_files`), then the usual maker checks; then gates, `TaskEnd`, and the merge check again against the new base.
- The overlap repair counts against the task's repairs (`repairs_of(T)`), cap `task_repairs`.
- When every task of the group is merged: the gates run on the build head under part `group-<first>-<last>` (`gated()`); a failure is `repair_or_ask(part, None, phase, task_repairs, builder)` in the phase session (or a fresh one with the phase's instruction, as `repair_or_ask` already does). `GroupDone { phase, first }` is appended after the group gates passed; the next task starts only then. The group's task worktrees and branches, now fully merged, are removed then (`worktree_remove`, `delete_branch`), so no branch of the run outlives its group.
- `finish()`'s "only gated work after the newest gated commit" check stays: merges are followed by the group's gate run, so the newest gated commit is the merged head.

**Rationale**: comparing by files touched (not by git conflicts) is the brief's rule and is what `[P]` promised. Building the working commit with `merge-tree` reuses 011's tested path and never touches the engineer's folder. Ticking at the merge keeps `tasks.md` out of every overlap.

**Alternatives considered**: rebasing T on the build head (rejected: rewrites the agent's commits, which Fallow takes apart anyway, and loses the two-sided view); merging and repairing only on a git conflict (rejected by the brief: "does not merge blindly").

## R-7 Sessions and the phase's handover

**Decision**: the phase session stays the session of the phase's sequential tasks and its repairs. The handover prompt (`loop_handover`) gets the tasks built alongside, with their merge commits, when the phase had a group. When the phase has no session yet (a phase that starts with a group and has no plain task), the handover's step starts a fresh session with the phase's instruction, as `repair_or_ask` already does for a lost session.

## R-8 Steps at task points

**Decision**: a maker before a task runs in the task's worktree (the build worktree, or the group task's), its commit gated as a new baseline (`maker_gated(..., gate = false)`, as a test-writer at a start today). A checker after a task checks the task's accepted commit (the build head, or the group task's head) in its own check worktree; `start()` for a review takes its commit from the head of the worktree its point belongs to, not always the build worktree. With `paths`, a task-point step runs only when that task's changes (its base to its head) touch them.

## R-9 Cost, time and slots

**Decision**: no new mechanism. `start()` already gives each step `remaining / (1 + running + queued)` of the cost budget and `limit_hit` is checked before every start (008 R-7); with `builders` tasks queued at once, the share follows. The time budget is the loop's wall clock, unchanged. The global `defaults.max_parallel` (default 4) and the memory room bound the agents running across all loops. The budget suggestion at the lock stays per task (parallel builders do not make tasks cheaper). The memory benchmark's reference run (4 concurrent steps) is not changed (changing it would need an amendment); the slot limit guarantees no more than `max_parallel` agents run, whatever their kind.

## R-10 Restarts

**Decision**: everything is derived from the event log, as the loop is today: groups from `GroupStarted`, merges from `TaskMerged`, overlaps from `TaskOverlap`, the end from `GroupDone`. A missing task worktree is re-created on its branch (`worktree_add` on the existing branch). `requeue_cut_short` already requeues every cut-short step, several at once. A merge is idempotent: before merging T, Fallow checks whether T's head is already an ancestor of the build head (a merge done just before a crash, its event not written) and then only appends `TaskMerged`.

## R-11 The lock statement

**Decision**: a pure function `groups::lock_statement(builders, phases, max_parallel) -> Vec<String>` gives the lines of FR-022: with `builders == 1`, none; with no group, "No [P] tasks: the pipeline's N builders will work as 1"; per group, "Phase <label> has k [P] tasks: up to min(N, k) builders at a time" (two groups in one phase give two lines); when `max_parallel < N`, "Your setting allows M agents at once: at most M builders". The feature view (`engine.rs`, where `budget_suggestion` is built) adds `builders` and `statement` to each entry of `pipelines`; `spec.js` shows the chosen pipeline's lines under the Pipeline field and re-renders on change.

## R-12 The designer without labels

**Decision**: `pipelines.js` drops the green/orange pills in the list, the "(not verified)" suffix in "Based on", and the bold "Not verifying." in the notice. The service adds `checks: Vec<String>` to each pipeline (`Pipeline::checks()`: plain sentences from its steps: "Fallow runs the gates after every task", "the reviewer checks every user story's acceptance scenarios at its checkpoint", "the security reviewer checks each story when api/** or auth/** changed", "the docs writer works once at the end", "up to 3 builders work on [P] tasks"). The guarantee line shows `checks` and, when `missing` is not empty, "A result of this pipeline ends as "Done, not verified by Fallow": <missing>". `verifying` and `missing` stay in the API: the agents page still refuses a save that would make a pipeline in use stop verifying (008 US3 AS5, unchanged), and the loop page still shows outcomes as today. The lock form keeps its "(not verified)" beside a pipeline whose results would not be verified: the engineer's decision limits the change to the designer and the pipeline list, and at the lock it is a statement about the result, as constitution I asks.

## R-13 The graph

**Decision**: `layout()` gets the task group inside the phase group: columns `task-start` units, the builder (its node shows "up to N builders on [P] tasks" when N > 1), the gates, `task-end` units; units (stacked columns) at every point, not only the checkpoint's end; a + between every pair of columns that maps to a point (`presetOf` gains `task-start`/`task-end`); the "next task" back-edge spans the task group; the repair back-edges: task-point and phase-point failures to the builder (≤ task_repairs), checkpoint failures to the builder (≤ review_rounds). The step form's "At" select shows for task steps that are not the builder or the gates; "On failure: a repair" is disabled at start points for checkers; "Alongside" lists checkers at the same point; the caps row gets "up to [N] builders on [P] tasks" (1–8). The loop page uses the same layout (`Fallow.pipelineLayout`), so it draws the new points too.

## R-14 Existing files and loops

**Decision**:
- Shipped Standard, Test first and Solo keep their text byte for byte, so `install_defaults` sees no change for them. A fourth default `assets/pipelines/parallel.toml` is added to `DEFAULT_PIPELINES`; `install_defaults` writes it on the next start because `.shipped.json` has no entry for it (its existing rule for a missing file without a record).
- Compatibility test: every `[[step]]` pipeline text in `tests/` and the 0.4.0 shipped defaults (copied into the test as fixtures) parse under 018 with the same steps, `builders == 1`, the same `verifying`/`missing`, and `to_toml` byte-identical to 0.4.0's.
- Loops: `pipeline_of()` parses the recorded TOML with the new parser; with `builders == 1` no group exists, so a 0.4.0 loop runs as before. Units of one keep their parts (R-2). The one change of behaviour: `parallel_with` outside a checkpoint's end now runs the unit at once; a loop recorded in 0.4.0 with such a unit that stopped in the middle of it resumes each cut-short step as before (`requeue_cut_short`) and then judges them together.
- No file is written: `Library::load` stays read-only.

## R-15 Run records

**Decision**: `LoopRecord` gains `builders_cap: u32`, `builders_most: u32` (the most group builds that ran at once, from `StepStarted`/`StepFinished` of group tasks), `groups_parallel: u32`, `overlap_repairs: u32`, each `#[serde(default)]` so records written before still read; `PIPELINES` (the known names) adds `parallel`. `StepRow.at` is written for task steps too. Only numbers and fixed names (constitution V); `tests/record_marker.rs` covers the new fields.

## R-16 Constitution IV, "a committed contract for each boundary"

**Decision**: the plan declares the split by the locked `tasks.md` (approved plan): `[P]` tasks are, by Spec Kit's definition, independent of each other ("different files, no dependencies"), so there is no boundary *between* them that needs a contract; the boundaries they build against are in the locked `plan.md` and `contracts/`, committed at the lock. "Owned files with no overlap" is not taken on trust: Fallow checks the files actually touched (R-6) and never merges an overlap. Recorded in the plan's Complexity Tracking for the reviewer.
