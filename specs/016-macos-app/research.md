# Research: Fallow as a desktop app on macOS (016)

Decisions taken during `/speckit-plan`, 2026-11-22. Facts about `wry` were checked against its
source (0.57.2, the version Fallow pins) on that day; facts about macOS against this Mac (macOS
26.7, Apple silicon). What could not be confirmed is marked **to measure**: it is settled by the
spike (M-18) or a check in the tasks, never assumed.

014's decisions carry over unless a decision here replaces them: one process (the service) owns
the window (014 R-2); no web server, the page through a custom scheme (R-3); one navigation
decision (R-4); close and Quit ask the page first (R-9); window ready and the benchmarks (R-14,
R-15); the `automation` build (R-15).

## M-1 The window: `wry` on an `NSWindow` Fallow makes itself

**Decision**: `wry` 0.57.2, the version Windows uses, on macOS too. Fallow makes the `NSWindow`
itself through `objc2-app-kit` and gives `wry` its content view (`raw-window-handle`'s `AppKit`
handle), as it gives it a Win32 window on Windows. No `tao` and no `winit` (still banned): AppKit's
own run loop is Fallow's loop (M-2).

**Rationale**: `wry` on macOS is WebKit's `WKWebView` (the system's web view, so nothing ships but
Fallow) with what Fallow uses on Windows: an asynchronous custom scheme (`WKURLSchemeHandler`),
navigation and new-window handlers, IPC, devtools on/off, background colour, an ephemeral data
store, and the `<input type=file>` open panel (`runOpenPanelWithParameters`). One library for both
platforms keeps `src/window/` one module with two frames.

**Cost** (V, VI, VIII):
- **New crates**: none at run time beyond what `wry` already brings on macOS (`objc2`,
  `objc2-foundation`, `objc2-app-kit`, `objc2-web-kit`, `objc2-core-foundation`, `block2`), named
  directly with the AppKit features Fallow uses, plus `objc2-user-notifications` (M-9, same
  project, same licences: MIT, Apache-2.0, Zlib).
- **V**: none of them makes a network request; what WebKit does is M-7.
- **`deny.toml`**: the graph's targets gain `aarch64-apple-darwin`; `tao`, `winit`, `gtk` and
  `webkit2gtk` stay banned.

**Alternatives considered**: `tao` + `wry` (a second event-loop library for what AppKit already
does); a hand-written `WKWebView` through `objc2-web-kit` (about the same code `wry` already is,
and two web view paths to maintain); Tauri (a framework).

Sources: `wry-0.57.2/src/wkwebview/mod.rs`, `navigation.rs`, `class/wry_web_view_ui_delegate.rs`.

## M-2 The main thread belongs to AppKit

**Decision**: on macOS AppKit, and `wry`'s `WKWebView` (it fails with `NotMainThread` elsewhere),
run only on the process's main thread. So, when the service has a display (M-12):
- the service's **main thread runs `NSApplication`**: the window, the menu bar icon, the app menu,
  dialogs and notifications;
- **the engine moves to a thread of its own** (`engine.serve(rx)` today runs on the main thread);
  when it returns (stop or Quit), it asks the main thread to terminate the app;
- other threads reach the main thread only through a queue it drains (the role
  `platform::notify::on_ui` plays on Windows), woken by an AppKit event; the same `on_ui`,
  `request_open` and `Open` interfaces, so `service/mod.rs` changes in one place.

Without a display (`fallow serve` over SSH), nothing of AppKit starts and the engine runs on the
main thread as today.

**Rationale**: the engine is single-threaded and owns all state (014 R-2); moving it whole to a
thread changes nothing in it. The pipe, request workers and agents already run on threads.

**Alternatives considered**: the window in a second process (014 R-2 rejected it); AppKit on a
background thread (not supported by AppKit or WebKit).

## M-3 The app bundle and who starts the service

**Decision**:
- **`Fallow.app`** (bundle identifier `dev.fallow.Fallow`, fixed for good, FR-020):
  `Contents/MacOS/fallow` (the one binary: the command line, the service and the window),
  `Contents/Info.plist`, `Contents/Resources/Fallow.icns` (from the existing `assets/icon`),
  `CFBundleExecutable = fallow`, `LSMinimumSystemVersion` (M-15), `NSHighResolutionCapable`,
  `LSApplicationCategoryType = public.app-category.developer-tools`, no usage-description keys
  Fallow does not need (FR-019).
- **Opened from Finder, the Dock or Spotlight**, LaunchServices starts `fallow` with no arguments
  and no terminal: it becomes the service and opens its window. When a service already runs,
  LaunchServices activates that process instead (one app instance per user), whose "reopen"
  shows the window (FR-002).
- **`fallow` in a terminal**, with no service running, asks LaunchServices to open the bundle it
  lives in (`NSWorkspace openApplicationAtURL`, without activating a second copy), so the service
  is always the app process with the bundle's identity: the Dock icon, the menu bar, notifications
  and permissions belong to Fallow, not to the terminal. Then it sends `{"cmd":"window"}` as on
  Windows. Outside a bundle (a `cargo build` binary), it starts `fallow serve` as today.
- **`fallow serve`** in a terminal stays: the service without LaunchServices, for development and
  SSH; with a display it still opens its window when asked (the Dock icon and name then come from
  the bundle only when the binary is inside one; checked by hand in T031).
- The `fallow-app` launcher of Windows (014 R-18) is not needed: a bundle's executable never gets a
  terminal.

**Rationale**: one binary keeps the package small and the `fallow` command exactly the app's
version. LaunchServices is what gives a process its app identity on macOS.

## M-4 The page through the custom scheme: `fallow://localhost/`

**Decision**: the same `answer` (014 R-3) behind `wry`'s custom scheme `fallow`. On macOS `wry`
serves it as **`fallow://localhost/`** (WebKit's form; Windows uses `http://fallow.localhost/`).
- The navigation decision (`window::nav`, 014 R-4) takes the Studio's origin as a parameter, one per
  platform, and stays unit-tested for both.
- The page's own requests use relative URLs; the CSP (`'self'`) covers the custom origin. Measured
  in the spike (M-18): answers after 2 to 20 s arrive, and a 5 MiB `POST` body arrives whole when
  sent as an `ArrayBuffer` (a `File` arrives empty).

## M-5 Links and new windows without "user-initiated"

`wry`'s macOS navigation handler gives Fallow the URL only, not whether the engineer clicked
(WebView2's `IsUserInitiated`, which 014 R-4 relies on).

**Decision**: on macOS, the window's navigation handler **refuses every navigation away from the
Studio** and the new-window handler refuses every new window. Links leave by the page: `app.js`
catches clicks on links to other addresses (a trusted `click` event, i.e. the engineer's) and
posts `open:<url>` over IPC; the window opens it in the default browser (`NSWorkspace openURL`)
after the same decision function accepts it (`http`/`https` only). Pages run only Fallow's own
scripts (CSP), so a click event is the engineer's.

The same IPC path is used on Windows too (one behaviour for both platforms), with WebView2's
`IsUserInitiated` kept as a second guard.

## M-6 The data store: ephemeral, settings in `prefs.json`

**Decision**: the web view uses WebKit's **non-persistent data store** (`with_incognito(true)`):
- nothing of WebKit's storage is written under `~/Library/WebKit/dev.fallow.Fallow` (VI: Fallow
  writes to its own folders);
- the one setting the page keeps in `localStorage` today (the chat panels' width, `chatWide`,
  FR-005) moves to Fallow's `prefs.json`, as 013 keeps the theme, read with the page's first data
  and written by a route; `sessionStorage` (drafts across a reload) works within the window's
  life, as on Windows;
- **measured** (spike, `spikes/016-macos/window/RESULTS.md`): with the non-persistent store
  WebKit wrote nothing under `~/Library`; the default store wrote
  `~/Library/WebKit/<app>/WebsiteData/ResourceLoadStatistics/*.db`. Memory and speed are the same
  with either store.

## M-7 What WebKit sends, and what Fallow switches off (FR-017)

**Decision**:
- **Fraudulent-website lookups** (Safe Browsing through Apple): off,
  `WKPreferences.fraudulentWebsiteWarningEnabled = false` on the web view's configuration.
- **Link previews**: off (`with_allow_link_preview(false)`).
- **Developer tools**: `isInspectable` only in debug and `automation` builds (`with_devtools`).
- **Spell-check and text services**: macOS's spell-checker is local; **to measure** by the trace.
- **The page loads nothing external** (CSP and M-5).
- **Cannot be switched off by Fallow** (release notes and README): WebKit is part of macOS and is
  updated with it by Apple; whatever the trace (M-16) shows WebKit's processes opening.
- **Measured** (CI `trace-macos`, 2026-11-23, macOS 15, window idle 10 minutes): no connection
  of WebKit's processes or of Fallow's own; `installer/macos/webkit-traffic.md` says so.

## M-8 Native integration (FR-011 to FR-013)

- **Folder picker**: `NSOpenPanel` (directories only, one), as a sheet on Fallow's window when it is
  open, run on the main thread; the request's worker thread waits for its answer, as
  `platform::picker::pick_folder` does on Windows. Typing a path stays.
- **Images by dialog**: the page's `<input type=file>` opens `NSOpenPanel` through `wry`'s UI
  delegate.
- **Images by drag and drop**: 014's page handlers (`dragover`/`drop` on the message box); `wry`'s
  native drop handler is not set, so WebKit delivers the drop to the page. Not yet seen in the
  spike (it cannot drag); checked by hand in T031.
- **Menus** (FR-007): AppKit gives editing shortcuts to a web view only through the app's menu, so
  Fallow builds the standard main menu: **Fallow** (About Fallow, Hide, Hide Others, Show All, Quit
  Fallow ⌘Q), **Edit** (Undo, Redo, Cut, Copy, Paste, Select All, with their standard selectors),
  **View** (Actual Size ⌘0, Zoom In ⌘+, Zoom Out ⌘−: the web view's `pageZoom`; Reload ⌘R: the
  Studio in place), **Window** (Minimize ⌘M, Zoom, Close ⌘W). No Developer menu in releases.
- **Links**: M-5.

## M-9 Notifications (FR-013, FR-014)

**Decision**: the `UserNotifications` framework (`UNUserNotificationCenter`), from the app process
(M-3), through `objc2-user-notifications`. The first time Fallow has something to notify (not at
launch), it asks for **provisional** authorization (alert, sound, provisional): macOS grants it
without a prompt, and notifications go quietly to Notification Center. The notifications setting
then offers "Show banners", which asks for full authorization (alert, sound) while the engineer is
there to answer the prompt; macOS's own "Keep → Deliver Prominently" in Notification Center does
the same. A click (`didReceiveNotificationResponse`) opens the window on the item's route, as on
Windows. The content rule is 008's (project, feature, what is needed).

When authorization is denied or the framework answers "not allowed", Fallow does not ask again;
the notifications setting says so ("macOS does not let Fallow show notifications: System Settings →
Notifications → Fallow"), and the menu bar icon marks waiting items (M-10). The "Needs you" inbox
is unchanged.

**Rationale**: a full-authorization prompt that nobody answers can end as a refusal (run 2 below),
and Fallow notifies precisely when the engineer is away. Provisional authorization needs no answer.

**Measured (2026-11-22, this Mac, macOS 26.7.3, managed; `spikes/016-macos/notify/RESULTS.md`)**,
a minimal ad-hoc signed app, no Apple certificate:
- run from a temporary folder (not registered with LaunchServices): refused at once,
  `UNErrorDomain` 1, "Notifications are not allowed for this application";
- from `~/Applications`, full authorization, nobody at the Mac: no answer in 120 s; a later run
  with the same bundle id read `denied` without asking;
- from `~/Applications`, a new bundle id, regular app, **provisional**: `granted = true`,
  status provisional, no prompt; the notification was delivered (`delivered = 1`; usernoted:
  "Authorization set … allow: YES … provisionally: YES");
- rebuilt and signed again (a new ad-hoc signature, new code hash): the authorization was kept
  (status provisional before asking) and the second notification delivered (`delivered = 2`).
  macOS keys the permission on the bundle id, so upgrades do not ask again.

**Conclusion**: an ad-hoc signed app notifies; FR-014's fallback stays for refusals. Still checked
in the tasks with someone at the Mac: the click on a notification reaching Fallow, and "Show
banners" giving full authorization.

## M-10 The menu bar icon (FR-008)

**Decision**: an `NSStatusItem` with a template image (from `assets/icon`, so it follows the menu
bar's light and dark) and a menu: Open Fallow, Notifications (a checkmark item), Quit Fallow. When
something needs the engineer, the icon shows a dot (a second template image) until the inbox is
empty; this also carries FR-014's fallback. It is part of the main thread's AppKit setup, not a
thread of its own as on Windows.

The app stays a regular Dock app while the service runs, with or without the window (a click on
the Dock icon reopens the window: `applicationShouldHandleReopen`), so Command-Tab and the Dock's
Quit work (US2).

## M-11 Close, Quit, logout (FR-009)

**Decision**:
- **Closing the window** (⌘W, the red button): 014 R-9's `Fallow.closeAsked()` first, then the
  window's state is written and the web view dropped; WebKit's processes for it end (measured, M-18:
  the web content process ends once Fallow calls `_close`; the Networking and GPU processes stay,
  ~40 MB in all; the benchmark waits 10 s).
- **Quit** from anywhere (⌘Q, the app menu, the menu bar icon, the Dock's Quit, `fallow quit`):
  every path reaches `applicationShouldTerminate`, which answers **TerminateCancel** and runs 014's
  Quit (the page's unsaved edits, then an `NSAlert` naming every running loop and session when any
  runs, as a sheet on the window when it is open); on confirm the engine stops as `fallow stop`
  does, and the main thread then terminates for good.
- **Logout, restart, shutdown**: `NSWorkspaceWillPowerOffNotification` marks the session's end;
  `applicationShouldTerminate` then answers **TerminateNow** after asking the engine to stop
  (bounded wait, no dialog), and runs resume at the next start (VII), as on Windows'
  `WM_ENDSESSION`.
- **Automatic termination and App Nap**: macOS may end or throttle an app with no window. Fallow
  disables automatic and sudden termination (`NSProcessInfo`), and holds an activity
  (`beginActivityWithOptions: UserInitiatedAllowingIdleSystemSleep`) while a loop or session works,
  so runs keep their pace with the window closed but the Mac may still sleep (spec edge case).

## M-12 A display or not (FR-029)

**Decision**: `platform::display::interactive()` on macOS asks the security session whether it has
graphic access (`SessionGetInfo`, `sessionHasGraphicAccess`), false over SSH. `fallow` with no
command and no display explains that the Studio needs Fallow's window and a display, and exits 1;
`fallow serve` runs without AppKit (M-2).

## M-13 The theme and the window's place (FR-003, FR-004)

**Decision**:
- **Theme**: the window's `appearance` is `NSAppearanceNameDarkAqua`, `NSAppearanceNameAqua`, or
  none (System) from 013's `prefs.json`, set before the window is shown, with the window's
  background and the web view's background colour set to 013's page background for that theme, so
  nothing flashes light. `theme:…` from the page (014's IPC) updates it; with System, AppKit
  follows the system by itself.
- **Place**: `window.json` (014 R-8) gains macOS's frame in points, its screen's frame, `zoomed`
  and `full_screen`; `window::state` (plain, tested) decides as on Windows: the saved frame when a
  screen still contains it, otherwise 1280 × 800 centred on the main screen's visible frame.
  macOS's own window restoration is off (`restorable = false`), so only Fallow's rule applies.

## M-14 Fallow's data folder moves out of Caches (VII)

On macOS Fallow keeps its data (runs, sessions, working copies) in `~/Library/Caches/fallow`
today. macOS may delete the contents of Caches when the disk is low, which would lose runs
(VII: runs survive).

**Decision**: the data folder on macOS becomes `~/Library/Application Support/Fallow`. At start, if
it does not exist and `~/Library/Caches/fallow` does, Fallow moves the old folder there once (a
rename on the same volume) and says so in the service log. The config folder is unchanged.
Windows and Linux are unchanged.

## M-15 Supported macOS versions (FR-030)

**Decision**: `LSMinimumSystemVersion` 14.0 (Sonoma): the oldest macOS Apple still updates and the
oldest that GitHub's hosted macOS images build for at release time; CI tests on the two newest
macOS images GitHub offers (today `macos-15` and `macos-26`). An older macOS refuses to open the app
with the system's own "requires a newer version of macOS" message (FR-030's clear message), and
the README names macOS 14 as the oldest. **To measure**: that the image list holds at the first
release.

## M-16 CI gates on macOS (constitution Development Workflow)

Each of 014's gates (R-16) gets a macOS form, in the existing `macos` job's family (separate jobs,
each under 60 minutes):
- **No listener**: release build, window open: `lsof -nP -iTCP -iUDP` for Fallow's process and
  WebKit's processes serving it (M-17) shows no listening socket.
- **No outbound connection** (SC-007's form): a 10-minute packet capture with process attribution
  (`tcpdump -i pktap,all -k`, as root on the runner) with the window open and idle: Fallow's process
  opens none; WebKit's processes open only what M-7 lists, or the check fails.
- **The window stays on the Studio**: the navigation decision's unit tests (both origins) and the
  release CSP.
- **The `automation` marker is absent** from the release binary (as Windows).
- **The per-user install**: a standard (non-admin) user made with `sysadminctl`; the install
  command run as that user installs into its `~/Applications` with no `sudo` and no prompt.
- **The signature** (FR-022): `codesign --verify --deep --strict` on the built app, and again on the
  app copied out of the published disk image after a download; `codesign -dv` reports an ad-hoc
  signature with the hardened runtime.
- **Tests and clippy** as today, plus `cargo test --features automation`.

## M-17 Budgets on macOS (FR-031)

**Decision**:
- **Memory**: each process's **physical footprint** (`proc_pid_rusage`, `ri_phys_footprint`:
  Activity Monitor's "Memory", the macOS counterpart of the private working set, constitution
  8.0.0), summed over Fallow's process and the WebKit processes serving its window. WebKit's
  processes (`com.apple.WebKit.WebContent`, `.Networking`, `.GPU`) are XPC services whose parent
  is `launchd`, not Fallow; they are found by their **responsible process**
  (`responsibility_get_pid_responsible_for_pid`, exported by `libsystem`), which is Fallow's pid.
  Measured: started as an app (LaunchServices) Fallow is the responsible process of its three
  WebKit processes; started from a terminal the terminal is, so the count starts Fallow as an app.
- **Window ready**, **activity latency**, **running memory**: 014 R-14 and R-15 unchanged, on the
  release build and the `automation` build of the app bundle, on the `macos-15` runner (Apple
  silicon). The `xtask bench` lines run on macOS too; the README's "memory only on Windows and
  Linux" goes.
- The release job does not run the benchmarks; the `main` CI does, and fails the build.

## M-18 The spike before tasks: done

Run 2026-11-22 on this Mac (screen locked, nobody at it), `spikes/016-macos/window/RESULTS.md`:
`wry` on Fallow's own `NSWindow` showing the real dashboard through `fallow://localhost/`.

- **Window ready**: p95 **84 ms** over 30 opens (first 169 ms); non-persistent store 88 ms. Budget
  1 s.
- **Memory**: ~75 MB with the window open (Fallow's side ~23 MB, WebContent ~30, GPU ~16,
  Networking ~4); ~40 MB closed; the service alone 2.9 MB. Budget 250 MB.
- **Uploads, long poll, hardened runtime**: 5 MiB as an `ArrayBuffer` arrives whole; answers after
  2 to 20 s arrive; ad-hoc signed with the hardened runtime and no entitlements, the window opens.

Three decisions follow from it:

1. **End the page on close**: `wry` 0.57.2's `Drop` keeps the `WKWebView` alive on purpose (it
   calls `retain()`), so its web content process never ends: one more per open. The window calls
   WKWebView's `_close` before dropping `wry`'s web view (30 of 30 opens: the process ended).
   FR-010 (nothing of the web view kept while closed).
2. **Keep a hidden page running**: when the window is hidden (minimized, covered, screen locked)
   WebKit suspends the page after ~3 s, and the Studio's long poll stalls. The web view's
   preferences turn off `pageVisibilityBasedProcessSuppression` and hidden-page timer throttling;
   with both off, every answer arrived.
3. **Images as bytes**: a `File` body arrives empty through the custom scheme; the chat sends
   `await f.arrayBuffer()` instead of the `File` (`screens/solo.js`), on every platform.

`_close`, `_setPageVisibilityBasedProcessSuppressionEnabled:` and
`_setHiddenPageDOMTimerThrottlingEnabled:` are WebKit SPI, stable for years but not public: each
is called only after `respondsToSelector:`, the service log says when one is missing, and a CI
check opens and closes the window and fails if a web content process is left or a 20 s answer is
lost. If `wry` fixes its drop, the `_close` call goes.

Left for the tasks, with someone at the Mac: drag and drop of an image onto the page; the
`macos-15` runner (the CI job's first run).

## M-19 Signing now, notarizing later (FR-022, FR-024)

**Decision**: the release job signs the assembled app once, after everything is in it:
`codesign --force --deep --options runtime --timestamp=none --sign - Fallow.app`, with an
entitlements file that is empty today. The **hardened runtime** is on now, because notarization
requires it; so adding a Developer ID later changes only the signing identity (`--sign "Developer
ID Application: …"`, `--timestamp`), one `notarytool submit --wait` and one `stapler staple` step,
driven by repository secrets that are absent today (the steps are skipped when the secrets are
missing). The bundle identifier, the layout, the package and the install command do not change;
the README drops its first-launch step (FR-024).

**Why it is not reported as damaged**: macOS says "damaged" when a bundle's contents no longer match
its signature. The app is signed after its last change, the disk image is made from the signed app
with `hdiutil` (which keeps it byte for byte), and CI verifies the signature of the app taken from
the published image (M-16).

**Each build is a new identity**: an ad-hoc signature's designated requirement is the code's hash,
so macOS treats every version as a new app for the folder permissions it remembers (Documents,
Desktop and Downloads). The notifications permission is kept: it is keyed on the bundle id
(measured, M-9). The release notes say so (FR-023). With a Developer ID this goes away.

## M-20 The release (FR-021)

**Decision**: `release.yml` gains a `macos` job beside the Windows one, on `macos-15` (Apple
silicon), from the same tag check:
- `cargo build --release --locked`, the `automation` marker check, the bundle assembled by
  `installer/macos/make-app.sh` (the plist from a template with the tag's version), signed (M-19);
- **`Fallow-<version>-macos-arm64.dmg`**: the app and a link to `/Applications`, made with
  `hdiutil create -format UDZO`; its `.sha256` beside it;
- the GitHub release is created by the Windows job as today; the macOS job uploads its two files to
  it (`gh release upload`), and the notes gain a macOS section: the SHA-256, "not notarized by
  Apple" and why, the first-launch steps (System Settings → Privacy & Security → Open Anyway), the
  install command, that permissions may be asked again after an upgrade, and M-7's list. Whichever
  job finishes first creates the release; the other uploads to it (`gh release view` first).

## M-21 The install command (FR-025 to FR-027)

**Decision**: `installer/macos/install.sh` in the repository, run as
`curl -fsSL https://raw.githubusercontent.com/Cem-Kalender/fallow/main/installer/macos/install.sh | sh`
(or with `-s -- --version 0.2.0`, or `--uninstall`). It is the engineer's own `curl`, not Fallow
(V: Fallow makes no requests). In order:
1. refuses an Intel Mac (`sysctl -n hw.optional.arm64` is not 1, which also sees through Rosetta),
   saying to build from source; refuses root (`id -u` = 0);
2. finds the version: the one named, or the latest release's tag from the redirect of
   `https://github.com/Cem-Kalender/fallow/releases/latest` (no API token);
3. downloads the `.dmg` and its `.sha256` into a new temporary folder, checks the sum with
   `shasum -a 256 -c`; on a mismatch, says so and installs nothing;
4. quits a running Fallow with `fallow quit --installer` (014's: the Quit confirmation, waits for
   the service to end); if the engineer cancels, stops;
5. mounts the image read-only (`hdiutil attach -nobrowse -readonly`), copies the app with `ditto`
   into `~/Applications` (made if missing; never `/Applications`, which needs admin rights for a
   standard user), detaches;
6. links `~/.local/bin/fallow` to the app's `Contents/MacOS/fallow`; if `~/.local/bin` is not on
   `PATH`, adds one marked line to `~/.zprofile` and says so;
7. prints what it did, and how to open Fallow.

Files fetched by `curl` carry no quarantine mark, so macOS shows no warning (SC-002). The script
never uses `sudo`, never changes system settings, and never removes quarantine marks or turns off
Gatekeeper (FR-025). `--uninstall` quits Fallow the same way, removes the app, the link and the
marked `~/.zprofile` line, and names the data folder (M-14) it keeps.

## M-22 Upgrades: a new version meets a running old one

**Decision**: when `fallow`, or the app opened from Finder, finds a running service of another
version (`{"cmd":"status"}` gains `version`), it offers to quit the old one with the Quit
confirmation and to start the new one; runs resume in it (US4 AS6). This also covers a copy dragged
from the disk image over a running Fallow, where Finder itself refuses to replace an app in use
and the README says to quit Fallow first. The same check applies on Windows.

## M-23 The constitution

016 needs no amendment:
- **V**: a released Mac build has no browser page and no listener (M-4, M-16); WebKit's traffic
  is listed (M-7); notifications follow the content rule (M-9).
- **VI**: per-user install without admin rights (M-21), data and settings in Fallow's own folders
  (M-6, M-14). The install command's line in `~/.zprofile` and the link in `~/.local/bin` are the
  engineer's install (like the Windows installer's `PATH` option), not Fallow's writes; noted for
  the engineer.
- **VIII**: budgets measured on macOS (M-17), with 014's `automation` deviation (Complexity
  Tracking).
- **Development Workflow**: the release built by CI from the tag, with a checksum per download, no
  self-update (M-20).
