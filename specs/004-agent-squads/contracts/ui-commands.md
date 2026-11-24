# Contract: UI, Commands and Command Line (004 additions)

Extends [003 contracts/ui-commands.md](../../003-agent-orchestration/contracts/ui-commands.md).
Every new command is in the palette (FR-031); new keys are checked against the known conflicts.

## Starting a burst

Palette "Coordinate this project…" (from a selected or focused session), or the offer when the first
session of a project starts:

```text
┌ Coordinate FallowSandbox ───────────────────────────────────────────────────────┐
│ Lead:      #1 Claude Code · FallowSandbox                                        │
│ Squad:     .fallow/squad.toml (lead, builder ×3, reviewer ×2, gate tests)        │
│ For:       [4] hours (max 8)                                                     │
│ Sessions:  up to [3] new sessions. Each runs its own agent and uses its own      │
│            plan or API usage.                                                    │
│ Publish:   draft pull request (the project has a remote)                         │
│                                                                                  │
│ During the burst, agents in this project deliver, create tasks and start          │
│ sessions without asking. They can never stop a session. Shift-X ends it.         │
│ Enter Start   Esc Cancel   (offer only: d Don't ask again for this project)      │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Squad problems are listed instead of "Start" (`.fallow/squad.toml:12: unknown level "admin"`).

## During a burst

| Where | What |
|---|---|
| Hint bar | `● coordinating 3:12:05 left` in both views, also while a session has focus. |
| Session list | A role column (`lead`, `builder`, `reviewer`, `gate`). |
| Orchestration view | A new first tab **Squad**: roles and sessions, gates with baseline / final results and classification, merge conditions met / unmet, repair rounds, time left, publish mode. |
| Session details (`i`) | Role, level, the exact command line it started with (FR-014). |
| Notices | "#4 reviewer's worktree changed: its verdicts no longer count"; "Repair cap reached for T3: your decision"; "All 4 sessions have waited 10 min: #2 waits for api-ready, …"; "Coordination ends in 10 min"; "Coordination ended". |

## Commands

| Command | Key (Squad tab) |
|---|---|
| Coordinate this project… | palette |
| Extend coordination… (new grant ≤ 8 h) | `+` |
| End coordination | `Shift-X` (as 003 revoke all) or palette |
| Make the selected session the lead | `L` |
| Merge now (local merge, all conditions met) | `m` (asks to confirm, names branch and commit) |
| Decide a stopped repair loop: hand it back once more / take over / cancel the part | `r` / `t` / `Delete` |

## Starting a role session with a bypass flag

```text
Role "builder" starts Codex with --dangerously-bypass-approvals-and-sandbox.
That agent will run any command without asking. Start it anyway?   y Yes   n No
```

## Command line

`fallow --coordinate <DURATION> [PATH]`: `DURATION` like `30m`, `4h`, `2h30m`, at most `8h`
(otherwise exit 2: "--coordinate: at most 8h"). The burst starts when the first session of that
project starts, with the default allowance and the project's squad, and that session is the lead.
Documented in 001's contracts/cli.md.
