# Data model: Fallow as a desktop app on macOS (016)

016 adds no run, conversation or project data. It extends 014's window state for macOS, moves one
page setting into `prefs.json`, moves the data folder on macOS, and defines the app bundle, the
release files and the install command's footprint.

## Window state (`<data>/window.json`), extended

014's fields stay for Windows. On macOS (research M-13):

| Field | Type | Rule |
|---|---|---|
| `frame` | `{x, y, width, height}`, numbers | The window's frame in points, macOS's global coordinates (origin bottom-left of the main screen); at least 800 × 500. |
| `screen` | `{x, y, width, height}`, numbers | The frame of the screen it was on, to recognise that screen. |
| `zoomed` | bool | Reopens zoomed. |
| `full_screen` | bool | Reopens in full screen on that screen. |

- Written when the window closes and when Fallow stops; read when it opens.
- Missing, unreadable, or its frame on no current screen (`NSScreen.screens`): 1280 × 800 centred
  on the main screen's visible frame (FR-003).
- One file per platform's fields; a file from the other platform's fields is treated as missing.
- macOS's own window restoration is off: only this file decides.

## Page settings (`<data>/prefs.json`), extended

| Field | Type | Rule |
|---|---|---|
| `theme` | `system` \| `light` \| `dark` | 013, unchanged. |
| `chat_wide` | object: screen name → bool | NEW (research M-6): the chat panels' width per screen, moved from the page's `localStorage` (`fallow.chatWide.<screen>`), on every platform. Missing means narrow. |

The page reads both with its first data and writes them through the existing settings route (the
theme's); unknown screen names are kept as they are.

## Data folder on macOS (research M-14)

| | Before 016 | After 016 |
|---|---|---|
| Data folder | `~/Library/Caches/fallow` | `~/Library/Application Support/Fallow` |
| Config folder | unchanged | unchanged |

- Moved once at start when the new folder does not exist and the old one does (a rename; on
  failure Fallow stops with the reason, it never runs on two folders).
- The service log says it moved, and from where.

## Window (in memory, the service), on macOS

014's states (**closed** → **opening** → **open** → **closing** → **closed**) unchanged, on the main
thread. macOS adds:
- **Reopen** (a click on the Dock icon, opening the app from Finder while it runs): as Open Fallow.
- **Web content process ended** (`wry`'s web-content-terminate handler): open shows the native
  message with Reload, as WebView2's crash on Windows; the engine is untouched.

## Quit, on macOS (research M-11)

014's steps unchanged; every path enters through `applicationShouldTerminate`:
- not the end of the session: answer Cancel, run **asked** → … → **stopping**; when the engine has
  stopped, terminate for good (a second `applicationShouldTerminate` then answers Now);
- the end of the session (`NSWorkspaceWillPowerOffNotification` seen): ask the engine to stop,
  wait at most 10 s, answer Now. No dialog.

## Activity hold (research M-11)

| State | When | Holds |
|---|---|---|
| idle | no loop step, operation, session turn or conversation turn running | nothing |
| working | any of them running | one `UserInitiatedAllowingIdleSystemSleep` activity, so App Nap does not throttle Fallow; idle sleep stays allowed |

Automatic and sudden termination are disabled for the service's whole life.

## Notifications permission (research M-9)

| State | Meaning | The notifications setting shows |
|---|---|---|
| not asked | Fallow has not needed one yet | the switch as today |
| provisional | granted without a prompt at the first notification; delivered quietly to Notification Center | the switch, and "Show banners" |
| allowed | macOS shows Fallow's notifications | the switch as today |
| refused | the engineer declined, or macOS does not allow it for this app | the switch, and "macOS does not let Fallow show notifications" with where to allow it |

Provisional at the first notification, full only from "Show banners"; a refusal is not asked again (macOS keeps it). In every
state the "Needs you" inbox and the menu bar icon's dot show waiting items.

## Menu bar icon

| State | Image |
|---|---|
| nothing waiting | Fallow's template image |
| something needs the engineer | the template image with a dot |

Menu: Open Fallow; Notifications (checkmark: on/off, the existing `notifications-off` file); Quit
Fallow.

## App bundle (`Fallow.app`)

| Item | Value |
|---|---|
| Bundle identifier | `dev.fallow.Fallow` (never changes, FR-020, FR-024) |
| Executable | `Contents/MacOS/fallow`: the command line, the service and the window (research M-3) |
| Icon | `Contents/Resources/Fallow.icns`, from `assets/icon` |
| Version | `CFBundleShortVersionString` = Cargo.toml's version; `CFBundleVersion` = the same |
| Minimum macOS | `LSMinimumSystemVersion` 14.0 (research M-15) |
| Signature | ad hoc, hardened runtime, empty entitlements (research M-19) |
| Not shipped | `fallow-fake-agent`, `fallow-fake-host` |

## Release files (per version tag, research M-20)

| File | Content |
|---|---|
| `Fallow-<version>-x64-setup.exe` and `.sha256` | Windows, as today |
| `Fallow-<version>-macos-arm64.dmg` | The signed `Fallow.app` and a link to `/Applications` |
| `Fallow-<version>-macos-arm64.dmg.sha256` | `shasum -a 256` line for the image |

## Install command's footprint (research M-21)

| Item | Where | Removed by `--uninstall` |
|---|---|---|
| The app | `~/Applications/Fallow.app` | yes |
| The command | `~/.local/bin/fallow` → the app's `Contents/MacOS/fallow` | yes |
| `PATH` line | one line in `~/.zprofile`, marked `# added by Fallow's installer`, only if `~/.local/bin` was not on `PATH` | yes |
| Data and config folders | not touched | no: named, kept |

A drag-and-drop install from the disk image puts the app wherever the engineer drops it; the README
says how to link `fallow` in that case (FR-026).
