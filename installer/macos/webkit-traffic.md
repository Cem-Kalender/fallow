<!-- 016 (FR-017, constitution V, T043): what WebKit's own processes were seen connecting to with
Fallow's window open and idle for 10 minutes (CI job `trace-macos`), copied into the release
notes. Hosts go one per line as `- host`; the trace flags a looked-up name not listed. Filled from
the run of 2026-11-23 (PR #7, macOS 15): no connection of WebKit's processes or Fallow's own. -->
Measured before this release: with Fallow's window open and idle for 10 minutes, neither Fallow
nor WebKit's processes opened any network connection.
