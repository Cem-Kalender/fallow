# Feature Specification: Fallow as a desktop app on macOS, released on GitHub

**Feature Branch**: `main` (the project's working branch since 2026-11-14; the spec lives in `specs/016-macos-app`)

**Created**: 2026-11-22

**Status**: Draft

**Input**: User description:

> "Have the desktop version working on mac too, and eventually released on github. Is that possible without having to pay apple every year?"
>
> The engineer's decision (2026-11-22), answering the proposal to ship it the way the Windows installer ships (unsigned, with the system's warning documented), with Apple's paid notarization left as a later step that changes nothing else: "yes".

## Context

014 made Fallow a standalone app on Windows: the Studio is shown only in Fallow's own window, through the system's web view; closing the window leaves Fallow running behind an icon; the system's folder picker, file dialogs and notifications are used; and a per-user installer is built by CI and published on GitHub Releases from a version tag, unsigned, with its SHA-256 beside it.

On macOS, Fallow builds and passes its tests (since 2026-11-17), but `fallow` opens the Studio as a page in the engineer's browser, on a local address with a secret per launch. It has no window, no menu bar icon, no folder picker, no notifications, no app in Applications or the Dock, and nothing to download. Serving the Studio to a browser is what constitution V (since 7.0.0) forbids for a released Fallow; on macOS it stands only because nothing has been released there.

014's clarifications already settled for macOS: the background icon lives in the menu bar; no Apple Developer ID will be bought, so macOS packages are not signed by Apple, and how users get past the system's warning is decided and documented when macOS is published (this feature).

**Why no paid Apple membership is needed**: macOS runs apps that Apple has not notarized once the user allows them in System Settings, the first time only. Apps on Apple silicon must carry a signature, but a signature made without a certificate (ad hoc) satisfies that. Files downloaded by a command-line tool rather than a browser are not marked as downloaded, so the system does not ask at all.

**What does not change**: what the Studio shows and does (007-015), the background service and its runs, the rule that a run never depends on the window (VII), that Fallow makes no network requests of its own (V), and the Windows app and its release.

**Out of scope**: Apple notarization and a Developer ID (this feature only keeps the door open, FR-024); the Mac App Store; Homebrew (the official cask collection no longer accepts apps the system's check rejects; a personal tap may follow in its own feature); Linux; updating Fallow from inside the app (V).

## Clarifications

### Session 2026-11-22

- Q: Must the macOS release be possible without the yearly Apple Developer Program? → A: Yes. The app is signed ad hoc, not notarized; the first-launch step is documented as the Windows SmartScreen step is; notarization can be added later without other changes.
- Q: What happens to the browser page on macOS? → A: It is retired from released builds, as 014 retired it on Windows (constitution V). It stays available in development and test builds, which are never released.
- Q: Does the Mac version get its own window like the Windows version? → A: Yes: one native Fallow window per user, as on Windows (FR-001 to FR-007); a released build never uses the browser.
- Q: Which Macs get a download? → A: Apple silicon only, as 014 decided. Intel Macs can build from source; GitHub's Intel Mac build machines are being retired, so an Intel package could not stay tested.
- Q: Planning found four changes beyond the spec; are they in? → A: Yes, all four (confirmed 2026-11-22): Fallow's data folder on macOS moves out of the folder macOS may empty (FR-032); the chat panels' width is kept with Fallow's other settings on every platform (FR-033); links open through the page on Windows too (FR-034); on the drag-and-drop route Fallow cannot ask before it is replaced, so that route says to quit first (US4 AS6).
- Q: What does "macOS shows a notification" mean by default? → A: Measured during planning: an app signed ad hoc gets quiet (provisional) permission without a prompt, so by default Fallow's notifications go to Notification Center without a banner. The engineer turns banners on from Fallow's notifications setting ("Show banners"), which brings macOS's prompt. This differs from Windows, where a toast shows at once.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Fallow opens in its own window on a Mac (Priority: P1)

The engineer starts Fallow from Applications, Launchpad, Spotlight, the Dock, or by typing `fallow`. Fallow's window opens on the dashboard, under Fallow's name and icon, and everything the Studio offers works in it as it does on Windows. Starting Fallow again brings the existing window to the front.

**Why this priority**: it is the feature. Without its own window, Fallow on a Mac is a browser tab that a release may not offer (V).

**Independent Test**: on a Mac, build the app bundle, start it from Finder and from a terminal, walk through the screens named in 013 FR-005 (dashboard, a project with its conversation, a spec session, a loop, pipelines, agents, the data page, with their dialogs and panels), and check each works as on Windows; start Fallow a second time and check the existing window comes to the front.

**Acceptance Scenarios**:

1. **Given** Fallow is installed and not running, **When** the engineer opens it from Applications, Spotlight or the Dock, **Then** the service starts and Fallow's window shows the dashboard within 1 s, titled "Fallow", with Fallow's icon in the Dock and the app switcher (Command-Tab).
2. **Given** the window is open, **When** the engineer opens Fallow again (Finder, Dock or `fallow`), **Then** no second window or Dock icon appears and the existing window comes to the front.
3. **Given** the service already runs without a window (`fallow serve`, or the window was closed), **When** the engineer opens Fallow, **Then** the window opens on the running service and every run continues untouched.
4. **Given** the engineer resizes, moves, zooms or makes the window full screen, **When** Fallow is restarted, **Then** the window reopens with the same size and position on the same display, or centred on the main display if that display is gone.
5. **Given** the system is in Dark appearance, **When** the window opens, **Then** both the page and the window's title bar follow 013's theme from the first frame, with no light flash.

---

### User Story 2 - Closing the window does not stop work (Priority: P1)

A loop is running. The engineer closes Fallow's window. The loop keeps running, Fallow stays reachable from its menu bar icon, and Quit is a separate action that asks before stopping running work, whether it comes from the menu bar icon, the app menu or Command-Q.

**Why this priority**: runs take hours (VII). On a Mac, Command-Q is muscle memory; it must not silently stop a run.

**Independent Test**: start a loop with the fake agent, close the window, check the loop advances, reopen the window from the menu bar icon, the Dock and Finder, then press Command-Q and check the confirmation names the running loop; cancel, quit again and confirm, and check the service stops and the loop resumes at the next start.

**Acceptance Scenarios**:

1. **Given** a loop is running, **When** the engineer closes the window, **Then** the loop and the service keep running, and Fallow's icon stays in the menu bar.
2. **Given** the window is closed, **When** the engineer chooses Open Fallow from the menu bar icon, or clicks Fallow in the Dock, **Then** the window opens where it was.
3. **Given** a run is active, **When** the engineer chooses Quit (menu bar icon, app menu, Command-Q, or Quit from the Dock icon's menu), **Then** Fallow asks for confirmation, naming every running loop and session that will pause; on confirm the service stops and the runs resume at the next start; on cancel nothing changes.
4. **Given** no run is active, **When** the engineer chooses Quit, **Then** Fallow quits without asking.
5. **Given** a run is active, **When** the engineer logs out or shuts down the Mac, **Then** Fallow lets the system proceed without blocking it, and the runs resume at the next start as after Quit.

---

### User Story 3 - The window behaves like a Mac app (Priority: P2)

The engineer adds a project with the Mac's own folder picker, attaches images from the system's open dialog or by dragging them in, uses the usual Command shortcuts, and gets Fallow's notifications from macOS where macOS allows it. Links open in the engineer's default browser.

**Why this priority**: the window works without these through the existing fallbacks (typing a path, the "Needs you" inbox), but a Mac app is expected to use the system's own dialogs, shortcuts and notifications.

**Independent Test**: on a Mac, add a project through the folder picker, attach an image in a solo session by dialog and by drag and drop, copy text out of the activity and paste a message with Command-C and Command-V, trigger a "needs you" item with the window closed, and click a pull request link; check each step uses the system's own dialog or notification and that the link opens in the default browser.

**Acceptance Scenarios**:

1. **Given** the dashboard, **When** the engineer clicks + to add a project, **Then** the system's folder picker opens and the chosen folder is added as today; typing a path stays possible.
2. **Given** a solo session, **When** the engineer attaches images by the open dialog or drags image files onto the message box, **Then** they are attached as on Windows.
3. **Given** a text field, **When** the engineer uses Command-C, Command-X, Command-V, Command-A, Command-Z and Shift-Command-Z, **Then** they work as in any Mac app, and the app menu offers Fallow's standard Edit and Window menus.
4. **Given** a link to a pull request or any other web address, **When** the engineer clicks it, **Then** it opens in the default browser and the window stays on the Studio.
5. **Given** notifications are on and macOS allows Fallow to show them, **When** something needs the engineer, **Then** macOS delivers a notification (to Notification Center by default, as a banner too once the engineer chose "Show banners") naming the project, the feature and what is needed and nothing else (V), and clicking it opens the window on the item.
6. **Given** macOS does not let Fallow show notifications (the engineer declined, or the system does not offer them to an app signed ad hoc), **When** something needs the engineer, **Then** the "Needs you" inbox, its count and the menu bar icon show it, and the notifications setting says why system notifications are unavailable.

---

### User Story 4 - Download and install from GitHub without an Apple membership (Priority: P2)

The engineer finds a Mac download on Fallow's GitHub release, next to the Windows installer, with its SHA-256 and instructions. They install it for their own account without an administrator password, allow it once in System Settings, and from then on open it like any app. An engineer who prefers the terminal installs it with one command that needs no System Settings step at all.

**Why this priority**: "eventually released on GitHub" is the second half of the request; it depends on US1-US2 working first.

**Independent Test**: push a version tag on a commit of `main` in a test repository and check the release carries the Mac download(s) and their checksums next to the Windows installer. On a Mac where Fallow was never installed, as a standard user: download through a browser, verify the checksum, install, follow the release notes' first-launch steps, and open Fallow; then, on another clean account, install with the one-line command and check Fallow opens with no warning. Uninstall by the documented steps and check projects are untouched.

**Acceptance Scenarios**:

1. **Given** a version tag on a commit of `main`, **When** the release is built, **Then** the GitHub release offers the Mac download for Apple silicon, with its SHA-256 beside it, next to the Windows installer, and release notes stating it is not notarized by Apple and how to open it the first time.
2. **Given** a standard user downloads the Mac package with a browser, **When** they install it, **Then** it installs into their own Applications folder (or the shared one if they have the rights) without an administrator password.
3. **Given** the downloaded app is opened the first time, **When** macOS refuses it as from an unidentified developer, **Then** following the release notes (System Settings → Privacy & Security → Open Anyway) opens Fallow, and later launches open it without a warning.
4. **Given** the engineer runs the one-line install command from the README, **When** it finishes, **Then** Fallow is in their Applications folder, `fallow` works in a new terminal, the download was checked against its published SHA-256 before anything was installed, and Fallow opens with no system warning.
5. **Given** the download is corrupt or does not match its SHA-256, **When** the install command runs, **Then** it installs nothing and says the checksum did not match.
6. **Given** an older Fallow is installed and running, **When** the engineer installs a newer version with the install command, **Then** Fallow asks to quit first with the Quit confirmation, the runs resume in the new version, and Fallow's data and settings are kept. By the drag-and-drop route, which macOS refuses while Fallow runs, the README and the disk image say to quit Fallow first, and a newer Fallow that finds an older one running offers to quit it.
7. **Given** Fallow is installed, **When** the engineer uninstalls it as the README says, **Then** the app and its `fallow` command are gone, project folders are untouched, and the README names Fallow's data folder for those who want it gone too.

---

### User Story 5 - The browser page is retired from Mac releases (Priority: P3)

A released Mac build shows the Studio only in Fallow's window, like the Windows build. Development and test builds keep the browser page for working on Fallow itself.

**Why this priority**: required by constitution V before the first Mac release, but invisible to anyone using Fallow's window.

**Independent Test**: with a release build running, check that no browser opens, that Fallow listens on no network port that serves the Studio, and that a script on the machine gets no Studio page or data with or without a key; with a development build, check the browser page still works.

**Acceptance Scenarios**:

1. **Given** a released Mac build, **When** the engineer types `fallow`, **Then** the window opens (or comes to the front) and no browser tab opens.
2. **Given** a released Mac build, **When** another program on the Mac requests any local address of Fallow, with or without a key, **Then** it gets no Studio page and no data.
3. **Given** a development or test build, **When** the engineer starts it, **Then** the browser page works as today, and the release pipeline fails if a release build carries it.
4. **Given** a session without a display (over SSH), **When** the engineer runs `fallow serve`, **Then** the service runs and its loops continue; `fallow` without a command explains that the Studio needs Fallow's window and a display.

---

### Edge Cases

- **The first launch of an app downloaded by browser**: macOS refuses it until the engineer allows it in System Settings. Since macOS 15 the old right-click → Open shortcut no longer bypasses this; the release notes and README MUST show the current steps, not the old shortcut.
- **"Fallow is damaged and can't be opened"**: shown when the app's signature no longer matches its contents. The release MUST NOT produce an app that triggers it (FR-022); the README names the cause and the remedy (download again, or use the install command).
- **Upgrading changes the app's signature** (every ad-hoc signed build is a new identity to macOS): permissions macOS gave the old version may be asked for again: access to Documents, Desktop or Downloads for a project there (the notifications permission is kept: macOS keys it on the app's identifier, research M-9). Fallow MUST keep working when they are declined, and the release notes MUST say this may happen.
- **A project in a folder macOS protects** (Documents, Desktop, Downloads, iCloud Drive): macOS asks the engineer once; if declined, adding the project fails with a message naming the folder and where to allow access, and runs in other projects are unaffected.
- **The web view crashes or the page fails to load**: the window shows a native message with a Reload button; the service and its runs are unaffected.
- **A stale lock or address left from a crash**: opening Fallow starts a fresh service and opens the window, as on Windows.
- **Two users on one Mac**: each has their own service, window and data; one user's window never connects to another user's service.
- **The window is on a display that is later disconnected**, or on a Space that no longer exists: it reopens on the main display.
- **A link opens a new window**: a clicked `target=_blank` link opens in the default browser; a window a script asks for is refused (FR-034). Neither ever becomes a second Fallow window.
- **Developer tools**: not available in a released build; available in development builds only.
- **The Mac goes to sleep during a run**: runs continue when it wakes, as they do after the window is reopened; Fallow does not keep the Mac awake.
- **The app is run from the disk image or Downloads instead of Applications**: macOS may run it from a temporary location; Fallow MUST still work, and the release notes say to move it to Applications first.

## Requirements *(mandatory)*

### Functional Requirements

**The window (as 014, on macOS)**

- **FR-001**: Fallow MUST show the Studio in its own window on macOS, using the system's own web view, with Fallow's name and icon in the title bar, the Dock and the app switcher.
- **FR-002**: Fallow MUST keep at most one window and one Dock icon per user. Opening Fallow while it runs MUST bring the existing window to the front, or open it if it was closed.
- **FR-003**: The window MUST remember its size, position, display and zoomed state across restarts, and reopen centred on the main display when the remembered display is gone.
- **FR-004**: The window's title bar MUST follow 013's theme choice (Light, Dark, System) from the first frame, with no light flash.
- **FR-005**: Every screen, dialog and state of the Studio (007-015) MUST work in the window as it does on Windows, including 013's theme, the chat panels' width setting and every keyboard shortcut the Studio defines, with Command in place of Control where macOS expects it.
- **FR-006**: The window MUST NOT navigate away from the Studio. Links and requests for a new window follow FR-034.
- **FR-007**: The window MUST support the standard Mac editing shortcuts and menus (Fallow, Edit, Window) and the zoom shortcuts, and MUST disable developer tools and web view context menu entries that leave the Studio in released builds.

**Running in the background**

- **FR-008**: Closing the window MUST NOT stop the service or any run. While the service runs, Fallow MUST show a menu bar icon with Open Fallow, Notifications on/off and Quit.
- **FR-009**: Every Quit (menu bar icon, app menu, Command-Q, the Dock icon's menu) MUST ask for confirmation naming every running loop and session when any is running, and MUST NOT ask otherwise. Runs paused by Quit MUST resume at the next start. A logout or shutdown MUST NOT be blocked; runs it interrupts resume at the next start.
- **FR-010**: The window's web view MUST run only while the window is open; with the window closed, Fallow's memory, counted with the system web view processes it keeps, MUST be under 60 MB, as on Windows.

**Native integration**

- **FR-011**: Adding a project MUST open the system's folder picker; typing a path MUST stay possible.
- **FR-012**: Attaching images MUST work through the system's open dialog and by dragging files onto the message box, with the same types and limits as on Windows.
- **FR-013**: Fallow's notifications (007 FR-003a) MUST be shown by macOS when macOS allows it, with the same content rule (project, feature, what is needed; never code or output), and clicking one MUST open the window on its item. The existing on/off switch MUST keep working.
- **FR-014**: Whether macOS shows notifications for an app signed ad hoc MUST be established before planning completes and recorded in the plan's research, and checked in CI on each supported macOS version. Where it does not, Fallow MUST rely on the "Needs you" inbox, its count and the menu bar icon (which MUST mark that something needs the engineer), and the notifications setting MUST say that system notifications are unavailable and why.

**Privacy and security (constitution V, VI)**

- **FR-015**: In a released build, the Studio MUST be reachable only from Fallow's own window. Whatever carries it to the window MUST reject every other client: other programs, browsers and other users on the Mac.
- **FR-016**: Fallow MUST NOT pass, show, log or write to disk any secret the window uses to reach the service.
- **FR-017**: The window MUST load nothing from outside Fallow. The web view's own network features (fraudulent-website lookups, spell-check or text services that leave the machine, telemetry, update checks) MUST be switched off where macOS allows; what macOS does not allow switching off MUST be listed in the plan and in the Mac release notes.
- **FR-018**: The browser page MUST be absent from released Mac builds, and the release pipeline MUST fail if a release build carries it (as the Windows release checks today). Development and test builds MAY keep it.
- **FR-019**: Fallow MUST request no macOS permission it does not use (camera, microphone, contacts, location, full disk access, accessibility, automation of other apps, login items).

**Packaging and release**

- **FR-020**: Fallow MUST be packaged as a standard Mac app named Fallow with its icon, a fixed bundle identifier that does not change between versions, its version, and the `fallow` command inside it, for Apple silicon only (014's decision, confirmed 2026-11-22). Intel Macs get no package; they can build Fallow from source, and the install command refuses an Intel Mac with that advice.
- **FR-021**: The release pipeline MUST build the Mac package(s) in CI from the same version tag on `main` as the Windows installer, and publish them on the same GitHub release, each with a SHA-256 file beside it, and the checksums also in the release notes.
- **FR-022**: The Mac app MUST be signed ad hoc as a whole after it is assembled, and CI MUST verify the signature of the finished app and of the app extracted from the published package, so that no download is reported by macOS as damaged.
- **FR-023**: The release notes and the README MUST state that the Mac app is not notarized by Apple and why (no paid Apple membership), give the first-launch steps for the current macOS (System Settings → Privacy & Security → Open Anyway), and say that permissions may be asked for again after an upgrade.
- **FR-024**: Signing with a Developer ID and notarizing MUST be addable later by giving the release pipeline a certificate and credentials, with no change to the app, its bundle identifier, the package layout, the install command or the README's install route other than dropping the first-launch step.
- **FR-025**: The repository MUST offer a one-line install command (in the README and the release notes) that, for the current user and without an administrator password: downloads the Mac package from a GitHub release (the latest, or a version the engineer names), checks it against its published SHA-256 before installing anything, installs the app into the user's Applications folder, puts `fallow` on the user's command path, quits a running Fallow first with the Quit confirmation, and prints what it did. It MUST NOT use administrator rights, change system settings, or turn off the system's security checks.
- **FR-026**: Installing by drag and drop from the downloaded package MUST also work for a standard user (into their own Applications folder), and the README MUST say how to make `fallow` available in the terminal in that case.
- **FR-027**: Uninstalling MUST be documented in the README: remove the app (and the `fallow` command, which the install command can do with an uninstall option); project folders are never touched; Fallow's data folder is named and kept unless the engineer removes it.

**Compatibility**

- **FR-028**: `fallow` with no command MUST open or focus the window and MUST NOT open a browser in a released build. `fallow status`, `fallow stop`, `fallow serve`, `fallow --version` and the agents' bridge MUST keep working as today.
- **FR-029**: Without a display, `fallow serve` MUST run the service, and `fallow` with no command MUST explain that the Studio needs Fallow's window and a display, and exit with an error.
- **FR-030**: Fallow MUST support the macOS versions the system's web view and GitHub's build machines support at release time, at least the two most recent major versions; the README MUST name the oldest supported version, and an older macOS MUST get a clear message rather than a crash.

**Budgets (constitution VIII)**

- **FR-031**: Fallow MUST meet the constitution's budgets on the installed Mac app, measured by CI benchmarks that fail the build: idle memory < 250 MB and running memory < 400 MB (each process's own memory, summed, for Fallow's process and every web view process serving its window), window ready < 1 s (launch to the Studio drawn in the window, p95 of 30 launches after 3 warm-ups) and activity latency < 2 s.
- **FR-032**: On macOS, Fallow's runs, records and settings MUST live in a folder macOS never empties on its own (`~/Library/Application Support/Fallow`); a Fallow that finds them in the old place (`~/Library/Caches/fallow`) MUST move them once, keep nothing in two places, and say in its log where they came from.
- **FR-033**: The chat panels' width MUST be kept with Fallow's other settings, on every platform, not in the web view's own storage.
- **FR-034**: On every platform, a link to an address outside the Studio MUST open in the default browser only when the engineer clicked it in the page; any other navigation away from the Studio MUST be refused.

### Key Entities

- **Mac app**: the Fallow app bundle: name, icon, bundle identifier (fixed across versions), version, the `fallow` command inside it, and its ad-hoc signature.
- **Mac package**: the Apple silicon download on a GitHub release, with its SHA-256 file.
- **Install command**: the one-line installer in the repository: downloads, verifies, installs for the current user, links `fallow`, and can uninstall.
- **Menu bar icon**: Fallow's presence while the service runs; Open Fallow, Notifications on/off, Quit, and a mark when something needs the engineer.
- **App window**: as 014 (size, position, display, zoomed state), kept in Fallow's own settings.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A standard Mac user who downloads Fallow from the GitHub release with a browser has its window open on the dashboard within 3 minutes, including the one-time System Settings step, following only the release notes.
- **SC-002**: A standard Mac user who uses the install command has Fallow open within 2 minutes, with no system warning and no administrator password.
- **SC-003**: Fallow's window shows the Studio within 1 s of launch (p95 of 30 launches after 3 warm-ups) on a Mac.
- **SC-004**: Fallow, counting its window, stays under 250 MB when idle and under 400 MB during the reference run on a Mac.
- **SC-005**: 100% of the screens and states listed in 013 FR-005 pass their existing acceptance checks inside the Mac window.
- **SC-006**: A loop started before the window is closed completes with the window closed, and a run interrupted by Command-Q resumes without repeating a completed step, in 100% of runs of the check.
- **SC-007**: No request from outside Fallow's window (browser, script, another user, with or without a key) receives any Studio page or data from a released Mac build, in 100% of the CI checks.
- **SC-008**: Every published Mac download passes the system's signature check after download and extraction, and matches its published SHA-256, in 100% of releases.
- **SC-009**: The Mac release costs nothing beyond what the project pays today: no Apple membership, no paid build service.

## Assumptions

- The window uses macOS's built-in web view, so the app ships no browser engine; the same window library Fallow already uses on Windows supports it.
- The installed Mac app holds both the window and the service, as on Windows; the window is the only client a released build accepts.
- GitHub's hosted macOS build machines are free for this public repository and can build, sign ad hoc, test and package the app.
- The existing macOS CI job (since 2026-11-17) keeps running the test suite; this feature adds the window, the package and the benchmarks to it.
- Engineers installing through a browser accept the one-time System Settings step, as Windows users accept SmartScreen's "Run anyway".
- The download package is a disk image (`.dmg`) with Fallow and a shortcut to Applications, the usual Mac format; the install command may download a compressed archive of the same app instead.
- The web view's memory on macOS may differ from Windows'; if the budgets are missed, the engineer decides (as 014 did after its spike), not the plan.
- The dependency policy lifts, for macOS, only what the window, menu bar icon, folder picker and notifications need, justified against constitution V, VI and VIII.
