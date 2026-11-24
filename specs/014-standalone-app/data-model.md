# Data model: Fallow as a standalone app (014)

014 adds no run, conversation or project data. It adds what the window needs to reopen as it was,
Quit's steps, and the install package's footprint.

## Window state (`<data>/window.json`)

| Field | Type | Rule |
|---|---|---|
| `x`, `y` | integer | The restored (not maximised) rectangle's top-left, in virtual-screen pixels. |
| `width`, `height` | integer | The restored rectangle's size; at least 800 × 500. |
| `maximized` | bool | Reopens maximised on the remembered screen. |

- Written when the window closes and when Fallow stops; read when the window opens.
- Unreadable, missing or off every screen (no monitor for the rectangle): the window opens at
  1280 × 800, fitted to the main screen's work area, centred (FR-003).
- No other window state is kept: which screen of the Studio was shown is the page's own (013 and
  earlier keep what they keep across reloads).

## Window (in memory, the service)

States: **closed** → **opening** (web view being made) → **open** → **closing** (asking the page)
→ **closed**.
- `fallow`, `fallow-app.exe`, the icon's Open Fallow and a notification click ask for **open**:
  closed → opening; opening, open or closing → brought to the front, never a second creation (one
  window, FR-002; WebView2's creation pumps messages, so requests can arrive while opening). A
  notification click also carries the item's route, which the page is sent to (when opening, once
  open).
- **Close** (the close button, the system menu's Close window): open → closing: the page is asked
  (`Fallow.closeAsked()`); its `close` answer, or 3 s without an answer, → closed: the state is
  written, the web view dropped and the environment released. The page keeping its edits →
  open.
- **No runtime** (R-12): closed stays closed, with the native message.
- **Web view crash** (`ProcessFailed`): open shows a native message with Reload; the engine is
  untouched.

## The theme seen by the window

Read from 013's `prefs.json` (`theme`: system, light, dark) and, for system, the user's
`AppsUseLightTheme`. The page tells the window of a change (contracts/window.md, `theme`).

## Quit

States: **asked** → (the page's unsaved edits, when the window is open) → (the running work, asked
on a worker thread) → **confirming** (a task dialog naming each loop and session; skipped when
nothing runs) → **stopping** (as `fallow stop`) or back to nothing on Cancel. The end of the Windows
session goes straight to **stopping**, with no dialog.

## Install package

| Item | Where |
|---|---|
| Programs | `%LOCALAPPDATA%\Programs\Fallow\fallow.exe` and `fallow-app.exe` (`fallow-fake-agent.exe` is **not** shipped) |
| Start menu | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Fallow.lnk` to `fallow-app.exe`, `AppUserModelID` `Fallow` |
| Uninstall entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Fallow_is1` (Inno Setup's) |
| Notification identity | `HKCU\Software\Classes\AppUserModelId\Fallow` (008), removed on uninstall |
| Data folder | `%LOCALAPPDATA%\fallow` (006, unchanged): kept on uninstall unless the engineer chooses; the uninstaller names it and says it holds the working copies of loops and sessions not merged yet |
| Config folder | 006's (the engineer's definitions): kept on uninstall and named |

`fallow.exe` keeps its data and config folders where 006 put them; the package changes nothing of
where Fallow writes at run time.
