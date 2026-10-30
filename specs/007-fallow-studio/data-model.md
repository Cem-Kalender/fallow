# Data Model: Fallow Studio

Entities, their fields, their rules, and how they change state. Storage is described in contracts/storage.md. 006's run entities (Step, GateRun, Verdict, SteeringEntry) are kept as they are, and are not repeated here.

## Project

| Field | Type | Rules |
|---|---|---|
| id | string | Short, stable, derived from the folder name, made unique |
| path | absolute path | Must exist when added; a missing path marks the project `missing` (spec, edge cases) |
| name | string | Defaults to the folder name |
| kind | `git` \| `folder` | Read at each start; a `folder` becomes `git` after the engineer accepts `git init` |
| spec_kit | `{version, installed_by_fallow}` | The version found, or the one installed (S-15) |
| pipeline | pipeline name | Default `standard` |
| autonomy | `strict` \| `normal` \| `free` | Default `normal` (FR-020a) |
| approved_commands | list of command prefixes | Each passes 006's command check: one program and what it runs, no chaining, no git or shells |
| budget_per_task | `{usd, minutes}` | Default `{0.25, 10}` (FR-016a) |
| notifications | bool | Default on (FR-003a) |
| conversation | conversation id | The project conversation (FR-034) |

**States**: `ok` and `missing`. Removing a project from Fallow deletes nothing in its folder (FR-001).

## Feature

| Field | Type | Rules |
|---|---|---|
| project | project id | |
| dir | `specs/NNN-name` | NNN is the next free number. Fallow numbers features the way Spec Kit's `feature_numbering` does |
| base_branch | branch name | The branch the feature started from; the merge target (S-9) |
| branch | `NNN-name` | Git projects only, created from `base_branch` when the feature starts. Documents, locks and answers are committed here (S-6, S-9) |
| build_branch | `fallow/NNN/build` | Created from the lock commit when the loop starts. Tasks, ticks and handovers are committed here (S-9) |
| stage | see below | |
| spec_session | conversation id | Forked from the project conversation (S-5) |
| step | Spec Kit step | `constitution`, `specify`, `clarify`, `plan`, `tasks`, `analyze`, `done` |
| pipeline, autonomy | | The project's values until set for this feature; fixed at the lock |
| locks | list of Lock | Newest last |
| loop | loop id | Set at the first lock |

**Stages**:

```text
spec_session ──lock──▶ locked ──loop starts──▶ building ──▶ ready_to_merge ──merge──▶ merged
      ▲                                           │  ▲            │
      └──────────────── unlock ───────────────────┘  │            └─▶ done_unverified ──merge──▶ merged
                                                      └── relock (affected stories reopen)
```

- `done_unverified` is the end state of a loop whose pipeline lacks gates or checkers (FR-015). It is shown as such, never as ready.
- `building` covers paused and waiting loops. Their reason is on the loop.

## Conversation

| Field | Type | Rules |
|---|---|---|
| id | string | |
| kind | `project` \| `spec` | |
| project, feature? | ids | |
| session | runtime session id | Null until the first turn. A spec session forks from its project's session (S-5) |
| worktree | path \| null | Git projects: Fallow's worktree for the conversation (S-6) |
| mode | `read_only` \| `approved` \| `ask` \| `run_anything` | Starts as `ask`; changed by the engineer only (FR-007a) |
| turns | list of Turn | |

**Turn**:
- `n`, `said`: the engineer's text;
- `edits[]`: the documents the engineer edited since the last turn, with their diffs;
- `reply`, `activity[]`;
- `documents_changed[]`;
- `commit?`;
- `discarded[]`: the changes outside the spec paths that were thrown away;
- `approvals[]`;
- `cost_usd`;
- `proposed_feature?`, `proposed_pipeline?`.

**Approval**:
- `command`;
- `answer`: `once`, `always` or `deny`;
- `prefix?`: the command prefix added for `always`;
- `at`.

## Lock

| Field | Type | Rules |
|---|---|---|
| n | int | 1 for the first lock |
| commit | sha | Commits spec, plan, tasks, the constitution and `.fallow/` changes on the feature branch |
| at | time | |
| pipeline, autonomy | | Fixed for the loop until the next lock |
| budget | `{usd, minutes}` | Suggested as tasks × budget_per_task, then edited by the engineer (FR-016a) |
| affected | list of `USn` | Lock 2 onwards: the stories reopened (S-17) |

**Checks** before a lock is allowed (S-8):
- no open clarification markers;
- analyze is done with no critical finding;
- `tasks.md` and `spec.md` parse;
- every user story has a must-fail scenario.

## Loop

006's Run, with these changes. It is a "loop" on the page and in the spec, and is still stored as a run (`runs\<id>`) and called `Run` in the code:

| Field | Type | Rules |
|---|---|---|
| feature, lock | ids | |
| baseline | `{commit, gates}` | Gates on the lock commit |
| phases | list of Phase | From `tasks.md` at the lock |
| sessions | map of phase to session id | A fresh session per phase (FR-010a) |
| questions | list of Question | |
| budget, used | `{usd, minutes}` | Paused time and time spent waiting for the engineer do not count |
| state | `running`, `paused`, `waiting`, `stopped`, `ready`, `done_unverified`, `merged` | `waiting` names what it waits for: the engineer, a slot, or a limit |

**Phase**:
- `n`, `title`;
- `story?`: `USn`;
- `tasks[]`;
- `state`: `pending`, `building`, `checkpoint`, `repairing`, `done`, `reopened` or `waiting`;
- `handover?`: the path of `handover/phase-N.md`, once committed;
- `review_rounds`.

**Task**:
- `id`: T###;
- `phase`;
- `story?`;
- `parallel`: the `[P]` mark, shown but not acted on in this version;
- `text`;
- `state`: `pending`, `current`, `accepted`, `failed` or `waiting`;
- `commit?`, `gate_run?`, `repairs`.

**Rules**:
- A task is `accepted` only after Fallow's checks: its claim, its commit and the gates against the baseline (FR-011).
- A phase is `done` when all its tasks are accepted, its handover is committed and not empty, and its checkpoint steps passed.
- A loop is `ready` when every phase is done and the pipeline is verifying (S-12). Otherwise it ends `done_unverified`.

## Question

| Field | Type | Rules |
|---|---|---|
| id | string | |
| loop, phase, task, step | ids | |
| asked_by | agent name | |
| text, options[] | | |
| state | `with_lead` \| `answered` \| `decided` \| `waiting_engineer` \| `overruled` | |
| by | `lead` \| `engineer` | |
| answer, recommendation? | | |
| commit | sha | The commit of the answer to `spec.md`'s Clarifications |
| affects | `USn` | The story the question is about. While it waits for the engineer, the loop waits (FR-021) |

**Transitions**:
- `with_lead` → `answered` or `decided`, which the engineer can later move to `overruled`;
- `with_lead` → `waiting_engineer` → `answered` (by the engineer).

**Rule**: a question is relayed to the engineer whatever the autonomy level when it touches an item the spec marks as the engineer's, or when it would change an acceptance scenario (FR-020a).

## Agent definition

| Field | Type | Rules |
|---|---|---|
| name | `[a-z0-9-]+` | Unique within its source; a project override replaces the user's by name |
| description | string | |
| runtime | runtime name | Must be a known runtime; `claude` in this version |
| model | string | Passed to the runtime; empty means the runtime's default |
| kind | `maker` \| `checker` \| `lead` | |
| may_run | list | Command prefixes, plus `gates` and `plan.commands` |
| returns | `work` \| `verdict` \| `reply` | Must match the kind |
| limits | `{minutes, turns, usd}` | Per step; optional |
| body | markdown | The standing instruction |
| source | `default` \| `user` \| `project` | |

## Pipeline

| Field | Type | Rules |
|---|---|---|
| name, description | | |
| steps | list of PipelineStep | Validity rules in S-12 |
| caps | `{task_repairs: 3, review_rounds: 3}` | |
| source | `default` \| `user` \| `project` | |

**PipelineStep**:
- `agent`: a definition's name, or `fallow:gates`;
- `when`: `task`, `phase`, `checkpoint` or `once`;
- `at?`: `start` or `end`, for `phase`, `checkpoint` and `once`;
- `paths[]?`;
- `on_fail`: `repair` or `engineer`;
- `parallel_with?`: the checker steps at the same point that run together.

**Derived**: `verifying`, true when `fallow:gates` runs per task and at least one checker runs at every checkpoint. When it is false, the steps that are missing are listed.

## Handover

A markdown file, `specs/NNN-name/handover/phase-N.md`, written by the phase's builder and committed by Fallow.

| Section | Content |
|---|---|
| Built | What the phase built, and where |
| Decisions | The choices made, and the questions answered on the way, with their commits |
| For the next phase | What the next builder should know |

It must have all three sections and not be empty (FR-010a).

## Activity event

From the runtime interface (S-13). Each event has:
- `t`, `agent`, `task?`;
- `kind`: `say`, `read`, `edit`, `write`, `run`, `search`, `ask`, `answer`, `check` or `error`;
- `text`;
- `detail?`: a diff, a command's output, or a question;
- `result?`: `ok`, `failed` or `refused`.

The kinds `check` (Fallow's commits, gates and verdicts) and `answer` are written by Fallow itself.

## Runtime

| Field | Type |
|---|---|
| name | `claude`, or `fake` in tests |
| installed, version | Found at start |
| models | Known model aliases |
