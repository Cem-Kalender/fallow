---

description: "Task list for 002 Agent Sessions and Multiple Projects"
---

# Tasks: Agent Sessions and Multiple Projects

**Input**: Design documents from `specs/002-agent-sessions/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Included. The plan's verification table (research R-9) and the constitution's quality
gates (control paths, benchmarks for every performance claim) require them. Tests marked [P] are
written first and must fail before their implementation.

**Organization**: by user story, in the spec's priority order. US1 is the MVP. US2 and US3 build on
US1; US4 and US5 extend the foundation.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1 … US5)
- Paths follow the plan's Project Structure (single crate `src/`, `tests/`, dev-only `xtask/`).

---

## Phase 1: Setup

**Purpose**: dependencies and module skeletons.

- [X] T001 Add `vt100 = "=0.18.1"` and `portable-pty = "0.9"` to `[dependencies]` in `Cargo.toml`, and the `windows-sys` feature `Win32_System_JobObjects`. Run `cargo deny check` and record any licence or ban change in `deny.toml` with a reason (research R-1, R-7; plan Dependency budget).
- [X] T002 [P] Create the module skeletons with doc comments only: `src/projects/mod.rs`, `src/projects/store.rs`, `src/session/mod.rs`, `src/session/pty.rs`, `src/session/term.rs`, `src/session/scrollback.rs`, `src/session/attention.rs`, `src/session/keys.rs`, `src/session/profiles.rs`, `src/session/platform/{mod.rs,unix.rs,windows.rs}`. Register them in `src/lib.rs`.
- [X] T003 [P] Create the UI skeletons `src/ui/workspace.rs`, `src/ui/projects.rs`, `src/ui/sessions.rs`, `src/ui/terminal_pane.rs` and `src/app/workspace.rs`, `src/app/events.rs`, and register them in `src/ui/mod.rs` and `src/app/mod.rs`.

---

## Phase 2: Foundational (blocking)

**Purpose**: the event loop, settings and the in-memory models every story uses.

**⚠️ CRITICAL**: no user story work starts before this phase is done, and every 001 benchmark (startup, idle, loaded, refresh CPU, new-process latency, search) stays green in CI after T005.

- [X] T004 Write `tests/event_loop.rs`:
  - input, worker, search, watch and session events all arrive through one `AppEvent` channel;
  - a burst of queued key events is marked as a burst (the 001 paste fix);
  - output from a session wakes the loop within 20 ms.

  Satisfies: research R-5.
- [X] T005 Rework the event loop (research R-5):
  - `src/app/events.rs` defines `AppEvent`: `Input(crossterm::Event, burst: bool)`, `Snapshot`, `SessionOutput(SessionId)`, `SessionExited(SessionId, ExitStatus)`, `Wake`.
  - It also spawns an input thread that forwards crossterm events and computes `burst`.
  - `src/app/event_loop.rs` blocks on `recv_timeout(250 ms)`, drains the channel, and redraws at most once per 16 ms while output streams.
  - Move the procmon, search and watch draining onto the same channel.
  - Keep the `--bench-stats` counters (ticks every wake, at least every 250 ms).
- [X] T006 [P] Add the `[sessions]` settings to `src/config/schema.rs` with validation verbatim from contracts/config.md: `scrollback_lines` "1000..=100000" (default 10000), `attention_delay_ms` "1000..=600000" (default 5000), `max_sessions` "1..=16" (default 8), `leave_key` (default "ctrl-]"; one key such as "ctrl-]", "ctrl-\\", "ctrl-5", "f12"). Add tests to `tests/config_merge.rs`.
- [X] T007 [P] Write `tests/profiles.rs`:
  - the built-ins `claude`, `codex`, `gemini`, `aider`, `shell` and their commands (contracts/config.md table);
  - `profiles.toml` merge by `id` (a user entry with a built-in's id replaces it; `enabled = false` hides it);
  - validation `id` "`[a-z0-9_-]{1,32}`", `name` "1..=64 chars", `command` non-empty, `env` names "`[A-Za-z_][A-Za-z0-9_]*`", errors as `file:line`;
  - env values never appear in `Debug` output or the profile view.
- [X] T008 Implement `src/session/profiles.rs`:
  - `AgentProfile { id, name, command, args, env, kind, available }` (data-model.md), the built-in table, and `profiles.toml` parsing and merge reusing 001's config machinery (`src/config/merge.rs`).
  - Command resolution: `PATH`, plus `PATHEXT` on Windows. On Windows, prefer a real `.exe`; resolve an npm `.cmd` shim to its `node` + script target; never run through `cmd.exe` (research R-7).
  - `available` is computed lazily, when the start menu opens, never at startup.
- [X] T009 [P] Write `tests/projects_model.rs` for the in-memory `ProjectList`:
  - add rejects a path that is not a readable folder, with the reason;
  - no two entries with the same canonical path ("case-insensitively on Windows and macOS");
  - `name` "1..=64 chars";
  - remove never touches the folder;
  - move up and down;
  - `present = false` for a missing folder;
  - limit 200 projects.
- [X] T010 Implement `ProjectList` and `ProjectEntry` in `src/projects/mod.rs` per data-model.md: ids unique per run, absolute paths via `std::path::absolute`, `selected`, add/remove/rename/move/relocate. The lazy 001 `Project` tree is created on first use in the editor view. No persistence yet (US4).
- [X] T011 Extend `AppState` in `src/app/state.rs` with `projects: ProjectList`, `sessions: SessionManager` (empty), and `workspace: Workspace { view, agents_focus, layout, process_filter }` (data-model.md). The command-line `PATH` becomes the first entry of the list. Keep all 001 behavior in the editor view and keep the 001 tests passing.

**Checkpoint**: 001 still behaves the same and passes CI; sessions can now be built.

---

## Phase 3: User Story 1 - Start an agent session in a project and work with it inside Fallow (Priority: P1) 🎯 MVP

**Goal**: start an agent in a project in a terminal pane that behaves like a real terminal, with keys passed through, a leave key, scrollback, copy, paste, resize, exit and restart.

**Independent Test**: quickstart S3–S7. With one project, start a session, talk to the agent, scroll back, leave, come back; the session kept running and nothing was lost.

### Tests for User Story 1

- [X] T012 [P] [US1] Write `tests/session_keys.rs`:
  - every key in a table (printable characters, `Enter`, `Esc`, `Tab`, `Shift-Tab`, `Backspace`, arrows in normal and application cursor mode, `Home`/`End`/`PgUp`/`PgDn`, `F1`–`F12`, `Ctrl-A`…`Ctrl-Z`, `Alt-x`, `Ctrl-C`, `Ctrl-Q`, `Ctrl-P`) becomes the expected bytes for the agent;
  - only the leave key (`Ctrl-]` in all its encodings: 0x1D, `Ctrl-5`, kitty form) is captured;
  - `Ctrl-]` twice sends one 0x1D;
  - a configured `leave_key` replaces it.

  Satisfies: FR-003, research R-3.
- [X] T013 [P] [US1] Write `tests/session_replies.rs`: feeding DSR `CSI 6n`, DA1 `CSI c`, DA2 `CSI > c`, XTVERSION `CSI > q`, OSC 10/11 queries, kitty query `CSI ? u` and DECRQM `?2026` produces exactly the replies in research R-6.
- [X] T014 [P] [US1] Write `tests/scrollback.rs`:
  - rows leaving the 1,000-row vt100 tier go to the compact tier with text and style runs and no trailing blanks;
  - the total is capped at `scrollback_lines` with the oldest lines dropped first;
  - line addressing is continuous across both tiers;
  - selection text and search span both tiers.

  Satisfies: research R-2, FR-004.
- [X] T015 [P] [US1] Write `tests/session_lifecycle.rs`, using an xtask persona as the agent:
  - Fallow spawns nothing at startup (FR-025);
  - a session starts in the project folder with the profile's args and env additions;
  - exit shows the status and keeps the last output;
  - restart uses the same profile and project;
  - on Windows the agent is in the session's Job Object;
  - dropping the `SessionManager` leaves no agent process running.
- [X] T016 [P] [US1] Write `tests/session_transcripts.rs` and add scrubbed recorded first screens to `tests/fixtures/transcripts/` for each built-in agent available on the developer machine: replaying each through `src/session/term.rs` yields the stored golden screen (SC-002).

### Implementation for User Story 1

- [X] T017 [US1] Implement `src/session/term.rs`:
  - wrap `vt100::Parser` with `Callbacks` for bell, title, OSC 52 (ignored for privacy unless the user copies), `on_scroll` into `scrollback.rs`, and `unhandled_csi` / `unhandled_osc`;
  - produce the R-6 replies into a reply buffer;
  - track `?2026` (synchronized output, with a 150 ms timeout), `?1004` (focus reporting), and OSC 9/777/7501 events for attention.
- [X] T018 [US1] Implement `src/session/scrollback.rs`: the compact tier (text plus style runs, wrap flag) capped by `sessions.scrollback_lines`, and one line-addressing API over both tiers for the renderer, selection and search (research R-2).
- [X] T019 [US1] Implement `src/session/keys.rs`: `KeyEvent` → bytes in legacy encoding, honoring application cursor and keypad modes from the parser; bracketed paste wraps pasted text when the agent enabled `?2004`; the leave-key matcher (research R-3).
- [X] T020 [US1] Implement `src/session/platform/unix.rs` and `src/session/platform/windows.rs`:
  - Unix: process group signals `kill(-pgid, SIGTERM)` and `SIGKILL`;
  - Windows: a Job Object created with `KILL_ON_JOB_CLOSE` and the agent assigned to it right after spawn; graceful stop by `ClosePseudoConsole`, force by `TerminateJobObject`.

  Satisfies: research R-7.
- [X] T021 [US1] Implement `src/session/pty.rs`:
  - `portable-pty` `openpty` at the pane size and `spawn_command` with `cwd` = project path, the profile's args and env additions, and no shell;
  - a reader thread (64 KiB reads → parser → replies to the writer → one coalesced `AppEvent::SessionOutput`);
  - a waiter thread (`child.wait()` → `AppEvent::SessionExited`);
  - resize debounced by 50 ms;
  - on Windows, `ClosePseudoConsole` off the UI thread while the reader drains.

  Satisfies: research R-7.
- [X] T022 [US1] Implement `Session` and `SessionManager` in `src/session/mod.rs` per data-model.md:
  - the fields `id`, `project`, `profile`, `child`, `term`, `size`, `state`, `state_since`, `last_output`, `bell`, `scroll`, `selection`;
  - states `Starting`, `Working`, `NeedsAttention`, `Exited(status)`, `Failed(reason)`, `Stopping`;
  - start, restart and close (exited only);
  - refuse a start beyond `max_sessions` with the reason.
- [X] T023 [US1] Implement `src/ui/terminal_pane.rs`:
  - render the vt100 screen into the ratatui buffer (colors 16/256/RGB, bold, dim, italic, underline, inverse, wide characters, cursor position and style);
  - scrolled-back views across both tiers, with a "N lines kept" marker at the top;
  - mouse drag selection, or `Shift`-drag when the agent enabled mouse reporting; mouse events go to the agent when it asked for them;
  - the exited banner with status and the restart hint.
- [X] T024 [US1] Implement session focus handling in `src/app/workspace.rs`:
  - while a session has focus, every key except the leave key goes to `keys.rs` and the PTY;
  - pastes go to the agent;
  - the leave key returns focus to Fallow;
  - focus-in/out reports (`?1004`) are sent when focus or visibility changes;
  - after leaving: `Shift-PgUp`/`Shift-PgDn` scroll back, `Ctrl-F` searches the scrollback, `Ctrl-C` copies the selection (001 clipboard).
- [X] T025 [US1] Add a minimal start flow in `src/app/workspace.rs` and `src/ui/sessions.rs`: `n` opens the profile menu for the selected project (available profiles only, lazily resolved); choosing one starts a session and focuses it. Add the command entries to `src/app/commands.rs`.
- [X] T026 [US1] Add the xtask persona "echo" (an agent that echoes typed bytes and answers DSR like a real program) in `xtask/src/fixtures.rs`. Implement `cargo xtask bench-sessions` echo latency in `xtask/src/bench_sessions.rs`: Fallow under the PTY harness starts a session of the persona; the time from a typed key to its echo on screen has p95 < 50 ms over 100 keys (SC-003).

**Checkpoint**: US1 works on its own: one project, start an agent, use it, leave, return, scroll, copy, paste, exit, restart.

*Notes from implementation (2026-10-19):*
- **Workspace pulled forward from US3.** A minimal agent workspace (projects | sessions | processes, `Ctrl-E` to the editor view, `Tab` cycling) was built in Phase 3, because a session can't be used without one. It is now the default screen. US3 (T038–T041) keeps the polish, hints and help.
- **T016 uses synthetic transcripts.** Recording a real agent's screen can capture account names and paths, so nothing from a real agent is committed. `cargo xtask record-transcript -- <agent>` records locally for review. A real-agent check runs instead in `tests/session_lifecycle.rs`: `claude --version` through its npm launcher and the pseudo-terminal, when installed.

---

## Phase 3b: User Story 6 - Each agent works in its own git worktree (Priority: P1)

**Goal**: sessions of git projects run in their own worktree on a new branch by default; the user
can choose the project folder or an existing worktree; closing offers keep/remove, never removing
uncommitted work (FR-031 to FR-037, research R-11).

**Independent Test**: quickstart S16–S18. Two default sessions in one git project run in separate
worktrees on separate branches; a change in one is absent from the other and from the project
folder; a dirty worktree can't be removed.

### Tests for User Story 6

- [X] T054 [P] [US6] Write `tests/worktree.rs` against a real temporary repository (`git init`, one commit). Check:
  - git detection by walking up to `.git` (directory or file), and a plain folder is not a git project;
  - the branch is `fallow/<profile>-<n>` with the first free `n`, and an existing branch is never reused or reset (FR-033);
  - the path is `<parent>/<project>.worktrees/<profile>-<n>`, and `sessions.worktree_root` overrides the parent (FR-034);
  - two worktrees are isolated: a file changed in one is unchanged in the other and in the project folder (SC-010);
  - `git worktree list` filtering keeps only `fallow/` branches;
  - removal is refused with uncommitted changes and with untracked files, succeeds when clean, and keeps the branch (FR-035);
  - git is never run without an explicit call: no git process when `AppState` starts (FR-037).
- [X] T055 [P] [US6] Extend `tests/session_lifecycle.rs`: a session started in a new worktree runs there (its `pwd`/`cd` prints the worktree path); restart reuses the same worktree; closing with "Remove" removes a clean worktree; a failed `worktree add` (for example a path that already exists as a file) leaves nothing behind and reports the error.

### Implementation for User Story 6

- [X] T056 [US6] Implement `src/session/worktree.rs` per research R-11: `is_git_project`, `list` (porcelain, `fallow/` branches only), `next_branch`, `create` (from `git rev-parse HEAD`), `is_clean` (`status --porcelain --untracked-files=normal`), `remove` (never `--force`), each a short `std::process::Command` of the user's `git` with the error text kept for the UI. Add the `[sessions]` settings `worktrees` ("auto" | "never", default "auto") and `worktree_root` (default empty) to `src/config/schema.rs` with tests in `tests/config_merge.rs`.
- [X] T057 [US6] Add `workdir: ProjectFolder | Worktree { path, branch }` to `Session` in `src/session/mod.rs` and `StartRequest`. The agent's `cwd` is the worktree path; `restart` reuses it; `Session::label` shows the branch (FR-036).
- [X] T058 [US6] Extend the start menu (`Overlay::Menu`) in `src/app/workspace.rs` and `src/ui/workspace.rs` with the "Where" section per contracts/ui-commands.md:
  - "new worktree (fallow/<profile>-<n>)" preselected for git projects when `worktrees = "auto"`;
  - "project folder";
  - each existing `fallow/` worktree of the project.

  The list is computed when the menu opens (FR-032, FR-037). On a create failure, show the error and keep the menu open with "project folder" selected (US6 scenario 7).
- [X] T059 [US6] Implement closing a worktree session in `src/app/workspace.rs`:
  - a dialog (001 `ui::confirm`) with "Keep the worktree" focused by default and "Remove the worktree (the branch … is kept)";
  - Remove is not offered when `is_clean` is false, and the dialog says why;
  - removal runs `worktree::remove` (FR-035).
- [X] T060 [US6] Show the branch in the session list, the overview and the pane title (`src/ui/workspace.rs`, `src/ui/terminal_pane.rs`). Make `Ctrl-E` from a session in a worktree open the editor view on the worktree folder (US6 scenario 8, `src/app/workspace.rs`).
- [X] T061 [US6] Extend `xtask/src/bench_sessions.rs` to run in a temporary git repository with the default choice, so the CI echo-latency run also covers worktree creation. Record the worktree creation time in the step summary (informational; no budget).

**Checkpoint**: sessions in git projects are isolated by default; US1 still works for plain folders.

---

## Phase 4: User Story 2 - Run and oversee several agent sessions at once (Priority: P2)

**Goal**: an overview of all sessions with state and time in state, "needs attention" detection and notices, one-key switching, split view, confirmed stop and quit.

**Independent Test**: quickstart S8–S11 and S13. Three sessions in two projects; the overview shows working, needs attention and exited correctly, and each session is one key away.

### Tests for User Story 2

- [ ] T027 [P] [US2] Write `tests/session_attention.rs` with an xtask "attention" persona (works, rings BEL, prints OSC 9, OSC 777 and OSC 7501 waiting, sets a Gemini-style title, goes quiet). Check:
  - each signal moves the session to `NeedsAttention` and new output moves it back to `Working`;
  - the quiet fallback fires after `attention_delay_ms` only while group CPU is under the 001 busy threshold;
  - a hidden session raises a notice;
  - the timing is within the delay + 2 s (SC-004).

  Satisfies: research R-4.
- [ ] T028 [P] [US2] Extend `tests/control_requires_confirmation.rs` (001 SC-008), or create it if 001 Phase 8 has not landed:
  - stopping a session is unreachable without a `ConfirmedToken` naming that session;
  - force stop needs its own token;
  - quitting with running sessions needs one confirmation listing them.

  Satisfies: FR-012, constitution IV.

### Implementation for User Story 2

- [ ] T029 [US2] Implement `src/session/attention.rs`:
  - the R-4 signal order (BEL, OSC 9/777/7501, title status, quiet fallback with group CPU from the latest snapshot);
  - state transitions and `state_since` per the data-model.md state machine;
  - notices for sessions that are not visible with focus;
  - per-project counts of sessions needing attention.
- [ ] T030 [US2] Add `Request::Track { key, session }` and `Request::Untrack` to `src/procmon/worker.rs`. Tracked processes are always group roots, labelled `#n project`, type "session". Expose group memory and CPU per session to the UI (research R-8, FR-013). Add the cases to `tests/grouping.rs`.
- [ ] T031 [US2] Implement the session overview in `src/ui/sessions.rs` (`Ctrl-T`):
  - all sessions across projects with project, profile, state, time in state, and group memory and CPU (FR-008);
  - `Enter` or double-click focuses a session;
  - `p` jumps to its process;
  - `w` closes an exited session;
  - `R` restarts.
- [ ] T032 [US2] Implement next/previous session (`Ctrl-N` / `Ctrl-B` outside sessions; palette "Next session" / "Previous session") and split view (`s`: two panes side by side in the middle column, each focusable) in `src/app/workspace.rs` and `src/ui/workspace.rs` (FR-010, FR-011).
- [ ] T033 [US2] Implement stop (`Shift-K`) through 001's ControlRequest lifecycle in `src/app/workspace.rs` and `src/session/platform/*`:
  - the confirmation names the session, the agent and the process count;
  - graceful stop, then a separately confirmed force stop on Linux and macOS (FR-012).
- [ ] T034 [US2] Implement the quit flow in `src/app/state.rs`: `Ctrl-Q` with running sessions shows one confirmation listing them (project, profile, process count). Confirming stops all gracefully, waits up to 5 s, then exits; on Windows the Job Objects guarantee nothing outlives Fallow (spec Edge Cases).
- [ ] T035 [US2] Add the LLM-panel jump `j` (processes started as sessions only) and the `#n project` label in `src/ui/processes.rs` (FR-013).
- [ ] T036 [US2] Extend `xtask/src/bench_sessions.rs` with:
  - a switch benchmark: time from `Ctrl-N` to the other session fully drawn, under 100 ms (SC-005);
  - a flood benchmark: 8 sessions of a "flood" persona printing continuously, key echo in the focused session p95 under 100 ms (SC-006).
- [ ] T037 [US2] Extend `cargo xtask bench-memory --loaded` in `xtask/src/bench_memory.rs`: after the 001 script, start 4 sessions of the "flood" persona, each printing 10,000 lines of 200-column colored text, then settle and sample as before. Fail at ≥ 150 MB; INVALID unless `--bench-stats` reports `sessions=4` (FR-029, SC-007). Add `sessions` to the stats line in `src/main.rs` and contracts/cli.md of 001.

**Checkpoint**: several sessions can be overseen, switched, split and stopped; the loaded budget includes sessions.

---

## Phase 5: User Story 3 - Agent-first default screen (Priority: P3)

**Goal**: Fallow opens on projects, sessions and processes; `Ctrl-E` switches to and from the editor view.

**Independent Test**: quickstart S1 and S12.

### Tests for User Story 3

- [ ] T038 [P] [US3] Add render tests to `src/ui/workspace.rs`:
  - at start, the projects, sessions and LLM processes panes are shown with focus on Projects, and no editor or file tree;
  - `Ctrl-E` shows the 001 layout for the selected project and back, restoring selection and focus;
  - the hint bar on the workspace shows "n New session", the session switch key, "Ctrl-E Editor" and "? Help", and still contains `Ctrl-P Commands` (the startup marker, 001 contracts/cli.md).

### Implementation for User Story 3

- [ ] T039 [US3] Implement the agent-workspace layout in `src/ui/workspace.rs` per contracts/ui-commands.md:
  - projects | sessions (list plus the active session pane) | processes and system;
  - a focused session takes the whole middle column;
  - small terminals collapse to tabs with a focused session at full width.

  Hook it into `src/ui/layout.rs` as the default view.
- [ ] T040 [US3] Implement view switching (`Ctrl-E`) and per-view focus memory in `src/app/state.rs` and `src/app/workspace.rs`. The editor view uses the selected project's lazy tree (FR-019, FR-020).
- [ ] T041 [US3] Add workspace hints and contexts (Projects, Sessions, Session) to `src/app/commands.rs` and `src/app/keymap.rs`. Add the help sections and the 002 rows of the known-conflicts table (contracts/ui-commands.md) to `src/ui/help.rs`, and extend `tests/keymap_conflicts.rs` with every new key (FR-021, FR-022).

**Checkpoint**: the default screen is agent-first; the editor is one key away.

---

## Phase 6: User Story 4 - Keep several projects open and browse between them (Priority: P4)

**Goal**: a managed, remembered project list with per-project sessions, tree and process filter.

**Independent Test**: quickstart S2, S13 (restart part) and S15.

### Tests for User Story 4

- [ ] T042 [P] [US4] Write `tests/projects_store.rs`:
  - `projects.toml` round trip (order, `selected`, names);
  - atomic write (a temporary file, then rename);
  - a failed write keeps the in-memory list and reports a notice;
  - invalid entries are skipped with `projects.toml:line: message`;
  - a command-line folder is added if new, selected and written;
  - the file never contains anything but paths, names and `selected`.

  Satisfies: FR-014, FR-015, FR-017, FR-026.

### Implementation for User Story 4

- [ ] T043 [US4] Implement `src/projects/store.rs` per contracts/config.md "projects.toml": read once after the first frame (FR-028), atomic write after every change, errors as `file:line`.
- [ ] T044 [US4] Implement the project list pane in `src/ui/projects.rs` and its commands in `src/app/workspace.rs`:
  - move, `Enter`, `a` add (path prompt; `Ctrl-O` in both views), `Delete`/`d` remove (confirmation when it has sessions; the folder is never touched), `F2`/`r` rename, `Shift-↑`/`Shift-↓` reorder, `e` open in the editor view;
  - row markers: session count and `●n` needing attention; "missing" rows (FR-018).
- [ ] T045 [US4] Implement the per-project process filter `f` in `src/ui/processes.rs` (processes whose cwd is inside the project, with the filter shown in the panel title, FR-016) and the per-project session list in `src/ui/sessions.rs`.
- [ ] T046 [US4] Extend `bench-startup` and `bench-memory --idle` in `xtask/src/bench_startup.rs` and `xtask/src/bench_memory.rs` to run with a generated `projects.toml` of 50 projects in a temporary `--config-dir`. The budgets are unchanged: startup p95 < 100 ms, idle < 50 MB (FR-028, FR-029).

**Checkpoint**: projects persist across restarts and can be browsed.

---

## Phase 7: User Story 5 - Choose which agents can be started (Priority: P5)

**Goal**: built-in profiles with their per-run notification flags, custom profiles, and reload.

**Independent Test**: quickstart S14.

### Tests for User Story 5

- [ ] T047 [P] [US5] Extend `tests/profiles.rs`:
  - built-in profiles carry the per-run notification flags of research R-4 (Claude Code `--settings '{"preferredNotifChannel":"terminal_bell"}'`; Codex `-c tui.notifications=true -c tui.notification_method="bel"`), and overriding the profile removes them;
  - no built-in edits any agent settings file;
  - a failing command shows its error in the session only.

### Implementation for User Story 5

- [ ] T048 [US5] Add the per-run notification flags to the built-in profiles in `src/session/profiles.rs`, and Aider's `--notifications-command` if verified (research R-4). List each flag in the profile view.
- [ ] T049 [US5] Add the profile menu details (name, command, args, env names only) and "Reload configuration" support for `profiles.toml` in `src/ui/sessions.rs` and `src/app/state.rs` (FR-024, FR-027).

**Checkpoint**: all user stories work independently.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T050 Extend the SC-007 canary in `xtask/src/canary.rs`: a session persona prints a canary. After use and quit, no file in the config directory or temp directory contains it, and a memory dump during the session finds it only in Fallow's live scrollback (FR-026, SC-008). Run the existing egress test with a session open.
- [ ] T051 [P] Add CI wiring in `.github/workflows/ci.yml` on all three OSes: `bench-sessions`, the extended `bench-memory --loaded`, and the 50-project startup and idle runs (constitution VI).
- [ ] T052 [P] Update `README.md` and help: sessions, the leave key, known gaps (no kitty keyboard protocol in sessions, no reflow), and the Windows note on npm `.cmd` shims (research R-6, R-7).
- [ ] T053 Run quickstart S1–S15 on Windows locally and record the results in `specs/002-agent-sessions/quickstart.md`. Add the macOS and Linux manual runs to the macOS hardware checklist of 001 (plan, Release blockers).

---

## Dependencies & Execution Order

### Phase dependencies

- Setup (Phase 1) → Foundational (Phase 2) → user stories.
- T005 (event loop) blocks everything that streams output.
- US1 (Phase 3) depends on Phase 2.
- US6 (Phase 3b) depends on US1: it changes where sessions start. It comes before US2.
- US2 and US3 depend on US1: they need sessions to show.
- US4 depends on Phase 2 only; it can run in parallel with US1.
- US5 depends on T008 only; it can run in parallel with US2 and US3.
- Polish depends on the stories it verifies.

### Key task-level dependencies

- T017, T018, T019 → T021 → T022 → T023, T024 → T025 → T026
- T020 → T021, T033, T034
- T029 needs T030 for group CPU
- T037 needs T026's personas
- T043 → T044, T046
- T008 → T025, T048, T049
- T022 → T056, T057 → T058, T059 → T060; T026 → T061

### Within each story

Tests marked [P] first; they must fail before implementation. Then the model, then the engine
(pty, term, attention), then the UI, then benchmarks and CI.

## Parallel Example: User Story 1

```text
# Tests together:
T012 tests/session_keys.rs   T013 tests/session_replies.rs   T014 tests/scrollback.rs
T015 tests/session_lifecycle.rs   T016 tests/session_transcripts.rs

# Engine pieces in parallel (different files):
T017 src/session/term.rs   T018 src/session/scrollback.rs   T019 src/session/keys.rs   T020 src/session/platform/*
```

## Implementation Strategy

### MVP first (User Story 1)

1. Phase 1 and Phase 2. 001's CI stays green after the event-loop rework (T005).
2. Phase 3: one project, one or more sessions, full terminal behavior. **Stop and validate** with
   quickstart S3–S7 against a real agent on each platform available, and `bench-sessions` echo
   latency in CI.

### Incremental delivery

US1 → US6 (worktrees by default) → US2 (oversight and the memory gate with sessions) → US3 (agent-first screen) → US4 (projects
persist) → US5 (profiles). Each increment keeps every constitution gate green in CI.

## Notes

- [P] tasks touch different files and have no dependency on unfinished tasks.
- Commit after each task or logical group; push to `001-llm-aware-tui-editor`.
- Never write session output, scrollback or environment values anywhere (FR-026, FR-027).
