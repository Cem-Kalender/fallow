# Contract: the window on macOS

What 014's `contracts/window.md` becomes on macOS. Anything not named here is as in 014.

## 1. Asking for the window (the per-user socket)

The pipe on macOS is 006's Unix socket in the data folder (`service.sock`, the folder private to
the user). The commands are 014's (`window`, `window` with `route`, `quit`, `quit` with
`confirmed`, `url` in `automation` builds), with these differences:

| Request | Difference on macOS |
|---|---|
| `{"cmd":"window",…}` | No `AllowSetForegroundWindow`: the service activates itself (`NSApplication activate`). The error for a service without a display is "Fallow was started without a display: stop it (`fallow stop`) and open Fallow from the Mac's desktop". |
| `{"cmd":"status"}` | Answers also `"version": "<Cargo.toml version>"`, on every platform (research M-22). |

## 2. The Studio in the window

- Scheme `fallow`, served as **`fallow://localhost/`** (WebKit's form). The first load is
  `fallow://localhost/`.
- Requests, answers, headers (CSP, `nosniff`, `no-store`) and the long poll as in 014 §2.
- The data store is non-persistent (research M-6).

## 3. Navigation (`window::nav`, both platforms)

The decision function takes the platform's Studio origin (`http://fallow.localhost` on Windows,
`fallow://localhost` on macOS):

| What | Windows | macOS |
|---|---|---|
| A navigation within the Studio's origin | Loads. | Loads. |
| A new window for the Studio's origin | Refused. | Refused. |
| A navigation or new window elsewhere, user-initiated per WebView2 | Cancelled; opened in the browser (014). | — (`wry` gives no such flag) |
| A navigation or new window elsewhere, any | Refused unless the row above applies. | Refused; nothing opened. |
| `open:<url>` from the page (§4), `http`/`https` | Opened in the default browser. | Opened in the default browser (`NSWorkspace openURL`). |
| `open:<url>`, any other scheme | Ignored. | Ignored. |
| A download | Refused. | Refused. |
| Reload (F5, Ctrl+R / ⌘R) | Reloads the Studio with the current route. | The same. |

## 4. The page's IPC messages, added

| Message | When | Window does |
|---|---|---|
| `open:<url>` | The engineer clicks a link to an address outside the Studio (a trusted `click` event, caught by `app.js` on every platform) | Opens it in the default browser if §3 accepts it. |

014's messages (`ready`, `theme:…`, `close`, `asking`, `kept`, `activity:…`) are unchanged.

## 5. The app menu (macOS)

| Menu | Items |
|---|---|
| Fallow | About Fallow; Hide Fallow ⌘H; Hide Others ⌥⌘H; Show All; Quit Fallow ⌘Q |
| Edit | Undo ⌘Z; Redo ⇧⌘Z; Cut ⌘X; Copy ⌘C; Paste ⌘V; Select All ⌘A |
| View | Actual Size ⌘0; Zoom In ⌘+; Zoom Out ⌘−; Reload ⌘R |
| Window | Minimize ⌘M; Zoom; Close ⌘W |

No Developer menu in releases; debug and `automation` builds allow WebKit's inspector.

## 6. The menu bar icon (macOS)

Open Fallow; Notifications (checkmark); Quit Fallow. A dot on the icon while something needs the
engineer (data-model, Menu bar icon).
