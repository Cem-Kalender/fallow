# Implementation Plan: Fallow Studio redesign

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is `008-studio-redesign`) | **Date**: 2026-11-01 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/008-studio-redesign/spec.md`

## Summary

008 reworks three screens of 007's page and one part of its loop:

- **The loop page**: verification instead of specification, with a "Verified by Fallow" row and evidence panels (research R-1), and steering in the header (R-2).
- **Pipelines and Agents** become two tabs (R-3):
  - an agent form that rewrites only the lines it changed (R-4);
  - "Save for" (R-5);
  - the pipeline drawn as a graph of its fixed steps (R-6).
- **The loop engine** runs a checkpoint's checkers marked `parallel_with` at the same time. It does so by tracking a set of running steps instead of one, with one agent call per checker (R-7).
- **A reference mockup** for the changed screens (R-8).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as 007).

**Primary Dependencies**: no new crates and no JavaScript libraries. The page stays plain HTML, CSS and JavaScript (007 S-2); the graph is SVG drawn by the page.

**Storage**: unchanged (007 contracts/storage.md). The loop's event log gains no new event kinds; the running set is not stored, because it follows from the log on resume, as `busy` does today.

**Testing**: `cargo test` with the fake runtime, as in 007.
- The verification summary in the loop view.
- The agent field rewrite (unit tests and the API).
- Parallel checkers: overlap in time, judged together, one repair, resume, and the slot cap.
- The graph layout and the screens: the visual check against the mockup (headless Chrome screenshots, as in 007's T085), which also exercises `layout()` on every default pipeline. The page has no JavaScript test runner; the visual check is the layout's test.
- SC-004 is a timing test in `tests/us8_parallel.rs`, so it runs in CI with `cargo test` and fails the build when broken.
- 007's tests stay green.

**Target Platform**: Windows 10/11 first, as 007.

**Project Type**: local service with a browser page (one crate plus `xtask`).

**Performance Goals**:
- SC-004: a checkpoint with two parallel checkers takes the slower one's time plus 10%.
- 007's budgets hold: page ready ≤ 500 ms, ≤ 60 MB idle, ≤ 200 MB with 4 agents.

**Constraints**:
- No outbound connections.
- Pipelines and definitions stay compatible: no new keys, and `parallel_with` keeps its meaning.
- Decisions stay replayable from the event log.

**Scale/Scope**:
- Pipelines of up to ~12 steps.
- Checkpoints with up to 4 parallel checkers.
- At most `max_parallel` agents at once across loops.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.1.0.

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Fallow's checks and the "verified / not verified" outcome are unchanged:<br>• Parallel checkers are judged together only when all have answered.<br>• An incomplete member is asked again.<br>• A pipeline without a checker shows "not checked" on every story. | PASS |
| II. The Engineer Decides | Steering moves, but nothing is removed. Every save of an agent or pipeline is the engineer's, with the scope stated. Merging still needs the engineer, with the evidence one click away. | PASS |
| III. Coordination in Code, Decisions in the Repo | Running alongside is scheduled by the engine from the pipeline. Agents still never talk to each other. Repairs are one per checkpoint round, recorded. | PASS |
| IV. Simplest Setup First | No new step kinds and no free-form edges (FR-012). `parallel_with` already existed; 008 makes it true. The default pipelines are unchanged. Running alongside can be switched off per pipeline (remove `parallel_with`) or for every loop (`max_parallel = 1`). | PASS |
| V. Local and Private | No network path. No new data is kept. | PASS |
| VI. Least Privilege | Only checkers (which never edit) run alongside each other. Each keeps its own worktree and rights. | PASS |
| VII. Runs Survive | Each running step resumes on its own; every cut-short step is taken up, not only the last (R-7). | PASS |
| VIII. Small and Measured | No dependencies. The graph is a short hand-written layout. SC-004 and 007's budgets are measured by `cargo xtask bench` and a parallel-checkpoint timing test. | PASS |

**Re-check after Phase 1 design**: PASS. The contracts add to the loop view's shape and to `PUT /api/agents`, and change the page's routes; no file format changes.

## Project Structure

### Documentation (this feature)

```text
specs/008-studio-redesign/
├── plan.md          # this file
├── research.md      # R-1 to R-8
├── data-model.md
├── quickstart.md    # Q1 to Q5
├── contracts/
│   ├── loop-view.md # the verification summary in GET /api/loops/<l>
│   ├── agents-api.md # PUT /api/agents/<name> with {fields}
│   └── page.md      # routes, header steering, the graph layout rules
├── mockup/          # the reference screens (FR-017)
├── checklists/requirements.md
└── tasks.md         # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── run/
│   ├── engine.rs      # Run.running (the set), Msg::Done per step, stop/shutdown for all, slots per step
│   ├── feature.rs     # checkpoint(): parallel groups; step(): start every queued member; requeue all cut-short
│   ├── machine.rs     # drive/stop/merge with the running set
│   ├── questions.rs   # ask/lead_done per running step; deliver compares the agent
│   ├── activity.rs    # verification summary per story; every running step "working"
├── studio/defs.rs     # put_agent with {fields}: rewrite only those frontmatter lines
│   └── dashboard.rs   # agents running counts steps
├── service/web/
│   ├── index.html     # nav links and the agents.js script tag
│   ├── app.js         # routes (deep links), shared "Save for"
│   ├── app.css        # verification row and panel, header steering, graph, agent form
│   └── screens/
│       ├── loop.js        # verification row, evidence panels, header steering
│       ├── pipelines.js   # pipeline list, graph editor, step inspector
│       └── agents.js      # NEW: the Agents tab
└── bin/fallow-fake-agent.rs  # a lock around its record of used steps (two agents at once)
tests/
├── us8_verification.rs  # NEW: the loop view's summary and evidence (US1)
├── us8_agent_fields.rs  # NEW: saving an agent's fields (US3)
├── us8_parallel.rs      # NEW: checkers alongside each other (US5)
└── (007's tests, adjusted where the loop view or the page changed)
```

**Structure Decision**: the same crate and page. One new screen (`agents.js`, served by `src/service/http.rs`), three new test files, and the engine change in `src/run`.

The order of the work follows the stories:
1. the loop page (US1, US2);
2. the two tabs and the agent form (US3);
3. the graph (US4);
4. parallel checkers (US5);
5. the mockup and the visual check (US6).

US5 is engine work and is independent of the page.

## Complexity Tracking

No violation needs justifying.
