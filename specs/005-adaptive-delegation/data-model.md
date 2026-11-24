# Data Model: Adaptive Delegation

Builds on 004's model (Burst, Squad, Role, GateRun, Evidence, RepairLoop). In memory unless marked
**on disk** (run records only, constitution v5.0.0 Principle VII).

## WorkManifest (read from the repository)

Parsed from `.fallow/work.toml` at a given commit ([contracts/work-manifest.md](./contracts/work-manifest.md)).

| Field | Type | Rules |
|---|---|---|
| commit | commit id | the commit it was read at |
| docs | paths | prose the manifest refers to; parts may not own only these |
| shared | globs | files any part may change |
| test_patterns | globs | override of the squad's test patterns |
| work | [Work] | ids unique |
| contracts | [Contract] | paths exist at the commit |
| reviews | [ReviewFocus] | names unique |

Validation errors carry file and line (as 004's squad file).

## Work

| Field | Type | Rules |
|---|---|---|
| id | short name | unique |
| build | `lead` \| `builder` \| `features` \| `parts` | default `lead` when the work is absent |
| reason | text | required unless `lead` |
| criteria | [Criterion] | at least one before the work is built (FR-027) |

## Criterion

| Field | Type | Rules |
|---|---|---|
| id | short name | unique within its work or part |
| text | text | what must hold |
| must_fail | bool | a case that must be rejected; a verdict needs at least one check with expect fail |

## Part (for `build = "parts"` or `features`)

| Field | Type | Rules |
|---|---|---|
| id | short name | unique |
| work | Work id | |
| owns | globs | non-empty; no overlap with other parts; at least one file outside test patterns and docs (FR-004, FR-006) |
| criteria | [Criterion] | as Work |
| builder | session id | runtime: set when a builder starts with it |
| base | commit id | runtime: the lead's commit it started from |

## Contract

| Field | Type | Rules |
|---|---|---|
| path | path | committed at the base |
| between | [Part id; 2] | for `parts`, each boundary needs one (FR-004) |
| tests | gate name? | contract tests declared as a squad gate |

## Signal (memory only)

| Field | Type | Rules |
|---|---|---|
| id | number | |
| from | session | a builder |
| kind | `question` \| `change` | `change` carries a diff |
| work, part | ids | |
| text, diff | text | memory only |
| raised | time | unanswered after the squad's limit: told to the engineer once |
| answer | {commit?, text} | `commit` required when the answer changes a decision (FR-022) |

State: open → answered (with or without a commit) → closed when the work is done.

## Verdict (extends 004's)

`checks: [{criterion, ran, expect, result}]`. Counts toward the merge only if every criterion of the
reviewed work has a check and at least one check expects fail (FR-029). A reviewer's worktree change
voids it (FR-030).

## Burst additions

| Field | Type | Notes |
|---|---|---|
| manifest_commit | commit id | latest read |
| decisions | per Work: build, reason, builders | shown in the Squad tab |
| signals | [Signal] | |
| switched_off | set of Component | for recorded runs (FR-017) |
| run | Run? | when the burst is a recorded run |

Component: `reviewer`, `gates`, `contracts`, `plan_commit_check`, `criteria_precondition`,
`ownership_check`, `verdict_check`, `lead_updates`, `parallel_builders`.

## TaskSet (read from the engineer's directory)

From `taskset.toml` ([contracts/task-set.md](./contracts/task-set.md)): setup files, and per task a
name, project path, starting commit, description file, hidden files with their target paths, hidden
test commands and a time limit. Fallow reads it and never writes it; the hidden files are copied only
into a run's own copy after the run's sessions are closed.

## Run

| Field | Type | Notes |
|---|---|---|
| id | number | |
| task | TaskSet task | |
| mode | `plain` \| `solo` \| `squad` | |
| switched_off | set of Component | |
| series | {seed, position}? | in memory only, while the series runs |
| copy | directory | the run's own repository under Fallow's cache (research R-11) |
| fingerprints | {task, hidden} | `git hash-object`, line endings normalised (R-12) |
| sessions | [{number, role, agent, model, version}] | |
| start_commit, final_commit | commit ids | the tested state is the lead's worktree (the plain session's in plain mode) as left |
| started, ended | time | |
| per part/role (numbered) | working, blocked, signals, repair rounds, interventions, gate and review outcomes, cost reported or unknown | |
| hidden | {passed, failed} | tests that could not run count as failed |
| state | running → ended (waiting for the close confirmation) → testing → recorded \| discarded | discarded if Fallow exits before recorded |

**On disk** as a RunRecord when the run was recorded ([contracts/run-record.md](./contracts/run-record.md)):
only the fields the amended Principle VII allows; part and role names become numbers.

## MemoryBudget

| Field | Type | Notes |
|---|---|---|
| budget_mb | number | setting, default 75% of total memory minus Fallow's |
| per_profile_mb | map | largest resident size seen per agent profile; 700 MB until seen |
| waiting | [start request] | proceed when a sample shows room |
