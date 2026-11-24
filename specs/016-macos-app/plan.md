# Implementation Plan: Fallow as a desktop app on macOS, released on GitHub

**Branch**: `main` (the project's working branch since 2026-11-14) | **Date**: 2026-11-22 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/016-macos-app/spec.md`

## Summary

- **014's window, on macOS**: the same `wry` on a window Fallow makes itself, here an `NSWindow`;
  WebKit's `WKWebView` shows the Studio through the custom scheme, now `fallow://localhost/`;
  nothing listens on any port in a release build (research M-1, M-4).
- **AppKit owns the main thread**: with a display, the service's main thread runs `NSApplication`
  (window, menus, menu bar icon, dialogs, notifications) and the engine moves whole to a thread of
  its own; without a display (SSH) nothing changes (M-2, M-12).
- **One binary in `Fallow.app`** (`dev.fallow.Fallow`): LaunchServices starts it as the service
  with its window; `fallow` in a terminal asks LaunchServices to open the app, so the Dock icon,
  notifications and permissions are Fallow's (M-3).
- **Mac behaviour**: the standard app menu (which is what gives a web view its editing shortcuts),
  a menu bar icon with a dot for waiting items, `NSOpenPanel` sheets, links opened by the page over
  IPC (WebKit tells Fallow nothing about clicks), Quit through `applicationShouldTerminate` from
  every place, logout never blocked, App Nap held off while work runs (M-5, M-8, M-10, M-11).
- **Notifications** through `UserNotifications`: measured, an ad-hoc signed app gets provisional
  authorization without a prompt, its notifications are delivered, and a new signature keeps the
  permission (M-9). Banners on request from the setting; when refused, the inbox and the menu bar
  dot carry it (FR-014).
- **Release without an Apple membership**: the app signed ad hoc with the hardened runtime, a
  `.dmg` per release on the same GitHub release as the Windows installer, its SHA-256 beside it;
  notarization later is a signing step and two commands behind secrets (M-19, M-20).
- **Install command**: `curl … | sh`, per user, checksum first, no `sudo`, no warning (M-21).
- **Found while planning**: Fallow's data folder on macOS is in `~/Library/Caches`, which macOS may
  empty; it moves to `~/Library/Application Support/Fallow` (M-14, VII).
- **Spike done** (M-18): window ready p95 84 ms, ~75 MB open, ~40 MB closed, nothing written by
  WebKit with the non-persistent store. It found three things the design now handles: `wry` keeps
  the closed web view alive (Fallow ends it with `_close`), a hidden page freezes the long poll
  (suppression off), and images sent as a `File` arrive empty (sent as bytes).

## Technical Context

**Language/Version**: Rust, edition 2024, `rust-version` 1.88, toolchain 1.99 (`rust-toolchain.toml`,
which already lists `aarch64-apple-darwin`); the page stays plain JavaScript and CSS compiled in.

**Primary Dependencies**:
- `wry` =0.57.2 becomes a dependency on macOS too (`cfg(any(windows, target_os = "macos"))`);
- macOS only, named directly at the versions `wry` already brings: `objc2` 0.6, `objc2-foundation`,
  `objc2-app-kit`, `objc2-web-kit` (0.3; the AppKit and WebKit features Fallow uses), `block2`;
- NEW on macOS: `objc2-user-notifications` 0.3 (M-9);
- three WebKit private calls (`_close`, `_setPageVisibilityBasedProcessSuppressionEnabled:`,
  `_setHiddenPageDOMTimerThrottlingEnabled:`; M-18), each behind `respondsToSelector:` and checked
  in CI (T042); private calls bar only the Mac App Store, not notarization of a download;
- `raw-window-handle` 0.6 on both platforms;
- `tiny_http`: unchanged rule (`automation` builds), and no longer for macOS release builds
  (`cfg(all(not(windows), not(target_os = "macos")))` plus the `automation` feature);
- `deny.toml`: graph targets add `aarch64-apple-darwin`; bans unchanged (`tao`, `winit`, `gtk`,
  `webkit2gtk`, HTTP clients).

**Storage**: `window.json` gains macOS fields; `prefs.json` gains `chat_wide`; the data folder on
macOS moves to `~/Library/Application Support/Fallow` (data-model.md).

**Testing**: `cargo test` as today on macOS (CI's `macos` job), and `cargo test --features
automation`; `window::nav` and `window::state` tests cover both platforms' rules on every OS; new
macOS CI checks (M-16): no listener, a 10-minute packet capture with process attribution, the
signature of the built and the downloaded app, the per-user install as a standard user, the
`automation` marker, closing the window during a loop; benchmarks on `macos-15` (M-17).

**Target Platform**: macOS 14 or newer on Apple silicon (`aarch64-apple-darwin`); Windows
unchanged. No Intel package (spec Clarifications).

**Project Type**: a desktop app: a Rust service with its own window showing a local web UI.

**Performance Goals**: window ready < 1 s (p95 of 30 opens after 3 warm-ups) and from a cold start
(`open -a` with no service running) < 1 s; activity latency < 2 s; on macOS as on Windows. With the
window closed < 60 MB (FR-010; spike: ~40 MB).

**Constraints**: idle < 250 MB and running < 400 MB (physical footprint of Fallow plus the WebKit
processes serving its window, summed: macOS's counterpart of constitution 8.0.0's private working
set); no listener and no browser page in a release; no network request of Fallow's own; per-user
install without admin rights; no paid Apple membership; nothing that blocks notarization later.

**Scale/Scope**: `src/window/` gains a macOS frame and view (about 800 to 1,200 lines: window,
menus, status item, alerts, open panel, app delegate, run-loop queue), and `nav`/`state` gain the
macOS rules; `platform/` gains macOS `display`, `picker` and `notify`; `service/mod.rs` starts the
engine on a thread under AppKit; `app.js` gains `open:` links and `chat_wide` from `prefs.json`;
`installer/macos/` (bundle script, plist template, install script); `release.yml` and `ci.yml`
gain macOS jobs; `xtask bench` gains macOS memory; README sections for macOS.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Not affected: the Studio's content and the loop are unchanged. |
| II. The Engineer Decides | Pass: every Quit, including ⌘Q and the Dock's, confirms and names running work (M-11); closing the window stops nothing; links leave only on the engineer's click (M-5); the install command never quits Fallow without the Quit confirmation (M-21). |
| III. Coordination in Code | Not affected. |
| IV. Simplest Setup First | Pass: one process, one binary, one web view library for both platforms; AppKit's own run loop, no event-loop library (M-1, M-2); the engine moved whole, not split. |
| V. Local and Private | Pass: no listener and no secret in a release (M-4, Q8); the window loads only `fallow://localhost` and opens only `http(s)` on a click (M-5); WebKit's fraudulent-website lookups and link previews off, the rest traced and listed (M-7, M-16); notifications keep 008's content rule (M-9); the install script is the engineer's `curl`, not Fallow. |
| VI. Least Privilege | Pass: per-user install into `~/Applications`, no `sudo`, checked as a standard user (M-16, M-21); no permission requested that Fallow does not use (FR-019); data in Fallow's own folders (non-persistent web data store, M-6; Application Support, M-14). The installer's `~/.local/bin` link and `~/.zprofile` line are the engineer's install, removed by `--uninstall` (noted for the engineer, M-23). |
| VII. Runs Survive | Pass, and strengthened: the data folder leaves Caches (M-14); logout and Quit stop as `fallow stop` does and runs resume; automatic termination off and App Nap held while work runs (M-11). |
| VIII. Small and Measured | Pass on the spike (M-18: window ready p95 84 ms, ~75 MB open, ~40 MB closed); CI keeps it (M-17): the four budgets on the installed Mac app, WebKit's processes counted by responsible pid; 014's `automation` deviation applies (Complexity Tracking). |
| IX. Runtime-Agnostic | Not affected. |
| Dependencies (Workflow) | `wry` on macOS and the `objc2` crates it already brings, plus `objc2-user-notifications`, justified against V, VI, VIII (M-1); bans unchanged. |
| CI gates (Workflow) | Each 014 gate gets a macOS form (M-16); the release is built by CI from the tag with a checksum per download and no self-update (M-20). |

**Re-check after Phase 1 design**: unchanged. The contracts add no listener in a release build, no
write place of Fallow's own beyond its data folder's new location, and no request of Fallow's own.

## Project Structure

### Documentation (this feature)

```text
specs/016-macos-app/
├── spec.md
├── plan.md              # this file
├── research.md          # M-1 to M-23
├── data-model.md        # window state, prefs, data folder, Quit, bundle, release files
├── quickstart.md        # Q1 to Q11
├── contracts/
│   ├── window-macos.md  # the socket's commands, scheme, navigation, IPC, menus on macOS
│   └── cli-macos.md     # `fallow` on macOS and the install command
├── checklists/
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── main.rs                  # `fallow` on macOS: LaunchServices, version check (M-3, M-22)
├── config/mod.rs            # macOS data folder: Application Support, one-time move (M-14)
├── service/
│   ├── mod.rs               # macOS: engine on its own thread under AppKit; no page in release
│   └── web/app.js           # `open:` links (both platforms), chat_wide from prefs.json
├── studio/files.rs          # prefs.json: chat_wide
├── window/
│   ├── mod.rs               # platform split: windows.rs / macos.rs behind one interface
│   ├── nav.rs               # the decision with the platform's origin; `open:` (both)
│   ├── state.rs             # macOS fields and rule (tested everywhere)
│   ├── quit.rs              # unchanged (plain)
│   ├── frame.rs, view.rs    # Windows, as today
│   └── mac/                 # NEW, macOS only
│       ├── app.rs           # NSApplication, delegate (reopen, terminate, power-off), main-thread queue
│       ├── frame.rs         # NSWindow, appearance, frame/state, restorable off
│       ├── view.rs          # wry WKWebView: scheme, non-persistent store, prefs (fraud, previews)
│       ├── menu.rs          # the main menu and the status item
│       └── alert.rs         # NSAlert (Quit, errors), open panel
├── platform/
│   ├── display.rs           # macOS: SessionGetInfo graphic access (M-12)
│   ├── picker.rs            # macOS: NSOpenPanel through the main-thread queue
│   └── notify.rs            # macOS: Tray = status item; UserNotifications (M-9, M-10)
installer/macos/
├── Info.plist.in            # NEW: the bundle's plist template
├── entitlements.plist       # NEW: empty (M-19)
├── make-app.sh              # NEW: assemble, icon, sign, verify, make the .dmg
└── install.sh               # NEW: the install command (M-21)
assets/icon/                 # Fallow.icns and the menu bar template images
spikes/016-macos/            # NEW, throwaway: notify/ (done), window/ (done, M-18)
ci/macos/notify-check/       # NEW: the notification check CI runs per macOS version (FR-014)
xtask/src/bench.rs           # macOS memory by responsible pid (M-17)
.github/workflows/ci.yml     # macOS gates, benchmarks, package on every push to main
.github/workflows/release.yml # the macOS job, upload to the same release (M-20)
deny.toml                    # graph targets + aarch64-apple-darwin
README.md                    # macOS install, first launch, permissions, uninstall, data folder
```

**Structure Decision**: one crate as today. The window stays one module with a Windows and a macOS
implementation behind the interface `service/mod.rs` already uses (`configure`, `open`, `open_for`,
`quit`, `on_ui`); the decisions (`nav`, `state`, `quit`) stay plain and tested on every platform.
Linux keeps the browser page until its own feature.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| VIII: running memory and activity latency measured on the `automation` build, not the installed app (014's deviation, now on macOS too) | They need a scripted run, and the release app accepts requests only from its window. The real window is open and counted. | A control surface in the shipped app. |

## Spec adjustments

The four changes found during planning were confirmed by the engineer on 2026-11-22 and are in
`spec.md`: the data folder move (FR-032, M-14), `chat_wide` in `prefs.json` (FR-033, M-6), links
through the page on both platforms (FR-034, M-5), and US4 AS6's drag-and-drop route (quit first;
M-22).
