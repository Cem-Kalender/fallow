# Contract: the command line after 014 (Windows)

Changes to 006's `contracts/cli.md`. Exit codes as 006: 0 success, 1 error, 2 bad usage.

| Command | Does | Output |
|---|---|---|
| `fallow` | Without a desktop (R-13): explains and exits 1. Otherwise starts the service in the background if it is not running, then asks it for its window (contracts/window.md). Opens no browser. | Nothing on success; the reason on failure (no desktop, no WebView2 runtime, the service did not start). |
| `fallow serve` | Unchanged: runs the service in the foreground. Its window opens only when asked. | As today, without a page address. |
| `fallow status` | Unchanged. | Unchanged. |
| `fallow stop` | Unchanged: stops without asking; runs resume at the next start. | Unchanged. |
| `fallow quit` | NEW. Asks the service what would pause; with running work shows the native Quit confirmation naming it; on confirm (or with nothing running) stops the service. `--installer`: the same, then waits until the service process (and the agents' `mcp-bridge` processes) have exited; exit 1 when the engineer cancels, so the installer stops. | Nothing; exit 1 when cancelled. |
| `fallow autostart` | Removed (FR-010 dropped). | — |
| `fallow --version`, `fallow mcp-bridge …` | Unchanged. | Unchanged. |

`fallow` and `fallow-app.exe` accept `--data-dir` and `--config-dir`, passed to the service they
start (the bench, R-14).

## `fallow-app.exe` (NEW, Windows GUI subsystem: no console)

| Command | Does |
|---|---|
| `fallow-app.exe` | What `fallow` does; errors are shown as native messages. The Start menu's target, and what the installer's "Launch Fallow" starts. |

Other platforms keep 006's behaviour (the browser page) until their own feature.
