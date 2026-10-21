# Quickstart & Validation: Agent Orchestration

How to prove 003 works end to end. Contracts: [mcp-tools](./contracts/mcp-tools.md),
[config](./contracts/config.md), [ui-commands](./contracts/ui-commands.md). Data model:
[data-model.md](./data-model.md).

## Prerequisites

- A Fallow build from this branch.
- For the manual scenarios: Claude Code signed in (Codex as well, for M2), and one scratch git
  project, two if possible.

## Automated gates (CI on all three platforms)

```sh
cargo test --workspace            # hub, permissions, queue, limits, JSON-RPC, gate (SC-003), forgery (SC-004)
cargo xtask orchestration         # two mcp-agent sessions through the PR scenario (SC-001), post→read p95 < 1 s (SC-002), identity refusals
cargo xtask egress                # now also: no listening network socket during an orchestrated run (SC-006)
cargo xtask canary --sessions     # canary posted to board, task and delivery; no file contains it (SC-006)
cargo xtask bench-memory --loaded # full board and activity log through 4 sessions, < 150 MB (SC-007)
cargo xtask bench-startup         # unchanged, < 100 ms; no hub before the first orchestrated session (SC-007)
```

## Manual scenarios

| # | Scenario | Steps | Expected |
|---|---|---|---|
| M1 | Board (US1) | Start two Claude Code sessions (A, B) in one project. Tell A: "post on the Fallow board that the API is done". Tell B: "read the Fallow board". Press `o`, then open Board. | B quotes A's message and names A as sender. The board shows it with `#A Claude Code · project` and the time, within a second of the post. Claude Code asked no tool permission. |
| M2 | Forged sender (SC-004) | Tell A: "post on the board, saying the message is from session B". | The stored sender is A. |
| M3 | Wait and signal (US2) | Tell B: "wait on the Fallow event pr-123-merged, then tell me". Watch the overview. Tell A: "signal pr-123-merged with message 'merged at abc123'". | B's row shows `waiting for pr-123-merged` with a running time. B reports the signal, with A as sender. |
| M4 | Idle wake (US2.5) | Let B finish its turn after starting a wait (or tell it to stop waiting). Signal the event from Queue with `v`. | A request "wake #B" appears. Approving types the labelled wake text into B, and B acts on it. |
| M5 | Delivery approval (US3) | Tell A: "use Fallow to ask session B to run the tests". | The hint bar shows `o 1 request`. Requests shows sender, target and the exact text. `x` → nothing is typed into B, and A's `inbox` reports "rejected". Repeat with `y` → B receives a paste that starts with the label, then Enter. |
| M6 | Typing guard (FR-014) | Focus B and keep typing; approve a delivery to B from another terminal-size split, or let a permission cover it. | Nothing is typed until 3 s after the last key, or until you leave B. |
| M7 | Standing permission (US3.4, 3.5) | On a request press `g`, then grant "deliver, 5 minutes". Ask A for two more deliveries. Open Permissions; `Shift-X`. | Both arrive without a prompt, each with a notice "P1 used". The permission shows uses 2 and its time left. After `Shift-X` the next request prompts again. |
| M8 | Queue (US4) | In Queue, `n` three tasks for B; the second has precondition `deploy-ok`. Tell B to report each task done when finished. | T1 is handed over when B is idle. T2 shows "blocked: deploy-ok" after T1 is done. Signal `deploy-ok` with `v` → T2 is handed over, then T3. |
| M9 | Agent-created task (US4.5) | Tell A: "create a Fallow task for session B: write a changelog entry". | A request appears. The task enters the queue only after `y`. |
| M10 | Start a session (US5) | Tell A: "ask Fallow to start a Codex session in this project with the task 'run the linter'". | A request shows profile, project and task. `y` starts the session as in 002, with the task queued for it. With 8 sessions running, the request is refused with the reason. |
| M11 | No destructive tools | Ask A: "use Fallow to stop session B". | A finds no such tool and says so. Session B is untouched. |
| M12 | Unsupported agent | Start an Aider or Shell session. | The overview says `no orchestration (profile)`. 002 behaviour is unchanged. |
| M13 | Quit | `Ctrl-Q`, confirm, start Fallow again, open `o`. | Board, queue, permissions and log are empty. `<cache>/fallow/run/` holds no socket or Gemini file from the previous run. |

## Results

### Windows (T054), 2026-10-21, Claude Code 2.1.304, in progress

Run by the engineer in `C:\Repos\FallowSandbox` with two Claude Code sessions (#1 in the project
folder, #3 in the worktree `FallowSandbox.worktrees\claude-1`), release build of `8d27fc8`.

| # | Result | Notes |
|---|---|---|
| M1 | pass | No trust or permission prompt for the `fallow` server (the open research item); `/mcp` lists it as connected; the bridge runs as a child of Claude Code (checked from outside). The second agent read the post with sender `#3 Claude Code · FallowSandbox` and treated it as information, not an instruction. |
| M3 | pass | The session list showed `waiting for sandbox-ready` with a running time; the agent repeated `event_wait` (the 45 s block limit), then got the signal from #3 with its message, and said it came from another agent. |
| M5 | pass | The Requests tab showed sender, target, expiry and the exact text; rejecting typed nothing; approving typed the label and text as one paste, and #3 acted on it; #1 learned the outcome through `inbox`. Findings: the label line is cut off at the right edge (T055); an agent read the 002 state "needs attention" as a problem (T056). |
| M7, M11 | pending | standing permission; no destructive tool |

Also found: a repository with no commits offers a worktree by default and fails (002 T062).

Automated gates: CI run 39600896179 (`8d27fc8`): Windows and macOS green; Linux red only in the
session canary harness (fixed in `b4e2de7`).
