# Implementation Plan: Agent Orchestration

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; see spec Assumptions) | **Date**:
2026-10-20 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/003-agent-orchestration/spec.md`

## Summary

Agents in Fallow sessions get coordination tools through the Model Context Protocol:
- a board per project plus a shared board;
- named events to signal and wait for;
- deliveries into other sessions;
- a task queue;
- requests to start sessions.

How it works:
- **Bridge**: each agent starts `fallow mcp-bridge` as an ordinary stdio MCP server. Fallow adds it
  for that run only, through the agent's own flags, without touching user files (research R-5).
- **Channel**: the bridge connects back over one user-only local channel: a Unix socket in Fallow's
  cache directory, or a named pipe on Windows (R-2).
- **Identity**: the bridge proves which session it belongs to with the kernel-reported peer process
  plus a per-run token (R-3).
- **Hub**: all state lives in an in-memory hub owned by the main loop (R-6).
- **Gate**: typing into a session is possible only with a `DeliveryGrant`. Only the engineer's
  approval or a matching, unexpired standing permission can create one (R-7, constitution IV
  v4.0.0).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85, as 001 and 002.

**Primary Dependencies**: 002's stack. Changes:
- `serde_json` moves from dev-dependencies to dependencies.
- New `windows-sys` features: `Win32_System_Pipes`, `Win32_Storage_FileSystem`, `Win32_System_IO`,
  `Win32_Security_Cryptography`.
- No new crate. See the Dependency budget.

**Storage**: none. Orchestration state is memory-only (FR-023). The channel socket, and the per-run
Gemini settings file, live in `<cache_dir>/fallow/run/` while Fallow runs.

**Testing**:
- `cargo test`: hub, gate, forgery, limits, JSON-RPC.
- A new `cargo xtask orchestration` with an `mcp-agent` fixture persona.
- `egress`, `canary` and `bench-memory` extended (R-9).
- CI on `ubuntu-24.04`, `macos-15`, `windows-2025`.

**Target Platform**: as 001 and 002: Linux, macOS 13+, Windows 10 1809+.

**Project Type**: single native terminal application. The `fallow mcp-bridge` subcommand is the
same binary.

**Performance Goals**:
- Post to read < 1 s p95, between agents and to the engineer's screen (SC-002).
- Revoke all takes effect before the next request (SC-005).
- Hub requests are handled on the main loop in microseconds and never delay a frame.

**Constraints**:
- RSS < 150 MB with 4 sessions, a full board and a full activity log (SC-007).
- Idle and startup unchanged: no hub, thread or channel before the first orchestrated session
  (FR-005).
- No network socket of any kind (FR-002).
- No async runtime.

**Scale/Scope**:
- Up to 16 sessions (002 limit).
- 1,000 messages per board, 24 MiB of stored text in total, 5,000 log entries, 500 tasks.
- 13 tools.

No NEEDS CLARIFICATION remains. The open **verify** items (Codex approval-mode name and timeouts,
Gemini timeout behaviour, the Unix ancestry walk through exiting wrappers) are checked in
implementation, with fallbacks named in research.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.* Constitution v4.0.0.

| Principle / gate | How the design complies | Result |
|---|---|---|
| I. Memory | Hub text capped at 24 MiB plus the 5,000-entry log (~1.5 MB). 002's loaded peak was ~30–40 MB on CI. `bench-memory --loaded` fills board and log through 4 sessions and keeps the 150 MB gate (R-9). Idle unchanged: no hub exists at idle. | PASS (to be measured) |
| II. Startup, no implicit background work | Hub, listener and threads are created when the first orchestrated session starts (FR-005; constitution II's new bullet). The accept loop is idle until a bridge connects. `bench-startup` is unchanged, plus a unit test that no hub exists before. | PASS |
| III. Terminal-only | Nothing graphical; MCP over stdio and a local channel. | PASS |
| IV. Observability over control (v4.0.0) | Deliveries into sessions need per-action approval or a standing permission. Permissions are scoped by source, target and action, last at most 8 h, are visible in one view, are revocable singly and all at once, and are announced at each use (FR-011 to FR-016). Sender labels are set by Fallow (R-3, R-8). No tool and no permission covers stop, pause, resume, kill or re-prioritize (FR-017): those keep 001's ControlRequest confirmations. Processes Fallow did not start are never contacted. | PASS |
| V. Least privilege | New sessions start only on approval or under a `StartSession` permission with a limit, within 002's limits (FR-021). Writes stay inside Fallow's cache directory (`run/`). No elevation. Fallow never reads the agent CLIs' own configuration files; for Gemini it checks only whether a system settings file exists. | PASS |
| VI. Benchmarked claims | SC-002 latency is measured by `xtask orchestration` in CI. SC-007 by `bench-memory` and `bench-startup`. SC-003/004/006 are CI tests. | PASS |
| VII. Privacy | One local channel, user-only (0700 directory, owner DACL, `PIPE_REJECT_REMOTE_CLIENTS`). No network socket; the egress test gains a listening-socket check (R-9). Content is memory-only and never logged. The canary covers board, task and delivery. Fallow makes no request on an agent's behalf (FR-024). The bridge reads no environment variable; the token travels in per-run arguments (R-5). | PASS |
| Workflow: dependency justification | Dependency budget below. | PASS |
| Workflow: typing into a session on another agent's behalf is tested | `DeliveryGrant` type gate plus a test driving every request path without valid approval (R-7, SC-003). | PASS |
| Workflow: no listening network socket in CI | Egress test extension on all three OSes (R-9). | PASS |

Re-check after Phase 1 design (data model, contracts): no violations, so no Complexity Tracking
entries. Two design points to keep in view:
- **Gemini**: auto-connecting it writes a 0600 per-run file containing the token. It lives in
  Fallow's cache directory and is deleted when the session exits.
- **Pre-allowed tools**: inside Claude Code and Codex, Fallow's tools are pre-allowed, so the agent
  asks no tool permission. Every action that crosses sessions is gated by Fallow itself.

## Dependency budget

| Change | Why | Memory and startup note |
|---|---|---|
| `serde_json` 1 (MIT/Apache-2.0): dev → regular dependency | JSON-RPC framing (R-1) | Already in the lock and built for tests. Its code is linked into the binary but runs only in the bridge and the hub. |
| `windows-sys` features `Win32_System_Pipes`, `Win32_Storage_FileSystem`, `Win32_System_IO`, `Win32_Security_Cryptography` | Named pipe, DACL, `ProcessPrng` (R-2, R-3) | Raw bindings; no runtime cost. |
| **Rejected** | `rmcp` (tokio), `interprocess` (second windows-sys copy, still needs raw FFI), `getrandom` as a direct dependency (not needed: `/dev/urandom`, `ProcessPrng`) | R-1, R-2, R-3 |

## Project Structure

### Documentation (this feature)

```text
specs/003-agent-orchestration/
├── plan.md                    # This file
├── research.md                # Phase 0 (R-1 … R-9)
├── data-model.md              # Phase 1
├── quickstart.md              # Phase 1
├── contracts/
│   ├── mcp-tools.md           # tools offered to agents; bridge invocation
│   ├── config.md              # [orchestration] settings; profiles.toml `orchestration`
│   └── ui-commands.md         # orchestration view, grant dialog, keys
├── constitution-amendment.md  # ratified 2026-10-19 (v4.0.0)
├── checklists/requirements.md
└── tasks.md                   # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

New and changed files on top of 002's tree:

```text
src/
├── main.rs, cli.rs              # CHANGED: `mcp-bridge` subcommand dispatched before any TUI setup
├── orchestration/
│   ├── mod.rs                   # NEW: Hub (data-model.md), created on the first orchestrated session
│   ├── board.rs                 # NEW: boards, messages, byte cap
│   ├── events.rs                # NEW: events, waits, interests, wake requests (R-4)
│   ├── delivery.rs              # NEW: Delivery state machine; DeliveryGrant (R-7)
│   ├── permissions.rs           # NEW: Permission, cover(), revoke all
│   ├── tasks.rs                 # NEW: queue state machine and advancement
│   ├── activity.rs              # NEW: bounded activity log
│   ├── tools.rs                 # NEW: tool list, argument validation, labelled results (contracts/mcp-tools.md)
│   ├── rpc.rs                   # NEW: JSON-RPC 2.0 framing, MCP 2025-11-25 lifecycle (R-1)
│   ├── channel.rs               # NEW: listener, accept loop, connection threads, hello check (R-2, R-3)
│   ├── identity.rs              # NEW: token, peer credentials, ancestry / Job membership (R-3)
│   ├── inject.rs                # NEW: per-profile run arguments and Gemini settings file (R-5)
│   └── bridge.rs                # NEW: `fallow mcp-bridge` stdio ⇄ channel relay
├── session/
│   ├── mod.rs                   # CHANGED: Link per session; deliver(grant, text); last-key time
│   ├── profiles.rs              # CHANGED: `orchestration` style per profile
│   └── platform/windows.rs      # CHANGED: expose the session Job handle for IsProcessInJob
├── app/
│   ├── events.rs                # CHANGED: AppEvent::Hub(link, request), Hub replies
│   ├── state.rs, workspace.rs   # CHANGED: hub on App; tick drives expiry, waits and the queue
│   ├── orchestration.rs         # NEW: overlay commands, approval, grant dialog
│   └── commands.rs, keymap.rs   # CHANGED: palette entries, `o`, `i`, overlay keys
├── config/schema.rs             # CHANGED: [orchestration]; profiles.toml `orchestration`
└── ui/
    ├── orchestration.rs         # NEW: overlay with Requests, Board, Queue, Permissions, Activity
    ├── sessions.rs, hints.rs    # CHANGED: "waiting for …", request count, link state
    └── help.rs                  # CHANGED: orchestration section, conflicts rows
tests/
├── orch_hub.rs, orch_gate.rs, orch_identity.rs, orch_rpc.rs, orch_queue.rs
xtask/src/
├── orchestration.rs             # NEW: PR scenario with two mcp-agent sessions; SC-002 latency
├── fixtures.rs                  # CHANGED: "mcp-agent" persona (scripted MCP client)
├── egress.rs                    # CHANGED: listening-socket check
├── canary_sessions.rs           # CHANGED: canary through board, task, delivery
└── bench_memory.rs              # CHANGED: --loaded fills board and log
```

**Structure Decision**: the same single crate. One new top-level module, `orchestration`. It talks
to sessions only through `Session::deliver(DeliveryGrant, …)` and read-only session state, which
keeps the SC-003 gate in one place. The bridge is a subcommand of the same binary, so there is
nothing extra to install or sign.

## Phases (for /speckit-tasks)

1. **Foundation**:
   - `serde_json` promotion and the windows-sys features, checked with `cargo deny`;
   - `[orchestration]` settings and the profile key;
   - `rpc.rs` with tests;
   - `channel.rs` and `identity.rs` on all three OSes;
   - `bridge.rs`;
   - the `inject.rs` styles;
   - the `mcp-agent` fixture;
   - the lazy hub creation with its startup test.
2. **US1 board**:
   - `whoami`, `session_list`, `board_post`, `board_read`;
   - sender labels, limits and rate;
   - the Board tab;
   - the forgery test (SC-004);
   - `xtask orchestration` latency (SC-002).
3. **US2 events**:
   - `event_signal`, `event_wait` (block limit, progress, cancel), `event_status`;
   - overview "waiting for";
   - the user signals from the Queue tab;
   - interests (the wake requests arrive with US3).
4. **US3 delivery**:
   - `deliver` and `inbox`;
   - the Requests tab;
   - `DeliveryGrant` and `Session::deliver`;
   - the typing guard;
   - permissions and the grant dialog;
   - revoke all;
   - wake deliveries;
   - the gate test (SC-003).
5. **US4 queue**:
   - tasks, preconditions, the free-target rule, advancement;
   - `task_create` / `task_list` / `task_report`;
   - the Queue tab editing;
   - the PR scenario end to end in `xtask orchestration` (SC-001).
6. **US5 start**: `session_start`, approval, the `StartSession` limit, 002 limit and memory checks.
7. **Polish**:
   - the Activity tab and filter;
   - help and conflicts rows;
   - the egress listening-socket check;
   - the canary;
   - `bench-memory`;
   - README;
   - quickstart manual run on Windows.

## Complexity Tracking

No constitution violations; nothing to justify.
