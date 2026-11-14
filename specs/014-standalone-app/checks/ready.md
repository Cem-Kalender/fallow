# Window ready (T015), 2026-11-14, the engineer's laptop, debug build

Measured with `FALLOW_BENCH=1` (the service logs `window ready <ms>` from the moment the window is
asked for to the page's first render), the service already running, the window closed between
opens, the engineer's real data (one project, 11 loops):

- first open after the service started: 878 ms (the WebView2 profile already existed);
- then 898, 765, 794, 806, 788 ms.

Under 1 s in a debug build; the release build and the CI benchmark (T028, p95 of 30 after 3
warm-ups, from `fallow-app.exe`'s start) decide whether the engine's first answers to the
dashboard need to be faster.
