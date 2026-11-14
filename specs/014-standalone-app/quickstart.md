# Quickstart: checking 014 (Windows)

Prerequisites: Windows 10 or 11 x64 with the WebView2 runtime; the package built by CI
(`Fallow-<version>-x64-setup.exe`) or a local build (`cargo build --release`); the fake agent
(`fallow-fake-agent`) for scripted runs.

## Q1 Install as a standard user (US4, FR-019, SC-001)

Run the setup as a user without administrator rights. Expect: no elevation prompt; Fallow in the
Start menu with its name and icon; installed under `%LOCALAPPDATA%\Programs\Fallow`. Start it from
the Start menu: the window shows the dashboard, titled Fallow, with Fallow's icon in the taskbar.

## Q2 One window (US1, FR-002)

With the window open, run `fallow` in a terminal and start Fallow again from the Start menu.
Expect: no second window, the existing one comes to the front (not just flashing in the taskbar),
no browser tab and no console window opens.

## Q3 Everything works in the window (FR-005, SC-004)

Walk 013 FR-005's screens: dashboard, a project with its conversation, a spec session, a loop,
pipelines, agents, the data page, with their dialogs and panels; switch the theme (the frame
follows); add a project with the folder picker; attach an image by dialog and by dragging; copy and
paste; open an attached image (it shows in the page's viewer); drop a file outside the message box
(nothing happens); click a pull request link (it opens in the default browser, the window stays).
The visual checks of 009, 010 and 013 run in the real window: `cargo build --release --features
automation`, the service started with `FALLOW_AUTOMATION=1` and `FALLOW_AUTOMATION_CDP=<port>`,
then the checks' scripts against that port.

## Q4 Closing keeps work (US2, FR-008, SC-005)

Edit an agent definition without saving and close the window: the page asks "Leave your
changes?"; keep them: the window stays. Start a loop with the fake agent, close the window. Expect:
the loop advances (`fallow status`), the icon stays, Fallow's web view processes are gone (Task
Manager). Reopen from the icon: the window is where it was. CI does the loop part on every push
(SC-005).

## Q5 Quit asks (US2, FR-009, SC-008)

With the loop running, choose Quit from the icon. Expect: a confirmation naming the loop; Cancel
changes nothing; Quit again and confirm: the service stops; `fallow`: the loop resumes without
repeating a completed step. With nothing running, Quit asks nothing.

## Q6 Nothing reaches the Studio but the window (FR-015, SC-006, SC-007)

With the release build running and the window open: the system's connection table lists no
listening port for `fallow.exe`; a browser or `curl` to any old address reaches nothing. Over
10 minutes idle, `fallow.exe` opens no outbound connection, and the web view's connections are
only those listed in research R-6.

## Q7 Budgets (FR-025, SC-002, SC-003)

`cargo xtask bench` on the release build (window ready, idle memory, idle after close) and on the
`automation` build (running memory, activity latency). Expect every line `ok`.

## Q8 Sign-out, upgrade, uninstall (FR-020, FR-021)

Sign out with a loop running: no dialog, and the loop resumes when Fallow is next opened. Install a newer package while a loop runs:
the installer shows the Quit confirmation; after it, the loop resumes in the new version. Uninstall:
the app, shortcut and notification identity are gone; projects and (by default) the
data folder stay, and the uninstaller named the folder.
