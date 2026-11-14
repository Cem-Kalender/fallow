---
description: "Task list for 014 Fallow as a standalone app"
---

# Tasks: Fallow as a standalone app

**Input**: Design documents from `specs/014-standalone-app/` (spec, plan, research R-1 to R-20,
data-model, contracts/window.md and cli.md, quickstart Q1 to Q8; the spike's
`spikes/014-window/` shows working `wry` code).

**Tests**: the spec asks for CI checks (SC-004 to SC-007, the per-user install, the budgets), so
tests are included.

**Build and test** on the engineer's machine: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=target/wip`, `-j 1`, one test file per command (the engineer's running Fallow
holds `target/debug`). Windows only; other platforms keep 006's browser page (`cfg(not(windows))`).

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

- [X] T001 Dependencies and policy (research R-1, R-3, plan Technical Context):
  - `Cargo.toml`: `wry = "=0.57.2"` and `raw-window-handle = "0.6"` under `[target.'cfg(windows)'.dependencies]`; `windows-sys` gains `Win32_System_Ole`, `Win32_Graphics_Dwm`, `Win32_UI_Controls`, `Win32_System_StationsAndDesktops`, `Win32_System_Diagnostics_ToolHelp`, `Win32_System_ProcessStatus`;
  - `tiny_http` becomes `optional = true` for Windows and stays plain for `cfg(not(windows))`; a feature `automation = ["dep:tiny_http"]`;
  - a second binary `[[bin]] name = "fallow-app"`, `path = "src/bin/fallow-app.rs"`;
  - `deny.toml`: remove `wry` from `deny`; keep `tao`, `winit`, `gtk`, `webkit2gtk`; add `[graph] targets = ["x86_64-pc-windows-msvc"]`;
  - check `cargo deny check` and record the build's time, the binary's size and the target folder's size in `specs/014-standalone-app/checks/build.md`.
- [X] T002 The application manifest (R-17): `assets/icon/make_res.py` adds an `RT_MANIFEST` (Common Controls 6.0, per-monitor DPI awareness v2, Windows 10/11 `supportedOS`, `asInvoker`) to `assets/icon/fallow.res`; `build.rs` links it into both `fallow.exe` and `fallow-app.exe`. Check `TaskDialogIndirect` resolves (a unit test that loads it with `GetProcAddress` on Windows).

---

## Phase 2: Foundational (the Studio without a transport, the UI thread)

**Purpose**: everything the window and the tests stand on. Nothing visible changes yet: the
browser page still works.

- [ ] T003 Split `src/service/http.rs` (R-3): a new `src/service/answer.rs` with `pub struct Request { method, path, query, headers, body }`, `pub struct Response { status, headers, body }` and `pub fn answer(req: Request, tx: &Sender<Msg>, theme: &KeptTheme) -> Response` holding every route of today's `handle` (pages, `app.css`, `app.js`, screens, `/api/*`, the long poll `/api/events`, the folder picker, 013's theme). `http.rs` keeps only the `tiny_http` transport: secret, cookie, origin and host checks, then `answer`. All existing tests still pass unchanged.
- [ ] T004 [P] New `tests/answer_routes.rs` (no socket): `GET /` carries 013's theme and the CSP header (`default-src 'self'; script-src 'self'; img-src 'self' data:; connect-src 'self'`); a JSON route answers; `GET /api/events?after=<seq>` answers at once when the sequence differs; an unknown path is 404.
- [ ] T005 Gate the HTTP transport (R-15): on Windows `http::Web` exists only with `feature = "automation"`; mark the tests that use it (`service_http`, `us11_*` and `us12_*` that call `Web::start`, `visual_009`, `visual_010`, `visual_013`, `web_theme`'s HTTP test) with `required-features = ["automation"]` in `Cargo.toml` (`[[test]]` entries); `.github/workflows/ci.yml` runs `cargo test --workspace --locked --features automation` and `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] T006 The UI thread (R-2): new `src/window/mod.rs` (Windows only). One thread that calls `OleInitialize`, creates a hidden message window and runs the Win32 message loop that `src/platform/notify.rs` runs today. The notification icon becomes a part of it: when `Shell_NotifyIconW(NIM_ADD)` fails the thread lives on and the icon is added again on the `TaskbarCreated` message. Other threads post to it only (`PostMessageW`): `Show { route: Option<String>, launched: Option<u64> }`, `Quit`, `Theme(String)`. `src/service/mod.rs::serve` starts it instead of `Tray::start`; `Ctx.page` and the notification click become a `Show` with the item's route.
- [ ] T007 Pipe commands (contracts/window.md §1) in `src/service/mod.rs` `cli_request`: `window` (with `launched`, `route`), `quit` (the running loops and sessions as `{"kind","project","name"}`, asked of the engine on the pipe's thread, never the UI thread) and `quit` with `confirmed` (answers the service pid, then stops as `stop`); `url` only with `feature = "automation"`.

**Checkpoint**: the suite passes; the service runs with its icon on the new UI thread; the browser page still opens with `fallow` (until T013).

---

## Phase 3: User Story 1 - Fallow opens in its own window (P1) 🎯 MVP

**Goal**: `fallow` and the Start menu open the Studio in Fallow's window, one per user, in 013's
theme from the first frame, where it was last time.

**Independent Test**: quickstart Q2 and Q3 with a local build (`fallow` then `fallow-app.exe`).

- [ ] T008 [P] [US1] `src/window/nav.rs`: the pure decision function of R-4 `decide(url: &str, new_window: bool, user_initiated: bool) -> Nav { Load, Browser(String), Refuse }` and its unit tests (every row of contracts/window.md §3: `http://fallow.localhost/…` loads, as a new window is refused; user-initiated `http(s)` elsewhere goes to the browser, not user-initiated is refused; `file:`, UNC, `ms-*:`, `search-ms:`, `javascript:`, `about:` other than `about:blank` refused).
- [ ] T009 [P] [US1] `src/window/state.rs`: `window.json` (data-model: `x`, `y`, `width`, `height` ≥ 800 × 500, `maximized`), read and written; with no screen for the rectangle (`MonitorFromRect(…, MONITOR_DEFAULTTONULL)`), 1280 × 800 fitted to the main screen's work area, centred. Unit tests for the fallback and for an unreadable file.
- [ ] T010 [US1] `src/window/frame.rs` (R-7, R-8): the window class with a background brush of 013's page background for the theme read at creation (`prefs.json`'s `theme`, and `AppsUseLightTheme` for System), `DWMWA_USE_IMMERSIVE_DARK_MODE`, title "Fallow" and Fallow's icon, the restored placement, `WM_DPICHANGED` (the suggested rectangle), `WM_SIZE` (the web view's bounds), `WM_SETTINGCHANGE` "ImmersiveColorSet" (frame theme with System), and the system menu's "Close window" and "Quit Fallow" items. No menu bar.
- [ ] T011 [US1] `src/window/view.rs` (R-1, R-3, R-4, R-6), from the spike's working code:
  - before the environment: remove `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, `WEBVIEW2_USER_DATA_FOLDER`, `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` and `WEBVIEW2_RELEASE_CHANNEL_PREFERENCE` from the process environment;
  - the environment created by Fallow (`webview2-com`, `CreateCoreWebView2EnvironmentWithOptions`) with user data in `<data>/webview`, browser arguments `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`, `IsCustomCrashReportingEnabled = true`; passed with `with_environment`;
  - `build_as_child` on the frame; `with_background_color` and `with_theme` from T010; `with_browser_accelerator_keys(true)`; devtools only with `debug_assertions` or `feature = "automation"`; `IsReputationCheckingRequired = false` on the settings;
  - `with_asynchronous_custom_protocol("fallow", …)`: each request converted to `answer::Request`, answered on its own worker thread, converted back;
  - the navigation and new-window handlers call `nav::decide` (with `IsUserInitiated` from the controller's events); `Browser(url)` goes to `platform::open_browser`; downloads refused;
  - the context menu without "open link in new window", "save as", "print" and "inspect" in release builds (`ContextMenuRequested` on the controller);
  - the IPC handler: `ready`, `theme:<light|dark|system>`, `close`, `activity:<session>` (contracts/window.md §4); anything else ignored.
- [ ] T012 [US1] No runtime (R-12) in `src/window/view.rs`: before building, `wry::webview_version()`; without one, a native message naming "Microsoft Edge WebView2 Runtime" with a button that opens Microsoft's download page in the default browser on click; the `window` request answers the error.
- [ ] T013 [US1] The launchers (R-2, R-13, R-18, contracts/cli.md): new `src/platform/display.rs` (`interactive()`: the process's window station is `WinSta0`); `src/main.rs` on Windows: `fallow` checks the desktop, starts the service if needed (`ensure_running`, passing `--data-dir`/`--config-dir` when given), calls `AllowSetForegroundWindow(<service pid>)`, sends `{"cmd":"window","launched":…}`, opens no browser; new `src/bin/fallow-app.rs` (`#![windows_subsystem = "windows"]`) does the same and shows errors with `MessageBoxW`. Other platforms unchanged.
- [ ] T014 [US1] The page (contracts/window.md §4, §5): `src/service/web/app.js` posts `ready` after the first render of `start()`, `theme:<choice>` when 013's control changes, `activity:<session>` when a session's activity line is drawn; `Fallow.closeAsked()` answers `close` at once without unsaved edits, otherwise shows 009's "Leave your changes?" and answers `close` only when the engineer leaves them. `window.ipc` is absent in the browser (automation transport, other platforms): every call is guarded.
- [ ] T015 [US1] Window ready (R-14, the engineer's decision after the spike): make the engine's first answers to the dashboard faster. Measure which of the dashboard's requests take longest (`GET /api/projects` and the others the dashboard makes at start), and cut their git processes (for example the project kind and remote checks per project: cached as `project_refusal`'s kind already is, refreshed by the ref watcher). Record before and after in `specs/014-standalone-app/checks/ready.md`.

**Checkpoint**: `fallow` opens the window on the dashboard; a second `fallow` brings it forward; the theme and placement hold across restarts.

---

## Phase 4: User Story 2 - Closing the window does not stop work (P1)

**Goal**: closing the window keeps the service, its runs and its icon; Quit asks when work runs.

**Independent Test**: quickstart Q4 and Q5.

- [ ] T016 [US2] Close (R-9, data-model Window states) in `src/window/mod.rs`: open → closing on `WM_CLOSE` and the system menu's Close window: `Fallow.closeAsked()` through `evaluate_script`; on `close` (or 3 s without an answer) write `window.json`, drop the web view and release the environment; the engineer keeping their edits → open. Opening, open or closing + `Show` → bring to the front, never a second creation.
- [ ] T017 [US2] Quit (R-9, data-model Quit) in `src/window/mod.rs`: from the icon (today's "Stop" renamed "Quit"), the system menu and the pipe: the page's unsaved edits first when the window is open; the running loops and sessions asked on a worker thread; with any, `TaskDialogIndirect` (owned by the window when open) naming each and saying they pause and resume at the next start; on confirm stop as `fallow stop`; nothing running → stop at once.
- [ ] T018 [US2] `fallow quit [--installer]` in `src/main.rs` (contracts/cli.md): asks `quit`, shows the same task dialog itself when the window is closed, sends `confirmed`; `--installer` then waits until the service pid has exited and no `fallow.exe mcp-bridge` of that service remains; exit 1 when cancelled.
- [ ] T019 [US2] The end of the Windows session in `src/window/mod.rs`: `WM_QUERYENDSESSION` answers TRUE, `WM_ENDSESSION` stops the service as `fallow stop` does, no dialog (R-9).
- [ ] T020 [P] [US2] Tests: `tests/quit_list.rs` (the engine's list of running loops and sessions for `quit`, on the fake runtime: none, a running loop, a working session); a unit test that `fallow quit --installer` exits 1 when the confirmation is cancelled (the dialog behind a trait so the test answers it).

**Checkpoint**: close the window during a loop: the loop finishes, the icon stays, no web view process is left.

---

## Phase 5: User Story 3 - The window behaves like a native app (P2)

**Goal**: the system's folder picker, file dialog, drag and drop, shortcuts and notifications.

**Independent Test**: quickstart Q3's native steps.

- [ ] T021 [US3] `src/platform/picker.rs`: the folder picker owned by Fallow's window when it is open (its handle kept by the UI thread); the topmost helper window goes.
- [ ] T022 [US3] `src/service/web/screens/solo.js` (R-5, R-4): `dragover`/`drop` on the message box attach dropped image files exactly as the file dialog does (same types and limits); `app.js` cancels every other drop (`preventDefault` on `dragover` and `drop` on `document`); the attachment thumbnails open in an in-page viewer (a `<dialog>` with the image, closed with Escape or a click) instead of `target=_blank` links.
- [ ] T023 [US3] Check by hand and record in `specs/014-standalone-app/checks/native.md`: copy, cut, paste, select all, undo, redo in text fields; zoom shortcuts; reload keeps the route; the context menu has no entry that leaves the Studio in a release build; a notification click with the window closed opens it on the item.

---

## Phase 6: User Story 4 - Install and uninstall for the current user (P2)

**Goal**: a per-user installer without elevation, and an uninstaller that keeps the engineer's data
unless asked.

**Independent Test**: quickstart Q1 and Q8.

- [ ] T024 [US4] `installer/fallow.iss` (R-11): Inno Setup 6, `PrivilegesRequired=lowest`, `DefaultDirName={localappdata}\Programs\Fallow`, both executables, a Start menu shortcut to `fallow-app.exe` with `AppUserModelID: "Fallow"`, an optional task adding the folder to the user's `PATH`; before install and uninstall `fallow.exe quit --installer` (stop when it exits 1), `CloseApplications=yes`; the last page's "Launch Fallow" (on by default) starts `fallow-app.exe`; the uninstaller removes `HKCU\Software\Classes\AppUserModelId\Fallow` and the shortcut, names the data folder (`%LOCALAPPDATA%\fallow`) and the config folder, asks whether to remove the data folder (default No) saying it holds the working copies of loops and sessions not merged yet; projects untouched.
- [ ] T025 [US4] CI job `package` in `.github/workflows/ci.yml`: builds the release binaries, installs Inno Setup when missing, builds `Fallow-<version>-x64-setup.exe` and its SHA-256, uploads both as an artifact; then creates a local standard user, runs the installer silently as that user (`/VERYSILENT /CURRENTUSER`), checks it installed under that user's profile without elevation, and uninstalls it (VI, R-16).

---

## Phase 7: User Story 5 - The browser page is retired safely (P3)

**Goal**: nothing of the Studio reaches a browser or another program; the commands work as before.

**Independent Test**: quickstart Q6.

- [ ] T026 [US5] Retire the web server on Windows (R-3, FR-015, FR-022, contracts/cli.md): `serve` starts no `http::Web` unless `feature = "automation"` and `FALLOW_AUTOMATION=1`; `service.json` and stderr carry no page address (the automation transport writes its port only); `fallow autostart` is removed; `fallow status`, `stop`, `serve`, `--version` and `mcp-bridge` unchanged.
- [ ] T027 [US5] CI gate in `.github/workflows/ci.yml` and `tests/no_listener.rs` (R-16): the release build's service, window open, has no listening TCP or UDP socket in its process tree (Fallow and its `msedgewebview2.exe` processes, from the system's connection table); the string marking the `automation` feature is absent from the release `fallow.exe`.

---

## Phase 8: Polish & cross-cutting

- [ ] T028 Benchmarks in `xtask/src/bench.rs` (R-14, R-15's table): memory as private working sets summed over Fallow and its web view processes (constitution 8.0.0, Toolhelp + the private working set counter), working sets summed printed beside it; window ready with `fallow-app.exe`, service running, window closed, p95 of 30 after 3 warm-ups, one data folder; idle memory window open (< 250 MB) and closed (< 60 MB); running memory and `solo_four` with the window open on the `automation` build (< 400 MB); `solo_four` activity from the page's `activity:` log; `solo_start`, `refwatch_latency` as today.
- [ ] T029 [P] CI job `trace` (SC-007, R-6): a 10-minute `pktmon` capture with the release window open and idle; Fallow's own process opens no connection; the web view's connections are compared with research R-6's list; anything else fails.
- [ ] T030 [P] The visual checks in the real window (SC-004): `specs/013-dark-mode/checks/shots.js` (and 009's, 010's) drive WebView2 through `FALLOW_AUTOMATION_CDP` on the `automation` build instead of headless Chrome; the reference set passes in light and dark.
- [ ] T031 [P] SC-005 check in `tests/close_keeps_loop.rs` (`automation` build, Windows): open the window, start a loop on the fake runtime, close the window, the loop completes.
- [ ] T032 CI layout (R-16): separate jobs for the builds and tests, the gates, the benchmarks, the trace and the package, each under 60 minutes.
- [ ] T033 Docs: `README.md` (install, the window, Quit), release notes of what the web view cannot switch off (FR-017: required diagnostic data, Evergreen runtime updates), the quickstart checked against the build.
- [ ] T034 Quickstart Q1 to Q8 by hand with the engineer (optional).

---

## Dependencies & Execution Order

- **Setup (T001-T002)** → **Foundational (T003-T007)** → user stories.
- **US1 (T008-T015)** first: every other story needs the window. T008 and T009 can run beside T010.
- **US2 (T016-T020)** after US1. **US3 (T021-T023)** after US1, beside US2. **US4 (T024-T025)** after US1 and T018 (`quit --installer`). **US5 (T026-T027)** after US1 and T005.
- **Polish** after the stories it measures: T028 after US1 and US2; T029 after T026; T030 and T031 after T005 and US2.

## Parallel Opportunities

- T004 beside T006 and T007 (different files).
- T008, T009 beside T010.
- T020 beside T021 to T023.
- T029, T030, T031 beside each other.

## Implementation Strategy

1. **MVP**: Setup, Foundational, US1: Fallow in its own window, the browser page still available
   with the `automation` build for the tests.
2. US2 (close and Quit), then US3 (native), then US5 (no listener), then US4 (the installer).
3. Polish: the benchmarks against constitution 8.0.0, the trace, the visual checks in the window.
4. A fresh reviewer after every phase; a commit and push per phase on `main` (noreply email).
