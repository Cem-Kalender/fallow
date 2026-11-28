# Contract: pipelines (018)

**Replaces** 007 contracts/pipeline.md's Format table rows `when`, `at`, `parallel_with`, its Checks, and the sentence "The designer shows the same warning while the pipeline is being edited"; **adds** the `builders` cap, the points and the `parallel` default. Where pipelines live, which wins, and "Verifying or not" (the rule) are unchanged.

## Format

```toml
name = "fan-out"
description = "Up to 3 builders on [P] tasks, a check after each task, three checkers at each checkpoint"

[caps]
task_repairs = 3        # failures per task (and per point outside checkpoints) before asking the engineer
review_rounds = 3       # failed checkpoints per story before asking the engineer
builders = 3            # 018: the most builders at once on a group of [P] tasks; 1..=8, default 1

[[step]]
agent = "test-writer"
when = "task"
at = "start"            # 018: before each task
on_fail = "engineer"

[[step]]
agent = "builder"       # the one maker per task without `at`
when = "task"
on_fail = "repair"

[[step]]
agent = "fallow:gates"
when = "task"
on_fail = "repair"

[[step]]
agent = "lint-checker"
when = "task"
at = "end"              # 018: after each task, on its commit
on_fail = "repair"

[[step]]
agent = "reviewer"
when = "checkpoint"
on_fail = "repair"
parallel_with = ["security-reviewer", "tester"]

[[step]]
agent = "security-reviewer"
when = "checkpoint"
on_fail = "repair"

[[step]]
agent = "tester"
when = "checkpoint"
on_fail = "repair"
```

| Key | Rules |
|---|---|
| `caps.builders` | 1..=8, default 1, written only when not 1. With more than 1, each group of `[P]` tasks (consecutive, in one phase, at least two) runs up to that many at once, each in its own working copy (spec FR-011 to FR-020). |
| `when` | `task`, `phase`, `checkpoint`, `once`, as 007. |
| `at` | `"start"` or `"end"` (default) for every step except the builder and `fallow:gates`. For `task`: before or after each task. A checker with `when = "task"` and no `at` runs after the task. |
| `paths` | As 007; at a task point, the task's own changes. |
| `on_fail` | `repair`: back to the builder of the work the point covers (the task; the phase's session for phase and once points; the story for checkpoints). `engineer`: the loop asks. At a start point a checker must use `engineer`. |
| `parallel_with` | Checkers at the same point (same `when` and `at`) that run at the same time, judged together when all have finished; the point passes only when all passed; their findings go to one repair, then all run again (spec FR-006). At every point. |

## Points, in the order a loop runs them

1. `once`, `at = start`
2. for each phase: `phase` start; `checkpoint` start of stories whose first phase this is; for each task (or group, below): `task` start, the builder, `fallow:gates` per task, `task` end; `fallow:gates` per phase; the handover; `phase` end; `checkpoint` end of stories whose last phase this is
3. `once`, `at = end`

Within a point: units in pipeline order; a maker is a unit of one.

**A group** (`builders > 1`, two or more consecutive `[P]` tasks): each task's task points, builder and gates run in its own working copy, up to `builders` tasks at once. Accepted tasks are merged in `tasks.md` order after the overlap check (a shared file: no merge, a repair on top, reported); when all are merged, the gates run on the merged result; then the next task.

## Checks (refused with the step's index)

- Exactly one maker with `when = "task"` and no `at`: the builder.
- `fallow:gates` only with `when = "task"` or `"phase"`, without `at` and without `paths`; the builder without `paths`.
- Every named agent exists (017 FR-019 wording). `on_fail = "repair"` only when a builder exists.
- `parallel_with` only between checkers at the same point.
- A checker with `at = "start"` and `on_fail = "repair"`: refused, "nothing of that point's work is built yet".
- `caps.builders` outside 1..=8: refused.

Every file valid under 0.4.0 stays valid, with the same meaning (spec FR-025, FR-027).

## Verifying or not (unchanged, 007 FR-015)

`fallow:gates` with `when = "task"`, and at least one checker at the end of every checkpoint without `paths`. Checkers at other points do not count. The designer and the pipeline list do not label a pipeline by it (spec FR-001); the designer states `checks` and, when not verifying, what a result will say (FR-002).

## The defaults

| Name | Steps |
|---|---|
| standard | unchanged |
| test-first | unchanged |
| solo | unchanged |
| parallel | **new.** `builders = 3`; builder per task, gates per task, reviewer alongside security-reviewer at checkpoints |
