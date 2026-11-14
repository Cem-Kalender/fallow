# Research: Fallow as a standalone app (014)

Decisions taken during `/speckit-plan`, 2026-11-14, revised after an independent review of the
plan the same day. Facts about WebView2 and `wry` were checked against their sources on that day
(links per decision). What could not be confirmed is marked **to measure**: it is settled by the
spike (R-20) or a check in the tasks, never assumed.

## R-1 The window: `wry` on Fallow's own window

**Decision**: `wry` 0.57 (MSRV 1.85, Fallow's own), built with `build_as_child` into a window
Fallow creates itself with `windows-sys`. No `tao` and no `winit`: the window lives on Fallow's own
UI thread (R-2). The default MSVC target links the WebView2 loader statically, so no
`WebView2Loader.dll` ships. Fallow creates the WebView2 environment itself (through `webview2-com`,
which `wry` brings) to set the options `wry` does not expose (R-6), and hands it to `wry`
(`WebViewBuilderExtWindows::with_environment`; **to measure** in the spike that this is accepted).

**Rationale**: WebView2 is a COM API. `windows-sys` has no COM interfaces and the WebView2
interfaces are not in the Win32 metadata, so without a library Fallow would hand-write about
fifteen vtables, their callback objects and the custom-scheme plumbing. `wry` is that library,
maintained by the Tauri project: a custom scheme with request bodies and delayed answers,
navigation and new-window handlers, IPC, theme, background colour, browser arguments and the user
data folder.

**Cost** (Development Workflow: a dependency is justified against V, VI, VIII):
- **V**: `wry` makes no network request of its own; what the web view does is R-6.
- **VI**: no privilege; WebView2's user data goes in Fallow's data folder (R-6).
- **VIII**: the web view's processes dominate memory (R-20); `wry` itself adds little at run time.
  It brings the `windows` and `webview2-com` crates, heavy to compile: build time, binary size and
  the target folder's size are recorded by the spike (C: is nearly full on the engineer's machine).
- **`deny.toml`**: `wry` comes off the ban list; `tao`, `winit`, `gtk` and `webkit2gtk` stay. The
  graph is restricted to `x86_64-pc-windows-msvc` (`[graph] targets`), since `wry` is a
  Windows-only dependency and its Linux dependencies would otherwise trip the bans.

**Alternatives considered**: `webview2-com` alone (saves `http`, `dpi`, `dunce` for 1 to 2
thousand lines of glue); hand-written COM (see the rationale); the `webview2` crate (abandoned,
2021); Tauri (a framework); `tao` + `wry` (a second event loop nothing needs).

Sources: https://docs.rs/crate/wry/latest, https://github.com/tauri-apps/wry (src/lib.rs,
src/webview2/mod.rs), https://github.com/wravery/webview2-rs.

## R-2 One process: the window lives in the service, on its own UI thread

**Decision**: the window is part of the service (`fallow serve`). A **UI thread** owns the window
and the notification icon:
- it calls `OleInitialize` first (a single-threaded apartment, and OLE drag and drop for WebView2);
- it runs the Win32 message loop that `platform::notify` runs today; the icon becomes one of its
  parts and no longer decides whether the thread lives. When the icon cannot be added (Explorer's
  taskbar not ready yet), the thread and the window still work, and the icon is added on the
  `TaskbarCreated` message;
- other threads reach it only by posting messages (`PostMessageW` to its hidden window): show or
  raise (with an optional route), quit, theme;
- WebView2's creation pumps messages while the window is **opening**: a second show, an icon menu
  or a balloon click arriving then only brings the window forward; it never starts a second
  creation (data-model, Window states);
- the UI thread never waits on the engine. Quit's list of running work is asked on a worker thread
  and posted back.

`fallow` (and the app launcher, R-18) start the service if needed, call
`AllowSetForegroundWindow(<service pid>)` so the service may bring its window to the front, then
send `{"cmd":"window"}` over the per-user pipe (contracts/window.md). One service per user (006)
gives one window per user (FR-002).

**Rationale**: the engine is single-threaded and owns all state; in the same process the window's
requests reach it through the channel the page's requests use today, with no transport to secure.

**Alternatives considered**: a window process talking to the service over the pipe (a second
process, and the pipe is reachable by every program of the user).

## R-3 The page reaches Fallow without a web server

**Decision**: the window loads `http://fallow.localhost/` through `wry`'s custom scheme
(`with_asynchronous_custom_protocol("fallow", …)`). Every request is answered inside Fallow's
process; nothing listens on any port.
- `src/service/http.rs` is split: a transport-neutral `answer(Request) -> Response` (method, path,
  query, headers, body in; status, headers, body out) in `src/service/answer.rs`, and the
  transports in front of it.
- The window's transport runs `answer` on a worker thread per request and hands the result to the
  responder, so the 25 s long poll of `/api/events` blocks no one.
- No secret, cookie or origin check: only the window can make requests to its own scheme. The
  content security policy stays (`default-src 'self'; script-src 'self'; img-src 'self' data:;
  connect-src 'self'`), so the page still runs only Fallow's own scripts.
- **Image uploads** send `await file.arrayBuffer()` instead of the `File` itself: WebView2 is
  reported to hand file-backed request bodies to the host empty. A 5 MiB upload through the scheme
  is a test.
- **`tiny_http`** stays as a second transport in front of the same `answer`:
  - on Windows only in the `automation` build (R-15);
  - on Linux and macOS as today (their page in the browser), until their own feature brings the
    window there (006: Windows first). So `tiny_http` is a dependency for `cfg(not(windows))` and
    for the `automation` feature.

**Rationale**: no port means FR-015 (only the window reaches the Studio) and FR-016 (no secret to
leak) hold by construction.

**Alternatives considered**: `tiny_http` on loopback with a secret handed to the window (a
listener every program can try); the `https` form of the scheme (nothing needs it).

Old addresses (FR-018): dropped by the engineer during planning. Fallow is not released, so no one
has an old address; with no listener, an old address reaches nothing.

## R-4 Links, new windows, downloads

**Decision**: one decision function, unit-tested, for every navigation and new-window request:
- `http://fallow.localhost/…`:
  - as a navigation: loads in the window;
  - as a new window: never a window and never the browser (`http://fallow.localhost` in a browser
    is loopback port 80, which is not Fallow). The page does not ask for one: the attachment
    thumbnails (`solo.js`), today links with `target=_blank`, open in an in-page viewer instead.
- `https://…` and `http://…` (other hosts), **only when the request is user-initiated**
  (`IsUserInitiated` on `NavigationStarting` / `NewWindowRequested`): cancelled in the window and
  opened in the default browser;
- anything else (`file:`, UNC paths, `ms-*:`, `search-ms:`, `javascript:`, …), or a request not
  user-initiated: dropped, nothing opened. `platform::open_browser` is only ever given an `http(s)`
  address.
- **Downloads**: refused.
- **Context menu**: the web view's menu stays for text (cut, copy, paste, select all) with the
  entries that leave the Studio removed in release builds ("open link in new window", "save as",
  "print", "inspect"), through `ContextMenuRequested` on the controller.
- **Developer tools**: off in release builds; on in debug and `automation` builds.

## R-5 Native integration

- **Folder picker** (FR-012): already native (008, `platform::picker`, `POST /api/projects/pick`);
  it now opens owned by Fallow's window (its handle kept by the UI thread), and the topmost helper
  window it uses today goes.
- **Images by dialog** (FR-013): the page's `<input type=file>` opens the system's file dialog in
  WebView2.
- **Images by drag and drop** (FR-013): the page has no drop handling today. 014 adds it: `dragover`
  and `drop` on the solo message box attach image files as the file dialog does; everywhere else
  the page cancels drops (`preventDefault`), so a dropped file never becomes a navigation. `wry`'s
  drop handler is not used (on Windows it disables external drops).
- **Shortcuts** (FR-007): WebView2 handles the editing and zoom shortcuts
  (`with_browser_accelerator_keys(true)`); reload reloads the Studio in place.
- **Notifications** (FR-014): already Windows' own (008: the icon's balloons, shown by Windows under
  Fallow's identity). A click now opens the window on the item.

## R-6 What the web view sends, and what Fallow can switch off

**Decision**:
- **Environment**: before creating it, the service clears `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`,
  `WEBVIEW2_USER_DATA_FOLDER`, `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` and
  `WEBVIEW2_RELEASE_CHANNEL_PREFERENCE` from its own environment: inherited from a shell, they could
  add a debugging port (a listener exposing the Studio) or another runtime.
- **User data folder**: `<data>/webview`. The data folder's user-only ACL may keep WebView2's
  sandboxed processes out: **to measure** in the spike; if so, `webview/` gets the ACEs WebView2
  documents, still for the current user only.
- **Browser arguments**: `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` (`wry`'s
  default, kept: passing arguments replaces it).
- **SmartScreen**: also `ICoreWebView2Settings8::IsReputationCheckingRequired = false`.
- **Crash reports**: `ICoreWebView2EnvironmentOptions3::IsCustomCrashReportingEnabled = true` (the
  environment Fallow creates, R-1), so no minidump goes to Microsoft.
- **Spell-check**: no WebView2 switch; whether it uses a network service is **to measure** by the
  SC-007 check.

**Cannot be switched off by Fallow (listed in the release notes, FR-017)**:
- Windows' required diagnostic data from the WebView2 runtime (optional data follows Windows'
  own setting);
- updates of the Evergreen WebView2 runtime through Microsoft Edge Update.

**Check (SC-007)**: a 10-minute network trace (`pktmon`, so short connections are not missed) with
the window open and idle: Fallow's own process opens no connection; the web view's processes open
only what this list names, or the check fails.

Sources: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy,
https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags.

## R-7 The theme from the first frame (FR-004)

**Decision**: the theme is 013's (`prefs.json` in the data folder) and, for System, the user's
`AppsUseLightTheme`:
- the window is made with `DWMWA_USE_IMMERSIVE_DARK_MODE` set and a class background brush of 013's
  page background for that theme, so the frame and the area before WebView2's first paint are
  already right; the web view gets the same `with_background_color` and `with_theme`;
- no Win32 menu bar (it cannot follow dark mode): Close and Quit are in the window's system menu
  (title bar icon, Alt+Space: "Close window", "Quit Fallow") and in the icon's menu. This replaces
  the "File menu" of spec US2 AS3, which names it only as one place Quit is found;
- the page tells the window of a theme change over IPC (`theme:…`); with System,
  `WM_SETTINGCHANGE` ("ImmersiveColorSet") updates the frame.

## R-8 Window state and screens (FR-003)

**Decision**:
- `window.json` in the data folder: the restored rectangle from `GetWindowPlacement`, and
  `maximized`. On open, used when `MonitorFromRect(…, MONITOR_DEFAULTTONULL)` finds a screen;
  otherwise 1280 × 800 fitted to the main screen's work area, centred.
- **DPI**: the executables carry an application manifest (R-17) declaring per-monitor DPI
  awareness (v2); the window handles `WM_DPICHANGED` (resizing to the suggested rectangle) and the
  web view follows the window's scale. Rectangles are kept in physical pixels, as
  `GetWindowPlacement` gives them.

## R-9 Close, Quit, the end of a Windows session (FR-008, FR-009, FR-011)

**Decision**:
- **Unsaved edits first**: 009's "Leave your changes?" (`beforeunload`) does not run when a web view
  is closed by its host. So close and Quit ask the page: the window calls the page's
  `Fallow.closeAsked()`; a page without unsaved edits answers `close` over IPC at once; one with
  them shows its own confirmation and answers `close` or nothing. A page that does not answer
  within 3 s (hung or crashed) is closed anyway.
- **Closing the window**: after the page's `close`, the window's state is written, the web view
  dropped and the WebView2 environment released, so its processes end (FR-011: the web view runs
  only while the window is open). The service, its runs and the icon stay.
- **The icon's menu**: Open Fallow, Notifications on/off, Quit (today's Stop, renamed).
- **Quit** (icon, system menu, `fallow quit`): the page's unsaved edits as above (when the window
  is open); then, when loops or sessions are working, a native confirmation (`TaskDialogIndirect`,
  owned by the window when open) names each, saying they pause and resume at the next start; on
  confirm, the service stops as `fallow stop` does. Without running work, no confirmation.
- **The end of the Windows session** (`WM_QUERYENDSESSION` / `WM_ENDSESSION`: sign-out, shutdown,
  the installer's Restart Manager): no dialog; the service stops cleanly, as `fallow stop` does
  (runs resume at the next start, VII).
- **Process count**: that the web view's processes are gone after a close is checked by the
  idle-after-close benchmark (R-15).

## R-10 No start at login (FR-010 dropped)

The engineer dropped FR-010 during planning: whoever wants to work in Fallow opens it from its
icon, and runs resume then (VII). Fallow writes no login item, has no setting for one, and `fallow
autostart` is removed.

## R-11 The install package (FR-019 to FR-021)

**Decision**: an Inno Setup 6 installer, built in CI from `installer/fallow.iss`:
- `PrivilegesRequired=lowest`: installs for the current user into
  `%LOCALAPPDATA%\Programs\Fallow`, no elevation prompt (VI);
- ships `fallow.exe` (the command line) and `fallow-app.exe` (R-18); a Start menu shortcut to
  `fallow-app.exe` carrying the app identity `Fallow` (008's), so notifications and the taskbar
  group under Fallow; adding `fallow.exe`'s folder to the user's `PATH` is an installer option;
- **before installing over a running Fallow, and before uninstalling**: the installer runs the
  installed `fallow.exe quit --installer`, which shows the Quit confirmation of FR-009 and **waits
  until the service process has exited** (and the agents' `mcp-bridge` processes with it); if the
  engineer cancels, the installer stops. `CloseApplications` is set as a second guard against
  locked files;
- **after an upgrade**: the installer's last page offers "Launch Fallow" (on by default), and runs
  resume in the new version when it starts (FR-021);
- **uninstall**: removes the app, its shortcut and the notification identity key;
  never touches projects. It names Fallow's data folder and config folder and asks whether to remove
  the data folder (default No), saying that it also holds the working copies of loops and sessions
  not merged yet; the config folder (the engineer's definitions) is kept and named.
- Unsigned (015 decides signing).

**Rationale**: free, a single `.exe`, per-user installs without elevation, `AppUserModelID` on
shortcuts, scripted steps, no runtime. Whether it is preinstalled on the `windows-2025` runner is
**to measure**; the CI step installs it when missing.

**Alternatives considered**: MSIX (cannot install unsigned outside developer mode); WiX MSI (much
heavier); NSIS (similar, weaker per-user defaults); a zip (no Start menu entry, no uninstaller).

## R-12 No WebView2 runtime (FR-024)

**Decision**: before making the window, Fallow asks for the runtime's version
(`GetAvailableCoreWebView2BrowserVersionString`, through `wry::webview_version`). Without one, a
native message names "Microsoft Edge WebView2 Runtime" and offers to open Microsoft's download page
in the default browser on the engineer's click; `{"cmd":"window"}` answers with that error. No
browser fallback. The runtime is part of Windows 11 and present on nearly all Windows 10 machines.

Source: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution.

## R-13 No display (FR-023)

**Decision**: `fallow` with no command checks that it runs on the interactive window station
(`GetProcessWindowStation` named `WinSta0`) before asking for a window; over SSH or as a Windows
service it is not, and `fallow` says the Studio needs Fallow's window and a desktop, and exits 1.
A service itself started without a desktop answers `{"cmd":"window"}` with "Fallow was started
without a desktop: stop it (`fallow stop`) and start it again from the desktop". `fallow serve`
runs as today.

## R-14 Window ready (FR-025, SC-002)

**Decision**:
- "Drawn" is the page's first render after `start()`: `app.js` posts `ready` over IPC. With
  `FALLOW_BENCH` set, the service logs `window ready <ms>` measured from the launcher's start time,
  which the launcher passes in its `window` request.
- **The measured launch**: `fallow-app.exe` with the service already running and the window closed
  (the everyday case: Fallow stays in the background, FR-008), p95 of 30 launches after 3 warm-ups,
  one data folder for all 33, so WebView2's profile exists after the first. A cold start of the
  service as well is recorded but not budgeted.
- Both launchers accept `--data-dir` and `--config-dir`, so the bench never uses the engineer's
  data. The service is one per user (the pipe), so the bench runs only where no other Fallow runs,
  as today.
- WebView2's cold start is the main risk to 1 s (Microsoft gives no figure): **to measure** in the
  spike (R-20).

## R-15 Benchmarks and tests without a listener

**Decision** (the engineer, during planning):
- **Release build**: idle memory with the window open, idle after closing the window, and window
  ready are measured on it.
- **`automation` build** (cargo feature, never in a release; CI checks its marker string is absent
  from the release binary):
  - adds the `tiny_http` transport in front of the same `answer`, on loopback with a per-launch
    secret, only when `FALLOW_AUTOMATION=1`; the bench gets the address and secret over the pipe
    (`{"cmd":"url"}`, kept only in this build); stderr carries the port only, never the secret;
  - lets WebView2 open a debugging port on loopback when `FALLOW_AUTOMATION_CDP=<port>` is set, so
    checks drive **the real window** (SC-004);
  - running memory and activity latency are measured on it with the real window open: the bench
    drives the reference run and the four sessions over the transport, and the page reports each
    activity line it draws (`activity:<session>` over IPC, logged with `FALLOW_BENCH`).
- **Tests**:
  - integration tests keep using the engine directly;
  - the page's route tests call `answer` directly, without a socket;
  - the tests that use the `tiny_http` transport (`service_http`, `us11_*`, `us12_*`, `visual_*`,
    `web_theme`) get `required-features = ["automation"]`; CI runs `cargo test --features
    automation` and `cargo clippy --all-features`, so none silently stops running;
  - the visual checks of 009, 010 and 013 run in the real window over the debugging port
    (SC-004), not in headless Chrome;
  - SC-005: an automated check on the `automation` build opens the window, starts a loop with the
    fake agent, closes the window, and checks the loop completes.
- **Memory**: Fallow's process plus every `msedgewebview2.exe` whose parent chain leads to it
  (Toolhelp snapshot), working sets summed (constitution VIII, Budgets: RSS). Shared pages are
  counted in each process, so the sum overstates; the private working set is recorded beside it.

**The benchmark lines after 014** (constitution VIII):

| Line | Build | Budget | Today |
|---|---|---|---|
| window ready | release | < 1 s (p95) | page ready < 500 ms |
| idle memory, window open | release | < 250 MB (Fallow + web view) | idle < 60 MB (service) |
| idle memory, window closed | release | < 60 MB (the service alone, as today) | — |
| running memory, window open | automation | < 400 MB | running < 200 MB (service) |
| solo_four memory | automation | < 400 MB, window open | < 200 MB (service) |
| solo_four activity | automation | < 2 s (p95), drawn in the window | < 2 s (view) |
| solo_start | automation | < 5 s (012 SC-001) | same |
| refwatch_latency | automation | < 5 s (012 SC-004) | same |

## R-16 CI gates and jobs

- **No listener**: the release build, window open, listens on no TCP or UDP port, neither
  `fallow.exe` nor its web view processes (the system's connection table, by process tree).
- **The window stays on the Studio** (V): the R-4 decision function is unit-tested; the release
  CSP is checked in the served `index.html`.
- **SC-007**: R-6's 10-minute trace.
- **VI, the per-user install**: hosted runners run elevated with UAC off, so the job creates a local
  standard user and runs the installer as that user (a generated password kept in the job only);
  the check is that it completes and installs under that user's profile.
- **The `automation` feature is absent** from the release binary (its marker string).
- **Jobs**: the build, gates and tests (release and `automation` builds), the benchmarks, the
  SC-007 trace and the package are separate jobs, so none exceeds the 60-minute limit.
- The package job builds `Fallow-<version>-x64-setup.exe` and its SHA-256 on every push to `main`;
  publishing is 015.

## R-17 The application manifest

**Decision**: `fallow.exe` and `fallow-app.exe` carry an application manifest (added to the
resources `assets/icon/make_res.py` builds today): Common Controls 6.0 (the task dialog), per-monitor
DPI awareness v2, the supported OS list (Windows 10 and 11), `asInvoker`. Without it,
`TaskDialogIndirect` is missing from the loaded `comctl32` and the window is bitmap-scaled on scaled
screens.

## R-18 No console window: `fallow-app.exe`

**Decision**: `fallow.exe` stays a console program (the command line). A second small binary,
`fallow-app.exe` (Windows GUI subsystem, same crate), is what the Start menu and the installer
start:
- it does what `fallow` does (start the service if needed, then the window); an error is shown as a
  native message instead of on a console;
- the service it starts runs without a console (`CREATE_NO_WINDOW`, as `ensure_running` does
  today), so no console can be closed to stop it (VII).

**Alternatives considered**: making `fallow.exe` itself a GUI program (the command line would
lose its output); re-spawning with `CREATE_NO_WINDOW` from a console `fallow.exe` (a console still
flashes on every Start menu launch).

## R-19 The constitution

014 needs no amendment for writes: with FR-010 dropped it adds no write place. One deviation stays
recorded in the plan's Complexity Tracking: running memory and activity latency are measured on the
`automation` build with the real window, where VIII says "on the installed app" (the engineer's
choice). 008's notification identity key, a write outside VI's list from before 014, is noted for
the engineer, not changed by 014.

## R-20 The spike before tasks

Before `/speckit-tasks`, a throwaway spike (`spikes/014-window`, outside the workspace like the
earlier spikes) measures on this machine and in CI: WebView2 memory for the Studio's dashboard
(sum and private), window ready with a running service, the processes left after the web view is
dropped, `with_environment` with Fallow's own options, the user data folder under the data folder's
ACL, a 5 MiB upload through the custom scheme, and the build's time, binary size and target folder
size. If idle memory is over 250 MB, the levers in order: `MemoryUsageTargetLevel = Low`
(`set_memory_usage_level`), `--renderer-process-limit=1`, then `--disable-gpu`; if still over, the
budget is the engineer's decision before tasks.
