# Contract: the window

How the window is asked for, how it gets the Studio, and what the page tells it. Windows only.

## 1. Asking for the window (the per-user pipe, 006)

The pipe is the one 006 made: per user, refused to other users. New commands:

| Request | Answer | Does |
|---|---|---|
| `{"cmd":"window","launched":<unix ms>}` | `{"ok":true}`, or `{"ok":false,"error":…}` | Shows the window, or brings it to the front. Answers once the window exists (not when it is drawn). `launched` is the launcher's start, for `window ready` (R-14). Error when the service has no desktop (R-13) or there is no WebView2 runtime (R-12; the native message is shown too). The launcher calls `AllowSetForegroundWindow(<service pid>)` first. |
| `{"cmd":"window","route":"#/…"}` | as above | Also sends the page to that route (a notification's item). |
| `{"cmd":"quit"}` | `{"ok":true,"running":[…]}` | **Asks nothing**; lists the loops and sessions that would pause (each `{"kind":"loop"\|"session","project":…,"name":…}`), for the caller's confirmation. |
| `{"cmd":"quit","confirmed":true}` | `{"ok":true,"pid":<service pid>}` | Asks the page about unsaved edits when the window is open (`{"ok":false,"error":"kept"}` when the engineer keeps them), then stops the service as `stop` does. The caller waits for `pid` to exit when it must (`fallow quit --installer`). |
| `{"cmd":"url"}` | `{"ok":true,"url":…}` | **`automation` builds only**, with `FALLOW_AUTOMATION=1`: the test transport's address with its secret. Absent from release builds. |

`stop` (006) is unchanged: it stops without asking (the terminal command is the engineer's own
explicit act). `fallow quit` (contracts/cli.md) is `quit` with a native confirmation.

## 2. The Studio in the window (custom scheme)

- Scheme `fallow`, served as `http://fallow.localhost/` (`wry`'s Windows form). The window's first
  load is `http://fallow.localhost/`.
- Every request (any method, its path, query, headers and body) is answered by `answer` (R-3) with
  the routes of 006 to 013's `web-api.md` unchanged, except:
  - no secret: no `?k=`, no cookie, no origin check (only the window can make these requests);
  - `GET /` is `index.html` with 013's theme, as today.
- Image uploads are sent as the file's bytes (`arrayBuffer`), not as a `File` (R-3).
- Answers are made on a worker thread per request; the long poll `GET /api/events` answers within
  25 s as today.
- Headers kept: `Content-Security-Policy` (`default-src 'self'; script-src 'self'; …` as today),
  `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`.

## 3. Navigation

One decision function (R-4), unit-tested:

| What | Result |
|---|---|
| A navigation to `http://fallow.localhost/…` | Loads. |
| A new window for `http://fallow.localhost/…` | Refused (the page never asks: images open in its own viewer). |
| A navigation or new window to an `https://` or `http://` address elsewhere, user-initiated | Cancelled in the window; opened in the default browser. |
| The same, not user-initiated | Refused; nothing opened. |
| Any other scheme (`file:`, UNC, `ms-*:`, `search-ms:`, `javascript:`, …) | Refused; nothing opened. |
| A download | Refused. |
| A file dropped outside the message box | Cancelled by the page; no navigation. |
| Reload (F5, Ctrl+R) | Reloads `http://fallow.localhost/` with the current route. |

## 4. The page's IPC messages (`window.ipc.postMessage`, page → window)

Plain strings; anything else is ignored.

| Message | When | Window does |
|---|---|---|
| `ready` | The page's first render after `start()` | With `FALLOW_BENCH` set, logs `window ready <ms>` (since `launched`, R-14). |
| `theme:light`, `theme:dark`, `theme:system` | The engineer picks a theme (013's control) | Sets the frame's dark mode (R-7). |
| `close` | The page's answer to `Fallow.closeAsked()`: no unsaved edits, or the engineer chose to leave them | Closes the window, or goes on with Quit (R-9). |
| `activity:<session id>` | An activity line is drawn for a session | With `FALLOW_BENCH` set (`automation` builds), logs `activity <session> <unix ms>` (R-15). |

No message changes anything but the window's own frame and the bench's log; the page's work goes
through the Studio's routes as today.

## 5. Window → page

- A route from `{"cmd":"window","route":…}` is set as the page's `location.hash`.
- `Fallow.closeAsked()`: called before closing the window and before Quit. The page answers `close`
  at once without unsaved edits; with them it shows 009's "Leave your changes?" and answers `close`
  only if the engineer leaves them.
- Nothing else; the page learns everything through its routes.

## 6. The `automation` transport (feature `automation`, never in a release)

With `FALLOW_AUTOMATION=1` in the service's environment, the `tiny_http` transport of 006 to 013
also runs: loopback only, a per-launch secret, the cookie and origin checks as before 014, the
same `answer`. The bench gets its address with the secret over the pipe (`{"cmd":"url"}`); the
service's stderr (written to `service.log`) carries the port only. With
`FALLOW_AUTOMATION_CDP=<port>`, WebView2 also opens its debugging port on loopback, for checks that
drive the real window (SC-004). Without the variables, nothing listens even in an `automation`
build.
