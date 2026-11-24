# Spike 016 results: the window (research M-18), 2026-11-22

Machine: the engineer's Mac (Apple silicon, macOS 26.7.3, managed), **screen locked** during every
run (nobody at the Mac). `wry` 0.57.2 on our own `NSWindow` (no tao), the Studio's real pages and
data forwarded from a `fallow serve` (release build, throwaway `HOME`) through `fallow://localhost/`
(one extra local HTTP hop the real app will not have). Built and run by `cargo build --release`
here, `target/` ignored. Not run on the `macos-15` runner.

## Window ready (open to the dashboard drawn with its data, service running)

| Store | First open | Median | p95 (30 opens) |
|---|---|---|---|
| default | 169 ms | 70 ms | **84 ms** |
| non-persistent (`with_incognito`) | 186 ms | 73 ms | **88 ms** |
| as an ad-hoc signed app, hardened runtime, non-persistent | 179 ms | 116 ms (3 opens) | – |

Budget 1 s: met by a factor of ten. (Windows' WebView2: p95 1.02 s.)

## Memory (physical footprint; this process plus the WebKit processes it is responsible for)

| | Default store | Non-persistent |
|---|---|---|
| Window open, settled 8 s (round 0 / round 29) | 73.1 / 76.3 MB | 70.5 / 75.3 MB |
| Window closed, 5 s later (round 0 / round 29) | 37.1 / 46.3 MB | 36.2 / 45.9 MB |
| The service alone (`fallow serve`) | 2.9 MB | |

Open: the spike ~23 MB, WebContent ~30 MB, GPU ~16 MB, Networking ~4 MB. Fallow's idle budget is
250 MB: about 80 MB with the window open. Closed, WebKit's Networking and GPU processes (~16 MB)
stay while the app runs; the page's process ends (with the fix below).

## Findings that change the design

1. **`wry` leaks the web view on macOS.** Its `Drop` removes the `WKWebView` from its window and
   then calls `retain()` on it on purpose (`wry-0.57.2/src/wkwebview/mod.rs`, `impl Drop`), so the
   web content process never ends: one more process per open (10 opens: 10 processes, 30 MB each).
   Calling WKWebView's `_close` (SPI) before the drop ends it every time (30 of 30 opens).
2. **A hidden page freezes.** With the window hidden (here: screen locked; for users: minimized or
   covered), `visibilityState` is `hidden` and after about 3 s WebKit suspends the page: timers
   stop and an answer arriving later than ~3 s (8, 12, 20 s tried) never reaches the page. The
   Studio's long poll (25 s) would stall. `WKPreferences`
   `_setPageVisibilityBasedProcessSuppressionEnabled:NO` and `_setHiddenPageDOMTimerThrottlingEnabled:NO`
   (SPI) keep it running: all four answers arrived, timers ran every second.
   `_setWindowOcclusionDetectionEnabled:NO` alone changed nothing.
3. **A `File` body arrives empty through the custom scheme** (0 of 5,242,880 bytes); the same bytes
   as an `ArrayBuffer` arrive whole. The chat sends pasted and dropped images as the `File`
   (`src/service/web/screens/solo.js`, `body: f`): it must send `await f.arrayBuffer()`, which also
   works on Windows.
4. **Non-persistent store writes nothing.** The default store wrote
   `~/Library/WebKit/<executable>/WebsiteData/ResourceLoadStatistics/*.db`; with `with_incognito`
   nothing was written under `~/Library` (checked by modification time).
5. **Hardened runtime + ad-hoc signature**: `flags=0x10002(adhoc,runtime)`, `codesign --verify
   --strict` passes, the window opens and draws through `open`, no entitlement needed. Started as
   an app, it is the responsible process for its WebKit processes (counted); started from a
   terminal, the terminal is, so the CI count must start Fallow through LaunchServices.

## Not measured here

- Drag and drop of an image onto the page, and the visible (unlocked) behaviour of finding 2's
  default: they need someone at the Mac; a check in the tasks.
- The `macos-15` runner: the CI job's first run.
