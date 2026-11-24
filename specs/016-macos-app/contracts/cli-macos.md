# Contract: the command line and the install command on macOS

## `fallow` on macOS

Changes to 014's `contracts/cli.md` for macOS. Exit codes as 006: 0 success, 1 error, 2 bad usage.

| Command | Does | Output |
|---|---|---|
| `fallow` | No display (research M-12): explains and exits 1. Inside `Fallow.app` (also through the `~/.local/bin` link) with no service running: asks LaunchServices to open the app, waits for its socket, then asks for the window. Outside a bundle: starts `fallow serve` in the background as today, then asks for the window. A running service of another version: offers to quit it (the Quit confirmation) and start this one (M-22). **Never opens a browser in a release build.** | Nothing on success; the reason on failure. |
| `fallow serve` | Runs the service in the foreground; with a display, its window opens when asked. | As 014. |
| `fallow quit [--installer]` | As 014, with the confirmation as an `NSAlert`. | As 014. |
| `fallow status`, `fallow stop`, `fallow --version`, `fallow mcp-bridge …` | Unchanged. | Unchanged. |
| *(launched by LaunchServices, no arguments)* | Becomes the service and opens its window. | Service log only. |

Development and test builds keep the browser page and `{"cmd":"url"}` (FR-018) as on Windows (014
research R-15): a build with the `automation` feature serves it when `FALLOW_AUTOMATION=1` asks, and
`FALLOW_AUTOMATION=1 fallow` then opens it in the browser as before 016 (T038, as built). A plain
build, like a release, has neither, and the release job checks the `automation` marker is absent.

## The install command (`installer/macos/install.sh`)

```sh
curl -fsSL https://raw.githubusercontent.com/Cem-Kalender/fallow/main/installer/macos/install.sh | sh
curl -fsSL …/install.sh | sh -s -- --version 0.2.0
curl -fsSL …/install.sh | sh -s -- --uninstall
```

| Situation | Behaviour | Exit |
|---|---|---|
| Intel Mac (`hw.optional.arm64` ≠ 1) | "Fallow's Mac download is for Apple silicon; on an Intel Mac, build it from source: <README link>". Nothing installed. | 1 |
| Run as root | "Run this as yourself, without sudo". Nothing installed. | 1 |
| Release or file not found | Names the version and the URL it tried. Nothing installed. | 1 |
| SHA-256 mismatch | "The download does not match its published SHA-256". Nothing installed; the temporary folder removed. | 1 |
| Fallow running, the engineer cancels the Quit confirmation | "Fallow is still running; nothing was changed". | 1 |
| Success | Lines saying: the version installed, `~/Applications/Fallow.app`, the `fallow` link, the `~/.zprofile` line if added, and "Open Fallow from Applications or type `fallow`". | 0 |
| `--uninstall` | Quits Fallow as above; removes the app, the link and the marked line; prints the data folder (`~/Library/Application Support/Fallow`) and config folder it kept. | 0 / 1 when cancelled |

The script uses only what macOS ships (`sh`, `curl`, `shasum`, `hdiutil`, `ditto`, `sysctl`). It
never calls `sudo`, `spctl`, `xattr -d com.apple.quarantine` or changes a system setting.
