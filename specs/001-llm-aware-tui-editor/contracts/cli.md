# Contract: Command Line

```text
fallow [OPTIONS] [PATH]
```

| Argument / option | Meaning |
|---|---|
| `PATH` | Project directory to open. If omitted, Fallow starts with no project (idle state). 002 (FR-017): the folder is added to the project list and selected; without it, Fallow opens on the remembered list. |
| `--all-users` | Show all users' processes (FR-031). Takes effect only when Fallow already runs elevated (root/Administrator). Otherwise Fallow starts normally and shows a notice. Fallow never requests elevation. |
| `--no-dns` | Disable provider hostname resolution for this run (same as `dns.enabled = false`). |
| `--config-dir <DIR>` | Use this directory instead of the platform config dir. |
| `--print-config-paths` | Print the resolved config file paths and exit. |
| `--version`, `-V` | Print the version and exit. |
| `--help`, `-h` | Print usage and exit. |

**Exit codes**: `0` normal quit; `1` startup failure (no terminal, unreadable `PATH`); `2` invalid
arguments.

## Test-only interface (hidden, not in `--help`)

These exist so CI can verify SC-002, SC-003, SC-007, SC-009 and the benchmarks without driving the
TUI. They are compiled into release builds because the benchmarks must measure the shipped binary,
but the snapshot options never start the TUI.

| Option | Behavior |
|---|---|
| `--snapshot-json` | Run two refresh cycles (needed for CPU deltas), print one JSON `Snapshot` (shape in data-model.md, `records` limited to LLM group members) to stdout, then exit. Respects `--no-dns`. |
| `--snapshot-hold <SECS>` | Same as `--snapshot-json`, then stay alive with all state in memory for `SECS` seconds so the harness can dump its memory (SC-007 canary test). |
| `--snapshot-stream <SECS>` | Run the refresh loop for `SECS` seconds and print one JSON line per refresh cycle: the timestamp, the cycle number and the `ProcessKey`s of the detected LLM processes. Lets latency tests (SC-004) watch processes appear and disappear. Respects `--no-dns`. |
| `--bench-stats <PATH>` | Starts the TUI normally. On exit, writes one line `pid=<own pid> ticks=<event-loop ticks> cycles=<procmon refresh cycles>` to `PATH`, so benchmarks can prove they measured a live Fallow (research R-12): the PID must equal the measured PID, and ticks or cycles must reach 90% of the expected count. `cycles` is 0 until the procmon worker exists. Since Phase 5 the line also ends with `buffers=<files opened> searches=<project searches run> copies=<copies made>`, the evidence `bench-memory --loaded` and `bench-search` check (T083, T084). |

The three snapshot options are mutually exclusive.

## Startup frame marker (benchmark contract)

The first complete frame always contains the key-hint bar. At startup no text input has focus,
so the bar contains `Ctrl-P Commands`. The startup benchmark (research R-13) detects the first
frame by that text. It must stay in the bar whenever no text input has focus. Changing it
requires updating the benchmark at the same time.
