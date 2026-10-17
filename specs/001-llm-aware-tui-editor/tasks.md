---

description: "Task list for 001-llm-aware-tui-editor"
---

# Tasks: LLM-Aware Terminal Editor

**Input**: Design documents from `/specs/001-llm-aware-tui-editor/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/ (cli.md, config.md, ui-commands.md), quickstart.md

**Tests**: Included. The spec and plan require specific automated tests (SC-002, SC-003,
SC-005–SC-010, SC-012), the constitution requires CI benchmarks (Principle VI) and the
canary test (Principle VII), and plan.md lists the test files.

**Task IDs**: T117–T122 were added after the first version and sit in the phase where they
belong, so IDs aren't in numeric order inside Phases 3a and 3b.

**Organization**: Phases follow the user-story priorities in spec.md: P1 process panel and
summary, P2 sorting, P3 editor, P4 detection details, P5 jump to working directory, P6
kill/pause/resume. Each phase ends with something that can be run and tested on its own.

**Development environment**: All builds run in CI or on a machine without the Defender ASR
block (plan.md, Development environment). Manual tasks say where they must run.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: User story label (US1–US6)
- Every task names the files it touches and the requirement (FR / SC / R / CT) it satisfies

## Path Conventions

Single Rust crate at the repository root (`src/`, `tests/`, `assets/`), plus a dev-only `xtask/`
crate, as in plan.md, Project Structure. Spikes stay in `spikes/` outside the workspace.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace, toolchain and policy files

- [X] T001 Create the root workspace manifest `Cargo.toml`:
  - package `fallow`: edition 2024, `rust-version = "1.85"`, `license = "MIT OR Apache-2.0"`, `publish = false`;
  - workspace members `[".", "xtask"]`, `exclude = ["spikes"]`;
  - `[profile.release]` with `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`.
  - Satisfies: plan Technical Context, Constitution I/II.
- [X] T002 [P] Pin the toolchain in `rust-toolchain.toml`: channel `1.99`, components `rustfmt` and `clippy`, and targets for x86_64/aarch64 Linux, macOS and Windows. Satisfies: plan Technical Context.
- [X] T003 [P] Create `deny.toml` for cargo-deny.
  - Licenses allowed: MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib.
  - `[bans]` deny `tokio`, `async-std`, `smol`, `hickory-resolver`, `wry`, `tao`, `winit`, `gtk`, `webkit2gtk`, `netstat2`.
  - Satisfies: Constitution III and the plan's Dependency budget ("Rejected").
- [X] T004 [P] Add `rustfmt.toml` (`max_width = 100`) and `clippy.toml`, and add `.cargo/config.toml` with `[alias] xtask = "run --package xtask --release --"`. Satisfies: plan Testing.
- [X] T005 Create `src/main.rs` and empty module roots: `src/app/mod.rs`, `src/ui/mod.rs`, `src/procmon/mod.rs`, `src/procmon/platform/mod.rs`, `src/config/mod.rs`, `src/cli.rs`. `main` returns exit code 0, 1 or 2 per contracts/cli.md. Satisfies: plan Project Structure.
- [X] T006 [P] Create the `xtask/` crate: `xtask/Cargo.toml` (portable-pty 0.9, plus libc / windows-sys for the platform), and `xtask/src/main.rs` with a subcommand dispatcher (`bench-startup`, `bench-memory`, `detect-fixtures`, `canary`, `egress`, `defender-check`, `gen-fixture`, `bench-search`) where each stub exits 2 with "not implemented". Satisfies: plan Project Structure, R-13.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The terminal shell, config loading, the platform trait, and CI running the memory and startup benchmarks on all three platforms. **No editor work.**

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Command line and terminal shell

- [ ] T007 Implement argument parsing in `src/cli.rs` per contracts/cli.md:
  - `PATH`, `--all-users`, `--no-dns`, `--config-dir <DIR>`, `--print-config-paths`, `--version`/`-V`, `--help`/`-h`;
  - exit codes `0` quit, `1` startup failure, `2` invalid arguments;
  - hidden `--snapshot-json`, `--snapshot-hold <SECS>` and `--bench-stats <PATH>`, parsed but not shown in `--help`.

  Write the parser by hand, with no argument-parsing crate, to keep startup cost down (FR-041). Satisfies: contracts/cli.md.
- [ ] T008 Add the hidden `--bench-stats <PATH>` option to `specs/001-llm-aware-tui-editor/contracts/cli.md`. On exit it writes `pid=<own pid> ticks=<event-loop ticks> cycles=<procmon refresh cycles>`, so benchmarks can prove they measured a live Fallow (R-12 lessons). Also add the hidden `--snapshot-stream <SECS>` option (T048). Satisfies: R-12, R-13.
- [ ] T009 Implement terminal lifecycle in `src/ui/terminal.rs`:
  - enter raw mode and the alternate screen;
  - enable bracketed paste;
  - enable mouse capture unless `ui.mouse = false`;
  - on Linux/macOS, push the kitty keyboard flags (`DISAMBIGUATE_ESCAPE_CODES`) **without querying support first**;
  - on Windows, skip the kitty flags (the console input API already reports modifiers);
  - restore everything (pop flags, disable mouse and paste, leave the alternate screen, leave raw mode) on normal exit **and** from a panic hook.

  Satisfies: FR-001, FR-067, FR-063, FR-008 (paste), FR-041.
- [ ] T010 Implement the main event loop in `src/app/event_loop.rs` and `src/app/state.rs`:
  - block on `crossterm::event::poll` with a **250 ms** timeout, drain the worker channels on every wake, and render only when state changed;
  - count every loop iteration in a `ticks` counter (the Phase 2 liveness counter for `--bench-stats`);
  - draw the **first frame before** any background thread does work.

  Satisfies: R-6, FR-041, Constitution II.
- [ ] T011 [P] Implement the pane layout in `src/ui/layout.rs`:
  - three panes (Files, Editor, LLM Processes with a System area below it), with placeholder content for now;
  - pane sizes stored in state;
  - below **100×24**, collapse the panes to tabs switched with `Tab`; below **40×10**, show only "terminal too small" and keep processing input. Never draw garbled output.

  Satisfies: spec edge case "very small terminal", contracts/ui-commands.md Layout.
- [ ] T012 [P] Implement the hint bar in `src/ui/hints.rs`. It always shows context-sensitive hints for the focused pane and mode, and it **must contain `Ctrl-P Commands` whenever no text input has focus** (the benchmark frame marker in contracts/cli.md). Satisfies: FR-060, contracts/cli.md.
- [ ] T013 Wire `Ctrl-Q` quit and `Tab` / `Shift-Tab` pane focus cycling into `src/app/event_loop.rs`, so the shell is usable. Satisfies: FR-065.

### Configuration

- [X] T014 [P] Define config types in `src/config/schema.rs` per contracts/config.md:
  - `Settings` with defaults `refresh_interval_ms = 2000`, `recent_window_minutes = 30`, `detection.browsers = false`, `dns.enabled = true`, `dns.refresh_minutes = 15`, `windows.read_working_directory = "on_detect"`, `editor.read_only_threshold_mb = 10`, `editor.tab_width = 4`, `ui.mouse = true`;
  - `Signature` and `Provider`.

  Validation, quoted from the contract:
  - `refresh_interval_ms` "minimum 500"; `dns.refresh_minutes` "1..=60";
  - signatures: "`id` and `kind` are required, plus at least one of `match.name` and `match.cmdline`"; "`local_server` is allowed only with `kind = "local_model_server"`";
  - providers: "`hostnames` entries must be plain DNS names, with no wildcards and no IP literals"; "`ranges` entries must be valid CIDR"; "A provider with neither hostnames nor ranges is rejected".

  Satisfies: FR-015, FR-021, FR-022a, R-9.
- [X] T015 [P] Create the built-in defaults: `assets/defaults/settings.toml`, `assets/defaults/signatures.toml` and `assets/defaults/providers.toml`.
  - Signatures: agent CLIs (claude-code, codex, aider, gemini-cli, opencode, goose), local model servers (ollama, llama-server, lm-studio, vllm, koboldcpp) with `local_server.default_ports`, AI-enabled editors (cursor, windsurf), and browsers (chrome, msedge, firefox, safari, brave) with `kind = "browser"`.
  - Providers: anthropic, openai, google, mistral, cohere, groq, deepseek, openrouter, xai, with hostnames, and `ranges` only where the provider publishes them, each with a source comment.

  Satisfies: FR-020, FR-022a/b, FR-023, FR-026.
- [ ] T016 Implement config loading and merging in `src/config/mod.rs` and `src/config/merge.rs`:
  - defaults embedded with `include_str!`; user files from `dirs::config_dir()/fallow/` or `--config-dir`;
  - "a user entry with the same `id` replaces the built-in entry completely. `enabled = false` disables it";
  - unknown keys produce warnings;
  - a parse error keeps the defaults and surfaces `file:line: message`;
  - `--print-config-paths` prints the resolved paths and exits.

  Satisfies: FR-021, R-9, spec edge case "config syntax error".
- [X] T017 [P] Write `tests/config_merge.rs`: override by id, disable a built-in entry, each validation rule from T014, a syntax error at a known line reported as `file:line`, unknown-key warnings. Satisfies: FR-021, R-9.

### Platform trait and snapshot model

- [ ] T018 [P] Define the snapshot model in `src/procmon/snapshot.rs` per data-model.md: `ProcessKey { pid, start_time }`, `ProcessRecord`, `Footprint { bytes, metric: PhysFootprint | PrivateWorkingSet | Pss | Rss | WorkingSet }`, `CwdState { Unknown, Pending, Known(PathBuf), Unavailable(reason) }`, `Connection { owner, local, remote, state: Established | Listen | Other }`, `DetectionSignal` (kinds `Signature{id}`, `ProviderConnection{provider_id, via: Resolved | Range}`, `LocalModelConnection{server}`; "No environment-based kind exists (FR-024)"), `LlmProcess`, `ProcessGroup`, `Status { Serving, LlmConnected, Busy, Idle }`, `SystemSummary`, `Snapshot`, and `Note`. Satisfies: FR-011, FR-012, FR-013, FR-016, FR-019, FR-024.
- [ ] T019 Define the platform trait in `src/procmon/platform/mod.rs`: `cmdline`, `cwd`, `footprint`, `tcp_table`, `suspend`, `resume`, `kill`, returning the T018 types. Add stub files `src/procmon/platform/linux.rs`, `macos.rs` and `windows.rs`, selected with `cfg`, that return `Unsupported`. The trait doc comment states: "No method may request or read a process environment (FR-024, Constitution VII)". Satisfies: plan Structure, R-1.

### Benchmarks wired into CI

- [ ] T020 [P] Port the PTY harness from `spikes/idle-budget/measure/src/main.rs` into `xtask/src/pty.rs`, with all R-12 lessons:
  - 120×40 PTY, absolute binary path and cwd;
  - drain the output and answer `ESC[6n` once per query;
  - detect the frame marker `Commands`;
  - record output bytes after warm-up;
  - after sampling, check the executable path of the measured PID;
  - pass `--bench-stats` and require `pid` equal to the measured PID;
  - liveness, selected by `--require ticks|cycles`: `ticks` requires ≥ 90% of the expected loop ticks (duration ÷ 250 ms). This is the default in Phase 2, before procmon exists. `cycles` requires procmon cycles ≥ 90% of expected (duration ÷ `refresh_interval_ms`) and becomes the default once T034 lands.

  A run that fails any check is INVALID (exit 2). Satisfies: R-12, R-13, Constitution VI.
- [ ] T021 [P] Port the RSS readers and the executable-path readers into `xtask/src/rss.rs`: Linux `/proc/<pid>/status` VmRSS, macOS `proc_pidinfo(PROC_PIDTASKINFO)`, Windows `WorkingSetSize`. Satisfies: R-13 (RSS per constitution).
- [ ] T022 Implement `--bench-stats` in `src/main.rs`: on exit write `pid`, the event-loop `ticks` (T010) and the procmon `cycles` (0 until T034). Satisfies: T008, R-12.
- [ ] T023 Implement `cargo xtask bench-startup` in `xtask/src/bench_startup.rs`:
  - 3 warm-ups, then 30 runs, timing from spawn to the frame marker;
  - fail if **p95 ≥ 100 ms** (constitution v3.0.1: p95 of 30 runs after 3 warm-ups). On failure, re-run the whole benchmark (warm-ups and 30 runs) once automatically; the second result is final;
  - print the p50, p95 and max, and append them to `GITHUB_STEP_SUMMARY`.

  Satisfies: FR-041, SC-006, R-13.
- [ ] T024 Implement `cargo xtask bench-memory --idle` in `xtask/src/bench_memory.rs`: start Fallow with no project, wait 30 s, sample RSS every 1 s for 60 s, and fail if the max is **≥ 50 MB** or the run is INVALID. Liveness uses `--require ticks` in Phase 2 (switched to `cycles` by T034). Upload the series as CSV. Satisfies: FR-040, SC-005, R-13.
- [ ] T025 Create `.github/workflows/ci.yml` with the matrix `ubuntu-24.04`, `macos-15`, `windows-2025`. Steps:
  - `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo deny check`;
  - `cargo build --release`, `cargo test --workspace`;
  - `cargo xtask bench-startup` and `cargo xtask bench-memory --idle`.

  The build fails if either budget is exceeded. Satisfies: FR-050, FR-053, Constitution VI.
- [ ] T026 [P] Write `tests/cli_args.rs`: every option in contracts/cli.md, exit code 2 on bad arguments, hidden options absent from `--help`. Satisfies: contracts/cli.md.

**Checkpoint**: `fallow` opens a full-screen shell with three empty panes and the hint bar, `Tab`
cycles focus and `Ctrl-Q` quits. CI is green on all three OSes with the startup and idle-memory
gates enforced.

---

## Phase 3a: User Story 1 - See every LLM process at a glance (Priority: P1) 🎯 MVP

**Goal**: A system-wide LLM process panel, grouped, with status, footprint and confidence, plus
the system summary, help overlay and command palette needed to operate it.

**Independent Test**: Start an agent CLI, `ollama serve` with a model, and a script calling a
provider API, then launch `fallow` (quickstart V1–V3). All three appear with the correct type,
PID, cwd, memory with metric label, CPU, uptime, status, provider or model, and confidence. The
summary adds up.

### Tests for User Story 1

- [ ] T027 [P] [US1] Write `tests/status_rules.rs`: table tests for FR-012 with "first match wins" in the order serving → LLM connected → busy → idle. Cover:
  - "serving" only for `LocalModelServer` with ≥ 1 established incoming connection, regardless of CPU;
  - incoming connections never make a server "LLM connected";
  - an idle keep-alive connection stays "LLM connected";
  - "busy" is `mean(cpu_samples) > 2.0`.

  Satisfies: FR-012.
- [ ] T028 [P] [US1] Write `tests/grouping.rs`: descendants grouped under the nearest LLM ancestor, nested LLM processes listed in `nested`, a group total "Each member counted once", and `llm_ram` counting members unique across groups. Satisfies: FR-013, FR-016.
- [ ] T029 [P] [US1] Write `tests/detect.rs`:
  - a signature match gives `Certain`;
  - a provider match requires "`Established` and remote port 443" plus an address in the provider set, and gives `Likely`; listen, other states and other ports never match;
  - a client of a local model server is `Likely`, with that server's model;
  - "A process whose only matching signature has `kind = Browser` is dropped unless `detection.browsers = true`";
  - PID reuse: a new `(pid, start_time)` never inherits the old process's record.

  Satisfies: FR-020, FR-022, FR-023, FR-025, FR-026, spec PID-reuse edge case.
- [ ] T030 [P] [US1] Write parser tests for the platform readers, with fixtures in `tests/fixtures/`:
  - `tests/platform_linux.rs`: `/proc/net/tcp`/`tcp6` fixtures, `smaps_rollup` `Pss:` parsing, `cmdline` NUL splitting, and proptest round-trips;
  - `tests/platform_macos.rs`: KERN_PROCARGS2 parser property tests, including leading empty arguments and truncated buffers, plus a test hook asserting that the reader's buffer is all zeros after every call, including error paths;
  - `tests/platform_windows.rs`: TCP table row parsing for IPv4/IPv6 and `CommandLineToArgvW` splitting.

  Satisfies: R-2, R-3, R-4, FR-024.

- [ ] T121 [P] [US1] Write `tests/platform_fallbacks.rs`, forcing every fallback path through injectable readers in each platform module:
  - Linux: `smaps_rollup` missing (kernel < 4.14) or unreadable → footprint from `statm` with metric `Rss`, marked per process;
  - Windows: `GetProcessMemoryInfo` with `PROCESS_MEMORY_COUNTERS_EX2` failing (builds before 1809) → `WorkingSetSize` with metric `WorkingSet`; a failed or disabled PEB read → `CwdState::Unavailable(reason)`;
  - macOS: `proc_pid_rusage` failing → footprint `None` with a reason, never a wrong metric;
  - all: the UI metric label matches the metric actually used.

  Satisfies: FR-019, R-3, plan Target Platform (minimum versions).

### Platform implementations

- [ ] T031 [P] [US1] Implement Linux in `src/procmon/platform/linux.rs`:
  - `cmdline` from `/proc/<pid>/cmdline`;
  - `cwd` by `readlink /proc/<pid>/cwd`;
  - `footprint` from `/proc/<pid>/smaps_rollup` `Pss:`, falling back to `/proc/<pid>/statm` resident pages × page size with metric `Rss`;
  - `tcp_table` from `/proc/net/tcp` and `tcp6` into one reused buffer, keeping only Established-to-:443, Listen on local model ports, and loopback or local connections to them, then mapping the inodes of interesting sockets to PIDs through `/proc/<pid>/fd` (stop when all are found, cache PID→inode).

  Never open `/proc/<pid>/environ`. Satisfies: R-1, R-3, R-4, FR-019, FR-024.
- [ ] T032 [P] [US1] Implement macOS in `src/procmon/platform/macos.rs`:
  - `cmdline` with a KERN_PROCARGS2 reader: one reusable buffer sized to `kern.argmax`; parse exactly `argc` strings after the exec path; allocate nothing for envp; **zero the entire buffer with `zeroize` before returning, on every path**;
  - `cwd` from `proc_pidinfo(PROC_PIDVNODEPATHINFO)` → `pvi_cdir`;
  - `footprint` from `proc_pid_rusage(RUSAGE_INFO_V4)` → `ri_phys_footprint`, with metric `PhysFootprint`;
  - `tcp_table` from `proc_pidinfo(PROC_PIDLISTFDS)` + `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`, reading `soi_kind`, `insi_fport`/`insi_lport` and `tcpsi_state` at the xnu `proc_info.h` offsets validated in the spike.

  **Not done until the macOS canary (T120) passes in CI.** Satisfies: R-2, R-3, R-4, FR-019, FR-024, Constitution VII.
- [ ] T033 [P] [US1] Implement Windows in `src/procmon/platform/windows.rs`:
  - `cmdline` from `NtQueryInformationProcess(ProcessCommandLineInformation)` with `PROCESS_QUERY_LIMITED_INFORMATION`, split with `CommandLineToArgvW`;
  - `footprint` from `GetProcessMemoryInfo` with `PROCESS_MEMORY_COUNTERS_EX2` → `PrivateWorkingSetSize`, falling back to `WorkingSetSize` with metric `WorkingSet` on builds before 1809;
  - `tcp_table` from `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)` for AF_INET and AF_INET6 into a reused buffer;
  - `cwd` from a PEB read that copies **only** `ProcessParameters.CurrentDirectory.DosPath` and its buffer. It never reads the `Environment` target, never opens `PROCESS_VM_READ` handles to protected or system processes, LSASS or other users' processes in default mode, and respects `windows.read_working_directory` (`on_detect` / `on_select` / `never`). If the read is not done or fails, the result is `CwdState::Unavailable(reason)`.

  Satisfies: R-1, R-3, R-4, R-10, FR-019, FR-024.

### Process monitoring

- [ ] T034 [US1] Implement the procmon worker in `src/procmon/worker.rs`:
  - a thread named `procmon` with a 256 KiB stack, looping every `refresh_interval_ms`;
  - sysinfo with `ProcessRefreshKind::nothing().with_cpu().with_user(UpdateKind::OnlyIfNotSet)`, so `cmd`, `cwd`, `environ`, `exe` and `memory` are never requested;
  - "Records without a start time are dropped";
  - the user filter: "the record's user must equal the current user unless all-users mode is on";
  - command lines cached per `ProcessKey` and read once;
  - `cpu_samples` as a ring of the last 5 refreshes;
  - footprint and cwd read **only for LLM group members**;
  - an `Arc<Snapshot>` sent over `sync_channel(1)`, and the cycle counter feeding `--bench-stats`;
  - switch the default liveness requirement in `xtask/src/bench_memory.rs` from `ticks` to `cycles` (≥ 90% of expected procmon cycles) once this lands.

  Satisfies: R-1, R-6, FR-010, FR-015, FR-030, R-12.
- [ ] T035 [US1] Implement detection in `src/procmon/detect.rs`:
  - signatures: `match.name` glob (case-insensitive on Windows and macOS) and `match.cmdline` regex on argv joined with spaces, compiled once at config load;
  - provider connections against the `ResolvedProviderSet` (Established, port 443);
  - local model servers found by signature, with their listening ports;
  - clients connected to those ports, carrying the server's model;
  - kind `Script` for an interpreter matched only by connection, `Unknown` otherwise;
  - the model from `model_from_cmdline`;
  - confidence = the highest among the signals;
  - browsers filtered out unless enabled.

  Satisfies: FR-020, FR-022, FR-023, FR-025, FR-026, FR-027.
- [ ] T036 [US1] Implement grouping in `src/procmon/group.rs`. The root is "An LLM process that has no LLM ancestor", members are all descendants, `nested` holds the LLM descendants, and the totals for footprint and CPU count each member once. Satisfies: FR-013.
- [ ] T037 [US1] Implement status evaluation in `src/procmon/status.rs` per data-model.md (first match wins), recomputed every snapshot ("Status is not sticky"). Satisfies: FR-012.
- [ ] T038 [US1] Implement the system summary in `src/procmon/summary.rs`:
  - `total_ram` and `available_ram` from the OS;
  - `llm_ram` as the sum of group totals with members unique across groups;
  - `local_model_ram` from groups whose root is a `LocalModelServer`;
  - `fallow_ram` from Fallow's own footprint with the same metric as the other figures;
  - `low_memory` = "`available_ram < 10% of total_ram`".

  Satisfies: FR-016, FR-019.
- [ ] T039 [US1] Implement provider resolution in `src/dns.rs`:
  - a thread named `dns`, started only **after the first frame**;
  - resolves only the configured provider hostnames with `ToSocketAddrs`, forward only, one at a time; never calls a reverse lookup;
  - repeats every `dns.refresh_minutes` (default 15) and swaps the `ResolvedProviderSet` atomically after each pass;
  - built-in ranges are loaded at start and used until results arrive or when lookups fail;
  - the thread is not started when `dns.enabled = false` or with `--no-dns`.

  Satisfies: FR-022a, FR-022b, FR-022c, FR-035, R-5.
- [ ] T040 [US1] Implement all-users mode in `src/procmon/worker.rs` and `src/app/state.rs`: `--all-users` takes effect only when Fallow is already elevated (root or Administrator); otherwise Fallow shows a notice and continues in default mode, and it never prompts for elevation. When active, rows show their owner and a persistent "elevated: all users" indicator appears. Satisfies: FR-030, FR-031.

### UI for the panel

- [ ] T041 [US1] Implement the process panel in `src/ui/processes.rs`:
  - columns: name, type, PID, working directory, memory with its metric label, CPU, uptime, status, provider/model, confidence;
  - groups expand and collapse with `→` / `←` / `Space`;
  - selection moves with `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End`;
  - "scanning…" before the first snapshot, and an empty state when no LLM processes are found;
  - the coverage note is always visible: "Not covered: containers, remote machines", plus "WSL2" on Windows;
  - Linux processes that fell back to RSS are marked per process.

  Satisfies: FR-010, FR-011, FR-013, FR-018, FR-019, FR-051.
- [ ] T042 [US1] Implement the system summary area in `src/ui/summary.rs`: total, LLM, local model, Fallow and remaining RAM; the metric in use; and "GPU memory (VRAM) not included" where that applies. Satisfies: FR-016, FR-019.
- [ ] T043 [US1] Implement the command registry in `src/app/commands.rs`: id, title, context and default key for every command in contracts/ui-commands.md. It is the single source for the palette, help and hints, so no command is defined anywhere else. Satisfies: FR-062, FR-060, FR-061.
- [ ] T044 [US1] Implement global and process-panel default bindings in `src/app/keymap.rs`:
  - `Tab` / `Shift-Tab`; `Alt-1` / `Alt-2` / `Alt-3`;
  - `F1`, and `?` only when focus isn't in a text input;
  - `Ctrl-P`, `Ctrl-Q`;
  - the panel's movement and expand keys.

  Satisfies: FR-065, FR-066.
- [ ] T045 [P] [US1] Implement the help overlay in `src/ui/help.rs`:
  - every binding, grouped by pane, generated from the T043 registry;
  - the status legend and the confidence and recent-changes wording, verbatim from contracts/ui-commands.md "Displayed semantics", including that "LLM connected … does not mean a request is in progress";
  - the notes: terminal text selection needs `Shift`-drag (iTerm2 `Option`, Terminal.app `Fn`); `Alt-1/2/3` on macOS needs Option-as-Meta; F-keys on Mac keyboards need `Fn`; "Esc, then Tab" leaves the editor;
  - the Known terminal conflicts table;
  - opens with `F1`, `?` (outside text input) or the palette entry "Help"; closes with `Esc`.

  Satisfies: FR-061, FR-066, FR-012 (UI explanation).
- [ ] T046 [P] [US1] Implement the command palette in `src/ui/palette.rs`: `Ctrl-P` opens it, typing filters, and each command shows its key binding. It includes "Help" and "Reload configuration" (stub until US4). In text inputs the hint bar shows `Ctrl-P › Help`. Satisfies: FR-062, FR-066.
- [ ] T047 [US1] Implement mouse support for the panel and layout in `src/ui/processes.rs` and `src/ui/layout.rs`: click selects a row, the wheel scrolls the pane under the pointer, and dragging a pane border resizes panes. Satisfies: FR-063.

### Test-only interface

- [ ] T048 [US1] Implement `--snapshot-json` and `--snapshot-hold <SECS>` in `src/cli.rs` and `src/procmon/json.rs`:
  - run two refresh cycles, print the Snapshot as JSON (records limited to LLM group members) and exit;
  - `--snapshot-hold` then keeps all state in memory for `SECS` seconds;
  - `--snapshot-stream <SECS>` prints one JSON line per refresh cycle (timestamp, cycle number, the `ProcessKey`s of detected LLM processes) for `SECS` seconds, so latency tests can watch processes appear and disappear;
  - never start the TUI; respect `--no-dns`;
  - use a small hand-written JSON writer, with **no serde_json in the product** (the plan lists serde_json as dev-only).

  Satisfies: contracts/cli.md.
- [ ] T054 [US1] **CT-1: re-measure idle memory on a real developer machine** once T031–T041 work.
  - Setup: run `cargo xtask bench-memory --idle` on a developer machine (not the ASR-blocked one) with ≥ 3 agent CLIs running, a local model server with a model loaded, language servers and a browser open.
  - Platforms: each platform available, plus the macOS hardware from T103.
  - Validity: full duration, measured PID = Fallow, cycles ≥ 90%, **≥ 1 LLM group detected**.
  - Pass: peak RSS < 50 MB.
  - Record: results, process counts and LLM group counts in `specs/001-llm-aware-tui-editor/research.md` R-12, next to the CI lower bound.

  Satisfies: CT-1, FR-040, Constitution I.

### Phase 3a CI checks (performance claims, privileges, macOS canary)

- [ ] T117 [US1] Implement `cargo xtask bench-refresh` in `xtask/src/bench_refresh.rs`:
  - a process-spawning fixture raises the total process count to **about 600** (idle child processes, released afterwards);
  - run Fallow in the PTY harness with the default 2 s refresh;
  - after a 30 s warm-up, sample Fallow's CPU time for 120 s from the OS (Linux `/proc/<pid>/stat` utime+stime, macOS `proc_pid_rusage` user+system time, Windows `GetProcessTimes`);
  - fail if the average exceeds **2% of one core**; record the process count actually reached;
  - add the step to `.github/workflows/ci.yml` on all three OSes.

  Satisfies: plan Performance Goals (refresh CPU), Constitution VI.
- [ ] T118 [US1] Implement `cargo xtask bench-new-process` in `xtask/src/bench_new_process.rs`:
  - run `fallow --snapshot-stream 60`;
  - start a stand-in process matching a built-in signature at a random offset within a refresh interval, and record the time until its `ProcessKey` first appears;
  - stop it and record the time until it disappears;
  - repeat 10 times; fail if any appearance or disappearance takes longer than **two refresh intervals (4 s at the default)**;
  - add the step to `.github/workflows/ci.yml` on all three OSes.

  Satisfies: SC-004, Constitution VI.
- [ ] T119 [US1] Run Fallow **unprivileged** in CI, in `.github/workflows/ci.yml` and `xtask/src/unprivileged.rs`:
  - **Windows**: create a standard local user (`net user fallowci <random> /add`, not in Administrators), then run `fallow --snapshot-json` and the core detection checks (a signature process, a provider connection, a local model server and client) as that user with a non-elevated token, through a scheduled task or `CreateProcessWithLogonW`;
  - **Linux and macOS**: run the same checks as a non-root user (`sudo -u nobody` or a created user), with the fixtures owned by that user;
  - fail if Fallow requests elevation or exits non-zero, or if any **core field** for the user's own processes (name, PID, cwd, footprint, CPU, uptime, status, provider/model, confidence) is missing or `Unavailable` because of privileges. Windows cwd may be `Unavailable` only when `windows.read_working_directory = "never"`.

  Satisfies: FR-030, Constitution V.
- [ ] T120 [US1] Implement the **macOS canary** in `xtask/src/canary.rs` and add it to the `macos-15` job in `.github/workflows/ci.yml`:
  - start a monitored child with an environment variable holding a random 32-byte canary and a command line matching an LLM signature, so Fallow reads it through KERN_PROCARGS2;
  - run `fallow --snapshot-hold 20` and dump its memory with `sudo lldb -p <pid> --batch -o "process save-core <file>"`;
  - search the core file, Fallow's stdout and screen output, logs, the config dir and the cache dir for the canary in UTF-8 and UTF-16LE;
  - any hit fails. This gates T032 (Constitution VII: the canary test runs on the platform where the command-line read returns environment data).

  Satisfies: SC-007, FR-024, Constitution VII.

**Checkpoint (Phase 3a, MVP)**: `fallow` with no arguments shows every LLM process with the
correct fields, groups and statuses, the summary, help and palette. This is enough to start
Phase 4 and later phases.

---

## Phase 3b: User Story 1 - Release verification (required before the first public release)

**Goal**: The automated proofs for the detection quality and privacy claims. **Required before
the first public release, but not blocking Phase 4 onward.**

**Independent Test**: In CI, `detect-fixtures`, `canary` and `egress` pass on all three OSes,
and the Defender job reports PASS with protection verified.

- [ ] T049 [US1] Implement `cargo xtask detect-fixtures` in `xtask/src/detect_fixtures.rs` with fixtures in `tests/fixtures/detect/`:
  - **real tools first**: install in CI and run, where possible, **at minimum Claude Code (`npm i -g @anthropic-ai/claude-code`), aider (`pip install aider-chat`) and Ollama** (official install script on Linux/macOS, installer on Windows). Record each one's real process name and command line into `tests/fixtures/detect/recorded/<tool>.json`, and use those recordings for tools that can't run in CI;
  - **stand-ins only for tools that can't be installed or recorded**, each marked `"stand_in": true` in `tests/fixtures/detect/manifest.toml` and reported separately in the results;
  - the LLM reference set has ≥ 20 tools in total;
  - **provider connections without live network**: a test-only `--config-dir` whose `providers.toml` defines a test provider with range `127.0.0.2/32`, plus a local TLS server on `127.0.0.2:443` started with `sudo` (Linux/macOS) or as Administrator (Windows). Interpreter scripts connect to it. One optional live check against real provider hosts runs but doesn't fail the job;
  - a stub local model server on 11434 with a client;
  - a non-LLM set of ≥ 20 processes: shells, build tools, language servers, and a browser with the browser category off;
  - run `fallow --snapshot-json` and assert ≥ 95% detected, 100% of the signature-covered tools `certain`, and ≤ 1 false positive. Report real and stand-in results separately.

  Satisfies: SC-002, SC-003.
- [ ] T050 [US1] Extend the **SC-007 canary test** in `xtask/src/canary.rs` (macOS is done in T120, Phase 3a) to Linux and Windows:
  - start a monitored child with an environment variable holding a random 32-byte canary and an LLM signature name, so Fallow reads its command line;
  - run `fallow --snapshot-hold 20`;
  - dump Fallow's memory: Linux via `/proc/<pid>/mem` with `sudo`; macOS via `sudo lldb -p <pid> --batch -o "process save-core <file>"`; Windows via `MiniDumpWriteDump` with `MiniDumpWithFullMemory`;
  - search the dump, Fallow's stdout and screen output, logs, the config dir and the cache dir for the canary in UTF-8 and UTF-16LE.

  Any hit fails. After this task the canary runs on all three platforms. Satisfies: SC-007, FR-024, Constitution VII.
- [ ] T051 [US1] Implement the **SC-009 egress test** in `xtask/src/egress.rs`:
  - Linux: `strace -f -e trace=network` on Fallow plus `tcpdump` on port 53;
  - macOS: `sudo tcpdump -i pktap,all -k` filtered to Fallow's PID;
  - Windows: poll `GetExtendedTcpTable` and UDP owner tables for Fallow's PID every 100 ms, and read `Microsoft-Windows-DNS-Client/Operational` events for its PID.

  It asserts that the only traffic is DNS queries for names in `providers.toml` (no reverse PTR lookups), and that a second run with `--no-dns` produces **zero** outbound traffic and no DNS queries. Satisfies: SC-009, FR-035, FR-022a, Constitution VII.
- [ ] T052 [US1] Implement the **Windows Defender check** in `xtask/src/defender_check.rs` per research.md R-10:
  1. Enable: `Set-MpPreference -DisableRealtimeMonitoring $false -DisableBehaviorMonitoring $false -DisableIOAVProtection $false -DisableScriptScanning $false`, then `Remove-MpPreference -ExclusionPath C:\, D:\`, then try `Update-MpSignature` (on failure, warn and continue).
  2. Verify with `Get-MpComputerStatus`: `AMRunningMode = "Normal"`, and `RealTimeProtectionEnabled`, `BehaviorMonitorEnabled` and `IoavProtectionEnabled` all `True`. `Get-MpPreference` must show no exclusion covering the binary or workspace. **Any failure fails the check**, and a run with protection off never passes.
  3. Exercise: run the T049 fixtures with `fallow --snapshot-json` under `on_detect`, and once under `on_select` with a simulated selection.
  4. Judge: fail on any `Get-MpThreatDetection` entry or any Defender operational event 1006, 1007, 1015, 1116 or 1117 newer than step 2.

  The check writes `defender-result.json` (`{"protection_verified": bool, "detections": n, "result": "PASS" | "FAIL"}`) and appends it to `GITHUB_STEP_SUMMARY`, so T116 can count consecutive passes. Satisfies: R-10, Constitution V.
- [ ] T053 [US1] Extend `.github/workflows/ci.yml`:
  - `cargo xtask detect-fixtures` on all three OSes and `cargo xtask canary` on Linux and Windows (macOS has run it since T120), blocking;
  - `cargo xtask egress` on all three, blocking;
  - `cargo xtask defender-check` as a **separate job** `defender` on `windows-2025` with `continue-on-error: true`. It is **non-blocking at first**: a failure is reported in the run summary and as a warning annotation but doesn't fail the workflow. It uploads `defender-result.json` as an artifact.

  Satisfies: SC-002, SC-003, SC-007, SC-009, R-10, FR-053.
- [ ] T116 [US1] Make the Defender job blocking once it has **passed on 5 consecutive runs** on `main`:
  - check consecutive passes with `gh run list --workflow ci.yml --branch main --json conclusion,databaseId` plus each run's `defender-result.json` artifact. A pass requires `protection_verified = true` and `result = "PASS"`; a run with protection off never counts;
  - then remove `continue-on-error` from the `defender` job in `.github/workflows/ci.yml` and add it to the required status checks of the `main` branch protection;
  - record the five run IDs in `specs/001-llm-aware-tui-editor/research.md` R-10.

  Satisfies: R-10, Constitution V.

- [ ] T122 [US1] Measure and publish the memory cost of the **opt-in features**, in `xtask/src/bench_memory.rs`, `.github/workflows/ci.yml`, `docs/memory-costs.md` and `README.md`:
  - `bench-memory --idle --all-users`: elevated (`sudo` on Linux/macOS, the elevated runner on Windows), with all users' processes shown;
  - `bench-memory --idle --browsers`: browser category enabled, with a headless Chromium running;
  - run both on all three OSes as separate, reporting CI steps. They don't count against the default 50 MB budget, but each profile's peak is published;
  - write the latest figures per OS to `docs/memory-costs.md` with run IDs, and link them from the README.

  Not blocking Phase 3a. Satisfies: Constitution I ("Opt-in features MUST document their memory cost"), Performance Budgets ("benchmarked separately and their cost published"), FR-026, FR-031.

**Checkpoint (Phase 3b)**: The canary, egress and detection fixtures block CI on all three OSes.
The Defender job reports on every run and becomes blocking after 5 consecutive passes (T116).

---

## Phase 4: User Story 2 - Find the process that is slowing the machine (Priority: P2)

**Goal**: Sorting by group totals, header-click sorting, and a low-memory warning.

**Independent Test**: Quickstart V5. With several LLM groups running, one of them large, press `m`:
the largest group comes first. Click the Status header to sort by status; click again to reverse.
Fill RAM until less than 10% is available: "RAM remaining" is highlighted.

- [ ] T055 [P] [US2] Write `tests/sorting.rs`: groups sort by **group totals** for memory and CPU, highest first; uptime, name, confidence and status order are stable; pressing the same key or clicking the same header again reverses. Satisfies: FR-014, US2 AS1–AS2.
- [ ] T056 [US2] Implement the sort state and comparators in `src/ui/processes.rs`: keys `m` / `c` / `u` / `n` (repeat to reverse), and **no** single key for confidence; palette commands "Sort by memory / CPU / uptime / name / status / confidence" registered in `src/app/commands.rs`; and a `▾` / `▴` indicator in the active header. Satisfies: FR-014, contracts/ui-commands.md (Process panel).
- [ ] T057 [US2] Implement column-header click sorting in `src/ui/processes.rs`: clicking any header, including status and confidence, sorts by it, and clicking again reverses. Satisfies: FR-063, FR-014.
- [ ] T058 [US2] Highlight "RAM remaining" in `src/ui/summary.rs` when `low_memory` is true. Satisfies: FR-016, US2 AS3.

**Checkpoint**: The P1 panel plus sorting. A tester can find the heaviest group within 10 s
(SC-001, validated in T109).

---

## Phase 5: User Story 3 - Edit code in a project (Priority: P3)

**Goal**: The Files pane with file operations, editing with highlighting, selection and
clipboard, find with a scope toggle, go to line, and disk-change safety. Includes the loaded
memory benchmark.

**Independent Test**: Quickstart V10, V11, V15 and V16 without using the process panel:
- open a project, edit, save;
- `Ctrl-F`, then `Tab` to "whole project", `Enter`, and open a result;
- `Ctrl-G 10`;
- select, copy and paste;
- create, rename and delete a file with confirmation;
- an agent edits an open file and nothing is lost.

### Tests for User Story 3

- [ ] T059 [P] [US3] Write `tests/buffer.rs`:
  - edits and undo/redo;
  - "undo is an edit log capped at 10 MB per buffer; the oldest entries are dropped first";
  - files over `editor.read_only_threshold_mb` (10) open `read_only`;
  - binary files are not opened as text.

  Satisfies: FR-003, spec edge case "large or binary file".
- [ ] T060 [P] [US3] Write `tests/disk_change.rs`, one case for each FR-006 transition, verbatim:
  - "`dirty = false` and the file changed: reload, show a notice, stay `InSync`";
  - "`dirty = true` and the file changed: go to `ChangedOnDisk` and prompt 'keep mine / load disk'. Nothing is written until the user chooses";
  - "The file was deleted or renamed: go to `DeletedOnDisk`, show a notice, keep the buffer, and set `dirty = true`".

  Satisfies: FR-006.
- [ ] T061 [P] [US3] Write `tests/search.rs`: `.gitignore` respected, results capped at 10,000 with state `Capped`, cancellation, file scope next/previous, project scope runs only on submit. Satisfies: FR-005.
- [ ] T062 [P] [US3] Write `tests/fileops.rs`: new file and new folder inside the selected folder or next to the selected file; rename updates the open buffer's path; delete requires a confirmation token; delete of an open file makes the buffer unsaved. Satisfies: FR-068, FR-034.
- [ ] T063 [P] [US3] Write `tests/clipboard.rs`: OSC 52 encoding (base64, `ESC ] 52 ; c ; … BEL`); copy with no selection is a no-op that returns the "Quit: Ctrl-Q" hint; a bracketed paste is inserted literally with no auto-indent; cut removes the selection. Satisfies: FR-008.
- [ ] T064 [P] [US3] Write `tests/keymap_conflicts.rs`:
  - every default key in contracts/ui-commands.md is bound to exactly the listed command in its context;
  - no key is bound twice in one context;
  - every key listed in the Known terminal conflicts table maps to a registered command;
  - every command has a palette entry.

  Satisfies: FR-064, FR-062.

### Implementation for User Story 3

- [ ] T065 [US3] Add the editor dependencies to `Cargo.toml`: `ropey`, `tree-sitter`, `tree-sitter-highlight`, the 15 grammar crates (Python, JavaScript, TypeScript/TSX, Rust, Go, C, C++, Java, C#, Bash, JSON, YAML, TOML, Markdown, HTML, CSS), `notify`, `grep-searcher`, `grep-regex`, `ignore`, `arboard`. Check them with `cargo deny`. Satisfies: plan Dependency budget, FR-004.
- [ ] T066 [US3] Implement `Buffer` in `src/editor/buffer.rs` per data-model.md:
  - `ropey::Rope` text;
  - `read_only` for files over 10 MB, with a warning;
  - binary detection (a NUL byte in the first 8 KiB);
  - `language` from the file extension;
  - the `EditLog` undo/redo "Capped at 10 MB";
  - `dirty` and `disk_state`.

  Satisfies: FR-003, spec edge cases.
- [ ] T067 [US3] Implement the lazy tree in `src/editor/tree.rs` (a node lists its children only when expanded) and the Files pane in `src/ui/files.rs`:
  - `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End`;
  - `Enter` or double-click opens a file or toggles a folder;
  - `←` collapses or goes to the parent; `→` expands.

  Satisfies: FR-002, contracts/ui-commands.md Files pane.
- [ ] T068 [US3] Implement opening a project: the `PATH` argument and `Ctrl-O` (a prompt with path input). Set `Project { root, tree, open_buffers }` in `src/app/state.rs`. Satisfies: FR-002.
- [ ] T069 [US3] Implement the shared confirmation dialog in `src/ui/confirm.rs`: default focus **Cancel**; `y`, or `Enter` on Confirm, confirms; `Esc`, `n` and `q` cancel; there is no "don't ask again". Confirming returns a `ConfirmedToken` that only this module can construct. Satisfies: FR-032, FR-068.
- [ ] T070 [US3] Implement Files pane operations in `src/editor/fileops.rs` and `src/ui/files.rs`:
  - `a` new file, `Shift-A` new folder, `F2` or `r` rename, `Delete` or `d` delete, each also in the palette;
  - name prompts support selection, copy, cut and paste;
  - delete needs a T069 confirmation showing the full path, file and folder counts for folders, and "Deletion is permanent. v1 has no trash integration";
  - renaming or deleting an open file updates the buffer or makes it unsaved.

  Satisfies: FR-068, FR-034.
- [ ] T071 [US3] Implement syntax highlighting in `src/editor/syntax.rs`: a static grammar registry whose highlight queries compile on first use of each language; highlight the visible range plus a margin; keep the parse tree per open buffer. Unused grammars must cost no RSS. Satisfies: FR-004, R-8.
- [ ] T072 [US3] Implement the editor view and editing in `src/ui/editor.rs`:
  - cursor movement, typing and line numbers;
  - `Tab` / `Shift-Tab` indent and outdent (`editor.tab_width`);
  - `Esc` releases focus when no find bar or selection is active, after which `Tab` switches panes, and the hint bar shows `Esc Tab: next pane`.

  Satisfies: FR-003, FR-065.
- [ ] T073 [US3] Implement buffer commands in `src/app/keymap.rs` and `src/ui/editor.rs`: `Ctrl-S` save, `Ctrl-Z` / `Ctrl-Y` undo and redo, `Ctrl-W` close, `Ctrl-PgDn` / `Ctrl-PgUp` next and previous buffer, plus the palette entries "Next buffer" / "Previous buffer". Satisfies: FR-003.
- [ ] T074 [US3] Implement selection in `src/editor/selection.rs` and `src/ui/editor.rs`: `Shift-←/→/↑/↓`, `Shift-Home` / `Shift-End`, `Ctrl-A` (and palette "Select all"), and mouse drag. Selection also works in the find box and name prompts. Satisfies: FR-008, FR-063.
- [ ] T075 [US3] Implement the clipboard in `src/clipboard.rs`:
  - copy and cut **always** emit OSC 52 **and** write the platform clipboard through `arboard`, opened lazily on first use and never at startup;
  - a failure on one side is silent; if both fail, show `Copy failed: clipboard unavailable`;
  - `Ctrl-C` with no selection does nothing and shows `Quit: Ctrl-Q` in the hint bar for 3 s;
  - `Ctrl-X` cuts;
  - `Ctrl-V` as a key reads the platform clipboard, or suggests the terminal's own paste when that is unavailable;
  - a bracketed paste event is inserted literally, with no auto-indent and no key interpretation.

  Satisfies: FR-008.
- [ ] T076 [US3] Implement project search in `src/editor/search.rs` with `grep-searcher`, `grep-regex` and `ignore`:
  - runs on a search thread spawned per search;
  - respects `.gitignore` and streams results to the UI;
  - can be cancelled;
  - "holds at most 10,000 results in memory", then sets the state to `Capped`.

  Satisfies: FR-005, R-8, FR-042.
- [ ] T077 [US3] Implement the find bar in `src/ui/find.rs`:
  - `Ctrl-F` opens it with scope "this file"; `Tab` inside the box toggles "this file" ⇄ "whole project", and the scope is always shown;
  - file scope: `Enter` / `↓` next, `↑` previous;
  - project scope: `Enter` runs the search (never while typing) and opens a results list where `Enter` or double-click opens a result at its line;
  - `Esc` cancels a running search or closes the bar.

  There is no `Ctrl-Shift-F` binding. Satisfies: FR-005, contracts/ui-commands.md Find bar.
- [ ] T078 [US3] Implement go to line in `src/ui/goto.rs`: `Ctrl-G` prompts for `line` or `line:column`; invalid input shows an inline error and moves nothing. Satisfies: FR-009.
- [ ] T079 [US3] Implement file watching in `src/editor/watch.rs` with `notify`: watch open files and expanded directories **non-recursively**, never a whole project tree; forward events over a channel; and apply the T060 transitions, including the "keep mine / load disk" prompt. Satisfies: FR-006, FR-042.
- [ ] T080 [US3] Implement the unsaved-changes warning on `Ctrl-Q` in `src/app/event_loop.rs`, listing the dirty buffers. Satisfies: FR-007.
- [ ] T081 [US3] Add the editor's mouse behavior in `src/ui/editor.rs` and `src/ui/files.rs`: click sets the cursor or selects a file, double-click opens, drag selects text, the wheel scrolls. Satisfies: FR-063.

### Loaded benchmark

- [ ] T082 [US3] Implement `cargo xtask gen-fixture` in `xtask/src/gen_fixture.rs`: a synthetic project of about 50,000 files and about 1 GB across the 15 highlighted languages, generated from fixed seeds and cached in CI by a hash of the generator. Satisfies: R-13 (resolves constitution TODO(LARGE_PROJECT_FIXTURE)).
- [ ] T083 [US3] Implement `cargo xtask bench-memory --loaded` in `xtask/src/bench_memory.rs`: open the fixture, run one project search, open five files of different languages through scripted keys, perform one copy (to exercise `arboard`), then sample for 60 s. Fail at **≥ 150 MB** or if the run is INVALID. Satisfies: FR-040, SC-005.
- [ ] T084 [US3] Implement `cargo xtask bench-search` in `xtask/src/bench_search.rs`: time from `Enter` in project scope to the first result row on the fixture; fail at ≥ 1 s (p95 of 10 runs). Satisfies: SC-010.
- [ ] T085 [US3] Add `gen-fixture` (cached), `bench-memory --loaded` and `bench-search` to `.github/workflows/ci.yml` on all three OSes. Satisfies: FR-053, Constitution VI.

**Checkpoint**: Editing works on its own. The loaded budget and search time are enforced in CI.

---

## Phase 6: User Story 4 - Discover an unknown LLM process and see why it was flagged (Priority: P4)

**Goal**: A details view of the matched signals, configuration reload, and the browser
category toggle.

**Independent Test**: Quickstart V6 and V13. Select a script detected only through its
connection and press `Enter`: the details show "provider connection: <provider> via resolved |
range, likely". Add a user signature and run "Reload configuration": the process becomes
`certain`. A syntax error in the user file shows `file:line` while built-in detection keeps
working.

- [ ] T086 [P] [US4] Extend `tests/detect.rs` with: a user signature overriding a built-in one by `id`; `enabled = false` disabling a built-in; a new user signature producing `Certain`; and each signal reported with its `via` method. Satisfies: FR-021, FR-025, US4 AS1–AS3.
- [ ] T087 [US4] Implement the details view in `src/ui/details.rs`, opened with `Enter` on a process: every signal with its confidence (signature id; provider plus "via resolved" or "via range"; local model server and model), and the full command line, wrapped. Satisfies: FR-025, US4 AS1–AS2.
- [ ] T088 [US4] Implement the "Reload configuration" command in `src/app/commands.rs` and `src/config/mod.rs`: re-merge all three files, recompile signatures, hand the new detection config to the procmon worker over its channel, and show `file:line: message` for any error while the previous valid config stays active. Satisfies: FR-021, R-9, US4 AS3.
- [ ] T089 [US4] Implement the palette command "Toggle browser category" in `src/app/commands.rs` and `src/procmon/detect.rs`. Browsers appear with type `browser` when enabled. The toggle lasts for the session, and `detection.browsers` sets the default. Satisfies: FR-026.

**Checkpoint**: Detection is explainable and configurable without restarting.

---

## Phase 7: User Story 5 - Jump from a process to its work (Priority: P5)

**Goal**: `g` opens the selected process's working directory with recent changes.

**Independent Test**: Quickstart V7. Run an agent in `~/a` and let it change two files, then select it and press `g`:
- the tree opens `~/a`;
- both files are listed newest first, with modification times and the disclaimer;
- an unreadable directory shows a message and the view stays where it was.

- [ ] T090 [P] [US5] Write `tests/recent.rs`: files within `recent_window_minutes` are listed newest first; the caps of "50,000 entries visited and 200 results" set `capped`; `.gitignore` is respected; the disclaimer text is present. Satisfies: FR-017.
- [ ] T091 [US5] Implement `RecentChanges` in `src/procmon/recent.rs`: an on-demand `ignore::Walk` of the working directory on a short-lived job thread, keeping mtimes within the window (30 min, configurable), "capped at 50,000 entries visited and 200 results", sorted descending, with the disclaimer "changes in this directory, not proven to come from this process". Satisfies: FR-017, FR-042.
- [ ] T092 [US5] Implement the `g` command and the recent-changes view in `src/ui/recent.rs` and `src/app/commands.rs`:
  - the Files pane switches its root to the process's cwd, and the list shows paths with modification times and the disclaimer, stating when a cap was hit;
  - an unreadable cwd shows a message and nothing changes;
  - while the view is shown, the cwd is watched non-recursively.

  Satisfies: FR-017, US5 AS1–AS3.
- [ ] T093 [US5] Implement the Windows cwd policy on selection in `src/procmon/worker.rs` and `src/ui/recent.rs`:
  - with `on_select`, the PEB read happens when a process is selected;
  - with `Unavailable`, `g` offers the executable's directory and a typed path instead, explaining why.

  Satisfies: R-10, FR-017.

**Checkpoint**: Monitoring and editing connect: from an agent to the files it touched.

---

## Phase 8: User Story 6 - Stop or pause a runaway process, with confirmation (Priority: P6)

**Goal**: Kill (graceful, with an optional confirmed force kill), pause and resume, only through confirmed, revalidated `ControlRequest`s.

**Independent Test**: Quickstart V8, V8b and V9.
- Select a dummy LLM process and press `Shift-K`, then `Esc`: it is still running.
- Press `Shift-K` and `y`: on Linux and macOS it receives SIGTERM and exits.
- A dummy that ignores SIGTERM is still alive after 5 s, so "Force kill" is offered. Cancelling leaves it running; confirming sends SIGKILL.
- On Windows, the Kill dialog says it is immediate with no cleanup, and `p` shows the Windows difference text.

- [ ] T094 [P] [US6] Write the **SC-008 confirmation test** in `tests/control_requires_confirmation.rs`:
  - (a) at compile level, `platform::{suspend, resume, kill}` are `pub(in crate::procmon)` and only `ControlRequest::execute` calls them; a `trybuild` compile-fail case shows they can't be called from `ui` or `app`;
  - (b) `execute` requires a `ConfirmedToken` (T069), which only the confirmation dialog can construct;
  - (c) cancel, `Esc`, `n` and `q` produce `Discarded`, with no platform call recorded by a mock platform;
  - (d) revalidation: if any target's `(pid, start_time)` changed, the result is `Aborted("process is gone")` and nothing is signalled;
  - (e) "No path reaches `Executing` without passing through `AwaitingConfirmation`";
  - (f) graceful kill on Unix: confirming Kill sends only `SIGTERM` to the mock platform. If the target exits within 5 s, the result is `Done(Ok)` and no force kill is offered;
  - (g) a target still alive after 5 s produces `AwaitingForceConfirmation`. **No `SIGKILL` is sent until a second, separate `ConfirmedToken` for `ForceKill` is presented**; the first Kill token can't be reused for it (compile-fail or runtime rejection);
  - (h) cancelling the force-kill confirmation sends nothing further and leaves `Done(StillRunning)`;
  - (i) a target that exits, or whose `(pid, start_time)` changes, during the 5 s wait or while the force dialog is open produces `Gone`, and no `SIGKILL` is sent;
  - (j) on Windows, Kill maps to a single `TerminateProcess` after one confirmation, with no force step.

  Satisfies: SC-008, FR-032, FR-033, Constitution IV.
- [ ] T095 [P] [US6] Implement Unix control in `src/procmon/platform/linux.rs` and `src/procmon/platform/macos.rs`:
  - `suspend` = `SIGSTOP`, `resume` = `SIGCONT`;
  - `terminate` = `SIGTERM` (the graceful Kill);
  - `force_kill` = `SIGKILL`, called only after the second confirmation (T097);
  - `is_alive(ProcessKey)` checks that PID **and** start time still match;
  - map `EPERM` to `PermissionDenied` and `ESRCH` to `Gone`.

  Satisfies: FR-032, FR-033, FR-052.
- [ ] T096 [P] [US6] Implement Windows control in `src/procmon/platform/windows.rs`:
  - `suspend` / `resume` with `NtSuspendProcess` / `NtResumeProcess` from `ntdll` on a `PROCESS_SUSPEND_RESUME` handle;
  - Fallow counts its own suspends per `ProcessKey` and resumes the same number of times;
  - `terminate` with `TerminateProcess` on a `PROCESS_TERMINATE` handle. This is immediate: the process runs no cleanup, so Windows has **no** separate force-kill step;
  - access denied maps to `PermissionDenied`.

  Satisfies: R-7, FR-052.
- [ ] T097 [US6] Implement the `ControlRequest` lifecycle in `src/procmon/control.rs` per data-model.md: `Requested → AwaitingConfirmation → (Discarded | Revalidating → (Aborted | Executing → Done(per-target Ok | PermissionDenied | Gone | Error)))`. Acting on a group includes the members in `targets` and sets `include_children`.

  Kill on Unix follows the extended lifecycle:
  - after `Executing` (SIGTERM), go to `WaitingForExit`, polling `is_alive` every 250 ms for up to **5 s**, off the UI thread;
  - targets that exit are `Ok`;
  - if any target is still alive, go to `AwaitingForceConfirmation`, which opens a **second** confirmation for action `ForceKill` listing only the remaining targets;
  - confirm → revalidate → `SIGKILL` → `Done`; cancel → `Done(StillRunning)`.

  `ForceKill` needs its own `ConfirmedToken` (data-model.md, ControlRequest, "Kill on Linux/macOS"). Satisfies: FR-032, FR-033.
- [ ] T098 [US6] Extend the confirmation dialog in `src/ui/confirm.rs` for process actions. It shows the action, process name, PID, owner (in all-users mode), group size and whether children are included. Action-specific text:
  - **Kill on Linux/macOS**: "Sends SIGTERM so the process can clean up. If it is still running after 5 s, you will be asked whether to force kill."
  - **Force kill** (second dialog): lists the processes still running and says "Sends SIGKILL. The process stops immediately and cannot clean up."
  - **Kill on Windows**: "Terminates immediately. The process cannot clean up or save state."
  - **Pause/resume on Windows**: the R-7 difference text: no notice to the process, stacking suspend counts, racy for new threads, windowed apps show "Not responding", children are not suspended unless the group is selected.

  A progress line "Waiting for exit… (n s)" is shown during the 5 s wait. Satisfies: FR-032, FR-052.
- [ ] T099 [US6] Wire `p` / `r` / `Shift-K` in `src/app/keymap.rs` and `src/ui/processes.rs`. "Force kill" has no key of its own: it is reachable **only** through the second dialog after a graceful Kill, so it can never be triggered directly. Show per-target results. `PermissionDenied` shows "permission denied" and **never** prompts for elevation. Satisfies: US6 AS1–AS6, FR-031.
- [ ] T100 [P] [US6] Write the user documentation for pause and resume differences in `docs/platform-differences.md` (Windows vs Linux and macOS, from research.md R-7) and link it from the help overlay in `src/ui/help.rs`. Satisfies: FR-052.

**Checkpoint**: All six stories work. Control is provably confirmation-gated, and force kill needs a second confirmation.

---

## Phase 9: Polish, Verification & Release

**Purpose**: Platform verification, usability validation, release packaging and signing.

### macOS release blocker (plan.md, Release blockers)

- [ ] T101 Build the S-2/S-4 probe in `spikes/macos-footprint/` (its own workspace, not shipped):
  - (a) print `ri_phys_footprint` for a PID every 5 s, with `top -l 1 -stats pid,mem -pid <pid>` alongside;
  - (b) the S-4 experiment: read KERN_PROCARGS2 for a test process started with a known environment, once with a full buffer and once with a buffer truncated at the end of argv, and report whether the truncated read contains any envp bytes.

  Satisfies: R-11, R-2 (S-4).
- [ ] T102 **When the macOS platform module is complete (T032, T095 and T101 done)**, generate `specs/001-llm-aware-tui-editor/checklists/macos-hardware.md`, a step-by-step checklist with exact commands for a physical Apple Silicon Mac (M1 or later; not a GitHub runner, not an Intel Mac):
  1. **S-2 / SC-012**: install Ollama; `ollama run llama3.2:3b` on an 8 GB Mac, `ollama run llama3.1:8b` otherwise; keep it busy with a repeated prompt; record Fallow's figure vs Activity Monitor and `top` every 5 s for 2 min, **per PID and per group**; pass if every pair is within 10%.
  2. **S-4**: run the T101 probe; record whether truncated reads exclude envp.
  3. **SC-007**: `cargo xtask canary` on the Mac.
  4. **Manual run-through in Terminal.app and iTerm2**: mouse selection, scrolling and pane resizing; hint bar; help overlay; command palette; colors; small-window collapse; and every **verify** row of the Known terminal conflicts table in contracts/ui-commands.md.

  Satisfies: plan Release blockers, CT-2, SC-012, SC-007, FR-064.
- [ ] T103 Run the T102 checklist on the borrowed Mac and record the results: item 1 in research.md R-11, item 2 in R-2, items 3 and 4 in a new research.md entry "macOS hardware verification". If S-4 shows truncated reads exclude envp, switch `src/procmon/platform/macos.rs` to truncated reads sized from argc and argv lengths, keeping the wipe for the full-read fallback (R-2). Update intercepted rows in contracts/ui-commands.md, with alternatives. Satisfies: release blocker, R-2, R-11, FR-064.
- [ ] T104 Remove the macOS "beta" label in `README.md` and in the release notes **only if** all four T102 items passed and are recorded (T103). Satisfies: plan Release blockers, CT-4.

### Other platform verification

- [ ] T105 [P] Verify SC-012 parity on Linux (`smem -P ollama`, PSS) and Windows (Task Manager "Memory (active private working set)") with a running local model, per PID and per group, and record the results in research.md R-3. Satisfies: SC-012, quickstart V4.
- [ ] T106 [P] Verify the Windows Terminal rows of the Known terminal conflicts table by hand on a Windows machine without the ASR block, using a release build: `Ctrl-C` with and without a Windows Terminal selection, `Ctrl-V` arriving as a bracketed paste, and every other default key. Record the results in contracts/ui-commands.md. Satisfies: FR-064.
- [ ] T107 Run quickstart.md V1–V16 on Linux, macOS and Windows and record pass or fail per scenario in `specs/001-llm-aware-tui-editor/checklists/quickstart-run.md`. Satisfies: all user stories.

### Usability validation

- [ ] T108 [P] Write the usability test protocol and results template in `specs/001-llm-aware-tui-editor/checklists/usability.md`:
  - SC-001: a developer unfamiliar with the setup finds the heaviest group within 10 s of launch;
  - SC-011: a new user without docs opens a file and sorts by memory within 2 minutes;
  - at least 5 participants.

  Satisfies: SC-001, SC-011.
- [ ] T109 Run the usability sessions and record the results in `checklists/usability.md`. Fix any blocker found, such as unclear hints, in `src/ui/hints.rs` and `src/ui/help.rs`. Satisfies: SC-001, SC-011.

### Release packaging and signing

- [ ] T110 Create `.github/workflows/release.yml`: build one self-contained binary per target (x86_64/aarch64 Linux with musl static, x86_64/aarch64 macOS, x86_64 Windows), run the full CI gates first, and attach SHA-256 sums. Satisfies: plan Language/packaging.
- [ ] T111 **Code signing for Windows releases** (R-12 ASR finding, CT-3). Options in order of preference:
  1. **SignPath Foundation**: free code signing for open-source projects. Apply with the public repository, then integrate the SignPath GitHub Action into `.github/workflows/release.yml` so that only artifacts built by that workflow are signed.
  2. **Azure Trusted Signing**: **verify eligibility first.** Availability for individual developers outside the US and Canada must be confirmed before choosing it. Record the finding in `docs/release.md`.
  3. An OV certificate from a commercial CA, stored in a GitHub environment secret.

  In every case:
  - verify each signed `.exe` with `signtool verify /pa /v` in the workflow;
  - document in `docs/release.md` the chosen option, and that unsigned, low-prevalence binaries are blocked by Defender ASR rule `01443614-CD74-433A-B99E-2ECDC07BFC25` on managed machines.

  Satisfies: CT-3, R-12.
- [ ] T112 [P] macOS signing and notarization in `.github/workflows/release.yml` (Developer ID Application certificate, `codesign --options runtime`, `xcrun notarytool submit --wait`, `stapler staple`), documented in `docs/release.md`.
  - **Notarization requires a paid Apple Developer Program membership (about $99/year).**
  - **Until that membership exists**, macOS releases ship **unsigned and labeled beta**. `README.md` and `docs/release.md` then give install instructions: download, verify the SHA-256 sum, remove the quarantine attribute with `xattr -d com.apple.quarantine ./fallow`, or allow it via System Settings › Privacy & Security after the first launch attempt.

  Satisfies: CT-3, plan Release blockers.

### Documentation and governance

- [ ] T113 [P] Update `README.md`: install, usage, key bindings summary, configuration paths, privacy statement (FR-035 / DNS exception), the platform support table, and the macOS beta status. Satisfies: CT-4.
- [ ] T114 Amend `.specify/memory/constitution.md` (PATCH; the startup statistic was already recorded in v3.0.1) to resolve TODO(REFERENCE_HARDWARE) with the GitHub-hosted runners `ubuntu-24.04`, `macos-15`, `windows-2025`, and TODO(LARGE_PROJECT_FIXTURE) with the `xtask gen-fixture` definition (T082), with an updated Sync Impact Report. Satisfies: R-13, Governance.
- [ ] T115 Before tagging a release, re-run CT-1 (T054) with the full product: editor open on a real project and several LLM processes running. Record the result in research.md R-12. Satisfies: CT-1, FR-040.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: none.
- **Foundational (Phase 2)**: depends on Setup and blocks every story.
- **US1 Phase 3a**: depends on Foundational. It is the MVP.
- **US1 Phase 3b** (release verification, T049–T053, T116, T122): depends on Phase 3a (T048 and the
  detection pipeline). It **does not block Phase 4 or later phases**, which may start as soon as
  3a is done. It **must be complete before the first public release** (T110).
- **US2 (Phase 4)**: depends on Phase 3a (it sorts the US1 panel).
- **US3 (Phase 5)**: depends only on Foundational. It can run in parallel with US1 and US2.
  - The shared confirmation dialog (T069) comes from US3.
- **US4 (Phase 6)**: depends on Phase 3a.
- **US5 (Phase 7)**: depends on Phase 3a (process selection) and US3 (Files pane and tree).
- **US6 (Phase 8)**: depends on Phase 3a and on T069 (confirmation dialog, US3).
- **Polish (Phase 9)**: depends on the stories each task verifies.
  - T110 (first public release) also requires all of Phase 3b. The Defender job may still be
    non-blocking at that point if T116's 5-pass condition hasn't been met, but its latest result
    must be PASS with protection verified.
  - T102 waits for the macOS module (T032, T095) and T101.
  - T104 waits for T103.

### Within each story

- Tests marked [P] first. They must fail before the implementation.
- Platform implementations (T031–T033, T095–T096) are [P] across OSes.
- Model, then worker, detection, grouping and status, then UI, then CI wiring.

### Key task-level dependencies

- T020/T021 → T023/T024 → T025 (benchmarks before CI)
- T018/T019 → T031–T033 → T034 → T035–T038 → T041–T042
- T043 → T044–T046 (registry before keymap, help and palette)
- T048 → T049–T052 → T053 → T116 (the snapshot interface before the CI tests that use it; the
  Defender job becomes blocking only after 5 consecutive passes)
- T048 → T117, T118, T119, T120 (Phase 3a CI checks need the snapshot interface)
- T120 → T032 done (the macOS command-line reader isn't done until the macOS canary passes)
- T034 → the bench-memory liveness default switches from `ticks` to `cycles`
- T053 → T122 (opt-in profiles reuse the Phase 3b CI wiring)
- T031–T041 → T054 (CT-1 needs working detection)
- T069 → T070, T094, T097–T099
- T082 → T083–T085

---

## Parallel Example: User Story 1

```text
# Tests together:
T027 tests/status_rules.rs   T028 tests/grouping.rs   T029 tests/detect.rs   T030 tests/platform_*.rs
T121 tests/platform_fallbacks.rs

# Platform implementations together (one per OS):
T031 src/procmon/platform/linux.rs   T032 src/procmon/platform/macos.rs   T033 src/procmon/platform/windows.rs

# UI pieces after T043:
T045 src/ui/help.rs   T046 src/ui/palette.rs
```

## Parallel Example: User Story 3

```text
# Tests together:
T059 buffer  T060 disk_change  T061 search  T062 fileops  T063 clipboard  T064 keymap_conflicts
```

## Parallel Example: User Story 6

```text
T094 tests/control_requires_confirmation.rs   T095 platform unix control   T096 platform windows control   T100 docs/platform-differences.md
```

---

## Implementation Strategy

### MVP first (User Story 1)

1. Phase 1 Setup, then Phase 2 Foundational. CI is green with the startup and idle gates on
   three OSes.
2. **Phase 3a** (T027–T048, T054, T117–T121): the working panel, with the refresh-CPU,
   new-process-latency, unprivileged and macOS canary checks. **Stop and validate** with quickstart
   V1–V3, and run CT-1 (T054) on a real developer machine. This is the MVP, usable locally and
   from CI builds.
3. **Phase 3b** (T049–T053, T116, T122) runs alongside the next phases. The detection fixtures,
   canary and egress tests become blocking CI gates as each lands. The Defender job starts
   non-blocking and becomes blocking after 5 consecutive passes. All of 3b must be done
   before the first public release.

### Incremental delivery

1. US1 Phase 3a, the MVP: a monitoring panel. Then Phase 3b in parallel with the stories
   below; it gates the first public release, not the next phase.
2. US2: sorting, to find the culprit.
3. US3: the editor, with the loaded budget enforced.
4. US4: explainable detection.
5. US5: jump to the work.
6. US6: control.
7. Phase 9: verification and release. macOS stays beta until T103/T104.

### Parallel team strategy

After Foundational:
- one developer on US1 3a → US2 → US4;
- one on Phase 3b (CI verification) once 3a's T048 exists;
- one on US3 (independent of US1);
- the platform tasks split by OS.

US5 and US6 start once US1 and US3 (T069) are in.

---

## Notes

- Every task cites FR / SC / R / CT identifiers from spec.md, research.md and plan.md.
- Manual tasks (T054, T103, T105, T106, T107, T109, T115) must run on a machine that can run
  locally built binaries (not the ASR-blocked development machine).
- Commit after each task or logical group, and stop at each checkpoint to validate the story on
  its own.
