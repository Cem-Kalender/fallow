# Quickstart & Validation: Agent Squads and Coordination Mode

How to prove 004 works end to end. Contracts: [squad-file](./contracts/squad-file.md),
[mcp-tools](./contracts/mcp-tools.md), [ui-commands](./contracts/ui-commands.md). Data model:
[data-model.md](./data-model.md).

## Prerequisites

- A Fallow build from this branch.
- For the manual scenarios: Claude Code signed in; a scratch git project with one commit and a test
  command that runs in under a minute (for example a tiny Rust or Node project); for Q6, a remote
  (GitHub) with branch protection on the target branch.

## Automated gates (CI on all three platforms)

```sh
cargo test --workspace            # squad parsing and validation, roles and tool filtering, restricted events,
                                  # gate classification, evidence checks, repair caps, deadlock notice, merge conditions
cargo xtask squad                 # SC-009: lead + 2 builders + reviewer + gate with scripted agents, local merge waits for the engineer
cargo xtask orchestration         # 003 scenario unchanged (no burst)
cargo xtask bench-memory --loaded # unchanged budget; SC-008 adds a burst of 4 sessions and a gate
cargo xtask bench-startup         # unchanged; a pending --coordinate burst creates nothing before the first frame
```

## Manual scenarios

| # | Scenario | Steps | Expected |
|---|---|---|---|
| Q1 | Start a burst (US1) | Focus #1 (Claude Code), palette "Coordinate this project…", 1 hour, 2 sessions, Enter. | The burst shows in the hint bar and the Permissions and Squad tabs; #1 is `lead`; a briefing note appears on the board; #1's `whoami` includes its briefing. |
| Q2 | Hand-offs without prompts | Ask #1: "start a builder and give it the task 'add a hello function with a test'". | A builder session starts in its own worktree without a prompt; its task is typed with the label naming #1 as lead; each use shows a notice. |
| Q3 | Roles hold (US2) | Ask the builder: "start another session" and "signal review-approved". | Both refused with "your role builder may not …"; nothing starts. |
| Q4 | Read-only reviewer | Ask #1 to start a reviewer; ask the reviewer to edit a file. | Claude Code refuses (plan mode); if a file is changed anyway (by hand in its worktree), Fallow warns and the reviewer's verdicts stop counting. |
| Q5 | Gate (US4) | Add a `[[gate]]` with your test command; start a burst; let the builder finish. | The baseline gate ran before the builder started; the final gate runs; the Squad tab shows its classification; agents cannot signal `gate-*`. |
| Q6 | Publishing (US6) | With a remote: all conditions met. Without a remote: press `m`. | Remote: the lead opens a draft pull request and reports its address; you merge on the platform. Local: Fallow merges exactly the gated commit after you confirm. |
| Q7 | Repair loop (US5) | Have the reviewer ask for changes three times with `repair_cap = 2`. | Two repair tasks go to the builder; the third time Fallow asks you. |
| Q8 | End | Wait for the end, or `Shift-X`. | "Coordination ended" notice and board note; the next delivery request prompts again. |
| Q9 | Launch option | `fallow --coordinate 1h <project>`, start a session. | The burst starts with that session as lead; `--coordinate 9h` exits with "at most 8h". |

## Results

Filled in during implementation (per OS, with CI run numbers).
