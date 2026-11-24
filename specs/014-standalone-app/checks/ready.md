# Window ready (T015), 2026-11-14, the engineer's laptop, debug build

Measured with `FALLOW_BENCH=1` (the service logs `window ready <ms>` from the moment the window is
asked for to the page's first render), the service already running, the window closed between
opens, the engineer's real data (one project, 11 loops):

- first open after the service started: 878 ms (the WebView2 profile already existed);
- then 898, 765, 794, 806, 788 ms.

Under 1 s in a debug build; the release build and the CI benchmark (T028, p95 of 30 after 3
warm-ups, from `fallow-app.exe`'s start) decide whether the engine's first answers to the
dashboard need to be faster.

After the phase 3 review, measured from the launcher's start (`launched`, as research R-14 asks),
debug build: 1262 ms when `fallow` also started the service, then 885 and 867 ms with the
service running.

Known, left for later: when the window closes, answers still on their way to the page (its long
poll) cannot be delivered and `wry` writes "PostMessage failed" to the service log; the web view's
processes end all the same.
