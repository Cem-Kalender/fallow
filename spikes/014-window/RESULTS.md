# Spike 014 results (research R-20), 2026-11-14

Machine: the engineer's laptop (Intel i5-1035G7, 7.6 GB RAM, Windows 11), WebView2 runtime
157.0.4341.46. `wry` 0.57.2 with `build_as_child` on our own Win32 window, OLE initialised on the UI
thread, the Studio's real pages and data forwarded from the engineer's running Fallow through the
custom scheme `http://fallow.localhost/` (one extra local HTTP hop per request that the real app
will not have). Not run in CI.

## Build

- Release build of the spike (wry, `windows` 0.62, `webview2-com` 0.39, and Fallow), one job,
  from nothing: **5 min 03 s**; an incremental rebuild 23 s.
- Spike target folder: **437 MB**. The spike's exe 0.6 MB (Fallow linked as a library adds what
  it uses; Fallow's own debug exe is 17 MB).

## Memory (dashboard drawn, settled 8 s)

| Counted as | Default | `MemoryUsageTargetLevel=Low` + `--disable-gpu` |
|---|---|---|
| Working sets summed (7 processes) | 310 to 324 MB | 274 to 276 MB |
| Private bytes summed | 133 to 140 MB | 100 to 105 MB |
| Private working sets summed (Task Manager's "Memory") | 88 MB | 61 MB |

- Of the 7 processes, 6 are WebView2's (browser, GPU, network and storage utilities, renderer,
  crashpad). They share the same large browser libraries, which the working-set sum counts in each.
- `--renderer-process-limit=1` changed nothing (one renderer already); `Low` alone: 292 MB summed;
  `--disable-gpu` alone: 290 MB summed, 100 MB private.
- Window closed (web view dropped): every WebView2 process is gone within the 10 s wait, every
  round; the process is back to 20 MB working set, 3 MB private working set.

## Window ready (open to the Studio drawn, the service already running)

- First open ever (WebView2 creates its profile): 1.66 s.
- Then, 33 opens, each with a new web view and environment (as FR-011 requires: nothing of the web
  view kept while the window is closed): median 0.91 s, **p95 1.02 s**.
- Each open is about 0.47 s to create the environment and controller (WebView2 starting its
  processes) and 0.35 to 0.52 s for the page: its HTML in about 25 ms, then 5 API requests to the
  engine (the dashboard's projects and loops), the rest.

## Other checks

- **Upload**: a 5 MiB `File` body and the same as an `ArrayBuffer` both arrived complete through
  the custom scheme (5,242,880 bytes): the empty-body problem reported for WebView2 did not occur.
- **`with_environment`**: present in `wry` 0.57.2 (`WebViewBuilderExtWindows`), so Fallow can
  create the environment with its own options (crash reports off); not exercised by the spike.
- **User data folder** in a normal temp folder worked; under Fallow's private-ACL data folder: not
  tried.
