# Contract: The Squad File

A squad is declared in the project, at `.fallow/squad.toml`, versioned with the code (spec FR-016).
Fallow reads it when a burst starts and never writes it. Problems are reported as
`.fallow/squad.toml:LINE: message`, and a burst with an invalid squad does not start (FR-017). A
running burst keeps the rules it started with (FR-018).

Without the file, the built-in squad below is used (FR-019). Roles can also be defined for all
projects in Fallow's config directory (`roles.toml`, same `[[role]]` format); a project's squad
file overrides them by name.

## Format

```toml
# .fallow/squad.toml
name = "default"                 # shown in the squad view and the briefing

publish = "auto"                 # "auto": draft pull request if the project has a remote, else local merge;
                                 # "draft-pr" | "local-merge"                                     (FR-028)
target_branch = ""               # empty: the branch checked out in the project when the burst starts
repair_cap = 2                   # FR-025: rounds per part before Fallow asks the engineer; 0..=10
wait_notice_min = 10             # FR-026: everyone waiting this long → the engineer is told; 1..=120

[[role]]
name = "lead"                    # [a-z][a-z0-9-]{0,31}; unique
description = "Plans, starts sessions, hands out tasks, publishes."
level = "edit-run"               # "read-only" | "edit" | "edit-run"                               (FR-008)
agent = "claude"                 # a profile id; the agent CLI this role runs
model = ""                       # optional; passed with the agent's model flag
tools = "all"                    # "all" or a list from: board, events, deliver, tasks, report, start, sessions
signals = ["*"]                  # event name patterns this role may signal; see "Restricted events"
max = 1                          # how many sessions of this role may run at once

[[role]]
name = "builder"
description = "Implements its assigned tasks in its own worktree."
level = "edit-run"
agent = "claude"
tools = ["board", "events", "deliver", "report", "sessions"]
signals = ["*-ready", "*-done", "*-blocked"]
max = 3

[[role]]
name = "reviewer"
description = "Reviews the builders' work against the spec. Read-only."
level = "read-only"
agent = "claude"
tools = ["board", "events", "report", "sessions"]
signals = ["review-*"]
max = 2

# Optional: flags for an agent, replacing Fallow's mapping for this role's level (FR-008).
[role.flags]
codex = ["--sandbox", "read-only", "--ask-for-approval", "never"]

[[gate]]
name = "tests"                   # Fallow signals gate-tests-passed / gate-tests-failed itself     (FR-020)
command = "cargo test"           # run in the session's worktree (final gate: the lead's)
windows = ""                     # optional per-OS overrides                                        (FR-022a)
macos = ""
linux = ""
timeout_min = 30                 # FR-022; 1..=240
baseline = true                  # run once on the starting commit before builders begin           (FR-021)
failures = "auto"                # how failing checks are named for classification: "auto" (known
                                 # runners), "exit-code", or a regex with one capture group

[merge]
mode = "on-approval"             # local merge only: "on-approval" (the engineer presses merge) | "automatic"
# Every condition must hold before the work is published (FR-028, SC-003).
requires = [
  "gate:tests",                  # the final run of gate "tests" passed (or failed only as the baseline did)
  "done:builder",                # every task handed to a builder is reported done, with evidence
  "signal:review-approved@reviewer",  # signalled by a session in role "reviewer" (every reviewer, if several)
]
```

## Rules

- **Levels** map to agent flags (research R-2): a role with level `read-only` or `edit` cannot be
  started on an agent that has no mapping and no `[role.flags]` for it (FR-008).
- **Tools**: `board` (post, read), `events` (signal within `signals`, wait, status), `deliver`,
  `tasks` (create tasks for other sessions), `report` (report its own tasks, with evidence), `start`
  (start sessions), `sessions` (list sessions, whoami). Only `tools = "all"` or a list with `start`
  may start sessions; the built-in squad gives `start` and `tasks` to the lead only.
- **Restricted events**: an event whose name matches any role's `signals` pattern can only be
  signalled by sessions in a role whose patterns match it (`*` in a pattern matches any characters).
  `gate-*` events are signalled by Fallow alone. Events no pattern matches are free, as in 003.
- **Preconditions with roles**: a task precondition may name a role, `review-approved@reviewer`; it is
  met only by a signal from a session in that role whose signals count (FR-013).
- **Read-only roles**: their sessions' worktrees must stay clean (FR-013).
- **Exactly one lead**: the session the burst starts from is in role `lead`, whatever its profile;
  a squad must define a role named `lead` (the built-in one is used if it does not).

## Built-in squad

The format above without the `[role.flags]` example is the built-in squad, with `publish = "auto"`
and no gates (a project adds gates in its own file; Fallow cannot guess a project's test command),
so its merge requires `done:builder` and `signal:review-approved@reviewer` only.
When the project uses Spec Kit, the briefing describes the Spec Kit pipeline (spec FR-006).
