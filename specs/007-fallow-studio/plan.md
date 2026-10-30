# Implementation Plan: Fallow Studio

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is `007-fallow-studio`) | **Date**: 2026-10-30 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/007-fallow-studio/spec.md`

## Summary

Fallow becomes a local dashboard for spec-driven development, with 006's verified loop as its engine. The engineer adds a project folder, talks with a lead agent about the project, and works out each feature in a spec session that follows Spec Kit. They then lock it. Fallow runs the locked `tasks.md` as one loop that the engineer can watch, and the engineer merges once.

How it works:
- **The page**: one service, one page with the mockup's five screens, written in plain HTML, CSS and JavaScript from the mockup itself, with Inter and JetBrains Mono shipped inside Fallow (S-1 to S-3).
- **The lead's chats** are resumable `claude -p` turns:
  - a spec session is forked from the project conversation;
  - each answer during a loop is forked from the spec session (S-5);
  - the lead works in worktrees, writes only spec documents, and runs commands by a chat mode the engineer cycles with Shift+Tab (S-6, S-7).
- **Lock**: Fallow reads `tasks.md` and `spec.md` strictly, and refuses what it cannot read (S-8).
- **The loop**: one agent step per task, and one builder session per phase with a committed handover. Fallow commits and gates every task, and checkers run at each user story's checkpoint. Repairs, resume and limits are 006's (S-9).
- **Questions** go through an `ask` tool on 006's MCP bridge to the lead, which answers, decides, or relays to the engineer by the autonomy level. Every answer is a commit (S-10).
- **Agents and pipelines** are markdown definitions and TOML pipelines from fixed building blocks, with defaults shipped (S-11, S-12). A result is "verified" only when the pipeline gated every task and checked every story.
- **Runtimes**: a runtime trait with Claude Code first, and the fake agent as the second runtime (S-13).
- **Notifications**: a notification-area icon with balloons (S-14).
- **Spec Kit** is embedded and installed into a project when it is added (S-15).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as before).

**Primary Dependencies**:
- **Kept**: `serde`, `toml`, `serde_json`, `globset`, `regex`, `tiny_http`, `windows-sys`.
- **Added**: no crates. `windows-sys` gains the `Win32_UI_Shell` and `Win32_UI_WindowsAndMessaging` features for the notification icon (S-14).
- **Run as external tools**: `claude` 2.1.x (verified flags: `--fork-session`, `--include-partial-messages`, `--permission-prompts`; `--permission-prompt-tool` is to be verified on a real run, S-7) and `git`.
- **Shipped files**:
  - Spec Kit 1.0.12 (MIT);
  - Inter and JetBrains Mono woff2 (OFL 1.1);
  - the default agents and pipelines.

**Storage**: contracts/storage.md.
- `projects.json` and conversation and loop event logs under `%LOCALAPPDATA%\fallow\`;
- definitions and settings under `%USERPROFILE%\.fallow\`;
- documents, answers and handovers in the project's repository.

**Testing**:
- **`cargo test`**, with `fallow-fake-agent` as the `fake` runtime, covering:
  - the loop per task and phase;
  - handovers and checkpoints;
  - false claims, unlock and resume;
  - `ask` at every autonomy level;
  - the format parsers (`tasks.md`, `spec.md`, definitions, pipelines);
  - chat modes to permission rules;
  - the API's checks.
- **Benchmarks**: 006's (`cargo xtask bench`).
- **Manual**: quickstart.md Q1–Q5 with real agents, and the visual check against the mockup.

**Target Platform**: Windows 10/11 x64 first. Platform code stays in `platform` modules, and notifications are a no-op elsewhere.

**Project Type**: a local service with a browser page (one Rust crate plus `xtask`).

**Performance Goals** (constitution VIII): the dashboard within 500 ms of start, and agent activity on the page within 2 s.

**Constraints**:
- Fallow's process under 60 MB idle and under 200 MB with 4 concurrent steps;
- no outbound connections from Fallow; loopback only;
- the engineer's rights only.

**Scale/Scope**: one engineer, a handful of projects, a few loops at once, at most 4 agent steps at a time, features of up to about 80 tasks, and feeds of thousands of lines.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.0.0. The amendment to v6.1.0 (constitution-proposal.md) states four rules that 007 relies on: the lead's answers, rights by kind, writing into projects, and unverified results. **Status**: PASS. The amendment was approved and applied on 2026-10-30.

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Every task is checked: its claim, its commit and the gates against the baseline. Checkers' verdicts are complete per `USn/k`, must-fail cases included. Handovers are checked. A pipeline that is not verifying ends `done_unverified`, never "ready" (S-12). A plain folder's chat states what cannot be checked (S-6) | PASS (amend: one bullet) |
| II. The Engineer Decides | No loop step before the lock, which is the plan's approval. One merge, by the engineer. Questions follow the autonomy level, reserved items and criteria changes always go to the engineer, and every answer can be overruled (S-10). Destructive actions are confirmed. Steering is recorded | PASS (amend: autonomy wording) |
| III. Coordination in Code, Decisions in the Repo | The stage machine schedules from the pipeline. No messaging tools; `ask` is routed by Fallow. Answers, unlocks and handovers are commits. Overruling and relocking reopen the affected stories (S-10, S-17) | PASS (amend: `ask` wording) |
| IV. Simplest Setup First | One builder by default. `[P]` is not parallelised. Standard is the default pipeline. Steps of a pipeline can be removed for comparison | PASS |
| V. Local and Private | Fonts and Spec Kit are shipped, nothing is downloaded, and the CSP stays `default-src 'self'`. Notifications are local and carry no code. Data stays in the user-only directories. No new network path | PASS (amend: notifications) |
| VI. Least Privilege | Rights by kind, enforced by the runtime's own permission rules. Chat modes use rules and a prompt tool, never skip-permissions. The lead edits spec paths only, in worktrees. Project writes are only where files are missing, on an add the engineer asked for | PASS (amend: rights by kind; project writes) |
| VII. Runs Survive | Loop and conversation event logs, flushed before acting. Resume per task in its phase session. Tested at every stage | PASS |
| VIII. Small and Measured | No new crates. Plain JS. Fonts about 150 KB and Spec Kit about 120 KB, embedded. 006's benchmarks gain a dashboard-ready measurement | PASS |

**Re-check after Phase 1 design**: PASS, subject to the amendment. The contracts add no network access and no messaging path between agents. The only writes outside Fallow's directories and its working copies are the ones the amendment names.

## Project Structure

### Documentation (this feature)

```text
specs/007-fallow-studio/
├── plan.md                  # this file
├── research.md              # S-1 to S-19
├── data-model.md
├── quickstart.md            # Q1 to Q8, and the visual check
├── constitution-proposal.md # 6.0.0 → 6.1.0, for approval
├── mockup/                  # the approved screens (the reference for FR-030)
├── contracts/
│   ├── web-api.md
│   ├── agent-definition.md
│   ├── pipeline.md
│   ├── runtime.md
│   ├── mcp-tools.md
│   ├── step-results.md
│   ├── spec-kit-files.md
│   └── storage.md
├── checklists/requirements.md
└── tasks.md                 # /speckit-tasks
```

### Source Code (repository root)

```text
assets/
├── fonts/                   # NEW: Inter, JetBrains Mono woff2 + OFL
├── spec-kit/                # NEW: Spec Kit 1.0.12 files + LICENSE (S-15)
├── agents/                  # NEW: default agent definitions (*.md)
└── pipelines/               # NEW: default pipelines (*.toml)
src/
├── main.rs                  # fallow (start + open dashboard), stop, mcp-bridge; `run` removed
├── service/
│   ├── http.rs              # routes of contracts/web-api.md; fonts; scopes for long polling
│   ├── notify.rs            # NEW: needs-you items → platform::notify (S-14)
│   └── web/                 # REWRITTEN from the mockup: index.html, app.css, app.js,
│                            #   screens/{dashboard,project,spec,loop,pipelines}.js
├── project/                 # NEW: projects.json, add/remove, Spec Kit install, features scan
├── conversation/            # NEW: project conversations and spec sessions, turns, modes,
│                            #   approvals, worktrees, commit-or-discard (S-5–S-7)
├── speckit/                 # NEW: tasks.md and spec.md readers, lock checks, ticking (S-8)
├── defs/                    # NEW: agent definitions and pipelines: parse, resolve, validate
├── run/                     # 006's loop engine
│   ├── machine.rs           #   reworked: phases, tasks, checkpoints, handovers (S-9)
│   ├── questions.rs         #   NEW: ask, lead answers, relay, overrule (S-10)
│   ├── relock.rs            #   NEW: unlock, affected stories (S-17)
│   ├── events.rs, state.rs, engine.rs, time.rs   # extended
├── agent/                   # the runtime interface (S-13)
│   ├── mod.rs               #   trait Runtime, Call, Event, End, Rights
│   ├── claude.rs            #   the Claude runtime (fork, partial messages, prompt tool)
│   ├── fake.rs              #   NEW: the fake runtime
│   ├── rights.rs            #   from allow.rs: rights by kind and by chat mode
│   ├── prompts.rs           #   instructions per step kind
│   └── schemas.rs           #   work, handover, verdict, reply, answer
├── mcp/                     # 006's bridge; tools: ask, progress, approve
├── verify/                  # 006's claims, gates and verdicts (criteria USn/k)
├── publish/local.rs         # 006's local merge
├── plan/                    # REMOVED (work.toml); its command check moves to project/
├── config/                  # settings moved to ~/.fallow; squad file kept for gates
└── platform/                # + notify (Windows notification icon)
tests/                       # per area, with the fake runtime
xtask/src/                   # + spec_kit.rs (refresh the embedded Spec Kit), bench (dashboard)
```

**Removed**: `fallow run`, the planner's turn flow (`plan_turn`), `.fallow/work.toml` and `src/plan`, parts (owned files, contracts and integration), `run.html`, and 006's tests of those, rewritten where the behaviour is kept.

**Structure Decision**: the single Rust crate stays, with new areas for projects, conversations, Spec Kit files and definitions. The loop engine of 006 is kept and reworked around phases and tasks.

The order of the work follows the user stories:
1. the page shell and the dashboard skeleton;
2. projects and spec sessions (US1);
3. the loop per task (US2);
4. the loop's view (US3);
5. `ask` (US4);
6. the full dashboard and notifications (US5);
7. definitions and pipelines (US6);
8. the second runtime (US7).

## Complexity Tracking

No violation needs justifying, provided the amendment in constitution-proposal.md is approved. The amendment, not an exception, is how 007's writes into projects and its lead's answers fit the constitution.
