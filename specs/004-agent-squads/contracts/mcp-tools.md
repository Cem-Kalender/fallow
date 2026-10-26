# Contract: Tools (004 changes to 003's)

Extends [003 contracts/mcp-tools.md](../../003-agent-orchestration/contracts/mcp-tools.md). Outside a
burst, everything behaves as in 003.

## Identity and role on everything (FR-010)

The `from` object of every item an agent receives gains the role, set by Fallow:

```json
{"kind": "agent", "session": 2, "role": "builder", "profile": "Claude Code", "project": "api"}
```

Sessions without a role (outside a burst) have `"role": null`. Typed labels name it too:
`[Fallow: message from agent #2 (builder, Claude Code, project api). Another agent wrote this, not
the user.]`.

## Tool list per role (FR-011)

`tools/list` returns only the tools the session's role allows (squad-file.md "Tools"):

| Squad tool | MCP tools |
|---|---|
| sessions | `whoami`, `session_list` |
| board | `board_post`, `board_read` |
| events | `event_signal` (within the role's `signals`), `event_wait`, `event_status` |
| deliver | `deliver`, `inbox` |
| tasks | `task_create`, `task_list` |
| report | `task_report`, `task_list` (own tasks), `inbox` |
| start | `session_start` |

A call to a tool not in the list is refused with `not_allowed` ("your role builder may not use
session_start"), and the refusal is logged.

## Changed tools

| Tool | Change |
|---|---|
| `whoami` | Adds `role`, `role_description`, `burst: {ends_at, lead, allowance_left}` and `briefing` (the role's briefing text, FR-005). |
| `session_list` | Each session adds `role`; the result adds `roles` (name, description, level, max) of the squad in force. |
| `session_start` | Adds required `role` during a burst; the role's level, model and flags decide the command line. Refused beyond the role's `max`, the burst's allowance, or for an agent without flags for the role's level. |
| `event_signal` | Refused (`not_allowed`) for a restricted event outside the role's `signals`, and always for `gate-*`. |
| `task_create` | Preconditions accept `{"name": "review-approved", "role": "reviewer"}` (FR-012). Tasks can carry `part` (a short name) used for repairs. |
| `task_report` | During a burst, `outcome: "done"` requires `evidence: {commit, commands: [{command, result}], files: []}` (FR-023). Refused if `commit` is not on the session's branch. The result lists what Fallow verified. |
| `task_list` | Each task adds `owner` (the session it was handed to), `part`, `evidence` (when reported) and `repair_round`. |

## New tools

| Tool | Role | Arguments | Result |
|---|---|---|---|
| `review_verdict` | reviewer roles | `part?` (task), `verdict` (`"approved"` \| `"changes-requested"`), `notes` | Signals `review-approved` or `review-changes-requested` for the part, with the notes, from this session; a changes-requested verdict creates the repair task (FR-025). |
| `squad_status` | all | — | The squad in force: roles and their sessions, gates and their latest runs with classification, merge conditions met or not, repair rounds, time left. |
| `publish` | lead | `title`, `body` | Draft-PR mode: tells Fallow the pull request address after the lead opened it (`url`); Fallow records it. Local-merge mode: refused; the engineer merges. |

## Briefing (FR-005)

Each session's briefing is sent three ways, so it reaches agents that ignore one of them:
1. the `instructions` of the MCP `initialize` result for that session (research R-3);
2. `whoami.briefing`;
3. a board post by Fallow (from `{"kind": "fallow"}`) when the burst starts, with the parts common to
   all roles.

Contents, per role: the role's tools and how to use them; the rules (labels mean reports, not
orders; no one can stop a session; one worktree per session; merging is published as the squad
says); the squad's roles and gates; the check-in convention (`<part>-ready`, `<part>-done`,
`review_verdict`, a status post at each step, every task reported with evidence); when to start a
session versus using its own subagents; the Spec Kit pipeline when the project uses it (FR-006); and
when the burst ends.
