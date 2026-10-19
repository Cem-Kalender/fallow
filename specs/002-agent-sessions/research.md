# Research: Agent Sessions and Multiple Projects

Phase 0 for [plan.md](./plan.md). Facts checked on 2026-10-19 against crates.io, docs.rs, the
crates' sources and the agents' official docs; items marked **verify** were not confirmed and are
checked during implementation. 001's research (R-1 … R-13) still applies; numbering here is local
to 002.

## R-1: Terminal emulator

**Decision**: `vt100` **=0.18.1** (MIT; deps `vte` 0.15, `unicode-width` 0.2, `itoa`; no async, no
image libraries; edition 2024, MSRV 1.85). Fallow renders `vt100::Screen` into the ratatui buffer
itself (about 150 lines, modelled on `tui-term`'s loop; `tui-term` 0.3.4 depends on vt100 0.16 and
is not used). Two-tier scrollback (R-2).

**Rationale**:
- Covers what the built-in agents use: alternate screen, 256 and 24-bit color, scroll regions,
  bracketed-paste tracking, mouse modes 9/1000/1002/1003 with default, UTF-8 and SGR (1006)
  encodings, application cursor/keypad modes, cursor visibility and style (DECSCUSR), wide
  characters, bold/dim/italic/underline/inverse, and callbacks for bell, title, OSC 52 and resize.
- `Callbacks::on_scroll` (new in 0.18) streams rows leaving the screen, which is what makes a
  compact second scrollback tier possible.
- Small dependency tree, already-compatible `unicode-width`, same author as the maintained
  `pty-process`.

**What vt100 does not do, and Fallow adds through `unhandled_csi` / `unhandled_osc`** (R-6):
query replies (DSR `CSI 6n`, DA1/DA2, XTVERSION, OSC 10/11 colors), synchronized output
(`?2026`), focus reporting (`?1004`), kitty keyboard protocol queries, OSC 8 links (ignored, text
kept), OSC 9/777/7501 notifications (used for attention, R-4). No reflow on resize; agents redraw
on resize, and scrollback lines keep their original width.

**Risks**: 0.18.1 was released on 2026-10-19 and 0.18.0 was a broken release. Pin `=0.18.1`, run
the transcript tests (R-9) against it, keep 0.16.2 as a fallback (no `on_scroll`; the second tier
would then be fed by diffing the top row on each scroll, which costs more CPU).

**Alternatives considered**:
- `alacritty_terminal` 0.26.0 (Apache-2.0): the broadest feature set (reflow, kitty keyboard,
  synchronized output, query replies built in) and 24-byte cells, but no hook to move rows out of
  its full-width scrollback (10k × 200 × 4 × 24 B = 192 MB), a larger and frequently changing API,
  and more dependencies. Fallback if reflow or kitty keyboard become requirements.
- `termwiz` / `wezterm-term`: `wezterm-term` is not published on crates.io (git or forks only), and
  the dependency tree is large. Rejected.
- `vte` alone: the grid, modes and scrollback would be several thousand lines of our own. Rejected
  for now.
- libghostty-vt bindings (`ghostty-vt` 0.7.2, `ratatui-ghostty` 0.2.0): best conformance, but an
  alpha C API, a Zig toolchain or prebuilt blobs, and unclear Windows support. Revisit later.

## R-2: Scrollback memory

**Decision**: two tiers per session.
1. vt100 keeps the newest **1,000** scrollback rows as full cells (32 B per cell, asserted in its
   source).
2. Rows that leave vt100's scrollback go through `on_scroll` into a Fallow-owned **compact store**:
   each line as its text plus style runs (`RowContents::write_formatted_basic` equivalent), with a
   wrap flag. Trailing blanks are not stored. Capped at `scrollback_lines` (default 10,000) in total.

Selection, search and scrolling address both tiers as one sequence of lines.

**Estimate** (worst case, 200-column panes, 4 sessions with full scrollback):

| Part | Size |
|---|---|
| vt100 tier: 1,000 × 200 × 32 B × 4 | 25.6 MB |
| visible grids: 50 × 200 × 32 B × 4 | 1.3 MB |
| compact tier: 9,000 lines × ~150 B × 4 | 5.4 MB |
| per-session buffers, channels, thread stacks (committed) | ~2 MB |
| **sessions total** | **~35 MB** |

With 001's measured loaded figures (12.7–25.3 MB peak on CI), the total stays well under 150 MB
(FR-029). Idle sessions cost little because vt100's scrollback grows only as rows arrive.

**Alternatives**: full-cell scrollback for all 10,000 lines (vt100 256 MB, alacritty 192 MB at 200
columns): over budget. Disk-backed scrollback: violates FR-026 (output never written to disk).

## R-3: Leave-session key

**Decision**: **`Ctrl-]`** (0x1D) leaves the session; pressing it twice sends one `Ctrl-]` to the
agent (in the agent's current key encoding). Configurable as `sessions.leave_key`. Fallow matches
both decodings of 0x1D (some legacy decoders report it as `Ctrl-5`) and the kitty form.

**Rationale**: none of the built-in agents bind it by default:
- **Claude Code, Codex CLI, Gemini CLI:** not in their default key tables.
- **opencode:** uses `Ctrl-Alt-]`, a different key.
- **Aider:** uses it only for prompt_toolkit's emacs character search, which still works by pressing
  it twice.

It is the telnet escape key, so "leave the session" is a familiar meaning. No default binding was
found in Windows Terminal, Terminal.app or iTerm2 (**verify** on the macOS hardware run, as for
001's keys). It doesn't collide with tmux (`Ctrl-B`) or screen (`Ctrl-A`).

**Alternatives**:

| Key | Why not |
|---|---|
| `Ctrl-\` | Runner-up, but hard to type on many non-US layouts. |
| `Ctrl-Space` | macOS input-source switch; Windows IME toggle. |
| `F12` | Gemini CLI's debug console; F-keys need `Fn` on Mac keyboards. |
| `Esc`, `Ctrl-O/G/T/R/S/B/X/Z`, `Shift-Tab` | Heavily used by the agents. |

Non-US layouts where `]` needs AltGr are why the key is configurable.

## R-4: Detecting "needs attention"

**Decision**: a session is "needs attention" on the first of these, from most to least reliable,
without reading conversation content:
1. **BEL** (0x07) from the agent.
2. **Notification escapes**: OSC 9 (iTerm2 style), OSC 777 `notify`, and OSC 7501 "Program Status"
   reporting "waiting" or "done" (Claude Code 2.1.295+). OSC 9;4 progress and OSC 7501 "working"
   mark the session working.
3. **Title status** (OSC 0/2): Gemini CLI's default titles start with ◇ Ready / ✋ Action Required /
   ✦ Working; "Action Required" and "Ready" count as needing attention.
4. **Quiet fallback**: no output for `attention_delay_ms` (5 s) while the process group's CPU is
   under the 001 busy threshold.

**Supporting decisions**:
- **Focus reporting (`?1004`)**: Fallow tells agents that asked for it whether their pane is visible
  and focused. Several agents only notify when they believe they are unfocused (Codex
  `notification_condition = unfocused`; Claude Code "when you appear to be away").
- **Built-in profiles turn notifications on for that run only**, with flags, never by editing the
  user's settings files:
  - Claude Code: `--settings '{"preferredNotifChannel":"terminal_bell"}'`. Its `auto` channel only
    notifies in iTerm2, Ghostty and Kitty.
  - Codex: `-c tui.notifications=true -c tui.notification_method="bel"`.
  - Gemini CLI: no per-run flag; it relies on title status (signal 3).
  - Aider: `--notifications-command` pointing to `fallow --notify-session <id>` (**verify**).

  Each flag is listed in the profile and can be removed by overriding the profile.
- Hooks (Claude Code, Codex, Gemini) give the most precise events but need a channel back to
  Fallow. That channel is feature 003's; 002 doesn't use hooks.

**Alternatives**: screen scraping for prompts (fragile, and close to reading conversation content).
Rejected.

## R-5: Event loop

**Decision**: replace 001's "block on `crossterm::event::poll` with a timeout" (001 R-6) with one
`std::sync::mpsc` channel of `AppEvent` that the main thread blocks on with `recv_timeout(250 ms)`.

Producers:
- an **input thread** forwarding crossterm events;
- the procmon worker;
- search;
- notify;
- for each session:
  - a **reader thread** that reads the PTY into its parser and sends `SessionOutput(id)`, coalesced:
    at most one pending per session;
  - a **waiter thread** that sends `SessionExited(id, status)`.

Rendering is capped at one frame per 16 ms while output streams. A pane with synchronized output
active (`?2026h`) is not redrawn until `?2026l` or a 150 ms timeout.

**Rationale**: with sessions, the main loop must wake on agent output, not only on keys. Polling
crossterm in a tight loop would cost idle CPU. The channel keeps 001's no-async model (constitution;
001 R-6). The 250 ms timeout keeps the liveness ticks of 001's benchmarks unchanged.

**Burst detection for pasted keys** (001 Windows fix) moves to the input thread: it marks an event
when more input is already queued.

**Alternatives**: async runtime (banned by the constitution's dependency policy); per-session
polling (CPU cost at idle).

## R-6: Replies an agent waits for

**Decision**: Fallow answers, on the reader thread, straight to the PTY writer:
- DSR `CSI 6n` → `CSI row;col R` (the cursor of that session's screen).
- DA1 `CSI c` → `CSI ?62;22c`.
- DA2 `CSI > c` → `CSI >0;10;0c`.
- XTVERSION `CSI > q` → `DCS >|fallow <version> ST`.
- OSC 10 / 11 color queries → the pane's default foreground and background.
- Kitty keyboard query `CSI ? u` → `CSI ?0u` ("no flags"; push and pop are acknowledged and ignored).
  Sessions use legacy key encoding in v1.
- DECRQM for `?2026` → "set/reset supported".

**Rationale**: ConPTY sends `CSI 6n` at startup (portable-pty creates the pseudoconsole with
`INHERIT_CURSOR`) and blocks until it gets an answer. Newer ConPTY also queries DA1. Crossterm- and
Ink-based agents probe these at startup and stall until a timeout without replies. vt100 never
replies.

**Gap accepted for v1**: no kitty keyboard protocol inside sessions. Claude Code then uses its
legacy bindings (`Ctrl-J` / `\` + Enter for a newline instead of `Shift-Enter`); listed in help as a
known gap (spec Edge Cases).

## R-7: Pseudo-terminals and processes

**Decision**: `portable-pty` **0.9.0** (MIT). It is already a dev-dependency of xtask and runs in
001's CI benchmarks on all three platforms, ConPTY included. It moves to a regular dependency.

Per session:
- `openpty` at the pane size, then `spawn_command` with `cwd` = the project folder and the profile's
  args and env additions. No shell, so there is no expansion.
- `take_writer` behind a `Mutex`, `try_clone_reader` for the reader thread, and `child.wait()` on
  the waiter thread.
- Resizes are debounced by 50 ms.

| | Linux and macOS | Windows |
|---|---|---|
| Process group | `setsid` makes the agent a session leader: pgid = pid | Each session's agent is placed in a **Job Object** created with `KILL_ON_JOB_CLOSE`; Fallow holds the job handle, so no agent or descendant outlives Fallow (spec Edge Cases) |
| Graceful stop (FR-012) | `kill(-pgid, SIGTERM)` | `ClosePseudoConsole`, which sends CTRL_CLOSE_EVENT |
| Wait | 001 kill flow: wait up to 5 s | same |
| Force stop (second confirmation, 001 FR-033) | `kill(-pgid, SIGKILL)` | `TerminateJobObject` |
| End of output | Linux `EIO` / macOS 0-byte read means EOF | The output pipe stays open after the child exits; exit is detected on the waiter thread, then `ClosePseudoConsole` runs off the UI thread while the reader drains |

**Windows executables**:
- npm installs agents as `name.cmd` shims. Running a `.cmd` needs `cmd.exe /d /s /c`, which brings
  the batch-argument escaping problem (CVE-2024-24576) and the "Terminate batch job (Y/N)?" prompt
  on Ctrl-C.
- Profile resolution therefore prefers a real `.exe` on `PATH`. For an npm `.cmd` shim it resolves
  the `node` + script target written in the shim.
- If neither works, Fallow refuses to start the profile and says why, rather than going through
  `cmd.exe`.

**ConPTY version**: Fallow uses the system ConPTY. portable-pty prefers a `conpty.dll` /
`OpenConsole.exe` placed next to the executable. Shipping those (NuGet
`Microsoft.Windows.Console.ConPTY`, MIT) is a release-packaging option recorded for the Windows
release task, not a requirement: it would mean shipping more than one file.

**Alternatives**: `pty-process` (Unix only); `rustix-openpty` (primitive only); `conpty` crate
(stale); `alacritty_terminal::tty` (only if R-1 switches to alacritty).

## R-8: Linking sessions to the process panel

**Decision**: the procmon worker gets `Request::Track { key: ProcessKey, session: SessionId }` and
`Request::Untrack`. A tracked process is always a group root (type "session", confidence "certain"
when its profile is a built-in agent, otherwise "session"), labelled `#n project` (FR-013). Its
children group under it as in 001. The session overview reads group memory and CPU from the latest
snapshot (FR-008). The "needs attention" CPU test uses the same figures.

**Rationale**: the shell profile and custom profiles may not match any 001 signature, but their
memory still belongs in the overview. Using the snapshot avoids a second process scan.

## R-9: Verifying sessions (tests and benchmarks)

| Check | How | Where |
|---|---|---|
| Fidelity (SC-002) | **Transcript tests**: recorded byte streams of each built-in agent's first screen and a few interactions (captured once on a developer machine, stored under `tests/fixtures/transcripts/`, secrets and paths scrubbed), replayed into the emulator and compared with golden screens. Plus a manual check per release against real agents (quickstart). | `tests/session_transcripts.rs` |
| Key passthrough (FR-003) | Every key in a table goes to the PTY with the expected bytes; only the leave key is captured. | `tests/session_keys.rs` |
| Query replies (R-6) | Feed each query and check the reply bytes. | `tests/session_replies.rs` |
| Attention (SC-004) | A test "agent" (xtask persona) that works, rings, prints OSC 9, sets a title, then goes quiet; check state changes and timing. | `tests/session_attention.rs`, CI |
| Latency (SC-003, SC-005, SC-006) | `cargo xtask bench-sessions`: echo latency for a typed key (p95 < 50 ms); switch-session redraw (< 100 ms); key echo with 8 sessions flooding output (< 100 ms). | CI, three OSes |
| Memory (SC-007, FR-029) | `bench-memory --loaded` extended: the reference fixture plus 4 sessions of the xtask "flood" persona, each printing 10,000 lines of 200-column colored text, then the existing settle and sample. Budget unchanged (< 150 MB). `--idle` gains a `projects.toml` with 50 projects. | CI, three OSes |
| Startup (FR-028) | `bench-startup` with a 50-project `projects.toml` present. | CI |
| Privacy (SC-008) | A persona prints a canary; after quitting, no file in the config dir or temp contains it; the existing egress test runs with a session. | CI |
| No process at startup (FR-025) | A test asserts no child process exists after start with projects and profiles configured. | `tests/session_lifecycle.rs` |

## R-10: Project list persistence

**Decision**: `projects.toml` in Fallow's config directory
([contracts/config.md](./contracts/config.md)), written atomically (temporary file, then rename) on
every change, read once after the first frame. It holds paths and names only.

**Rationale**: the config directory is the only place Fallow may write outside user-saved files
(Principle V). TOML matches the other config files and their `file:line` error reporting.

**Alternatives**: a separate state directory (XDG state, `%LOCALAPPDATA%`): more paths to resolve,
document and test for no benefit at this size. Possible later move if state grows.
