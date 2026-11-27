# Quickstart: checking 018

Everything runs on the fake runtime (constitution IX). Commands from the repository root.

## 1. The whole suite

```sh
cargo test --workspace --features automation
```

Must pass, including the new tests named below and every test that already used a pipeline (they are the compatibility check of US5).

## 2. Labels gone (US1)

```sh
cargo test --features automation --test us18_designer
```

Checks the Pipelines tab's HTML (through the visual fixture) for Standard, Solo and a phase-only reviewer: no "verifying"/"not verifying" text, no `pill green|orange` in the list, the designer or "Based on"; the `checks` sentences; the "Done, not verified by Fallow" sentence with its `missing`. A loop on Solo ends with "Done, not verified by Fallow: no review at checkpoints".

By eye, as 017's check: `FALLOW_VISUAL_ROOT=<empty folder> cargo test --test visual_018 --features automation -- --ignored --nocapture` serves the Pipelines tab with Standard, Solo, `parallel` and a pipeline using every point; then `specs/018-pipeline-orchestration/checks/pipelines.js` (as `specs/017-agent-file-fields/checks/agents.js`) at 1440 × 900 and 390 × 844: no label, the points and their +, units drawn as branches, keyboard reach of every + (FR-029a).

## 3. Agents at every point (US2)

```sh
cargo test --features automation --test us18_points
```

A pipeline with a maker before each task, a checker after each task, two checkers alongside each other at every phase end, and a checker once at the end, on a three-phase feature. Expected: the order of steps follows contracts/pipeline.md "Points"; the two phase-end checkers' steps overlap in time; one failing makes one repair with its finding and both run again; a task-end finding is a repair of the task counted against `task_repairs`. Refusals: two makers alongside each other; a checker at a start with `on_fail = "repair"`; the gates with `at`.

## 4. Builders over [P] tasks (US3)

```sh
cargo test --features automation --test us18_builders
```

A feature whose phase 2 has `T004 [P]`…`T007 [P]` (T005 and T007 both change `calc/shared.py`) and a plain `T008`; pipeline `builders = 3`; fake steps that each take 2 s. Expected:
- three builder steps overlap; the fourth starts when one is accepted; never more than 3 (and never more than `max_parallel = 2` in the variant);
- each in `worktrees/task-T00x` on `<build>-T00x`;
- T004, T005, T006 merged in order; T007 held back with `TaskOverlap { files: ["calc/shared.py"], with: ["T005"] }`, repaired on the working commit, gated, merged;
- the group's gates run on the merged head before T008's step is queued;
- a group gate failure (a fake gate that fails on the merged head only) leads to one group repair;
- a restart while three builders run: each resumes in its own session and worktree; no `TaskMerged` twice;
- the run's cost limit reached with builders running: no new start, `LimitReached`, the loop asks;
- with `builders = 1`, the same feature runs as 0.4.0 (one session, no `GroupStarted`).

## 5. The lock statement (US4)

```sh
cargo test --features automation --test us18_lock
cargo test --lib run::groups
```

Features with 0, 1 and 4 `[P]` tasks per phase, pipelines with `builders` 1 and 3, `max_parallel` 4 and 2: the feature view's `statement` lines as research R-11, and SC-007 (the most builders observed equals the statement).

## 6. Existing pipelines and loops (US5)

```sh
cargo test --features automation --test us18_existing
cargo test --lib defs::pipeline
```

A data folder as 0.4.0 left it, written by the test itself (the 0.4.0 shipped pipeline texts kept as constants in the test): unedited Standard, edited Test first, an own pipeline with a path-limited security reviewer alongside the reviewer, a project pipeline with a docs-writer once at the end, a loop stopped mid-phase, a finished Solo loop. Expected: nothing rewritten (file hashes unchanged), `parallel.toml` added, every pipeline loads with the same steps and `verifying`, `to_toml` identical to 0.4.0's, the stopped loop resumes without repeating a step, the finished loop's page as before.

## 7. Budgets

The CI `bench` job (`.github/workflows/ci.yml`), unchanged reference run: idle and running memory, window ready, activity latency.
