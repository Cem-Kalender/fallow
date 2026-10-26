# Implementation Plan: Agent Squads and Coordination Mode

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; see spec Assumptions) | **Date**:
2026-10-26 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/004-agent-squads/spec.md`

## Summary

A **burst** lets a project's agent sessions coordinate without per-action approval, led by the
engineer's own session. It is 003's standing permission with a fixed project scope and a time
limit, plus a **squad** that Fallow enforces.

How it works:
- **The squad file**: read from the project's `.fallow/squad.toml`, or a built-in one. It declares
  roles, gates and merge conditions, and is frozen for the burst (R-1).
- **Roles**: fixed per session by Fallow and stamped on everything others receive. Each role's tools
  and signals are enforced by the hub. Its permission level becomes the agent CLI's own flags, per
  agent (R-2).
- **Briefings**: each role gets its own, through the MCP server instructions and `whoami` (R-3).
- **Gates**: command sessions whose exit status Fallow turns into signals itself. Baseline and final
  runs are compared to classify failures (R-5, R-6).
- **Evidence**: task reports carry it, and Fallow checks it with read-only git (R-4).
- **Repairs**: they go back to the session a part was handed to, up to a cap.
- **Publishing**: no agent merges. The lead opens a draft pull request when the project has a
  remote. Otherwise Fallow merges the gated commit when the engineer presses merge.

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85, as 001–003.

**Primary Dependencies**: no new crates. `toml`/`serde` (squad file), `regex` (failure names, event
patterns), 003's orchestration modules, 002's sessions, worktrees and profiles.

**Storage**: the squad file is read from the project, never written. Roles for all projects may live
in `<config>/fallow/roles.toml`, written by the user. "Don't ask again for this project" is a flag in
`projects.toml`. Everything else is in memory only (FR-029).

**Testing**:
- `cargo test`: squad parsing, roles, enforcement, classification, evidence, repair loops, deadlock
  detection, merge conditions.
- A new `cargo xtask squad` with `mcp-agent` scripts on three OSes (SC-009).
- `bench-memory --loaded` with a burst.

**Target Platform**: Linux, macOS 13+, Windows 10 1809+.

**Project Type**: the same single native terminal application.

**Performance Goals**:
- Hub decisions (role checks, gate signals) in microseconds on the main loop.
- Git checks on a worker thread, at most every 2 minutes per read-only session.
- Notices within one tick of their condition (SC-006, SC-007: within 1 minute).

**Constraints**:
- No change to startup or idle memory (SC-008).
- A pending `--coordinate` creates nothing before the first session.
- No network calls.
- No writes to the project except the engineer's confirmed local merge.

**Scale/Scope**:
- One burst per project; up to 16 sessions (002 limit).
- Squads: ≤ 16 roles, ≤ 8 gates.
- 3 agent CLIs mapped.

No NEEDS CLARIFICATION remains. **Verify** items:
- Codex flag values by installed version (R-2);
- the `uvx` form of the Spec Kit command (R-8).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution v4.0.0.

| Principle / gate | How the design complies | Result |
|---|---|---|
| I. Memory | Burst state is small (roles, gate results, evidence ≤ 200 paths per report). Failure names come from scrollback already in memory. `bench-memory --loaded` runs a burst of 4 sessions plus a gate under the 150 MB gate (SC-008). | PASS (to be measured) |
| II. Startup, no implicit background work | Nothing exists before a burst. `--coordinate` is only a remembered value until the first session of that project starts. Git checks and the deadlock check run only during a burst the engineer started, which is opt-in. `bench-startup` unchanged. | PASS |
| III. Terminal-only | Unchanged. | PASS |
| IV. Observability over control | A burst is 003's standing permission: scoped to one project, ≤ 8 h, visible (hint bar, Permissions and Squad tabs), announced at each use, revocable at once (`Shift-X`). It never covers stop, pause, resume or kill. Roles only narrow what a permission allows. Labels are set by Fallow (R-3). No agent is handed the merge (FR-028). | PASS |
| V. Least privilege | Sessions and gate sessions start only under the burst's permission with an allowance and role maxima (the "standing permission with a session limit" of V). Gates are declared by the engineer in the squad they started the burst with. Repository writes by Fallow happen only on the engineer's confirmed local merge (R-4). Read-only git calls only otherwise. Agents' repository permissions come from the CLI flags of the role's level, and bypass flags need confirmation (FR-014). | PASS |
| VI. Benchmarked claims | SC-002–SC-007 are CI tests; SC-008 is `bench-memory`/`bench-startup`; SC-009 is `xtask squad` on three OSes. | PASS |
| VII. Privacy | No network calls by Fallow. Draft pull requests and the Spec Kit install run in the agents' own sessions. Gate output, evidence and briefings are memory-only. Nothing is written into the project except the confirmed merge. | PASS |
| Workflow: typing into a session on another agent's behalf is tested | Unchanged 003 gate. The burst is a permission like any other, and the 003 SC-003 test is extended with burst and role cases. | PASS |
| Workflow: control paths tested | A test asserts no tool, role or permission can stop, pause or kill a session, and that no merge task is ever handed to an agent. | PASS |
| Workflow: dependency justification | No new dependency. | PASS |

Re-check after Phase 1 design (data model, contracts): no violations. One point to keep in view:
- **Read-only depends on the agent CLI.** Repository read-only for agents is enforced by the agent
  CLI, so Fallow's guarantee is the outcome check (FR-013: worktree changes void the role's verdicts).
  The spec states this, and the squad view repeats it.

## Project Structure

### Documentation (this feature)

```text
specs/004-agent-squads/
├── plan.md              # This file
├── research.md          # Phase 0 (R-1 … R-9)
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   ├── squad-file.md    # .fallow/squad.toml and roles.toml
│   ├── mcp-tools.md     # role on every item, tool lists per role, changed and new tools, briefing
│   └── ui-commands.md   # burst dialog, Squad tab, commands, --coordinate
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

New and changed files on top of 003's tree:

```text
src/
├── cli.rs, main.rs                 # CHANGED: --coordinate <duration>
├── orchestration/
│   ├── squad.rs                    # NEW: squad and roles.toml parsing, validation (file:line), built-in squad
│   ├── roles.rs                    # NEW: levels → agent flags (R-2), bypass detection, tool and signal filters
│   ├── burst.rs                    # NEW: burst lifecycle on top of permissions, lead, allowance, end and extend
│   ├── briefing.rs                 # NEW: per-role briefing text (≤ 3,500 chars) and whoami's full text (R-3)
│   ├── gates.rs                    # NEW: gate runs, failure names (R-5), classification
│   ├── evidence.rs                 # NEW: evidence checks via git (R-4)
│   ├── repair.rs                   # NEW: owners, rounds, caps; deadlock detection (R-7)
│   ├── publish.rs                  # NEW: merge conditions, readiness, local merge, draft-PR record
│   ├── git.rs                      # NEW: the read-only git calls of R-4 on a worker thread
│   ├── mod.rs, tools.rs            # CHANGED: Party::Fallow and roles on Party; tool filtering; new and changed tools
│   ├── events.rs, tasks.rs         # CHANGED: restricted events, role preconditions, task parts and owners
│   └── inject.rs                   # CHANGED: role flags merged with 003's per-run flags
├── session/profiles.rs             # CHANGED: level flag mappings per built-in agent
├── projects/store.rs               # CHANGED: "don't ask again for this project" flag
├── app/orchestration.rs, workspace.rs   # CHANGED: burst commands, offer at first session, role sessions, gate sessions, merge
└── ui/orchestration.rs, workspace.rs    # CHANGED: Squad tab, role column, burst dialog, hint bar
tests/
├── squad_file.rs, squad_roles.rs, squad_gates.rs, squad_evidence.rs, squad_publish.rs
xtask/src/
├── squad.rs                        # NEW: SC-009 end to end
├── mcp_agent.rs                    # CHANGED: steps for verdicts, evidence, publish
└── bench_memory.rs                 # CHANGED: a burst in --loaded
```

**Structure Decision**: the same crate. Squads are a layer on top of the 003 hub. A burst is a 003
permission, so every 003 path keeps its tests, and roles only narrow it. Git access is a new small
module on a worker thread, separate from 002's worktree code, which stays for explicit actions.

## Phases (for /speckit-tasks)

1. **Foundation**:
   - `Party::Fallow` and roles on `Party`;
   - squad parsing with the built-in squad;
   - level → flags per agent with exact-vector tests;
   - the git worker.
2. **US1 burst** (P1, MVP):
   - burst lifecycle and the three ways in;
   - the briefing (instructions, `whoami`, board);
   - the Spec Kit offer;
   - the hint bar and Squad tab basics;
   - end and extend;
   - `xtask squad` skeleton.
3. **US2 roles** (P2):
   - role sessions with flags;
   - tool filtering;
   - restricted events and role preconditions;
   - the read-only worktree check;
   - bypass confirmation;
   - the role column and details.
4. **US3 squads** (P3): the squad file in force, frozen rules, merge conditions, the Squad tab.
5. **US4 gates** (P4): gate sessions, Fallow's signals, baseline and final runs, failure names and
   classification, time limits.
6. **US5 evidence and repair** (P5): evidence in reports with git checks, owners and repair loops
   with caps, the deadlock notice.
7. **US6 review and publishing** (P6): `review_verdict`, two reviewers and disagreements, draft-PR
   and local-merge publishing, the foreign-commit warning.
8. **Polish**:
   - help and conflict rows;
   - README;
   - `bench-memory` with a burst;
   - quickstart Q1–Q9 on Windows;
   - the macOS checklist item.

## Complexity Tracking

No constitution violations; nothing to justify.
