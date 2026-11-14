# Feature Specification: Fallow as a standalone app

**Feature Branch**: `main` (the project's working branch since 2026-11-14; the spec lives in `specs/014-standalone-app`)

**Created**: 2026-11-14

**Status**: Draft

**Input**: User description: "Fallow Studio as a standalone IDE like VS Code (014). Today Fallow runs on localhost and is opened in a browser. It becomes an installable app with its own window, as allowed by constitution 7.0.0, decided by the engineer on 2026-11-14: Fallow's own desktop window only (no code editor, terminal or extensions for the engineer), one combined memory budget (Fallow plus its window's web view: < 250 MB idle, < 400 MB running, window ready < 1 s), and window only: the browser page is retired. Publishing the downloads in a public release repository is a separate feature (015)."

## Context

Today the engineer types `fallow` in a terminal, or starts the service at login. A background service does the work, and the Studio is a page on a local address that opens in the engineer's browser, with a new address and key at every start. The engineer finds Fallow among their browser tabs, bookmarks do not survive a restart, and Fallow has no icon, no entry in the start menu or the dock, and on macOS and Linux no notifications and no folder picker (008 gave Windows both).

Constitution 7.0.0 (2026-11-14) makes Fallow a standalone app: the Studio is shown only in Fallow's own window, through the operating system's web view, and is no longer offered to browsers.

**What does not change**: what the Studio shows and does (007-013), the background service that runs the loops, the rule that a run never depends on the window being open (VII), and that Fallow makes no network requests of its own (V).

**Out of scope**: Linux and macOS (006: Windows first; each gets its own feature, which brings the window there); a code editor, a terminal or extensions for the engineer (decided 2026-11-14); a public download page, any code signing and publishing releases (015); updating Fallow from inside the app (V: the engineer downloads a new version).

## Clarifications

### Session 2026-11-14

- Q: When the engineer closes Fallow's window, what happens to Fallow itself? → A: Fallow keeps running in the background with an icon in the notification area or menu bar, whether or not a run is active; the engineer reopens it from the icon or the app list and quits it from the icon's menu (or the app menu).
- Q: Which feature produces the install packages? → A: 014 builds an unsigned install package per platform in CI and is done when those install, uninstall and upgrade correctly; 015 publishes them. The engineer will not pay for an Apple Developer ID, so macOS packages stay unsigned by Apple (015 decides how users get past the system's warning and documents it).
- Q: Which install format(s) does Fallow ship for Linux? → A (deferred with Linux support, see the next-but-one answer): Two: a per-user archive whose install script puts Fallow in the user's own folders (no administrator rights) and adds it to the app list, and a `.deb` for those who prefer the system package manager (needs administrator rights, declares the web view libraries as dependencies).
- Q: Which processor types do the packages cover? → A: Windows x64, macOS on Apple Silicon, Linux x64 once each is supported. No Intel Mac, Windows on ARM or ARM Linux packages.
- Q (during /speckit-plan): What does an old browser address of the Studio show? → A: Nothing of Fallow's: Fallow is not released yet, so no one has an old address, and the window gets its pages from Fallow without any web server. An old address gets the browser's own "can't connect" page. The notice of FR-018 and US5 AS2 is dropped.
- Q (during /speckit-plan): Does Fallow start at login (FR-010)? → A: No. Dropped by the engineer: whoever wants to work in Fallow opens it from its icon, and runs resume then. No login item, no setting for it, and `fallow autostart` goes too.
- Q (during /speckit-plan): How are running memory and activity latency measured when the installed app accepts no scripted requests? → A: On a test build with a cargo feature `automation` (never in a release) that adds the test-only HTTP path, enabled by an environment variable; idle memory and window ready are measured on the release build itself.
- Q: Which platforms does 014 deliver? → A: Windows only, as 006 decided for the first version ("Windows only; Linux and macOS later"); the answers above about Linux and macOS packages apply when those platforms become supported, in their own feature. On 2026-11-14 the test suite on Linux failed 22 of the 226 tests that ran before it was stopped, and macOS was never tested after 005.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Fallow opens in its own window (Priority: P1)

The engineer starts Fallow like any other app: from the Start menu, or by typing `fallow`. Fallow's window opens with the dashboard, under Fallow's name and icon, and everything the Studio offers today works in it. Opening Fallow again while it is open brings the existing window to the front instead of opening a second one.

**Why this priority**: it is the feature. Without its own window Fallow is still a browser tab.

**Independent Test**: install Fallow on Windows, start it from the Start menu and from a terminal, walk through the screens named in 013 FR-005 (dashboard, a project with its conversation, a spec session, a loop, pipelines, agents, the data page, with their dialogs and panels), and check each works as it does in the browser today; start Fallow a second time and check the existing window comes to the front.

**Acceptance Scenarios**:

1. **Given** Fallow is installed and not running, **When** the engineer starts it from the system's app list, **Then** the service starts and Fallow's window shows the dashboard within 1 s, titled "Fallow" with Fallow's icon in the taskbar and the app switcher.
2. **Given** Fallow's window is open, **When** the engineer starts Fallow again (app list or `fallow`), **Then** no second window opens and the existing one comes to the front.
3. **Given** the service is already running with no window (started with `fallow serve`, or the window was closed), **When** the engineer starts Fallow, **Then** the window opens on the running service, and every run continues untouched.
4. **Given** the window is open, **When** the engineer resizes, moves or maximises it and later restarts Fallow, **Then** the window reopens with the same size and position on the same screen, or centred on the main screen if that screen is gone.
5. **Given** the system is set to dark, **When** Fallow's window opens, **Then** both the page and the window's own frame and title bar follow 013's theme from the first frame.

---

### User Story 2 - Closing the window does not stop work (Priority: P1)

A loop is running. The engineer closes Fallow's window. The loop keeps running, Fallow stays reachable from its icon in the notification area, and a notification still brings the engineer back to the item that needs them. Quitting Fallow is a separate, explicit action that asks before stopping running work.

**Why this priority**: runs take hours (VII). An app whose close button silently stopped them would break the core promise.

**Independent Test**: start a loop with the fake agent, close the window, check the loop advances, reopen the window from the icon and from the app list, then quit from the icon's menu and check the confirmation names the running loop; cancel, quit again and confirm, and check the service stops and the loop resumes at the next start.

**Acceptance Scenarios**:

1. **Given** a loop is running, **When** the engineer closes the window, **Then** the loop keeps running, the service keeps running, and Fallow's icon stays in the notification area.
2. **Given** the window is closed and something needs the engineer, **When** the engineer clicks the notification, **Then** Fallow's window opens on that item.
3. **Given** a run is active, **When** the engineer chooses Quit (from the icon's menu, or the window's File menu), **Then** Fallow asks for confirmation, naming every running loop and session that will pause; on confirm the service stops and the runs resume at the next start; on cancel nothing changes.
4. **Given** no run is active, **When** the engineer chooses Quit, **Then** Fallow quits without asking.
5. **Given** the window is closed, **When** the engineer chooses Open Fallow from the icon's menu, **Then** the window opens where it was.

---

### User Story 3 - The window behaves like a native app (Priority: P2)

The engineer adds a project with the system's own folder picker, attaches images from the system's file dialog or by dragging them in, copies and pastes with the usual shortcuts, and gets Fallow's notifications from Windows. Links to pull requests and other web pages open in the engineer's own browser, never inside Fallow.

**Why this priority**: the window works without these on day one through the existing fallbacks (typing a path, the notification icon 008 added), but a standalone app is expected to use the system's own dialogs, drag and drop and notifications.

**Independent Test**: on Windows, add a project through the folder picker, attach an image in a solo session by dialog and by drag and drop, copy text out of the activity and paste a message, trigger a "needs you" item with the window closed, and click a pull request link; check each step uses the system's own dialog or notification and that the link opens in the default browser.

**Acceptance Scenarios**:

1. **Given** the dashboard, **When** the engineer clicks + to add a project, **Then** the system's folder picker opens, and the chosen folder is added as today; typing a path stays possible.
2. **Given** a solo session, **When** the engineer attaches images by the file dialog or drags image files onto the message box, **Then** they are attached as in the browser today.
3. **Given** the window, **When** the engineer uses the system's shortcuts for copy, cut, paste, select all, undo and redo in a text field, **Then** they work as in any native app.
4. **Given** a link to a pull request, a check result or any other web address, **When** the engineer clicks it, **Then** it opens in the engineer's default browser and Fallow's window stays on the Studio.
5. **Given** notifications are on, **When** something needs the engineer, **Then** Windows shows its own notification, naming the project, the feature and what is needed and nothing else (V), and clicking it opens the item.

---

### User Story 4 - Install and uninstall for the current user (Priority: P2)

The engineer installs Fallow from its Windows install package, without an administrator password, and finds it in the system's app list. Uninstalling removes the app and leaves the engineer's projects untouched, and tells them where Fallow's own data is if they want to delete it too.

**Why this priority**: a standalone app has to be installable to be standalone; the public download page that hands out these packages is 015.

**Independent Test**: as a standard (non-administrator) Windows user, install the package, start Fallow from the Start menu, uninstall it, and check the app list entry and shortcuts are gone while the project folders and (unless chosen) Fallow's data folder remain.

**Acceptance Scenarios**:

1. **Given** a standard user without administrator rights, **When** they install Fallow's Windows package, **Then** the install completes without an elevation prompt and Fallow appears in the system's app list with its name and icon.
2. *(Dropped during planning: Fallow does not start at login.)*
3. **Given** Fallow is installed, **When** the engineer uninstalls it, **Then** the app and its app list entry are removed; project folders are untouched; Fallow's data folder is kept unless the engineer chose to remove it, and the uninstaller names the folder.
4. **Given** an older Fallow is installed and running, **When** the engineer installs a newer version, **Then** the installer asks to quit Fallow first (with the same confirmation as Quit), and after the install the runs resume in the new version.

---

### User Story 5 - The browser page is retired safely (Priority: P3)

Typing `fallow` now opens the window, and nothing of the Studio is served to a browser any more. The terminal commands keep working. (Fallow is not released yet, so no one has an old address to show a notice on: clarified during planning.)

**Why this priority**: few engineers use Fallow today, but constitution V forbids serving the Studio to a browser, so the old path must be closed rather than left half open.

**Independent Test**: with the new version running, check that Fallow listens on no network port, and that a browser or a script can reach nothing of the Studio, with or without an old key; run `fallow status`, `fallow stop` and `fallow serve` and check they work as before.

**Acceptance Scenarios**:

1. **Given** the new version, **When** the engineer types `fallow` in a terminal, **Then** the window opens (or comes to the front) and no browser tab opens.
2. *(Dropped during planning: Fallow is not released, so there is no old tab or bookmark; an old address reaches nothing.)*
3. **Given** the new version, **When** another program on the machine requests the Studio's address with or without a key, **Then** it gets no Studio page and no data.
4. **Given** a machine without a display (a server, or a session over SSH), **When** the engineer runs `fallow serve`, **Then** the service runs and its loops continue; `fallow` without a command explains that the Studio needs Fallow's window and a display.

---

### Edge Cases

- **The system's web view is missing or too old** (a Windows machine without the WebView2 runtime): Fallow MUST say so in a native message naming what to install, and MUST NOT fall back to a browser.
- **The window's page fails to load or the web view crashes**: the window shows a native message with a Reload button; the service and its runs are unaffected.
- **The service is not running but a stale lock or address is left from a crash**: starting Fallow detects it, starts a fresh service and opens the window, as `fallow` does today.
- **Two users on one machine**: each has their own Fallow service, window and data; one user's window never connects to another user's service.
- **The window is on a screen that is later disconnected**: it reopens on the main screen (US1 AS4).
- **Notifications are turned off in the system for Fallow**: Fallow's "Needs you" count and inbox still show everything; nothing else depends on the notification.
- **The engineer presses the reload shortcut or the page navigates in error**: the Studio reloads in place and keeps what 013 and earlier features keep across reloads; it never shows another site.
- **A link opens a new window** (a `target=_blank` link, a script): it opens in the default browser, never as a second Fallow window.
- **Developer tools**: not available in a released build; available in a development build only.
- **The service runs and the engineer never opens the window** (`fallow serve`, or after closing it): memory stays within the idle budget of the service alone, and the window's web view is started only when the window opens.

## Requirements *(mandatory)*

### Functional Requirements

**The window**

- **FR-001**: Fallow MUST show the Studio in its own window, using the operating system's web view, on Windows. The window MUST carry Fallow's name and icon in the title bar, the taskbar and the app switcher.
- **FR-002**: Fallow MUST keep at most one window per user. Starting Fallow while it runs MUST bring the existing window to the front, or open it if it was closed.
- **FR-003**: The window MUST remember its size, position, screen and maximised state across restarts, and reopen centred on the main screen when the remembered screen is gone.
- **FR-004**: The window's frame and title bar MUST follow 013's theme choice (Light, Dark, System) from the first frame, with no light flash.
- **FR-005**: Every screen, dialog and state of the Studio (007-013) MUST work in the window as it does in the browser today, including 013's theme, the chat panels' width setting and every keyboard shortcut the Studio defines.
- **FR-006**: The window MUST NOT navigate away from the Studio. Any link to another address, and any request to open a new window, MUST open in the engineer's default browser, only on the engineer's click.
- **FR-007**: The window MUST support the system's standard editing shortcuts (copy, cut, paste, select all, undo, redo) in text fields and the system's zoom shortcuts, and MUST disable developer tools and the web view's own context menu entries that leave the Studio (for example "open in new window") in released builds.

**Running in the background**

- **FR-008**: Closing the window MUST NOT stop the service or any run, whether or not a run is active; only Quit (FR-009) stops Fallow. While the service runs without a window, Fallow MUST show an icon in the notification area with Open Fallow, Notifications on/off and Quit.
- **FR-009**: Quit MUST stop the service only after a confirmation that names every running loop and session, when any is running (constitution II, VII); without running work Quit MUST NOT ask. Runs paused by Quit MUST resume at the next start, as `fallow stop` does today.
- **FR-010**: *(Dropped during planning, 2026-11-14: Fallow does not start at login; the engineer opens it when they want to work in it.)*
- **FR-011**: The window's web view MUST run only while the window is open. With the window closed, Fallow's memory MUST return to within the idle budget measured for the service alone.

**Native integration**

- **FR-012**: Adding a project MUST open the system's folder picker; typing a path MUST stay possible.
- **FR-013**: Attaching images MUST work through the system's file dialog and by dragging files onto the message box, with the same types and limits as today.
- **FR-014**: Fallow's notifications (007 FR-003a) MUST be shown by Windows, with the same content rule (project, feature, what is needed; never code or output), and clicking one MUST open the window on its item. The existing on/off switch MUST keep working.

**Privacy and security (constitution V, VI)**

- **FR-015**: The Studio MUST be reachable only from Fallow's own window. Whatever carries it to the window MUST reject every other client: other programs, browsers and other users on the machine, with or without a key from an earlier version.
- **FR-016**: Fallow MUST NOT pass, show, log or write to disk any secret the window uses to reach the service.
- **FR-017**: The window MUST load nothing from outside Fallow. The web view's own network features (safe-browsing lookups, spell-check services, telemetry, update checks) MUST be switched off where the platform allows; what a platform does not allow switching off MUST be listed per platform in the plan and in the release notes.
- **FR-018**: *(Dropped during planning, 2026-11-14: Fallow is not released, so no one has an old address; with no web server, an old address reaches nothing of Fallow's.)*

**Install and uninstall**

- **FR-019**: Fallow MUST be packaged for Windows x64 as an install that a standard user can complete without elevated privileges, and that adds Fallow to the Start menu with its name and icon.
- **FR-020**: Uninstalling MUST remove the app, its app list entry, and its notification identity, MUST NOT touch project folders, and MUST keep Fallow's data folder unless the engineer chooses to remove it; it MUST name that folder.
- **FR-021**: Installing a newer version over a running one MUST ask to quit Fallow first, with the Quit confirmation of FR-009; runs MUST resume in the new version.

**Compatibility**

- **FR-022**: `fallow` with no command MUST open or focus the window and MUST NOT open a browser. `fallow status`, `fallow stop`, `fallow serve`, `fallow --version` and the agents' bridge MUST keep working as today. `fallow autostart` is removed (FR-010 dropped).
- **FR-023**: Without a display, `fallow serve` MUST run the service as today, and `fallow` with no command MUST explain that the Studio needs Fallow's window and a display, and exit with an error.
- **FR-024**: When the system's web view is missing or unusable, Fallow MUST show a native message naming what to install, and MUST NOT fall back to a browser.

**Budgets (constitution VIII)**

- **FR-025**: Fallow MUST meet the 7.0.0 budgets on the installed app on Windows, measured by CI benchmarks that fail the build: idle memory < 250 MB and running memory < 400 MB (Fallow's process plus its window's web view processes, window open), window ready < 1 s (launch to the Studio drawn in the window, p95 of 30 launches after 3 warm-ups), and activity latency < 2 s.

### Key Entities

- **App window**: Fallow's one window per user. Holds its size, position, screen and maximised state, kept in Fallow's own settings on the machine.
- **Background icon**: the notification-area or menu-bar presence while the service runs; offers Open Fallow, Notifications on/off and Quit.
- **Install package**: the Windows x64 package; installs for the current user, registers Fallow's name and icon, and is removed by the uninstaller.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On Windows, a standard user installs Fallow and has its window open on the dashboard within 2 minutes of starting the installer, without an administrator prompt.
- **SC-002**: Fallow's window shows the Studio within 1 s of launch (p95 of 30 launches after 3 warm-ups) on Windows.
- **SC-003**: Fallow, counting its window, stays under 250 MB when idle and 400 MB during the reference run, on Windows.
- **SC-004**: 100% of the screens and states listed in 013 FR-005 pass their existing acceptance checks inside the window.
- **SC-005**: A loop started before the window is closed completes with the window closed, in 100% of runs of the CT check.
- **SC-006**: No request from outside Fallow's window (browser, script, another user, with or without an old key) receives any Studio page or data, in 100% of the CI checks.
- **SC-007**: Fallow's own process opens no outbound network connection while the window is open and idle for 10 minutes on Windows (the web view's unavoidable traffic, if any, listed per FR-017).
- **SC-008**: After the engineer quits Fallow from the window and restarts it, every paused run resumes without repeating a completed step, in 100% of runs.

## Assumptions

- The window uses Windows' built-in web view (WebView2), present on current Windows 10 and 11, so the app does not ship a browser engine.
- The background service and the window are both part of the installed Fallow; the window is a client of the service and is the only client the Studio accepts.
- Window state and the theme are kept with Fallow's other settings on the machine (as 013 keeps the theme).
- Supported platform: Windows 10 and 11 on x64. Linux and macOS follow in their own features (Clarifications).
- The Windows package is produced, unsigned, by CI in 014; publishing it for download is 015, which also decides whether it gets signed. No Apple Developer ID will be bought, which matters once macOS is supported.
- The project's dependency policy still bans window and web view libraries under the retired terminal-only rule; the plan lifts only what the window needs, justified against constitution V, VI and VIII.
