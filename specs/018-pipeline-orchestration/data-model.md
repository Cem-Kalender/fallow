# Data model: Pipelines as an orchestration system (018)

Only what 018 adds or changes. Everything else is as 007, 008, 011 and 017 describe it.

## Pipeline (file, `src/defs/pipeline.rs`)

| Field | Change |
|---|---|
| `caps.builders` | **New.** `u32`, 1..=8, default 1; not written when 1 (research R-1). |
| `step.at` | Now allowed on `when = "task"` steps other than the builder and `fallow:gates`. |
| `step.parallel_with` | Same field; honoured at every point (research R-3). |

Checks (refused with the step's number; contracts/pipeline.md):
- exactly one maker with `when = "task"` and no `at` (the builder);
- `fallow:gates` with `when = "task"` or `"phase"`, no `at`, no `paths`;
- `parallel_with` only between checkers at the same point (`when` and `at` equal);
- a checker at a start point (`at = "start"`) only with `on_fail = "engineer"`;
- `builders` within 1..=8.

Derived, not stored:
- `verifying`, `missing`: unchanged rule (007 FR-015).
- `checks: Vec<String>`: plain sentences of what the pipeline checks (research R-12).
- `Point` of a step: `(when, at)` with task-end for a task checker without `at`.

## Point (runner, `src/run/feature.rs`)

`OnceStart | PhaseStart(n) | TaskStart(id) | TaskEnd(id) | PhaseEnd(n) | CheckpointStart(story) | CheckpointEnd(story) | OnceEnd`. Order fixed by `tasks.md` (spec FR-004). A point holds units in pipeline order.

## Unit

The steps at one point connected by `parallel_with` (checkers only), or one step alone. Queued together, judged together, one repair for their findings.

| Unit | Part of a member's step | Part of the unit's repair |
|---|---|---|
| one step, phase / once / checkpoint start | as 0.4.0 (`<agent>@phase-N-start`, …) | as 0.4.0 (the step's part) |
| several checkers, phase / once point | each member its own part | the point: `phase-N-start|end`, `once-start|end` |
| task point (one or several) | `<agent>@<task>-start|end` | the task (`part = task id`, `task = id`) |
| checkpoint end | the story (as 008) | the story (as 008) |

## Group (`src/run/groups.rs`, derived from the locked `tasks.md` and the pipeline)

| Field | Meaning |
|---|---|
| `phase` | the phase number |
| `tasks` | consecutive `[P]` task ids of the phase, at least two, in `tasks.md` order |
| `base` | the build head when the group started (event `GroupStarted`) |
| per task: `branch`, `worktree` | `<build_branch>-<id>`, `worktrees/task-<id>` |
| per task: `base` | `base`, or the working commit's build parent after an overlap |
| per task: `merged` | the merge commit on the build branch (event `TaskMerged`) |
| per task: `overlaps` | each overlap: files and the tasks that changed them (event `TaskOverlap`) |
| `lock` | the loop's lock when the group started (R-6a) |
| `done` | merged and the merged result gated (event `GroupDone`), or dropped by a new lock (`GroupDropped`) |
| `dropped` | dropped by a new lock: its unmerged tasks were reset (R-6a) |

State of a group task: `queued → building → (repairing)* → accepted → (overlap → repairing → accepted)* → merged`; the group: `started → all merged → gated → done`, with `repairing` on a failed group gate.

Only when `caps.builders > 1` and the run of `[P]` tasks has at least two tasks; otherwise there is no group and tasks run as in 0.4.0.

## Events (`src/run/events.rs`), appended to the run's log

| Event | Fields | When |
|---|---|---|
| `GroupStarted` | `phase: u32, tasks: Vec<String>, base: String` | the runner reaches a group's first task |
| `TaskMerged` | `task: String, commit: String` | a group task was merged into the build branch and ticked |
| `TaskOverlap` | `task: String, files: Vec<String>, with: Vec<String>, working: String` | a group task was held back; `working` is the commit its repair starts from |
| `GroupDone` | `phase: u32, first: String, tasks: Vec<String>, left: Vec<String>` | the merged result of the group passed the gates; `left`: what could not be removed |
| `GroupDropped` | `phase: u32, tasks: Vec<String>, dropped: Vec<String>, left: Vec<String>` | a new lock was taken in while the group was in progress (R-6a): `dropped` were reset to not built |

Each is folded into `LoopState.groups: Vec<GroupState>` (`#[serde(default)]`, so state from before 018 reads as no groups). `Step` is unchanged: a group task's steps carry `task`, `phase`, `def` and their `worktree` as today.

## LoopState (`src/run/state.rs`)

| Field | Change |
|---|---|
| `groups` | **New**, `Vec<GroupState>`, default empty. |
| `pipeline` | Unchanged: the TOML recorded at the lock; with one builder, byte-identical to 0.4.0's. |

## Feature view (`GET` of a feature, `engine.rs`)

Each entry of `pipelines` adds `builders: u32` and `statement: Vec<String>` (research R-11).

## Pipelines view (`GET /api/pipelines`)

Each pipeline adds `checks: Vec<String>`; `caps` carries `builders`. `verifying` and `missing` stay (used by the agents page and the loop page); the Pipelines tab no longer renders them as a label.

## Loop view (`src/run/activity.rs`)

`pipeline.caps.builders`; `pipeline.steps[].at` for task steps; `groups: [{ phase, tasks: [{ id, state, worktree, merged, overlaps: [{files, with}] }], done }]`.

## Run record (`src/measure/record.rs`)

`builders_cap`, `builders_most`, `groups_parallel`, `overlap_repairs`: `u32`, `#[serde(default)]`. `PIPELINES` adds `parallel`.
