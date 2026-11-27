# Implementation Plan: Pipelines as an orchestration system

**Branch**: `018-pipeline-orchestration` | **Date**: 2026-11-27 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/018-pipeline-orchestration/spec.md`

## Summary

- **The labels go** from the Pipelines tab (list, designer, "Based on"); the service gives each pipeline plain `checks` sentences, and the designer says what a result will say when something is not checked. Outcomes are unchanged; the lock form keeps "(not verified)" (research R-12).
- **Points everywhere**: `at` on task steps (before/after each task), checkers per task, checkers at a start with `on_fail = "engineer"`; one `point()` runner in `feature.rs` walks the fixed order of contracts/pipeline.md and runs units of checkers alongside each other at every point, with 008's judge-together logic lifted out of `checkpoint()` (R-1 to R-3, R-8).
- **Builders over `[P]` tasks**: `[caps] builders` (1..=8). A group of consecutive `[P]` tasks runs up to N at once, each in `worktrees/task-<id>` on its own branch and session; Fallow merges in `tasks.md` order after comparing touched files, repairs an overlap on a 011-style working commit, and gates the merged group before going on (R-4 to R-7, R-10).
- **The lock statement**: a pure `groups::lock_statement` feeds the lock form (R-11).
- **Existing files and loops**: nothing taken away from the format; `to_toml` byte-identical with one builder; units of one keep 0.4.0's parts; a fourth shipped default `parallel` (R-14). Run records add four counters (R-15).

## Technical Context

**Language/Version**: Rust, edition 2024 (as `Cargo.toml`). The page stays plain JavaScript and CSS, compiled in.

**Primary Dependencies**: none added (git ≥ 2.38 for `merge-tree --write-tree`, already required by 011).

**Storage**: the run's event log and state as today, with four new events and `LoopState.groups` (`#[serde(default)]`); worktrees under the run's `worktrees/` (`task-<id>`) and branches `<build_branch>-<id>`, removed with the run like `build`.

**Testing**: `cargo test --workspace --features automation`; unit tests in `src/defs/pipeline.rs`, `src/run/groups.rs`, `src/measure/record.rs`; integration tests on the fake runtime: `tests/us18_designer.rs`, `us18_points.rs`, `us18_builders.rs`, `us18_lock.rs`, `us18_existing.rs`; the existing pipeline tests (`us2_pipeline_steps.rs`, `us8_parallel.rs`, `us6_defs.rs`, …) unchanged as the compatibility check; `tests/visual_018.rs` with `checks/pipelines.js` by hand, as 017.

**Target Platform**: Windows 10/11 x64 and macOS on Apple silicon (releases); Linux for development.

**Project Type**: a desktop app (014, 016): a Rust service with its own window showing a local web UI.

**Performance Goals**: a group of 4 equal `[P]` tasks with 3 builders takes ≤ 60 % of its time with 1 (SC-002, fake runtime).

**Constraints**: constitution VIII budgets unchanged; the reference run is not changed; the slot limit (`defaults.max_parallel`, default 4) bounds agents at once whatever the pipeline allows.

**Scale/Scope**:
- `src/defs/pipeline.rs`: `Caps.builders`, the checks, `checks()`; tests (≈ +150 lines);
- `src/defs/mod.rs`: `parallel` in `DEFAULT_PIPELINES`; `assets/pipelines/parallel.toml`;
- `src/run/groups.rs` (new, ≈ 250 lines): groups, the lock statement, the overlap check, the merge;
- `src/run/feature.rs`: `point()`, units at every point, group scheduling in `phase()`, `step()`'s rule while steps run, `start()`'s worktree and review commit per point, `work_done()` for group tasks, handover prompt (≈ +500/−200 lines);
- `src/run/machine.rs`: `drive()`'s early return on `needs` lets group decisions through for a group task's question (R-4);
- `src/run/events.rs`, `state.rs`: four events, `GroupState`;
- `src/agent/prompts.rs` (`loop_task`, `loop_handover`, `update_conflicts`): the group line, the tasks alongside in the handover, the overlap repair;
- `src/run/activity.rs`: loop view `groups`, `at` for task steps, `builders`;
- `src/run/engine.rs`: the feature view's `builders`/`statement`; `src/studio/defs.rs`: `checks`;
- `src/measure/record.rs`: four counters, `parallel`;
- `src/service/web/screens/pipelines.js` (labels, task points, units everywhere, builders field), `loop.js` (group rows), `spec.js` (statement), `app.css`;
- documents: 007 and 008 marked as amended (FR-030).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Pass. The result rule is unchanged (gates after every task, an independent checker at every story's checkpoint end, FR-021). Each group task is checked and gated on its own commit by Fallow; the merged group is gated again before anything builds on it (FR-016); an overlap is never merged on an agent's word; `finish()`'s "only gated work" check stands. Removing labels changes no outcome: "Done, not verified by Fallow: <what>" stays, never in a verified one's words or colours; the lock form keeps "(not verified)". A task-end checker is a checker: read-only, its working copy watched as today. |
| II. The Engineer Decides | Pass. The lock form states what the builders will do before the engineer locks (FR-022); caps end in a question; pause, stop and take over apply to every builder; a waiting question holds only its task and what follows the group (FR-018). Nothing new reaches the target branch. |
| III. Coordination in Code | Pass. Fallow decides groups, the number of builders, the merge order and every repair from the locked `tasks.md`, git and its own records (R-5, R-6). Builders do not message each other; each gets the locked documents, the handovers and its task. |
| IV. Simplest Setup First | Pass, see Complexity Tracking. One builder by default (`builders` 1; Standard, Test first, Solo unchanged). Parallel parts only where the approved plan declares them (`[P]` in the locked `tasks.md`), with owned files checked by the files actually touched (R-6). No investigation steps added. Every component can be switched off for a recorded run: the pipeline chosen at the lock sets `builders` and the steps, and the record keeps `builders_cap`, `builders_most`, `groups_parallel`, `overlap_repairs` (R-15). |
| V. Local and Private | Pass. No network use; the record's new fields are numbers (`tests/record_marker.rs`). |
| VI. Least Privilege | Pass. Each builder's rights are the maker's on its own worktree (`rights::maker` with the protected documents, as today); checkers `rights::checker`. Fallow writes only to the run's worktrees and branches (VI's list). |
| VII. Runs Survive | Pass. Groups, merges and overlaps are events; a merge already done is detected by ancestry before re-merging (R-10); cut-short steps of several tasks are requeued together, as 008 does for checkers. |
| VIII. Small and Measured | Pass. No dependency; the slot limit and memory room hold builders back like any step; the reference run and budgets are unchanged; any claim that builders pay waits for recorded runs (the record now carries what is needed). |
| IX. Runtime-Agnostic | Pass. Everything is decided from git and Fallow's records; tested on the fake runtime through the runtime contract. |
| CI gates (Workflow) | Pass. The false-claims tests gain a group case (a builder claiming a file it did not change in its worktree); the no-push test is unaffected (merges are local to the run's branches); the resume test gains a stop during a group. |

**Re-check after Phase 1 design**: unchanged. The contracts add fields, no route, no write place beyond the run's own worktrees and branches.

## Project Structure

### Documentation (this feature)

```text
specs/018-pipeline-orchestration/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── pipeline.md     # replaces parts of 007 contracts/pipeline.md
│   └── web-api.md      # amends 007 web-api.md, 008 loop-view.md and page.md
├── checks/
│   └── pipelines.js    # the designer check by hand (as 017's agents.js)
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
assets/pipelines/parallel.toml          # new shipped default
src/defs/pipeline.rs                    # Caps.builders, checks, checks()
src/defs/mod.rs                         # DEFAULT_PIPELINES + parallel
src/run/groups.rs                       # new: groups, lock statement, overlap, merge
src/run/mod.rs                          # mod groups
src/run/feature.rs                      # point(), units everywhere, group scheduling, start/work_done per worktree
src/run/machine.rs                      # drive(): a group task's question holds only its task (R-4)
src/run/events.rs, src/run/state.rs     # GroupStarted, TaskMerged, TaskOverlap, GroupDone; GroupState
src/agent/prompts.rs                    # group task line, handover with tasks alongside, overlap repair
src/run/activity.rs                     # loop view: groups, at, builders
src/run/engine.rs                       # feature view: builders, statement
src/studio/defs.rs                      # checks in the views
src/measure/record.rs                   # four counters, parallel
src/service/web/screens/pipelines.js    # labels gone, points, units, builders
src/service/web/screens/loop.js         # group rows, overlap lines
src/service/web/screens/spec.js         # the lock statement
src/service/web/app.css
tests/us18_*.rs, tests/visual_018.rs    # new
specs/007-fallow-studio/…, specs/008-studio-redesign/…   # marked amended (FR-030)
```

**Structure Decision**: the existing single crate with its compiled-in page; one new module, `src/run/groups.rs`, so the group logic is testable without a loop.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Constitution IV asks for "a committed contract for each boundary" before work is split into parallel parts | The split is declared by the locked `tasks.md`: `[P]` means "different files, no dependencies", so no boundary lies *between* the tasks of a group; the boundaries they build against are in the locked `plan.md` and `contracts/`, committed at the lock. "Owned files with no overlap" is checked by Fallow on the files actually touched; an overlap is repaired, never merged blindly (research R-16). | Requiring a contract per `[P]` task would make `[P]` unusable with Spec Kit's own templates; trusting `[P]` without the file check would break "owned files". |
| `step()` decides inside an active group while steps run | Builders must overlap; without it a finished task waits for the slowest (R-4). | Deciding everything while steps run would let steps outside the group act on a build branch the group is changing. |
| Several worktrees and branches per run | A working copy per builder is the only way to know what each touched and keep them from seeing each other's half-done work (VI, III). | One shared worktree cannot attribute changes to tasks. |
