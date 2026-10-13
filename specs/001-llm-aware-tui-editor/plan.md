# Implementation Plan: LLM-Aware Terminal Editor

**Branch**: `001-llm-aware-tui-editor` | **Date**: 2026-10-13 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-llm-aware-tui-editor/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command; its definition describes the execution workflow.

## Summary

Fallow is a full-screen terminal editor with a live, system-wide panel of LLM processes. It ships
as one Rust binary per platform (Linux, macOS, Windows). The UI uses ratatui and crossterm on a
single main thread. Process monitoring runs on a worker thread that combines sysinfo (enumeration
and CPU) with a small Fallow-owned platform layer for the sensitive and platform-specific reads:
command line, working directory, footprint metric, TCP connections, and pause/resume. That layer
is what guarantees that process environments are never read and that memory figures use the
FR-019 metric. Provider hostnames are resolved forward-only on an optional background thread. The
editor uses ropey buffers, lazily initialized tree-sitter grammars, notify watches limited to
what is open, and on-demand search with ripgrep's library crates. CI on all three platforms
enforces the constitution's memory and startup budgets.

**Status**: Design complete.
- **No planning gates remain.** Spike S-3 passed on all three platforms on 2026-10-13 (see
  "Gates before tasks"), so `/speckit-tasks` can run.
- D-1 was resolved by constitution v3.0.0.
- S-2 is now a macOS release blocker, not a planning gate (see "Release blockers").

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85, pinned in `rust-toolchain.toml`.

**Primary Dependencies**: ratatui, crossterm (with bracketed paste, mouse capture and kitty
keyboard enhancement flags), arboard, sysinfo, ropey, tree-sitter, tree-sitter-highlight,
15 tree-sitter grammar crates, notify, grep-searcher, grep-regex, ignore, toml, serde, regex,
globset, ipnet, compact_str, smallvec, zeroize, dirs. Platform: `libc` (Linux, macOS),
`windows-sys` (Windows). Dev only: portable-pty, serde_json, proptest. Each dependency is
justified in the Dependency budget section below.

**Storage**: None beyond the user's TOML config files and files the user saves.

**Testing**: `cargo test` (unit and integration), proptest for parsers (`/proc/net`,
KERN_PROCARGS2, TOML merge), and `cargo xtask` for benchmarks, detection fixtures, canary and
egress checks (research R-13).

**Target Platform**: Linux (x86_64, aarch64; kernel ≥ 4.14 for PSS, older kernels fall back to
RSS), macOS 13+ (arm64, x86_64), Windows 10 1809+ (x86_64; `PrivateWorkingSetSize`).

**Project Type**: Single native terminal application (one binary) plus a dev-only `xtask` crate.

**Performance Goals**:
- First frame in under 100 ms (p95).
- Process refresh every 2 s, using under 2% of one core on average with about 600 processes.
- Search returns its first results in under 1 s on the reference fixture (SC-010).
- Input-to-render latency under 16 ms in the editor.

**Constraints**:
- RSS under 50 MB idle and under 150 MB with the reference project (constitution I).
- No async runtime.
- No reads of process environments.
- No outbound traffic except provider DNS.
- No elevation required.
- No background indexing.

**Scale/Scope**:
- About 600 to 2,000 visible processes.
- Up to about 50 LLM processes and about 500 group members.
- Projects up to about 50,000 files and 1 GB.
- Files up to 10 MB editable (larger ones read-only).
- 15 highlighted languages.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Constitution v3.0.0. Evaluated before Phase 0 and after Phase 1 against v2.0.0. Re-run on
2026-10-13 against v3.0.0 after the D-1 amendment. The results below are current.

| Principle / gate | How the design complies | Result |
|---|---|---|
| I. Memory efficiency | Desk estimate is about 8–16 MB idle and 20–60 MB loaded (R-12). Per-snapshot data covers only LLM group members. Search and recent-files results are capped. Grammars are not paged in until used. Undo is capped. CI fails at 50 or 150 MB. S-3 measured peak idle RSS of 5.60 MB (Linux), 7.59 MB (macOS) and 11.46 MB (Windows) for the real crate set. | PASS (idle measured; loaded budget still to be measured by `bench-memory --loaded`) |
| II. Startup < 100 ms, no implicit background work | First frame renders before the first process scan finishes (the panel shows "scanning…"). DNS starts after the first frame. Watches cover only open files and expanded directories. Search and recent-files scans run on demand. | PASS |
| III. Terminal-only | ratatui and crossterm. No GUI, webview or browser runtime anywhere in the dependency tree (checked by `cargo deny` bans). | PASS |
| IV. Observability over control | Control happens only through the ControlRequest lifecycle (data-model.md): confirmation → revalidation of (PID, start time) → execution. The default focus is Cancel and there is no "don't ask again". SC-008 is tested. | PASS |
| V. Least privilege, read-only | No elevation requested. `--all-users` only takes effect if Fallow is already elevated. Writes go only to user-saved files. On Windows, `PROCESS_VM_READ` handles are opened only on demand for detected LLM processes and never to protected processes. CI verifies this against Defender with real-time protection explicitly enabled and checked (R-10). | PASS |
| VI. Benchmarked claims | `xtask bench-startup` and `bench-memory` run on all three platforms in GitHub Actions and fail the build when over budget (R-13). Constitution TODOs for the reference hardware and fixture are resolved in R-13. | PASS |
| VII. Privacy | No telemetry. DNS is limited to configured provider hostnames, forward only, and can be disabled (R-5). No payload inspection. The environment is never requested. Linux and Windows read the command line without it. On macOS, the KERN_PROCARGS2 buffer is wiped before return, nothing after argv is parsed, and truncated reads are used if S-4 confirms they exclude envp (R-2). The SC-007 canary test runs on all three platforms, including macOS, as v3.0.0 requires. | PASS (v3.0.0) |
| Workflow: dependency justification | See the Dependency budget section below. | PASS |
| Workflow: control paths tested | SC-008 test: there is no code path to `platform::signal_*` except through `ControlRequest::execute`, which requires a confirmed token. | PASS |
| Workflow: network code paths | The only network-capable code is the `dns` module (`ToSocketAddrs`). A CI egress test (`xtask egress`) captures traffic and fails on anything except DNS queries for the configured names. | PASS |

## Dependency budget

| Crate | Why | Memory and startup note |
|---|---|---|
| ratatui, crossterm | TUI and terminal I/O, including mouse (user-specified) | Diff-based rendering. Buffers are about 1.5 MB at 250×70. |
| sysinfo | Cross-platform enumeration, parent, owner, CPU | Restricted `ProcessRefreshKind`, with cmd, cwd, environ, exe and memory all `Never` (R-1). |
| ropey | Text buffers | About 1.1× the file size. |
| tree-sitter, tree-sitter-highlight, 15 grammars | Highlighting | Adds about 20–30 MB on disk. Untouched grammars cost no RSS (R-8). |
| notify | File change detection | Watches only open files and expanded directories. |
| grep-searcher, grep-regex, ignore | Project search, `.gitignore` | Used on demand only. |
| toml, serde | Config | |
| regex, globset | Signature matching | Compiled once at config load. |
| ipnet | Provider ranges | |
| compact_str, smallvec | Avoid heap allocations for small strings and vectors in snapshots | |
| zeroize | Wipe the macOS KERN_PROCARGS2 buffer (R-2) | |
| dirs | Platform config dir | |
| arboard | Platform clipboard fallback for copy, cut and `Ctrl-V` (FR-008). OSC 52 is written directly, with no crate. | Opened lazily on the first clipboard use, never at startup. On X11 it keeps a helper thread alive after a copy, so the selection stays available. That costs about 1 MB and only after the first copy. `bench-memory --loaded` performs one copy so that this cost is measured. |
| libc / windows-sys | Platform layer | `windows-sys` is raw bindings with no runtime cost. |
| **Rejected**: tokio, hickory-resolver, netstat2, crossbeam, anyhow in hot paths | R-5, R-4, R-6 | |

## Gates before tasks

`/speckit-tasks` must not run until this is closed:

| Gate | Owner / method | Pass criterion | Status |
|---|---|---|---|
| **S-3** Idle-budget prototype (R-12) | The `spike-idle-budget.yml` CI workflow (runs on push, or by manual dispatch) on `ubuntu-24.04`, `macos-15` and `windows-2025`, with the RSS series uploaded as an artifact. Same measurement harness on all three. Windows moved from local to CI on 2026-10-13 because of the ASR block (R-12, and "Development environment" below). | Peak RSS ≤ 25 MB over 5 min on each platform, recorded in research.md R-12 | **CLOSED 2026-10-13**: PASS. Linux 5.60 MB, macOS 7.59 MB, Windows 11.46 MB (run 38411827336) |

Closed or moved:

| Item | Resolution |
|---|---|
| D-1 macOS command line and environment | Option (a). Constitution v3.0.0, Principle VII. R-2 updated. |
| S-2 macOS footprint vs Activity Monitor | No longer a gate. Now a macOS platform task with SC-012 as its acceptance test, and a macOS release blocker (below). |
| S-4 truncated KERN_PROCARGS2 | Runs on the physical Mac as part of the macOS release blocker. If truncated reads exclude envp, the reader uses them (R-2). |
| R-10 Windows cwd and Defender | Resolved as a design. The CI job explicitly enables Defender real-time protection, verifies it, and fails if it can't (R-10). |

## Development environment

**Local development can't happen on the current machine.** Its Microsoft Defender Attack
Surface Reduction policy (rule `01443614-CD74-433A-B99E-2ECDC07BFC25`, "Block executable files
from running unless they meet a prevalence, age, or trusted list criterion") blocks every freshly
compiled executable, cargo build scripts included (research.md R-12). Development will move to
a **personal machine or GitHub Codespaces**.

| Decision | Status |
|---|---|
| Development environment: personal machine or GitHub Codespaces | **OPEN**: record the choice here once decided |

Until then, all builds, tests and measurements run in CI. Code signing for Windows and macOS
releases is a follow-up for tasks (R-12, product implication).

## Release blockers

Items that don't block planning or tasks, but block a release or a release label.

### macOS hardware verification (OPEN)

macOS ships labeled **"beta"** until it has been verified on a **physical Apple Silicon Mac** (M1
or later; not a GitHub-hosted runner, not an Intel Mac). Verification is planned on a borrowed
Mac.

**When the macOS platform module is complete**, generate a step-by-step checklist with exact
commands, as a task deliverable (`specs/001-llm-aware-tui-editor/checklists/macos-hardware.md`).
It covers:
1. **S-2 / SC-012**: Fallow's memory figure vs Activity Monitor with Ollama running a model (3B if
   the Mac has 8 GB, 8B otherwise), per PID and per group, within 10%. Method in research.md R-11.
2. **S-4**: whether a truncated KERN_PROCARGS2 read excludes the environment (research.md R-2).
3. **SC-007**: the canary test on macOS (`cargo xtask canary`).
4. **Manual run-through in Terminal.app and iTerm2**:
   - mouse selection, scrolling and pane resizing;
   - key hints bar, help overlay and command palette;
   - colors;
   - small-window collapse.

The "beta" label is removed only when all four pass **and** the results are recorded in
research.md (R-11 for item 1, R-2 for item 2, a new "macOS hardware verification" entry for items
3 and 4).

**Follow-up**: When a README exists, add this item to its platform support section (macOS: beta
until hardware verification passes). There is no README yet, so the task that creates it must
include this.

## Project Structure

### Documentation (this feature)

```text
specs/001-llm-aware-tui-editor/
├── plan.md              # This file
├── research.md          # Phase 0: R-1 … R-13
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   ├── cli.md           # command line + hidden test interface + frame marker
│   ├── config.md        # settings / signatures / providers TOML
│   └── ui-commands.md   # commands, keys, dialog, displayed semantics
└── tasks.md             # Phase 2 (/speckit-tasks — not created here)
```

### Source Code (repository root)

```text
Cargo.toml                    # workspace: fallow (bin) + xtask
rust-toolchain.toml
deny.toml                     # cargo-deny: licenses + bans (GUI/webview/async runtimes)
assets/defaults/
├── settings.toml
├── signatures.toml
└── providers.toml
src/
├── main.rs                   # arg parsing, terminal setup, first frame, thread start
├── app/
│   ├── state.rs              # AppState, focus, panes, modes
│   ├── commands.rs           # command registry (palette + keymap source)
│   ├── keymap.rs
│   └── event_loop.rs         # crossterm poll + channel drain + render
├── ui/
│   ├── layout.rs             # pane sizes, mouse resize, small-terminal collapse
│   ├── files.rs  editor.rs  processes.rs  summary.rs
│   ├── hints.rs  help.rs  palette.rs  confirm.rs  details.rs  recent.rs
│   └── theme.rs
├── editor/
│   ├── buffer.rs             # ropey + undo log + disk_state
│   ├── syntax.rs             # lazy grammar registry, visible-range highlighting
│   ├── search.rs             # grep/ignore, cancel, caps
│   ├── watch.rs              # notify wiring
│   └── tree.rs               # LazyTree
├── procmon/
│   ├── worker.rs             # refresh loop → Arc<Snapshot>
│   ├── snapshot.rs           # model types (data-model.md)
│   ├── detect.rs             # signatures, provider/local-model matching, confidence
│   ├── group.rs              # descendant grouping, totals
│   ├── status.rs             # FR-012 evaluation
│   ├── summary.rs
│   ├── recent.rs             # FR-017 on-demand scan
│   ├── control.rs            # ControlRequest lifecycle (FR-032/033)
│   └── platform/
│       ├── mod.rs            # trait Platform { cmdline, cwd, footprint, tcp_table, suspend, resume, kill }
│       ├── linux.rs
│       ├── macos.rs          # incl. KERN_PROCARGS2 reader with zeroize (R-2)
│       └── windows.rs        # incl. on-demand PEB cwd (R-10), NtSuspendProcess (R-7)
├── dns.rs                    # provider resolution thread + cache (R-5)
└── config/
    ├── mod.rs  merge.rs  schema.rs
tests/
├── control_requires_confirmation.rs   # SC-008
├── status_rules.rs                    # FR-012 table tests
├── grouping.rs
├── config_merge.rs
├── disk_change.rs                     # FR-006
└── platform_<os>.rs                   # parsers vs fixtures, live smoke tests
xtask/
└── src/main.rs               # bench-startup, bench-memory, detect-fixtures, canary, egress, defender-check, gen-fixture
spikes/                        # throwaway, excluded from workspace build
├── idle-budget/              # S-3
└── macos-footprint/          # S-2, S-4
.github/workflows/ci.yml      # matrix: ubuntu-24.04, macos-15, windows-2025 (Windows job runs defender-check)
.github/workflows/spike-idle-budget.yml   # S-3 on Linux + macOS, manual dispatch
```

**Structure Decision**: One binary crate with module boundaries, rather than many crates. A
multi-crate split would add build complexity without memory benefit. Platform code is isolated
behind one `Platform` trait with one file per OS. `xtask` is the only other workspace member and
is never shipped. Spikes live outside the workspace so they can't leak into the product.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| *(Resolved)* D-1: the macOS command-line read (`KERN_PROCARGS2`) also returns environment values. No longer a violation: constitution v3.0.0 Principle VII allows it with a wiped buffer and the canary test on macOS. | Command-line signatures are how interpreter-hosted agents are recognized with confidence "certain" (`node …/claude`, `python -m aider`). macOS has no argv-only API (R-2). | Option (a) chosen 2026-10-13. Not chosen: (b) process names only on macOS, because it weakens detection, and (c) an opt-in command-line read, with the same effect by default. Truncated reads are adopted if S-4 confirms they exclude envp. |
| Custom platform layer instead of only sysinfo/netstat2 | FR-024 and SC-007 can only be guaranteed if Fallow owns the command-line, cwd and connection reads. FR-019 metrics aren't available in sysinfo. | sysinfo's memory is RSS or working set, and its internals can change what is read per field. netstat2 allocates every socket per call (R-1, R-4). |
