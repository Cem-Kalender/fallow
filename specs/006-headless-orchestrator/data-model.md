# Data Model: Headless Orchestrator

All state lives in a run's event log (research R-2). The entities below are what the fold of the
log produces. They are also what the page reads.

## Run

| Field | Type | Rules |
|---|---|---|
| `id` | integer | Unique per service, increasing. Names the run's branches (`fallow/r<id>`) |
| `repo` | path | The repository the run was started in. Must be a git repository with at least one commit |
| `target` | branch name | The branch the result is meant for. Default: the branch checked out at start |
| `start_commit` | commit id | The commit the run starts from. Uncommitted changes are left out, and the engineer is told |
| `task` | text | The task as the engineer wrote it |
| `mode` | `solo` \| `squad` \| `plain` | `plain` only in measured runs (FR-029). Solo vs squad follows the approved plan (`builder` vs `parts`) |
| `stage` | see transitions | |
| `autonomy` | `high` \| `medium` \| `low` | Default `medium` (FR-017). Each change is an event with its time |
| `limits` | cost (USD), time (min) | Default $10 and 120 min (FR-038), from settings, the squad file or the run |
| `roles` | role → (agent, model) | Resolved at start from settings, then the squad file, then the run (FR-037) |
| `repair_cap` | integer | Default 2 per part (FR-008) |
| `switched_off` | set of components | Only for measured runs (FR-030) |
| `recorded` | boolean | Whether a run record is written (FR-028) |

**Stage transitions**:

```text
planning ──approve──► building ──► integrating ──► gating ──► reviewing ──► ready ──► publishing ──► done
   │                    ▲  │            │             │            │           │           │
   │                    │  └─(part)─────┴─────────────┴────────────┘           │           │
   │                    └──────────── repair (≤ cap) ◄─────────────────────────┴───────────┘
   └── any stage ──► paused (limit reached, engineer pause) ──► (back to the same stage)
   └── any stage ──► stopped (engineer stop or reject)
```

- `ready`: verified, waiting for the engineer's merge or pull request.
- `publishing`: pull request open, waiting for checks and reviews (US6). Without a protected
  remote, `ready → done` happens on the engineer's local merge.
- `paused` remembers the stage it came from. `done` and `stopped` are final.

## Plan

The committed `.fallow/work.toml` on the plan branch, in 005's format with the changes in research
R-8.

| Field | Rules |
|---|---|
| `commit` | The plan branch commit in force. Every decision moves it |
| `build` | `builder` (one part, `main`) or `parts` |
| `parts[]` | `id`, `owns` (globs: code and its tests, no overlap between parts), `criteria[]` |
| `criteria[]` | `id` unique within its part; `text`; `must_fail`. Addressed as `part/id`. At least one `must_fail` per part |
| `contracts[]` | `path`, `between` (two parts). One committed contract per boundary between parts |
| `shared` | globs any part may change |
| `docs` | prose documents (Spec Kit's spec, plan, tasks where present) |

A plan is **approvable** only when it parses, passes the checks above, and is committed on the plan
branch.

## Part

| Field | Rules |
|---|---|
| `id` | From the plan; `main` in solo |
| `branch` | `fallow/r<run>/<part>` |
| `session` | The builder's agent session id, for resuming and taking over |
| `state` | `waiting` \| `building` \| `blocked` \| `done` \| `reopened` \| `repairing` \| `with_engineer` \| `paused` |
| `result_commit` | The last verified commit |
| `repairs` | Count, against the run's `repair_cap` |
| `takes` | Decision commits the part must contain (R-9). A result missing one is refused |

`done → reopened` happens when a decision changes the part's criteria or a contract it is `between`
(FR-013), and when the engineer adds a criterion to it.

## Step

One non-interactive agent call (research R-3), or one action of Fallow's own (gate, integrate,
publish), which is recorded the same way.

| Field | Rules |
|---|---|
| `n` | Sequence within the run |
| `kind` | `plan_turn`, `build`, `take`, `repair`, `decide`, `review`, `integrate`, `gate`, `publish`, `plain` |
| `role` | `planner`, `builder`, `reviewer`, or `fallow` for Fallow's own steps |
| `part` | For build, take and repair steps |
| `agent`, `model` | From the run's roles |
| `session` | Agent session id; a new uuid, or the one being resumed |
| `worktree` | The step's working copy |
| `base` | The commit the step started from |
| `instruction` | The text given to the agent (kept in the run directory, never in a run record) |
| `limits` | Time (default 30 min per step) and the cost left in the run |
| `state` | `queued` \| `waiting_capacity` \| `running` \| `finished` \| `interrupted` |
| `result` | The structured result (contracts/step-results.md), or the failure reason |
| `verification` | Outcome of Fallow's checks (R-7): `accepted` or `rejected`, with reasons |
| `cost_usd`, `duration` | As reported (`total_cost_usd`), and wall-clock |
| `activity` | `steps/<n>.jsonl`: reads, edits, commands, outputs, progress lines |

A step is `interrupted` when the service stopped while it ran. On resume it becomes `running` again
under the same session, or is redone (R-2).

## Decision

| Field | Rules |
|---|---|
| `id` | Sequence within the run |
| `question` | As asked (`ask_decision`), with the asking step and part |
| `recommendation` | The planner's, when the engineer is asked |
| `decided_by` | `planner` \| `engineer` |
| `answer` | Text |
| `commit` | The plan branch commit that records it; none if it changes nothing |
| `state` | `open` \| `deciding` \| `needs_engineer` \| `answered` \| `overruled` |
| `affected_parts` | Computed from the commit (R-9) |

The engineer is asked (`needs_engineer`) when the planner cannot decide, or when the autonomy level
reserves the question: Medium and Low for contract or criteria changes.

## Gate run

| Field | Rules |
|---|---|
| `gate` | The declared name |
| `commit` | What it ran on: the start commit (baseline), a part's result, or the integration |
| `kind` | `baseline` \| `part` \| `integration` |
| `outcome` | `passed` \| `failed` \| `timed_out`, with the failing tests |
| `class` | Against the baseline: `clean`, `pre-existing failures only` or `regression` (004) |

## Verdict

| Field | Rules |
|---|---|
| `step` | The review step |
| `answers[]` | `criterion` (`part/id`), `ran` (what was run), `result`, `passed`; for must-fail criteria, the failing case |
| `complete` | Every criterion of the review's scope answered, with must-fail evidence. Partial verdicts in one review add up |
| `approved` | Complete, and every answer passed |

## Steering action

| Field | Rules |
|---|---|
| `kind` | `approve_plan`, `answer`, `overrule`, `add_criterion`, `change_criterion`, `note`, `pause`, `resume`, `stop`, `take_over`, `hand_back`, `set_autonomy`, `set_limits`, `merge`, `request_changes`, `reject` |
| `at` | Time, and the run's stage |
| `target` | A part, step, decision or criterion, where it applies |
| `detail` | The engineer's text (comment, note, criterion) |
| `effects` | Events it caused (steps queued, parts reopened) |

Every steering action counts as an intervention (FR-020), except `approve_plan` and `merge`, which
the success criteria count separately.

## Publication

| Field | Rules |
|---|---|
| `kind` | `local_merge` \| `pull_request` |
| `rules_source` | `branch_rules` \| `protection` \| `checks_only` \| `none`: where the CI knowledge came from (FR-025) |
| `required_checks`, `required_reviews`, `up_to_date_required` | From the rules |
| `pr` | Number and URL |
| `checks[]` | Name, state, required or not |
| `merged_commit` | When done |

## Run record (measured runs only)

005's record, unchanged in what it may hold (constitution V):
- mode, components switched off;
- fingerprints of the task and the hidden tests;
- agent, model and version identifiers per step;
- commit identifiers, start time, durations;
- counts (steps, repairs, decisions, interventions, hidden tests passed and failed);
- outcomes as fixed values;
- reported cost and tokens.

Parts and roles are referred to by number.

## Settings and squad file additions

See contracts/config.md:
- per-role agent and model;
- limits;
- builder allowlist additions;
- autonomy default;
- per-step time limit;
- concurrency cap.
