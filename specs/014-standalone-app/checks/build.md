# Build cost after T001 (2026-11-14, the engineer's laptop)

- Dependencies added on Windows: `wry` 0.57.2, `raw-window-handle` 0.6, and through `wry`:
  `webview2-com` 0.39, `windows` 0.62 (and its `windows-*` crates), `http`, `dpi`, `dunce`,
  `cookie`, `thiserror`. 60 crates in the Windows graph in all. Licenses all on the allowed list;
  no banned crate (`tao`, `winit`, `gtk`, `webkit2gtk`, the HTTP clients) in the graph.
- Debug build of the binaries (`-j 1`, no debug info, `wry` and `windows` not built before):
  2 min 03 s. The spike's release build of the same stack from nothing: 5 min 03 s.
- Sizes (debug): `fallow.exe` 17.2 MB (before 014: 17.1 MB; `wry` is not used yet),
  `fallow-app.exe` 0.16 MB. The spike's release target folder: 437 MB.
