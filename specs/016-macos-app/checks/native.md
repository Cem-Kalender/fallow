# Native checks by hand (T031)

What needs a person at an unlocked Mac: clicks, drags, macOS's own prompts and what the screen
shows. Everything that can run without one is automated elsewhere (see the last section).

**Status: not done yet.** Built on 2026-11-22/23 on a Mac whose screen was locked throughout, so
nothing below could be seen or clicked. To be done with the release build in `~/Applications`
(`installer/macos/make-app.sh`, then the install command or a copy), on an unlocked screen.

| # | Check | Expected | Result |
|---|---|---|---|
| 1 | In a text field: ⌘C ⌘X ⌘V ⌘A ⌘Z ⇧⌘Z | Each works as in any Mac app (Edit menu) | |
| 2 | ⌘+ (and ⌘=), ⌘−, ⌘0 | The page zooms in, out, back to actual size | |
| 3 | ⌘R on a project's page | The Studio reloads on the same page | |
| 4 | Attach an image with the open dialog | The image is attached | |
| 5 | Drag an image onto the message box | The image is attached (research M-18: not yet seen) | |
| 6 | Drop a file anywhere else in the window | Nothing happens; the window stays on the Studio | |
| 7 | Close the window, make an item need you, click the notification | The window opens on the item | |
| 8 | Notifications setting (a project's page): "Show banners" | Note whether macOS shows a prompt at all when going from quiet to full permission (the spike never saw it); after Allow, banners show and the button goes | |
| 8a | The first notification of a fresh install | It arrives quietly in Notification Center (no banner, no prompt) | |
| 8b | Decline macOS's notifications (System Settings → Notifications → Fallow → off) | The project page says "macOS does not let Fallow show notifications" with where to allow it; the "Needs you" list and the dot still show waiting items | |
| 8c | "+" to add a project | macOS's folder dialog as a sheet on the window; the chosen folder is added; Cancel adds nothing; typing a path still works | |
| 8d | Widen a chat (Expand), quit Fallow, open it again | The chat is still wide | |
| 9 | Minimize the window for a minute while a loop runs, then show it | The page is up to date at once (T042 checks the long poll too) | |
| 10 | Add a project in `~/Documents`, decline macOS's question | The message names `~/Documents` and Files and Folders → Fallow | |
| 11 | Open Fallow from the disk image without moving it, and from `~/Downloads` | The window works; note whether notifications are allowed there | |
| 12 | `fallow serve` in a terminal, inside the bundle (`~/Applications/Fallow.app/Contents/MacOS/fallow serve`) and a `cargo build` binary | Note what the Dock shows (name, icon) in each case (research M-3) | |
| 13 | The menu bar icon | Fallow's outline icon; Open Fallow, Notifications (checked), Quit Fallow; a dot while something needs you | |
| 14 | Quit (⌘Q) with a loop running | An alert (a sheet on the window) names the loop; Cancel keeps it, Quit pauses it | |
| 15 | A link to GitHub in a loop's page (a PR or check) | Opens in the default browser; the window stays on the Studio | |
| 16 | `fallow` from Terminal with Fallow already open behind other windows | Fallow's window comes to the front | |
| 17 | Log out with a loop running | The logout is not held up; the loop resumes at the next start | |

## Checked without a person (on the locked Mac, throwaway `HOME`)

- The window opens with `fallow`, cold and warm (one window; six cold starts in a row), and from
  the bundled binary started as the app; the Studio's dashboard renders in it (read back over
  IPC).
- Its place is kept and reused; closing (the red button's path, `performClose:`) asks the page,
  keeps the place and ends the page; a session turn finishes with the window closed
  (`tests/window_mac.rs`).
- `fallow stop`, `fallow quit` (also two at once) and a Quit Apple event end the service cleanly.
- Unit tests: navigation, `open:` links, window place, terminate decision, protected-folder
  message, chat width in `prefs.json`, bundle detection.
