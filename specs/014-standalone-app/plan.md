# Implementation Plan: Fallow as a standalone app

**Branch**: `main` (the project's working branch since 2026-11-14) | **Date**: 2026-11-14 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/014-standalone-app/spec.md`

## Summary

- **One process, one window**: the service (`fallow serve`) shows the Studio in its own window on
  its UI thread (which also holds the notification icon), through WebView2, using `wry` on a window
  Fallow makes itself (research R-1, R-2).
- **No web server**: the window gets every page and API answer from Fallow's own process through a
  custom scheme (`http://fallow.localhost/`). A release build listens on no port, holds no secret
  and serves no browser (R-3).
- **`fallow`**, and **`fallow-app.exe`** (no console: the Start menu), start the
  service if needed and ask it, over the per-user pipe, to show or raise its window (R-2, R-18,
  contracts/window.md).
- **Closing the window** asks the page about unsaved edits, then drops the web view and keeps the
  service, its runs and its icon; **Quit** stops Fallow after a confirmation that names the running
  work (R-9).
- **Native**: the folder picker (008), the system file dialog and drag and drop, links in the
  default browser, Windows' notifications, the frame in 013's theme from the first frame (R-4, R-5,
  R-7).
- **Install**: an unsigned per-user Inno Setup package built in CI (R-11). No start at login: the
  engineer opens Fallow when they want to work in it (FR-010 dropped).
- **Measured**: idle memory and window ready on the release build; running memory and activity
  latency on an `automation` test build, with the real window open (R-15).
- **First, a spike** (R-20) measures WebView2's memory, start time and build cost on this machine
  before tasks are written; memory over budget goes back to the engineer.

## Technical Context

**Language/Version**: Rust, edition 2024, MSRV 1.85 (as `Cargo.toml`); the page stays plain
JavaScript and CSS compiled in (`include_str!`).

**Primary Dependencies**:
- added: `wry` 0.57 (Windows only; brings `webview2-com`, `windows`, `raw-window-handle`, `http`),
  justified in R-1;
- `windows-sys` gains the features for OLE, DWM, the task dialog, monitors, the window station and
  process snapshots (`Win32_System_Ole`, `Win32_Graphics_Dwm`, `Win32_UI_Controls`,
  `Win32_System_StationsAndDesktops`, `Win32_System_Diagnostics_ToolHelp`);
- `tiny_http` becomes a dependency for `cfg(not(windows))` (their browser page, until their own
  feature) and for the `automation` feature;
- an application manifest for both executables (R-17); a second binary `fallow-app` (R-18);
- `deny.toml`: `wry` allowed, the graph restricted to `x86_64-pc-windows-msvc`.

**Storage**: Fallow's data folder: `window.json` (R-8), `webview/` (WebView2's user data, R-6).

**Testing**: `cargo test` as today, file by file, and `cargo test --features automation` for the
tests of the HTTP transport; page routes tested through `answer` with no socket; the visual checks
in the real window over WebView2's debugging port (`automation` build); new CI checks for no
listener, no outbound connection (a 10-minute trace), the navigation decision, the installer as a
standard user, closing the window during a loop (SC-005), and the budgets (R-15, R-16).

**Target Platform**: Windows 10 and 11, x64, with the WebView2 Evergreen runtime.

**Project Type**: a desktop app: a Rust service with its own window showing a local web UI.

**Performance Goals**: window ready < 1 s (p95 of 30 launches after 3 warm-ups); activity latency
< 2 s.

**Constraints**: idle < 250 MB and running < 400 MB, private working sets of Fallow plus its web
view processes (constitution 8.0.0); window ready < 1 s; no network
request of Fallow's own; no listener in a release build; per-user install without elevation; no
browser fallback.

**Scale/Scope**: one window, about 25 screens and states (013 FR-005) unchanged in content; the
page gains drag and drop, an in-page image viewer, `closeAsked` and three IPC messages; the page's
server code (about 1,300 lines in `http.rs`) split into `answer` and two transports; a new
`src/window/` module (UI thread, window, web view, state, Quit) of roughly 1,000 to 1,500 lines;
`fallow-app`; the manifest; the installer script; CI jobs.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Not affected: the Studio's content is unchanged (FR-005). |
| II. The Engineer Decides | Pass: Quit confirms and names every running loop and session (FR-009); closing the window stops nothing and keeps 009's unsaved-edits question (R-9); links leave only on the engineer's click (R-4). |
| III. Coordination in Code | Not affected. |
| IV. Simplest Setup First | Pass: one process and one window on the existing message loop (R-2); `wry` without `tao`; no framework, no build step for the page. |
| V. Local and Private | Pass: no listener and no secret (R-3), WebView2's environment variables cleared so none can add a debugging port (R-6); the window loads only `fallow.localhost` and opens only `http(s)` addresses, in the browser, on a click (R-4); the web view's network features off where WebView2 allows, the rest listed (R-6) and traced in CI; notifications unchanged (008). |
| VI. Least Privilege | Pass: the per-user install needs no elevation, checked as a standard user (R-11, R-16); the web view's data is in Fallow's data folder; 014 adds no write place (no login item: FR-010 dropped). |
| VII. Runs Survive | Pass: the window is a view; closing it, or the web view crashing, leaves the engine and its runs untouched; Quit and the end of the Windows session stop as `fallow stop` does (runs resume, R-9); no console window that could be closed (R-18). |
| VIII. Small and Measured | Pass, to be shown by CI: the four budgets measured with the window counted, each benchmark line listed (R-15); idle after closing the window measured too (FR-011). The spike (R-20) measured 88 MB idle (private working sets, constitution 8.0.0) and window ready p95 1.02 s; the engine's first answers to the dashboard are made faster to bring it under 1 s. Running memory and activity latency are measured on the `automation` build (Complexity Tracking). |
| IX. Runtime-Agnostic | Not affected. |
| Dependencies (Workflow) | `wry` justified against V, VI, VIII (R-1); `deny.toml` lifts only `wry`. |
| CI gates (Workflow) | Updated (R-16): "loopback only" becomes "no listener", for Fallow and its web view; "the window loads nothing external and stays on the Studio" is the unit-tested navigation decision and the release CSP; the outbound-connection gate is a trace covering the window; the per-user install gate runs as a standard user. |

**Re-check after Phase 1 design and the review**: unchanged. The contracts add no listener in a
release build, no write place, and no request of Fallow's own.

## Project Structure

### Documentation (this feature)

```text
specs/014-standalone-app/
├── spec.md
├── plan.md              # this file
├── research.md          # R-1 to R-20
├── data-model.md        # the window's state, Quit, the package
├── quickstart.md        # Q1 to Q8
├── contracts/
│   ├── window.md        # the pipe commands, the custom scheme, the page's IPC messages
│   └── cli.md           # `fallow` and its commands after 014
├── checklists/
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── main.rs                  # `fallow` asks for the window; `quit`; no browser (Windows)
├── bin/fallow-app.rs        # NEW: the GUI-subsystem launcher (the Start menu's target)
├── service/
│   ├── mod.rs               # serve: no web server on Windows; the UI thread
│   ├── answer.rs            # NEW: the Studio's requests, transport-neutral (from http.rs)
│   ├── http.rs              # the tiny_http transport: automation builds, other platforms
│   └── web/                 # app.js (IPC, closeAsked), solo.js (drop, image viewer, upload)
├── window/                  # NEW, Windows only
│   ├── mod.rs               # the UI thread: OLE, message loop, posted messages, Quit, session end
│   ├── frame.rs             # the Win32 window, system menu, DWM theme, DPI, placement
│   ├── view.rs              # wry: environment, scheme, navigation decision, context menu
│   └── state.rs             # window.json
├── platform/
│   ├── notify.rs            # the icon, part of the UI thread; Open Fallow, Notifications, Quit
│   └── display.rs           # NEW: interactive desktop or not
assets/icon/make_res.py      # the manifest added to the resources (R-17)
installer/
└── fallow.iss               # NEW: Inno Setup, per user
spikes/014-window/           # NEW, throwaway: the measurements of R-20
xtask/src/bench.rs           # the benchmark lines of R-15
tests/                       # route tests through `answer`; HTTP tests behind `automation`
.github/workflows/ci.yml     # separate jobs: builds, gates, tests, benchmarks, trace, package
deny.toml                    # wry allowed; graph restricted to the Windows target
```

**Structure Decision**: one crate as today. The window is a module of the service, compiled only
on Windows (`cfg(windows)`); on other platforms `fallow` keeps opening the page as today until
their own feature (006).

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| VIII: running memory and activity latency measured on the `automation` build, not the installed app (R-15) | They need a scripted run, and the release app accepts requests only from its window. Chosen by the engineer on 2026-11-14; the real window is open and counted; the build differs only by the test transport, off unless an environment variable is set. Recorded in the proposed amendment (R-19). | A pipe command in the release app to start runs would be a control surface in what ships. |
