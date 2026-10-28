# Implementation Plan: Adaptive Delegation

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; see spec Assumptions) | **Date**:
2026-10-28 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/005-adaptive-delegation/spec.md`

## Summary

004's squads split one feature by role and default to parallel builders. 005 turns the orchestrator
around: the lead builds in one loop by default and delegates only where the work is prepared for it,
and every delegation choice can be measured against a plain single session.

How it works:
- **The work manifest** (`.fallow/work.toml`, written by the lead, read by Fallow at a commit)
  records each piece of work, how it is built and why, parts with owned files, acceptance criteria,
  contracts and reviewer focuses (R-1).
- **Enforcement at session start**: builders start only for work the manifest delegates, and parallel
  parts only with owned files that do not overlap and a committed contract per boundary (R-2).
- **Decisions in the repo**: builders raise structured signals to the lead only, and the lead answers
  with commits (R-4). A done report must contain every commit that changed its part's criteria or
  contracts (R-5). Edits outside owned files are reported (R-6).
- **Verification**: reviewers run at a new `check` level, which can run commands but not edit (R-3).
  Their verdicts must answer every criterion, including a case that must fail (R-7). The gates are
  004's.
- **The lead**: told only what needs a decision (R-8), and replaceable from the repo and
  `squad_status` (R-9).
- **Measurement**:
  - three run modes (plain, solo, squad) from a task set, each run in its own copy of the project with
    the same permission level, inputs and end rule (R-11, R-13);
  - hidden tests on the worktree as left, after the run's sessions are closed with one confirmation
    (R-11, R-13);
  - what switching each component off does (R-17);
  - fingerprints via git (R-12);
  - cost from Claude Code's own status line report (R-10);
  - run records on disk under the amended Principle VII (R-15).
- **Memory**: starts are capped by the resident memory sessions actually use (R-14).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85, as 001–004.

**Primary Dependencies**: no new crates.
- `toml`/`serde` for the manifest and the task set;
- `serde_json` for run records and the status line JSON;
- `globset` for owned files;
- 004's git worker, gates and temporary worktrees;
- 002's process sampler.

**Storage**:
- The manifest is read from the project at a commit, never written by Fallow.
- The task set is read from the engineer's directory.
- Run records are written to `<cache>/fallow/runs/` (Principle VII v5.0.0).
- Everything else stays in memory.

**Testing**:
- `cargo test`:
  - manifest parsing and validation;
  - delegation refusals;
  - signals;
  - the plan-commit and ownership checks against real temporary git repos;
  - verdict completeness;
  - decision-only lead updates;
  - run lifecycle and records, including the CI marker test of the constitution gate;
  - the memory budget.
- `cargo xtask squad` gains scenarios (scripted agents):
  - a single loop;
  - a prepared split with a signal answered by a commit;
  - a refused done report;
  - a recorded plain run and squad run with hidden tests.

**Target Platform**: Linux, macOS 13+, Windows 10 1809+. The engineer's own target is a 4 GB Linux
VPS.

**Project Type**: the same single native terminal application.

**Performance Goals**:
- Manifest reads and checks on the git worker, at session starts, reports and gates only (no
  polling).
- Hub decisions stay in microseconds.

**Constraints**:
- No change to startup or idle memory.
- Nothing is created before a burst or a run.
- No network calls.
- Fallow writes only:
  - run records and run copies in its cache (a run copy is removed after the run);
  - temporary checkouts for gates (as in 004);
  - the engineer's confirmed local merge (004).
- Recorded runs only read the engineer's repository (to fetch the starting commit).

**Scale/Scope**:
- One burst or run per project at a time; series run sequentially.
- Manifests up to 64 pieces of work and 32 parts; task sets up to 100 tasks.
- Run records are about 2 KB each.

No NEEDS CLARIFICATION remains. Verify items (research):
- Codex and Gemini flags for `check`;
- the `--settings` status line form on Windows;
- the resident memory of a Claude Code session on Linux.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution v5.0.0.

| Principle / gate | How the design complies | Result |
|---|---|---|
| I. Memory | The manifest, signals and run state are small and exist only during a burst or run. Records are on disk, not held in memory beyond the Runs tab's list. `bench-memory --loaded` with a burst is kept. | PASS (to be measured) |
| II. Startup, no implicit background work | Nothing at startup. Task sets are read when the engineer opens the Runs tab or starts a run. Records are listed on demand. | PASS |
| III. Terminal-only | Unchanged. | PASS |
| IV. Observability over control | Bursts remain 003 standing permissions. Recorded runs start only on the engineer's explicit command, and a series is one explicit request for its listed runs. Typing the fixed description into a run's first session is the engineer's action, like their own paste. Closing a run's sessions at its end is a destructive action: one confirmation per run names every session and the action; nothing closes without it. | PASS |
| V. Least privilege | The new `check` level narrows editing tools; its commands may still write, which the worktree check and a notice cover (R-3). Fallow's writes: run records and run copies in its own cache directory (a copy is a repository fetched from the engineer's, which is only read); 004's temporary checkouts for gates; nothing else. The status line command (`fallow usage-report`) only forwards what Claude Code gives it. | PASS |
| VI. Benchmarked claims | SC-005–SC-008 are CI tests or `xtask` scenarios. SC-001–SC-004 are the engineer's comparison results, reported as measured on their task set, not as a general claim. | PASS |
| VII. Privacy | Records hold only the amended fields (no series seed, end reason or other extras, R-15); model and version are the agent's reports, never a profile's configured text; the CI marker test covers names, paths, test names and a custom model alias. Cost comes from the agent's own status line over the local channel. No transcripts or network data are read. Hidden tests are read from the engineer's directory and copied only into a run's copy after its sessions are closed. | PASS |
| Workflow: typing on another agent's behalf | Unchanged 003 gate; signals reach the lead through the same approved delivery path as 004's updates. | PASS |
| Workflow: control paths | No new control path. The test that no tool can stop or kill a session is extended to the new tools. | PASS |
| Workflow: dependency justification | No new dependency (fingerprints via `git hash-object`, R-12). | PASS |
| Workflow: run-record marker test (new in v5.0.0) | Implemented as a CI test (R-15). | PASS |

Re-check after Phase 1 design: no violations. Points to keep in view:
- **Hidden tests are hidden by placement, not by permission.** Agents run as the engineer. The spec,
  the task set contract and the registration notice all say so.
- **Cost is the agent's estimate.** Claude Code computes it at list price, and other agents report
  nothing yet. Comparisons show cost as "reported", with unknowns marked.

## Project Structure

### Documentation (this feature)

```text
specs/005-adaptive-delegation/
├── plan.md                 # This file
├── research.md             # Phase 0 (R-1 … R-16)
├── data-model.md           # Phase 1
├── quickstart.md           # Phase 1 (Q1–Q7)
├── constitution-amendment.md   # ratified, v5.0.0
├── contracts/
│   ├── work-manifest.md    # .fallow/work.toml and squad file additions
│   ├── task-set.md         # taskset.toml and hidden tests
│   ├── run-record.md       # the on-disk record
│   ├── mcp-tools.md        # changed and new tools, briefings, lead updates
│   └── ui-commands.md      # Squad and Runs tabs, commands, settings
├── checklists/requirements.md
└── tasks.md                # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

New and changed files on top of 004's tree:

```text
src/
├── orchestration/
│   ├── manifest.rs        # NEW: work.toml parsing at a commit, validation (file:line), delegation checks
│   ├── signals.rs         # NEW: signal store, raise/answer/list, unanswered timer
│   ├── delegation.rs      # NEW: start refusals (R-2), plan-commit check (R-5), ownership check (R-6)
│   ├── runs.rs            # NEW: task sets, run lifecycle, series, interventions, end detection
│   ├── record.rs          # NEW: run records on disk (allowed fields only), list and delete
│   ├── usage.rs           # NEW: status line reports per session (R-10)
│   ├── memory.rs          # NEW: memory budget and waiting starts (R-14)
│   ├── squad.rs, roles.rs # CHANGED: `check` level and flags, work:done, squad additions, built-in squad
│   ├── lead.rs, repair.rs # CHANGED: decision-only updates; quiet status and lead deadlock notices removed
│   ├── publish.rs         # CHANGED: work:done; verdict completeness
│   ├── briefing.rs        # CHANGED: lead, builder, reviewer, investigator texts
│   ├── tools.rs, mod.rs   # CHANGED: new tools, session_start arguments, deliver limit, consult_lead removed
│   └── git.rs             # CHANGED: show at commit, log over paths, merge-base, hash-object
├── cli.rs, main.rs        # CHANGED: `fallow usage-report` (status line bridge)
├── session/mod.rs         # CHANGED: a usage-only link (token, no MCP tools) for plain run sessions
├── session/worktree.rs    # CHANGED: run copies; builder branches named after work and part (R-18)
├── orchestration/inject.rs # CHANGED: the status line settings for Claude Code sessions
├── app/orchestration.rs   # CHANGED: run and series commands, intervention counting, memory waits
└── ui/orchestration.rs    # CHANGED: Work and Signals in the Squad tab; the Runs tab
tests/
├── squad_manifest.rs, squad_delegation.rs, squad_signals.rs, squad_runs.rs, squad_memory.rs
xtask/src/
├── squad.rs               # CHANGED: single-loop, prepared-split and recorded-run scenarios
└── mcp_agent.rs           # CHANGED: steps for signals and checks in verdicts
```

**Structure Decision**: the same crate, as a layer on 004's hub. The manifest and delegation checks
use 004's git worker. Runs reuse bursts (solo and squad) and 002's session start (plain). Hidden tests
reuse 004's gate sessions, run in the lead's worktree of the run copy. Nothing in 003's permission paths changes.

## Phases (for /speckit-tasks)

Measure first (Anthropic: measure before adding complexity). The MVP is phases 1–3: plain and solo
runs compared on hidden tests, which also shows whether 004's gates and reviewer pay (SC-004).

1. **Foundation**:
   - git additions (show at commit, log over paths, hash-object, fetch into a run copy);
   - the manifest parser with file:line errors;
   - the `check` level with exact flag vectors;
   - `work:done` and `review:approved`;
   - the built-in squad.
2. **US1 core: the lead builds by default** (P1):
   - the lead's task from the burst description;
   - start refusals for undelegated work;
   - briefings;
   - the Work section of the Squad tab.
3. **US2 measurement** (P2, MVP end):
   - task sets, run copies and fingerprints;
   - plain and solo runs;
   - the end rule and the close confirmation;
   - hidden tests;
   - interventions;
   - the status line usage link;
   - run records with the marker test;
   - the Runs tab;
   - series;
   - `xtask` recorded-run scenario.
4. **US1 rest and US3 decisions in the repo** (P1, P3):
   - prepared parallel parts and `features`;
   - signals;
   - the plan-commit and ownership checks;
   - non-lead manifest changes;
   - the deliver limit;
   - branch names;
   - squad runs;
   - `xtask` prepared-split scenario.
5. **US4 verification** (P4): the criteria precondition; verdict completeness; reviewers per focus; the
   `check` level notice.
6. **Switching components off** (FR-017): each component's effect (R-17), recorded in runs.
7. **US5 lead** (P5): decision-only updates; the replacement briefing and status; rebuilding after a
   restart; discarding a run in progress.
8. **US6 memory** (P6): the budget over session footprints, measured sizes, waiting starts.
9. **Polish**:
   - help and conflict rows;
   - README;
   - `bench-memory` with a burst;
   - quickstart Q1–Q7 on Windows and on the Linux VPS;
   - retire 004's built-in squad text and quiet-status tests;
   - the Verify items.

## Complexity Tracking

No constitution violations; nothing to justify.
