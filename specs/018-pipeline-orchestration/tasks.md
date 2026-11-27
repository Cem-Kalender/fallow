---
description: "Task list for 018 Pipelines as an orchestration system"
---

# Tasks: Pipelines as an orchestration system

**Input**: design documents from `specs/018-pipeline-orchestration/`:
- spec (US1-US5, FR-001 to FR-030, SC-001 to SC-008);
- plan;
- research R-1 to R-16;
- data-model;
- contracts/pipeline.md, contracts/web-api.md;
- quickstart sections 1 to 7.

**Tests**: requested (the project writes tests first). Each story's tests are written and seen failing before its implementation. They use the fake runtime (`fallow-fake-agent` scripts: `kind`, `part`, `files`, `output`, `sleep_ms`); the designer is also checked by hand with `tests/visual_018.rs` and `checks/pipelines.js`.

**Build and test**: `cargo test --workspace --features automation`, one test file per command while iterating. Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --features automation -- -D warnings` before each commit. Every existing test must keep passing at the end of every phase: they are the compatibility check of US5 (spec FR-025, FR-027).

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

- [ ] T001 Add the empty module `src/run/groups.rs` (a module comment naming 018 research R-5, R-6, R-11) and `pub mod groups;` in `src/run/mod.rs`; the crate builds.
- [ ] T002 [P] Add test helpers to `tests/common/mod.rs`: `pub const PIPELINE_040_STANDARD/TEST_FIRST/SOLO: &str` holding the 0.4.0 shipped texts exactly as `assets/pipelines/*.toml` are at origin/main (copied byte for byte); `pub fn parallel_tasks_feature(root, n_parallel, shared: &[(&str, &str)])` writing a calc feature whose phase 2 has `n_parallel` consecutive `- [ ] T00x [P] [US1] …` tasks followed by one plain task (the files each task changes given by the caller, so two tasks can share one); `pub fn pipeline_with(builders: u32, extra_steps: &str) -> String` building Standard with `[caps] builders` and extra `[[step]]` text.

---

## Phase 2: Foundational (the format and the loop's records)

**Purpose**: the pipeline format and the new events every story builds on. No behaviour changes yet: with `builders = 1` and no new points, everything runs as 0.4.0.

### Tests (write first, see them fail)

- [ ] T003 [P] Unit tests in `src/defs/pipeline.rs` (`mod tests`): `builders` defaults to 1, accepts 1 and 8, refuses 0 and 9 ("builders must be between 1 and 8"); `to_toml()` of a pipeline with `builders = 1` holds no `builders` key and equals 0.4.0's output for each of `PIPELINE_040_*` (copy the three texts into the test module as constants); `at = "start"` / `"end"` accepted on a `when = "task"` maker that is not the builder and on a `when = "task"` checker; a `when = "task"` checker without `at` reads as task end; `at` on the builder or on `fallow:gates` refused naming the step; a checker with `at = "start"` and `on_fail = "repair"` refused with "nothing of that point's work is built yet", with `on_fail = "engineer"` accepted (at phase, checkpoint and once starts); two makers with `parallel_with` refused ("only checkers at the same point run together", unchanged text); `parallel_with` between checkers at a phase end accepted; exactly one builder still required.
- [ ] T004 [P] Unit tests in `src/run/state.rs` (`mod tests`): folding `GroupStarted { phase, tasks, base }`, `TaskMerged { task, commit }`, `TaskOverlap { task, files, with, working }`, `GroupDone { phase, first }` into `LoopState.groups` (data-model.md "Group"); a state JSON written before 018 (no `groups`) reads with `groups` empty; each event's name in `Event::name()` (`group_started`, `task_merged`, `task_overlap`, `group_done`).

### Implementation

- [ ] T005 In `src/defs/pipeline.rs`: `Caps.builders: u32` with `#[serde(default = "one", skip_serializing_if = "is_one")]`, "1..=8, default 1"; replace the checks per contracts/pipeline.md "Checks" (remove "a checker runs at phases and checkpoints, not per task" and "a checker at the start has nothing to check yet"; add the builder/gates `at` check, the start-point `on_fail` check, the `builders` range check); `Pipeline::builder()` returns the `when = "task"` maker without `at`; add `Step::point_at()` giving `At::End` for a task checker without `at`. Make T003 pass.
- [ ] T006 [P] In `src/run/events.rs` and `src/run/state.rs`: the four events of data-model.md "Events" and `GroupState` / `GroupTask` folded into `LoopState.groups` (`#[serde(default)]`). Make T004 pass.

**Checkpoint**: `cargo test --workspace --features automation` passes unchanged.

---

## Phase 3: User Story 1 - The designer says what a pipeline checks, without labels (Priority: P1) 🎯 MVP

**Goal**: no "verifying"/"not verifying" label or colour in the Pipelines tab; plain `checks` sentences; the "Done, not verified by Fallow" sentence when something is not checked (FR-001 to FR-003).

**Independent Test**: quickstart section 2.

### Tests for User Story 1 (write first, see them fail)

- [ ] T007 [P] [US1] Unit test in `src/defs/pipeline.rs`: `Pipeline::checks()` for Standard ("Fallow runs the gates after every task." and "The reviewer checks every user story's acceptance scenarios at its checkpoint."), Solo (gates only), a pipeline with a path-limited security reviewer ("… when api/** or auth/** changed"), a docs-writer once at the end, and `builders = 3` ("Up to 3 builders work on [P] tasks at the same time, each in its own working copy."); a pipeline whose only checker runs at every phase end has `missing` holding "no review at checkpoints" (FR-021, US1 AS3: checkers elsewhere do not count).
- [ ] T008 [P] [US1] Integration test `tests/us18_designer.rs`: `GET /api/pipelines` gives `checks` for each pipeline and keeps `verifying`/`missing` (contracts/web-api.md); the Pipelines tab's script (`src/service/web/screens/pipelines.js`, read as text from the compiled assets the service serves) contains no `pill green">verifying`, `pill orange">not verifying`, `(not verified)` in "Based on", nor `<b>Not verifying.</b>`; a loop on Solo (fake runtime, `CALC` feature) ends with `Outcome { verified: false, missing: ["no review at checkpoints"] }` and its stage reason "every phase is done, not verified by Fallow: no review at checkpoints" (FR-003, must fail as verified).

### Implementation for User Story 1

- [ ] T009 [US1] `Pipeline::checks()` in `src/defs/pipeline.rs` (research R-12), naming agents by their names and points in plain words. Make T007 pass.
- [ ] T010 [US1] Add `checks` to `pipelines_view`, `check_pipeline` and `put_pipeline` results in `src/studio/defs.rs`.
- [ ] T011 [US1] In `src/service/web/screens/pipelines.js`: remove the pills in the list (line with `pill green">verifying`), the " (not verified)" suffix in "Based on", and "<b>Not verifying.</b>" in the notice; the guarantee line shows `checks` as sentences and, when `missing` is not empty, `A result of this pipeline ends as "Done, not verified by Fallow": <missing>.`; update the file's header comment. Keep `spec.js`'s lock form unchanged (research R-12). Make T008 pass (FR-001, FR-002).

**Checkpoint**: US1 complete; the suite passes.

---

## Phase 4: User Story 2 - Agents at every point, one after another or side by side (Priority: P1)

**Goal**: steps before/after each task, units of checkers alongside each other at every point, the fixed order of contracts/pipeline.md "Points" (FR-004 to FR-010, FR-028, FR-029, FR-029a).

**Independent Test**: quickstart section 3.

### Tests for User Story 2 (write first, see them fail)

- [ ] T012 [P] [US2] Integration test `tests/us18_points.rs`, `a_checker_after_each_task_checks_its_commit_and_its_finding_is_a_task_repair`: pipeline Standard plus `lint-checker` (a checker defined in the test's agents folder on the fake runtime) with `when = "task"`, `at = "end"`; check the review step of T002 is queued after T002's gates and before T003's build, its worktree is at T002's accepted commit; a scripted finding gives a `Repair` with `part = "T002"`, `task = "T002"`, counted in the task's repairs; with `task_repairs = 1` a second finding asks the engineer ("allow one more repair") and T002 is not accepted (US2 AS1, AS2).
- [ ] T013 [P] [US2] In `tests/us18_points.rs`, `a_maker_before_each_task_runs_first`: `test-writer` with `when = "task"`, `at = "start"`, `on_fail = "engineer"`; its `Make` step for each task runs before that task's build, its commit is gated as a baseline (`GateRun` kind baseline on its commit) and the builder's step base is that commit (US2 AS3).
- [ ] T014 [P] [US2] In `tests/us18_points.rs`, `checkers_alongside_each_other_at_a_phase_end_overlap_and_are_judged_together`: `reviewer` and `security-reviewer` at `when = "phase"`, `at = "end"`, `parallel_with`, each `sleep_ms = 2000`; their steps overlap in time (as `tests/us8_parallel.rs::checkers_alongside_each_other_overlap_and_are_judged_together`); one finding from each → one `Repair` with part `phase-1-end` carrying both findings; then both run again; one failing and one passing → the phase does not end (US2 AS4, AS5, AS6).
- [ ] T015 [P] [US2] In `tests/us18_points.rs`, `the_order_of_points_follows_tasks_md`: a pipeline with one step at every point (once start/end makers, phase start/end checkers with `on_fail = "engineer"` at start, task start/end, checkpoint start/end) on the two-story calc feature; the sequence of queued steps' `(kind, part)` equals the order in contracts/pipeline.md "Points" (FR-004).
- [ ] T016 [P] [US2] In `tests/us6_defs.rs` (new test `the_designer_refuses_what_the_points_do_not_allow`): `POST /api/pipelines/check` refuses two makers alongside each other, a checker at a start with `on_fail = "repair"`, `fallow:gates` with `at`, the builder with `at`, each naming the step number; accepts a checker after each task (US2 AS7, AS8, AS10).

### Implementation for User Story 2

- [ ] T017 [US2] In `src/run/feature.rs`: introduce `Point` (data-model.md) and `point()` that runs the steps of one point as units in pipeline order (research R-2): a unit of one calls today's `extra()`; units of several checkers use a helper lifted from `checkpoint()` (queue all members together after the point's repair is settled; judge when all finished; incomplete members asked again each on its own, `CHECK_TRIES`; one `repair_or_ask` with all findings, `on_fail = "engineer"` asking first). Parts per data-model.md "Unit": units of one keep 0.4.0's part names; units of several at phase/once points repair under the point's part (`phase-N-start|end`, `once-start|end`); `round_bounds`/`round_start` count from the latest repair of that part. `checkpoint()` uses the same helper for its units (behaviour of `tests/us8_parallel.rs` unchanged) (FR-005, FR-006, FR-007).
- [ ] T018 [US2] In `src/run/feature.rs` `step()` and `phase()`: replace the three loops (once start, phase start, phase end, once end) with `point()` calls in the order of contracts/pipeline.md "Points"; add `TaskStart(id)` before a task's build and `TaskEnd(id)` after its gates, with parts `<agent>@<task>-start|end`; a task-end checker's failure is `repair_or_ask(task, Some(task), …, task_repairs, builder)`; a task is `Accepted` only after its `TaskEnd` point passed; a task-start maker's commit goes through `maker_gated(…, gate = false)` and the builder starts from it (research R-8, FR-008). `paths` at task points compare the task's base to its head.
- [ ] T019 [US2] In `src/run/feature.rs` `start()` and `alongside_of()`: a review step's commit is the head of the worktree its point belongs to (the build worktree here; a group task's worktree in US3), not always the build head; "round begun" and `alongside_of()` work per point, not only per story (research R-3).
- [ ] T020 [P] [US2] Loop view in `src/run/activity.rs`: `steps[].at` for task steps (`"start"`, `"end"`; builder and gates `"end"`), `caps.builders`.
- [ ] T021 [P] [US2] Designer graph in `src/service/web/screens/pipelines.js` (research R-13): `layout()` builds the task group (task-start units, builder, gates, task-end units) inside the phase group; `unitsOf()` applied at every point; `presetOf()` maps a + to every point including `task-start`/`task-end`; `nodeKind()` names "BEFORE EACH TASK" / "AFTER EACH TASK"; the step form shows "At" for task steps that are not the builder or the gates, disables "On failure: a repair" for a checker at a start, lists "Alongside" checkers at the same point; `toToml()` writes `at` for task steps and `builders` only when not 1; the caps row gets "up to [N] builders on [P] tasks" (a number input, 1 to 8, labelled) and the builder node shows it when N > 1 (FR-028, FR-029); every new control has a label, is reachable by Tab and has 4.5:1 contrast (FR-029a). Make T016's designer-facing parts consistent with the service.
- [ ] T022 [US2] Run T012-T016 and the existing `tests/us2_pipeline_steps.rs`, `tests/us8_parallel.rs`, `tests/us6_defs.rs`; fix until all pass.

**Checkpoint**: US2 complete; loops with one builder behave as 0.4.0 except where a pipeline uses the new points.

---

## Phase 5: User Story 3 - Builders fan out over [P] tasks (Priority: P1)

**Goal**: groups of `[P]` tasks run up to N builders at once in their own worktrees; merged in `tasks.md` order after the touched-files check; overlaps repaired on top; the merged group gated (FR-011 to FR-020, FR-023, FR-024).

**Independent Test**: quickstart section 4.

### Tests for User Story 3 (write first, see them fail)

- [ ] T023 [P] [US3] Unit tests in `src/run/groups.rs`: `groups(phase, builders)` gives consecutive `[P]` runs of at least two tasks, never across phases, none with `builders == 1`, a single `[P]` task alone is no group; `overlap(mine, theirs, tasks_md)` ignores the feature's `tasks.md` and returns the shared files sorted.
- [ ] T024 [P] [US3] Integration test `tests/us18_builders.rs`, `three_builders_run_four_parallel_tasks_and_merge_them_in_order`: `parallel_tasks_feature(root, 4, …)` with disjoint files, `pipeline_with(3, "")`, each build `sleep_ms = 2000`; at most 3 builds of the group run at once and at least 3 overlap in time; each build's worktree ends in `task-T00x`; `TaskMerged` for T004..T007 in that order; the build branch holds each task's tick in `tasks.md`; the group's gates (`GateRun` part `group-T004-T007`) run on the merged head before T008's build is queued; T008 runs in the phase's session; the phase's handover instruction names T004..T007 as built alongside with their merge commits; after `GroupDone` no `task-T00x` worktree or branch is left (US3 AS1, AS3, AS7; FR-016, FR-016a, FR-017).
- [ ] T025 [P] [US3] In `tests/us18_builders.rs`, `the_setting_for_agents_at_once_limits_the_builders`: settings `defaults.max_parallel = 2`; never more than 2 builds running at once (US3 AS2).
- [ ] T026 [P] [US3] In `tests/us18_builders.rs`, `an_overlap_is_repaired_on_top_and_reported`: T005 and T007 both write `calc/shared.py`; T007 is not merged after its first accepted commit; `TaskOverlap { task: "T007", files: ["calc/shared.py"], with: ["T005"] }`; a `Repair` with `task = "T007"` in T007's session whose instruction names `calc/shared.py`, T005 and both sides' changes; a scripted repair that leaves a conflict marker is refused ("still holds a conflict marker") and counted in T007's repairs; a clean repair is gated and merged; the loop view's `groups[0].tasks` shows T007's overlap (US3 AS4, AS5; FR-015, FR-024; must fail: T007's first commit is never an ancestor of the build head).
- [ ] T027 [P] [US3] In `tests/us18_builders.rs`, `a_merged_group_that_fails_the_gates_is_repaired_before_the_next_task`: a gate that fails only when two marker files of T004 and T005 are both present; each task's own gates pass; the group gate fails; one `Repair` with part `group-T004-T007`; T008 is not queued until it passes (US3 AS6).
- [ ] T028 [P] [US3] In `tests/us18_builders.rs`, `one_builder_or_no_group_runs_as_before`: the same feature with `builders = 1`, and a feature without `[P]` tasks with `builders = 3`: no `GroupStarted`, every build in the `build` worktree and the phase's session (US3 AS8, AS9).
- [ ] T029 [P] [US3] In `tests/us18_builders.rs`, `a_question_holds_only_its_task`: T005's builder asks a question for the engineer; T004, T006 and T007 go on and are accepted; T008 is not queued until the answer (US3 AS10, FR-018).
- [ ] T030 [P] [US3] In `tests/us18_builders.rs`, `the_cost_limit_stops_new_builders`: loop cost limit reached while three builds run: no new step starts, `LimitReached`, the stage is paused with the question; each running step's budget was at most its share (US3 AS11, FR-023).
- [ ] T031 [P] [US3] In `tests/us2_resume.rs` (new test `a_restart_while_builders_run_resumes_each_task`): stop the engine while three builds run; after restart each is requeued in its own session and worktree, no `TaskMerged` appears twice for one task, no accepted task is built again (US3 AS12, FR-020).
- [ ] T032 [P] [US3] In `tests/us2_false_claims.rs` (new test `a_group_builder_claiming_files_it_did_not_change_is_refused`): a group task's builder reports a file it did not change in its worktree → refused "files claimed but not changed"; a builder that edits `spec.md` in its worktree → undone and refused (FR-019; constitution CI gate I).
- [ ] T033 [P] [US3] In `tests/record_marker.rs`: a run record of a loop with a group holds `builders_cap`, `builders_most`, `groups_parallel`, `overlap_repairs` as numbers and no marker text; a record written before 018 still reads (research R-15).

### Implementation for User Story 3

- [ ] T034 [US3] In `src/run/groups.rs`: `groups(phase: &PhaseSpec, builders: u32) -> Vec<Vec<String>>` and `overlap(...)` (research R-5, R-6). Make T023 pass.
- [ ] T035 [US3] In `src/run/groups.rs`: `start_group(run, l, phase, tasks)` appends `GroupStarted` with the build head as `base` and creates `worktrees/task-<id>` on branch `<build_branch>-<id>` at `base` (`git::worktree_add` with a branch, re-created on its branch when missing after a restart); `try_merge(run, l, task)`: skip-and-record when the task head is already an ancestor of the build head; otherwise the overlap check (files from the task's base, the feature's `tasks.md` excluded); no overlap → `git::merge_into(build_wt, head, "Fallow: merge <task> (built alongside)")`, tick the task in `tasks.md` with `speckit::tasks::tick` and commit only that file, append `TaskMerged`; overlap → `git::merge_tree(task_head, build_head)` + `git::commit_tree(tree, [task_head, build_head])`, move the task's branch and worktree there, append `TaskOverlap { working }`.
- [ ] T036 [US3] In `src/run/feature.rs` `phase()`: when the next unaccepted task begins a group (`groups::groups(p, pl.caps.builders)`), run the group: `start_group` once; keep at most `builders` tasks between first step queued and accepted, queueing each task's `TaskStart` point and build in its worktree; process each task as a task (gates on its commit, `TaskEnd`); `try_merge` accepted tasks in `tasks.md` order; an overlap queues `repair_or_ask(task, Some(task), …)` with the overlap prompt (T039) and the repair's result is checked for markers and side files as `update::resolved` does (`git::marker_lines`, `git::side_files`) before the usual checks; when all are merged, `gated(run, ctx, "group-<first>-<last>", head)` with `repair_or_ask` on failure (phase session or a fresh one); append `GroupDone` and remove the group's task worktrees and branches (`git::worktree_remove`, `git::delete_branch`); only then the next task (FR-012 to FR-017).
- [ ] T037 [US3] In `src/run/feature.rs` `step()`: while steps run, decide only inside the active group (research R-4): process finished group steps, start queued group steps, run `try_merge`; everything else returns as today. `take_answers` still only when nothing runs. A question waiting for the engineer from a group task holds only that task (FR-018): in `step()` the `questions::waiting` branch skips the group's other tasks, and in `src/run/machine.rs` `drive()` the early return on `run.state.needs` lets `feature::drive` through when the waiting question's step is a group task (research R-4); every other `NeedsEngineer` (caps, limits, failed starts) still holds the whole loop.
- [ ] T038 [US3] In `src/run/feature.rs` `start()` and `work_done()`: a step of a group task runs in that task's worktree; its builder starts a new session (never `phase_session`), its repairs resume it; `phase_session()` ignores steps of group tasks; `work_done()` does not tick `tasks.md` for a group task, checks the commit is on `<build_branch>-<id>` instead of the build branch, and `locked_changes` expects `tasks.md` unchanged for it.
- [ ] T039 [P] [US3] In `src/agent/prompts.rs`: `loop_task` gets an optional list of tasks built alongside ("T005, T006 and T007 are built at the same time in other working copies: do only <task>"); `loop_handover` gets the tasks built alongside with their merge commits; a new `loop_overlap(task, files_with_logs, with)` in the shape of `update_conflicts` ("both tasks' work must keep working").
- [ ] T040 [P] [US3] Loop view in `src/run/activity.rs`: `groups` as contracts/web-api.md "The loop view" (task `state` from its steps and events, `worktree`, `merged`, `overlaps`); history lines for `TaskOverlap` and `TaskMerged` naming files and tasks.
- [ ] T041 [P] [US3] In `src/service/web/screens/loop.js` and `src/service/web/app.css`: one row per task of an active group with its builder's state and worktree; an overlap line "T007 changed calc/shared.py, which T005 changed: repaired on top of T005" (FR-024); labelled for screen readers.
- [ ] T042 [P] [US3] In `src/measure/record.rs`: `builders_cap`, `builders_most`, `groups_parallel`, `overlap_repairs` (`u32`, `#[serde(default)]`), computed from `LoopState.groups` and the group tasks' steps; `PIPELINES` adds `parallel`. Make T033 pass (FR-026).
- [ ] T043 [P] [US3] Add `assets/pipelines/parallel.toml` (contracts/pipeline.md "The defaults": `builders = 3`; builder per task; gates per task; reviewer with `parallel_with = ["security-reviewer"]` and security-reviewer at checkpoints, both `on_fail = "repair"`) and its entry in `DEFAULT_PIPELINES` in `src/defs/mod.rs`; update `the_built_in_library_has_the_defaults` (4 pipelines) in `src/defs/mod.rs` and `the_shipped_defaults_parse_and_only_solo_is_unverified` in `src/defs/pipeline.rs`.
- [ ] T044 [US3] Run T023-T033 and the whole suite; fix until all pass. Check SC-002 in `three_builders_run_four_parallel_tasks_and_merge_them_in_order`: the group's wall time with 3 builders ≤ 60 % of the same scenario with `builders = 1` (assert it).

**Checkpoint**: US3 complete.

---

## Phase 6: User Story 4 - At the lock, Fallow says what will actually happen (Priority: P2)

**Goal**: the lock form's statement (FR-022).

**Independent Test**: quickstart section 5.

### Tests for User Story 4 (write first, see them fail)

- [ ] T045 [P] [US4] Unit tests in `src/run/groups.rs`: `lock_statement(builders, phases, max_parallel)` for builders 1 (no line), 3 with no `[P]` ("No [P] tasks: the pipeline's 3 builders will work as 1."), 3 with a group of 4 in phase 3 ("Phase 3 has 4 [P] tasks: up to 3 builders at a time."), 3 with a group of 2 ("… up to 2 builders at a time."), two groups in one phase (two lines), a single `[P]` task (no group: counts as none), and `max_parallel = 2` ("Your setting allows 2 agents at once: at most 2 builders.").
- [ ] T046 [P] [US4] Integration test `tests/us18_lock.rs`: the feature view's `pipelines[]` carry `builders` and `statement` for a feature with 0 and with 4 `[P]` tasks; after locking with `parallel` and running on the fake runtime, the most group builds observed at once equals the number the statement gave (SC-007); `spec.js` renders the chosen pipeline's statement under the Pipeline field (checked as text in the served script: it reads `statement` and re-renders on `change` of `#lk-pipeline`).

### Implementation for User Story 4

- [ ] T047 [US4] `lock_statement` in `src/run/groups.rs` (research R-11). Make T045 pass.
- [ ] T048 [US4] In `src/run/engine.rs` `feature_view`: each `pipelines` entry adds `builders` and `statement` from the feature's `tasks.md` (as `budget_suggestion` reads it) and `settings.defaults.max_parallel`.
- [ ] T049 [US4] In `src/service/web/screens/spec.js`: under the Pipeline field of the lock form, the chosen pipeline's `statement` lines (`role="status"`), updated when the select changes; the option text keeps " (not verified)". Make T046 pass.

**Checkpoint**: US4 complete.

---

## Phase 7: User Story 5 - Existing pipelines and loops go on working (Priority: P1)

**Goal**: every 0.4.0 pipeline file and loop works as before; nothing rewritten (FR-025 to FR-027).

**Independent Test**: quickstart section 6. Note: T050-T052 may be written right after Phase 2 and kept passing through every later phase; they are listed here as the story's own check.

### Tests for User Story 5 (write first, see them fail where they cover new behaviour)

- [ ] T050 [P] [US5] Unit test in `src/defs/pipeline.rs`, `every_0_4_0_pipeline_reads_the_same`: for the 0.4.0 shipped texts and 007's contract example, the parsed steps, `builders == 1`, `verifying` and `missing` equal 0.4.0's (as asserted by the existing tests), and `to_toml()` is byte-identical to 0.4.0's output (constants captured from origin/main in the test).
- [ ] T051 [P] [US5] Integration test `tests/us18_existing.rs`, `a_0_4_0_data_folder_loads_without_change`: write a data folder as 0.4.0 left it (`.shipped.json` with the three 0.4.0 fingerprints, unedited Standard, edited Test first, an own pipeline with a path-limited security reviewer alongside the reviewer, a project pipeline with a docs-writer once at the end); start the service; every file's bytes unchanged; `parallel.toml` added; every pipeline listed with the same steps and no problem; a file invalid for a reason 018 keeps (unknown agent) still reported (US5 AS1-AS3).
- [ ] T052 [P] [US5] In `tests/us18_existing.rs`, `a_loop_locked_before_resumes_and_a_finished_one_shows_as_before`: a loop whose recorded pipeline is `PIPELINE_040_STANDARD`, stopped mid-phase (events written by driving it on 018 then stopping, since the TOML is byte-identical), resumes without repeating a completed step and finishes verified; a finished Solo loop's view gives the same outcome and `missing`; a pipeline saved from the designer with one builder and no new points holds no `builders` key and no task `at` (US5 AS4-AS6).

### Implementation for User Story 5

- [ ] T053 [US5] Fix whatever T050-T052 find (expected: none beyond Phases 2-5); confirm `Library::load` and `install_defaults` in `src/defs/mod.rs` write nothing but the new default.
- [ ] T054 [P] [US5] In `README.md` ("Agents and pipelines"): the points, `builders` on `[P]` tasks, the `parallel` default, and that a pipeline using 018's fields cannot be used by Fallow 0.4.0 (US5 AS6).

**Checkpoint**: all stories complete.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T055 [P] Mark the replaced parts of earlier documents (FR-030): in `specs/007-fallow-studio/contracts/pipeline.md` (the `at` and `parallel_with` rows, the Checks, "The designer shows the same warning"), `specs/007-fallow-studio/spec.md` (FR-010, FR-010a, FR-011, FR-027, US6 AS3) and `specs/008-studio-redesign/spec.md` (FR-009, US3's independent test, US4 AS5, FR-011, FR-013, FR-014, FR-015): a line "Amended by 018: <what>" pointing to `specs/018-pipeline-orchestration/spec.md`, as 017 marked 007 and 008.
- [ ] T056 [P] Add `tests/visual_018.rs` (ignored, by hand, as `tests/visual_017.rs`): serves the Pipelines tab with Standard, Solo, `parallel` and a pipeline using every point; and `specs/018-pipeline-orchestration/checks/pipelines.js` (as `specs/017-agent-file-fields/checks/agents.js`) checking at 1440 × 900 and 390 × 844: no label text or pill, a + at every point, units drawn as branches, every + reachable by Tab with a visible focus.
- [ ] T057 Run the quickstart sections 1-7, `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --features automation -- -D warnings`; check the CI `bench` job's budgets are unchanged.

---

## Dependencies & Execution Order

- **Setup (T001-T002)** → **Foundational (T003-T006)** → stories.
- **US1 (T007-T011)** depends only on Foundational.
- **US2 (T012-T022)** depends on Foundational; independent of US1 except that both touch `pipelines.js` (T011 before T021).
- **US3 (T023-T044)** depends on US2 (`point()`, per-point review commits, `TaskStart`/`TaskEnd` run inside a group task's worktree).
- **US4 (T045-T049)** depends on Foundational and on `groups()` (T034); it can run alongside US3 after T034.
- **US5 (T050-T054)** tests can be written after Foundational and must pass at every checkpoint; T053 closes after US3.
- **Polish (T055-T057)** last; T055 and T056 can start any time after US2.

Within a story: tests first (seen failing), then implementation in the order listed; tasks in the same file are sequential.

### Parallel opportunities

- T002 with T001; T003 with T004; T005 with T006.
- US1: T007 with T008.
- US2: T012-T016 together (T012-T015 are one new file: write them in one pass, or split by function); T020 with T021.
- US3: T023-T033 together (different files except T024-T030 in one new file); T039-T043 together after T036-T038.
- US4: T045 with T046.
- US5: T050-T052 together; T054 any time.

### Example: US3's tests together

```text
T023 src/run/groups.rs (unit)
T024-T030 tests/us18_builders.rs
T031 tests/us2_resume.rs
T032 tests/us2_false_claims.rs
T033 tests/record_marker.rs
```

## Implementation Strategy

1. **MVP**: Setup, Foundational, US1. The engineer's first request (no labels) ships on its own; nothing else changes.
2. **US2**: agents at every point, with units everywhere; loops with one builder unchanged.
3. **US3**: builders over `[P]` tasks, the riskiest part, on top of US2's points.
4. **US4**: the lock statement.
5. **US5** stays green throughout; its tests are the gate before merge. Then Polish.
