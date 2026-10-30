# Contract: pipelines

A pipeline is a TOML file named `<name>.toml`. Fallow looks for it in three places, and the last one found wins:

1. built into Fallow (the defaults);
2. `~/.fallow/pipelines/`;
3. `.fallow/pipelines/` in a project.

The spec session, the lock and the merge are not steps of a pipeline: every loop has them (spec, User Story 2).

## Format

```toml
name = "standard-security"
description = "Standard, plus a security review when api/ or auth/ changes"

[caps]
task_repairs = 3        # gate failures per task before asking the engineer
review_rounds = 3       # failed checkpoints per story before asking the engineer

[[step]]
agent = "builder"
when = "task"           # task | phase | checkpoint | once
on_fail = "repair"

[[step]]
agent = "fallow:gates"  # built in: Fallow runs the project's gates against the baseline
when = "task"
on_fail = "repair"

[[step]]
agent = "reviewer"
when = "checkpoint"
on_fail = "repair"

[[step]]
agent = "security-reviewer"
when = "checkpoint"
paths = ["api/**", "auth/**"]
parallel_with = ["reviewer"]
on_fail = "repair"

[[step]]
agent = "docs-writer"
when = "once"
at = "end"
on_fail = "engineer"
```

| Key | Rules |
|---|---|
| `agent` | A definition's name, or `fallow:gates` |
| `when` | `task`: every task. `phase`: every phase. `checkpoint`: every user story's phase. `once`: the whole loop |
| `at` | For `phase`, `checkpoint` and `once`: `"start"` or `"end"` (the default). A checkpoint step at the end is the story's checkpoint |
| `paths` | Globs. The step runs only when the changes since its last run touch one of them |
| `on_fail` | `repair`: back to the phase's builder session with the reasons. `engineer`: the loop asks |
| `parallel_with` | Checker steps at the same point that run at the same time, within the concurrency cap |

## Checks (refused with the step's index)

- Exactly one `maker` step with `when = "task"`: the builder.
- `fallow:gates` only with `when = "task"` or `"phase"`.
- Every named agent exists. `on_fail = "repair"` only when a builder exists.
- A `when = "task"` step that is a checker is refused: checkers run at phases and checkpoints.
- A checker at the start (`at = "start"`) is refused: there is nothing to check yet.

## Verifying or not (FR-015)

A pipeline is **verifying** when both hold:
- `fallow:gates` runs with `when = "task"`;
- at least one checker runs at the end of every checkpoint, with no `paths` limit.

A loop with a pipeline that is not verifying ends as `done_unverified`, and the page names what is missing ("no gates", "no review at checkpoints"). The designer shows the same warning while the pipeline is being edited.

## The defaults

| Name | Steps |
|---|---|
| standard | builder per task, gates per task, reviewer at checkpoints |
| test-first | test-writer at the start of every user story's phase (`when = "checkpoint"`, `at = "start"`), builder per task, gates per task, reviewer at checkpoints |
| solo | builder per task, gates per task. Not verifying: no review |
