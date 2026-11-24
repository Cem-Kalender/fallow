# Quickstart: checking 016 (macOS)

Prerequisites: a Mac with Apple silicon and macOS 14 or newer; the disk image built by CI
(`Fallow-<version>-macos-arm64.dmg`) or a local bundle (`cargo build --release` then
`installer/macos/make-app.sh`); the fake agent (`fallow-fake-agent`) for scripted runs. Contracts:
[window-macos.md](contracts/window-macos.md), [cli-macos.md](contracts/cli-macos.md).

## Q1 Install from the browser, as a standard user (US4, FR-022, FR-023, SC-001)

As a user without admin rights, download the `.dmg` from the release with Safari; check
`shasum -a 256` against the published sum; open the image and drag Fallow to the Applications link
(or to `~/Applications`). Open Fallow. Expect: macOS refuses it once as from an unidentified
developer, never as "damaged"; System Settings → Privacy & Security → Open Anyway opens it; later
launches open without a warning.

## Q2 Install with the command (US4, FR-025, SC-002)

On another clean account, run the one-line install command. Expect: no password asked, no system
warning when Fallow opens, the app in `~/Applications`, `fallow --version` in a new terminal. Run it
again with a corrupted `.sha256` served (a local copy of the script pointed at a test release):
nothing installed, the mismatch named. On an Intel Mac: the script refuses and points to building
from source. (Under Rosetta on Apple silicon, `arch -x86_64 sh …`, it installs: the machine is Apple
silicon, which `hw.optional.arm64` reports either way, contracts/cli-macos.md.)

## Q3 One window (US1, FR-001, FR-002)

Open Fallow from Finder, Spotlight and the Dock, and run `fallow`. Expect: one window, one Dock icon,
titled Fallow with its icon in the Dock and in Command-Tab; no browser tab opens.

## Q4 Everything works in the window (FR-005, FR-007, SC-005)

Walk 013 FR-005's screens as in 014's Q3: theme switches (the title bar follows, no light flash on
open in Dark), the folder picker (a sheet on the window), images by dialog and by dragging, ⌘C ⌘V ⌘Z
⇧⌘Z ⌘A in text fields, ⌘+ ⌘− ⌘0, ⌘R (the Studio reloads in place), a pull request link (opens in
the default browser), a file dropped outside the message box (nothing happens), the chat panels'
width (kept after a restart, now from `prefs.json`).

## Q5 Closing keeps work (US2, FR-008, SC-006)

Start a loop with the fake agent; close the window with ⌘W. Expect: the loop advances; the menu
bar icon and the Dock icon stay; Open Fallow and a Dock click reopen the window where it was.
Automated: the `automation` build's check opens the window, starts the loop, closes the window,
waits for completion.

## Q6 Quit from everywhere (US2, FR-009)

With a loop running, try ⌘Q, the app menu's Quit, the menu bar icon's Quit, the Dock's Quit and
`fallow quit`. Expect: each asks, naming the loop; Cancel changes nothing; Quit stops the service,
and the next start resumes the loop without repeating a completed step. With nothing running: no
question. Log out with a loop running: logout is not blocked; the loop resumes at the next start.

## Q7 Notifications (US3 AS5, AS6, FR-013, FR-014)

With the window closed, make an item need the engineer. Expect (research M-9): a
notification naming project, feature and what is needed, and its click opening the item;
"Show banners" in the setting brings macOS's prompt, and Allow gives banners.
Decline the permission (or on a Mac where it is refused): the menu bar icon shows its dot, the
inbox shows the item, and the notifications setting says why system notifications are unavailable.

## Q8 No browser page in a release (US5, FR-015, FR-018, SC-007)

With the release app open: `lsof -nP -iTCP -sTCP:LISTEN` shows nothing of Fallow's or of its
WebKit processes; `curl` to any local port gets nothing of the Studio. A test build still serves
its browser page when asked: `cargo build --features automation`, then `FALLOW_AUTOMATION=1
target/debug/fallow` opens it in the browser (a plain `cargo build`, like a release, has none).
Automated: `cargo xtask no-listener` on the release app (CI), `tests/release_no_page.rs`.

## Q9 Budgets (FR-031)

`cargo xtask bench` on macOS: window ready p95 < 1 s; idle < 250 MB with the window open (Fallow +
its WebKit processes, physical footprint); under 60 MB 10 s after closing the window (FR-010); from a
cold start (`open -a`, no service running) the window drawn < 1 s (`--app <Fallow.app>`, CI only:
LaunchServices uses the real `HOME`); on the `automation` build, running < 400 MB and activity < 2 s.
`close_ends_page` checks the WebKit private calls (no page left after a close, a minimized page
still updated), `close_keeps_work` that work goes on with the window closed (Q5's automated half).
`cargo xtask screens` walks Q4's screens in light and dark (pictures, page errors, width, ground).

## Q10 Upgrade and uninstall (US4 AS6, AS7, FR-027)

With an older version running, run the install command for a newer one. Expect: the Quit
confirmation first; after it, the new version opens and runs resume; data and settings kept. Then
`--uninstall`: the app, the link and the `~/.zprofile` line are gone; projects and the data
folder are untouched and the data folder is named.

## Q11 Release (US4 AS1, FR-021, SC-008)

Push a version tag on `main` in a test fork. Expect: one GitHub release holding the Windows
installer and the macOS disk image, each with its `.sha256`; the notes' macOS section (not
notarized and why, first-launch steps, the install command, permissions after upgrades, WebKit's
list); the CI signature check on the image downloaded from the release passes.
