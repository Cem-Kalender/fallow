# Implementation Plan: Agent Sessions and Multiple Projects

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; see spec Assumptions) | **Date**:
2026-10-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-agent-sessions/spec.md`

## Summary

Fallow gains agent sessions, a remembered list of projects, and an agent-first default screen:
- **Sessions**: each session runs an agent CLI in a pseudo-terminal (`portable-pty`, ConPTY on
  Windows). Fallow emulates the terminal itself (`vt100` 0.18.1) and renders it into its own
  ratatui buffer.
- **Scrollback**: two tiers keep 10,000 lines per session inside the memory budget. The newest
  1,000 lines are full cells; older lines move to a compact store (research R-1, R-2).
- **Event loop**: becomes one channel fed by an input thread, the procmon worker, and a reader and
  a waiter thread per session (R-5).
- **Attention**: comes from the bell, notification escapes and title status, then a quiet-and-idle
  fallback (R-4).
- **Process panel**: each session's agent is tracked by procmon as a labelled group (R-8).
- **Projects**: persisted in `projects.toml` in Fallow's config directory (R-10).
- **Editor**: 001's editor becomes the secondary view of the selected project.

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (toolchain 1.99, `rust-toolchain.toml`),
as 001.

**Primary Dependencies**: 001's stack (ratatui 0.30, crossterm 0.29, ropey, tree-sitter, notify,
grep-*, ignore, arboard, …), plus:
- `vt100 =0.18.1`;
- `portable-pty 0.9`, moving from xtask's dev-dependencies to a regular dependency;
- on Windows, extra `windows-sys` features for Job Objects.

See the Dependency budget.

**Storage**: `projects.toml` (written by Fallow) and `profiles.toml` (user) in the config directory;
nothing else. Session output stays in memory (FR-026).

**Testing**: `cargo test`; transcript replay tests; `cargo xtask` benchmarks extended with sessions
(R-9); CI on `ubuntu-24.04`, `macos-15`, `windows-2025`.

**Target Platform**: as 001: Linux, macOS 13+, Windows 10 1809+ (ConPTY floor).

**Project Type**: single native terminal application plus the dev-only `xtask` crate (unchanged).

**Performance Goals**:
- Typed key to agent echo p95 < 50 ms (SC-003).
- Session switch drawn < 100 ms (SC-005).
- Key echo < 100 ms with 8 flooding sessions (SC-006).
- Render ≤ 1 frame per 16 ms while output streams.

**Constraints**:
- RSS < 50 MB idle (with project list), < 150 MB with the reference project and 4 full sessions
  (FR-029).
- First frame < 100 ms.
- No async runtime; no network.
- No process started without an explicit user action (FR-025, constitution V).

**Scale/Scope**:
- Up to 16 sessions (8 by default), 10,000 scrollback lines each.
- Up to 200 projects.
- 5 built-in profiles.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution v4.0.0.

| Principle / gate | How the design complies | Result |
|---|---|---|
| I. Memory | Two-tier scrollback: ~35 MB worst case for 4 sessions at 200 columns (R-2). 001's loaded peak was 12.7–25.3 MB on CI. `bench-memory --loaded` gains 4 flooding sessions and keeps the 150 MB gate (R-9). Idle unaffected: no session exists at idle; `projects.toml` is small. | PASS (to be measured) |
| II. Startup, no implicit background work | `projects.toml` read after the first frame; profiles' availability checked when the start menu opens; no session or thread per session until the user starts one (FR-025, FR-028). `bench-startup` runs with 50 projects listed. | PASS |
| III. Terminal-only | The terminal emulator is a Rust library rendering into Fallow's own TUI; no GUI, webview or external terminal program. | PASS |
| IV. Observability over control (v4.0.0) | Sessions are "Fallow sessions". The user's own typing is not an action by Fallow. Stop and force stop go through 001's ControlRequest with per-action confirmations (FR-012); quitting asks once, listing all sessions. No standing permissions exist in 002 (they arrive with 003). Processes Fallow did not start are untouched. | PASS |
| V. Least privilege | A process starts only on an explicit start or restart (FR-025), with Fallow's own privileges and environment plus profile additions. Writes: `projects.toml` only. On Windows a Job Object ensures nothing outlives Fallow. | PASS |
| VI. Benchmarked claims | SC-003/005/006 latency (`bench-sessions`), SC-007 memory and startup, and SC-004 attention timing are CI benchmarks on three OSes (R-9). | PASS |
| VII. Privacy | Output and scrollback memory-only; no logging of output; the environment is passed to the agent without being read (only configured addition names are shown); no network code; canary test extended to sessions (R-9). The local orchestration channel is 003's, not 002's. | PASS |
| Workflow: dependency justification | Dependency budget below. | PASS |
| Workflow: control paths tested | Stop and force stop reuse 001's ControlRequest and its SC-008 test; a new test asserts no child is spawned without a start command (R-9). | PASS |
| Workflow: no listening sockets | 002 opens none; the 003 CI check will cover it. | PASS |

Re-check after Phase 1 design (data-model, contracts): no change; no violations, so no Complexity
Tracking entries.

## Dependency budget

| Crate | Why | Memory and startup note |
|---|---|---|
| `vt100 =0.18.1` (MIT) | Terminal emulation for sessions (R-1) | 32 B per cell; bounded by the two-tier scrollback (R-2). Nothing allocated until a session starts. Deps `vte`, `unicode-width` (already present), `itoa`. Pinned exactly: 0.18.0 was broken. |
| `portable-pty 0.9` (MIT) | Pseudo-terminals on Linux, macOS, Windows (R-7) | Already built and exercised in CI as an xtask dev-dependency. Brings `filedescriptor`, `nix 0.28`, `serial2` (BSD-2-Clause OR Apache-2.0), `winapi` 0.3 on Windows: all within `deny.toml`. Duplicate versions of `nix`/`winapi` beside newer ones: accepted (bans allow multiple versions). |
| `windows-sys` features `Win32_System_JobObjects` | Job Object for sessions (R-7) | Raw bindings, no runtime cost. |
| **Rejected** | `alacritty_terminal` (memory, no scrollback hook), `termwiz`/`wezterm-term` (unpublished, heavy), `tui-term` (pins vt100 0.16), libghostty bindings (alpha, Zig), any async runtime | R-1, R-5 |

## Project Structure

### Documentation (this feature)

```text
specs/002-agent-sessions/
├── plan.md              # This file
├── research.md          # Phase 0 (R-1 … R-10)
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   ├── config.md        # [sessions] settings, profiles.toml, projects.toml
│   └── ui-commands.md   # views, keys, leave key, conflicts
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

New and changed files on top of 001's tree:

```text
src/
├── app/
│   ├── event_loop.rs        # CHANGED: one AppEvent channel; input thread; frame cap (R-5)
│   ├── events.rs            # NEW: AppEvent and the input thread
│   ├── state.rs             # CHANGED: Workspace (view, focus, layout), project list, sessions
│   ├── workspace.rs         # NEW: agent-workspace commands and key handling
│   ├── commands.rs, keymap.rs   # CHANGED: new commands and contexts (Projects, Sessions, Session)
├── projects/
│   ├── mod.rs               # NEW: ProjectList, add/remove/rename/move, missing detection
│   └── store.rs             # NEW: projects.toml read (after first frame) and atomic write
├── session/
│   ├── mod.rs               # NEW: Session, SessionManager, states (data-model.md)
│   ├── pty.rs               # NEW: spawn (portable-pty), reader and waiter threads, resize, stop
│   ├── term.rs              # NEW: vt100 parser + callbacks, query replies (R-6), title/bell/OSC
│   ├── scrollback.rs        # NEW: compact tier, unified line addressing for scroll/select/search
│   ├── attention.rs         # NEW: R-4 signals and the quiet fallback
│   ├── keys.rs              # NEW: KeyEvent → bytes for the agent (legacy encoding, app cursor mode, paste)
│   ├── profiles.rs          # NEW: built-ins, profiles.toml merge, PATH/PATHEXT and .cmd shim resolution
│   └── platform/{unix.rs, windows.rs}   # NEW: process group signals; Job Object
├── procmon/worker.rs        # CHANGED: Request::Track / Untrack (R-8)
├── config/schema.rs         # CHANGED: [sessions] settings; profiles.toml
└── ui/
    ├── workspace.rs         # NEW: agent-workspace layout (projects | sessions | processes)
    ├── projects.rs          # NEW: project list
    ├── sessions.rs          # NEW: session list and overview
    ├── terminal_pane.rs     # NEW: vt100 screen → ratatui buffer, both scrollback tiers, selection
    └── layout.rs, hints.rs, help.rs   # CHANGED: two views, new hints and help sections
tests/
├── projects_store.rs, profiles.rs, session_keys.rs, session_replies.rs,
├── session_attention.rs, session_lifecycle.rs, session_transcripts.rs, scrollback.rs
└── fixtures/transcripts/    # recorded first screens of the built-in agents (scrubbed)
xtask/src/
├── bench_sessions.rs        # NEW: echo latency, switch time, flood responsiveness
├── fixtures.rs              # CHANGED: "flood" and "attention" personas
└── bench_memory.rs          # CHANGED: --loaded adds 4 flooding sessions
```

**Structure Decision**: the same single crate as 001, with two new top-level modules (`projects`,
`session`) behind narrow interfaces. Platform-specific process control stays behind one small
module per OS, like 001's `procmon::platform`.

## Phases (for /speckit-tasks)

1. **Foundation**:
   - the event-loop rework (R-5), which must keep every 001 benchmark green;
   - `[sessions]` settings and `profiles.toml`;
   - `portable-pty` and `vt100` added and checked with `cargo deny`.
2. **US1 sessions**:
   - spawn, reader and waiter threads, query replies, key passthrough, leave key;
   - the terminal pane, two-tier scrollback, selection and copy, resize;
   - exit and restart;
   - transcript, key and reply tests;
   - `bench-sessions` (echo latency).
3. **US3 default screen**: the agent workspace and view switching (it needs sessions to show).
4. **US2 several sessions**:
   - overview, attention (R-4) and notices;
   - next/previous, split, stop via ControlRequest, quit confirmation;
   - procmon tracking (R-8);
   - the flood and switch benchmarks;
   - the `--loaded` memory gate with 4 sessions.
5. **US4 projects**: list, persistence, missing folders, the process filter, the command-line
   folder.
6. **US5 profiles**: built-ins with per-run notification flags (R-4), Windows shim resolution
   (R-7), custom profiles, configuration reload.
7. **Polish**:
   - help and conflicts table;
   - the privacy canary with sessions;
   - the startup benchmark with 50 projects;
   - quickstart validation on all three OSes.

## Complexity Tracking

No constitution violations; nothing to justify.
