# Data Model: Agent Squads and Coordination Mode

Entities from [spec.md](./spec.md) "Key Entities". 003's `Hub`, `Permission`, `Task`, `Event`,
`Delivery`, `Party` and 002's `Session`, `Worktree` are reused. Everything lives in the in-memory hub
and ends with Fallow (spec FR-029); the squad file is read, never written.

## Party (003, extended)

`Party::Fallow` joins `User` and `Agent { session, role, profile, project }`: Fallow's own signals
(gates) and board posts (briefings, "coordination ended"). No agent can act as it. `Agent` gains
`role: Option<RoleName>`.

## Burst

One per project at a time.

| Field | Type | Rules |
|---|---|---|
| project | ProjectId | The lead's project. |
| lead | SessionId | Exactly one; the engineer can name another (FR-003). |
| permission | PermissionId | The 003 standing permission: source and target `Project(project)`, deliver + create tasks + start up to `allowance` (FR-002). |
| allowance | u8 | Sessions the burst may start; default 3, `1..=sessions.max_sessions` (FR-001). |
| started / ends | Instant | `ends - started` ≤ 8 h; extending replaces `ends` with a new grant (FR-004). |
| squad | Squad | Frozen copy of the squad in force when the burst started (FR-018). |
| start_commit | String | `HEAD` of the project when the burst starts (baseline, FR-021). |
| target_branch | String | Squad's, or the branch checked out at start. |
| roles | Map<SessionId, RoleName> | Fixed per session for its life (FR-009). |
| state | Running \| Ended(reason) | reason: time, engineer, Fallow quit. |

Transitions: `Running → Ended(time | engineer)`; an extension keeps `Running`. A pending burst from
the launch option (`--coordinate`) waits for the first session of the project; nothing is created
before it (constitution II).

## Role

| Field | Type | Rules |
|---|---|---|
| name | String | `[a-z][a-z0-9-]{0,31}`, unique in the squad. |
| description | String | 1..=200 chars, shown in briefings and `session_list`. |
| level | ReadOnly \| Edit \| EditRun | Mapped to agent flags (research R-2), or `flags` override. |
| agent | profile id | Default `claude`. |
| model | Option<String> | Passed with the agent's model flag. |
| tools | All \| set of Tool | Tool = board, events, deliver, tasks, report, start, sessions. |
| signals | Vec<Pattern> | Events this role may signal (FR-012). |
| max | u8 | Sessions of this role at once, `1..=16`. |
| flags | Map<profile id, Vec<String>> | Optional per-agent override. |

A role whose resolved flags bypass the agent's permission checks (research R-2 lists them per agent)
carries `bypasses = true` and needs the engineer's confirmation before its session starts (FR-014).

## Squad

| Field | Type | Rules |
|---|---|---|
| name | String | |
| source | Builtin \| File(path) | |
| roles | Vec<Role> | Must contain `lead`. |
| gates | Vec<Gate> | |
| publish | DraftPr \| LocalMerge | `auto` resolved at burst start: DraftPr if the project has a remote. |
| merge | OnApproval \| Automatic | Local merge only. |
| merge_requires | Vec<Condition> | Condition = Gate(name) \| AllDone(role) \| Signal(event, role). |
| repair_cap | u8 | `0..=10`, default 2. |
| wait_notice | Duration | `1..=120` min, default 10. |

## Gate and GateRun

| Gate field | Type | Rules |
|---|---|---|
| name | String | Events `gate-<name>-passed` / `-failed` are Fallow's alone (FR-020). |
| command | String | Plus optional `windows` / `macos` / `linux` overrides (FR-022a). |
| timeout | Duration | `1..=240` min, default 30 (FR-022). |
| baseline | bool | Run on `start_commit` before builders begin (FR-021). |
| failures | Auto \| ExitCode \| Regex | How failing checks are named (research R-5). |

| GateRun field | Type | Rules |
|---|---|---|
| gate | name | |
| kind | Baseline \| Final \| Repair | |
| session | SessionId | A command session (profile with no agent, no orchestration). |
| commit | String | What it ran against. |
| state | Running \| Passed \| Failed(exit) \| TimedOut | From the session's exit status only. |
| failing | Set<String> | Names of failing checks, from the session's output in memory. |
| class | Pass \| BaselineOnly \| Regression \| Fail | Final runs: compared with the baseline's `failing`; `Fail` when there is no baseline. |

## Evidence

Attached to a task report during a burst (FR-023).

| Field | Type | Rules |
|---|---|---|
| commit | String | 7..=40 hex; Fallow checks it is on the reporting session's branch (`git merge-base --is-ancestor`). |
| commands | Vec<(command, result)> | ≤ 20, each ≤ 500 chars; claims, not verified. |
| files | Vec<String> | ≤ 200 paths; Fallow compares with `git diff --name-only <start_commit>..<commit>` and notes differences. |
| verified | Vec<String> | What Fallow checked ("commit on fallow/claude-2", "files match"). |

## RepairLoop

| Field | Type | Rules |
|---|---|---|
| part | TaskId | The task the failure concerns; `None` for failures not tied to one part (they go to the lead). |
| owner | SessionId | Recorded at the part's hand-over (FR-025a). |
| rounds | u8 | Increments when a repair task is created. |
| cap | u8 | From the squad; at `cap`, the next failure asks the engineer (FR-025). |

## MergeReadiness

Derived, shown in the squad view: each `merge_requires` condition with met / unmet and why, the
gated commit, and the publish mode. "Ready" when every condition is met. With local merge, the
engineer's merge records `merged_by_fallow` commits; any other new commit on `target_branch` during
the burst raises a warning (FR-028a).

## Limits

| Item | Limit |
|---|---|
| Burst duration | ≤ 8 h (constitution IV) |
| Burst session allowance | default 3, ≤ `sessions.max_sessions` |
| Roles per squad | 16 |
| Gates per squad | 8 |
| Evidence commands / files | 20 / 200 |
| Gate output kept for failure names | the session's scrollback (002 limits) |
