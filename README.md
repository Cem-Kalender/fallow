# Fallow

Fallow is a terminal workspace for developers who run several LLM coding agents at once. It
opens on your projects and the agent sessions running in them. Each session is a full terminal
for an agent CLI (Claude Code, Codex, Gemini CLI, Aider or a shell), in its own git worktree by
default. Next to them, a live panel shows every LLM process on the machine with its memory, CPU,
status and working directory. A file editor is one key away. Fallow itself stays small enough to
leave the RAM to the agents.

**Status: pre-release.** It builds from source and passes its CI gates on Linux, macOS and
Windows; there are no release binaries yet. The specifications, plans and research are in
[`specs/`](specs/), and the project's principles are in
[`.specify/memory/constitution.md`](.specify/memory/constitution.md).

## Budgets

Enforced by CI benchmarks on all three platforms:
- under 50 MB of memory when idle, under 150 MB with a large project open and 4 agent sessions
  each holding a full scrollback (agents themselves not counted);
- first screen in under 100 ms, with 50 remembered projects;
- a key typed into a session is on screen in under 50 ms (p95), and under 100 ms while seven
  other sessions print as fast as they can;
- no root or administrator rights needed;
- no data leaves the machine, except optional DNS lookups of known LLM provider hostnames.
  Session output and scrollback stay in memory and are never written anywhere.

## Getting started

```sh
cargo build --release
target/release/fallow [PATH]      # PATH is added to the project list and selected
```

| Key | What it does |
|---|---|
| `Ctrl-O` or `a` | Add a project (folder). The list is remembered. |
| `n` | Start an agent session in the selected project. Git projects get a new worktree (`fallow/<agent>-<n>`) by default. |
| `Enter` | Focus the selected session. Every key now goes to the agent. |
| `Ctrl-]` | Leave the session (keys go to Fallow again). Press it twice to send `Ctrl-]` to the agent. |
| `Ctrl-N` / `Ctrl-B` | Next / previous session |
| `Ctrl-T` | All sessions of all projects |
| `Ctrl-E` | Switch between the agent workspace and the editor view of the selected project |
| `f` | Show only the selected project's LLM processes |
| `?` / `F1`, `Ctrl-P` | Help with every key, and the command palette |
| `Ctrl-Q` | Quit. Running sessions are listed and stopped; agents never outlive Fallow. |

Sessions that need you (the agent rang the bell, sent a notification, or went quiet) are
marked, and the project list counts them. The built-in Claude Code and Codex profiles turn on
their bell notifications for that run only, with command-line flags; Fallow never edits an
agent's settings files.

Configuration lives in the platform config directory (`fallow --print-config-paths`):
`settings.toml` (including `[sessions]`), `profiles.toml` for your own agent profiles, and
`projects.toml`, the remembered project list, which Fallow writes. The palette's "Reload
configuration" reads `profiles.toml` again.

## Known gaps

- **No kitty keyboard protocol inside sessions.** Agents fall back to their legacy keys: in
  Claude Code, `Shift-Enter` does not insert a newline; use `Ctrl-J` or `\` then `Enter`.
- **No reflow on resize.** When a pane changes size, the agent is told the new size and redraws;
  text already in the scrollback keeps its old line breaks.
- **Windows: npm-installed agents.** npm installs agents as `.cmd` shims. Fallow runs the program
  a shim points to (`node` with its script, or the agent's `.exe`) instead of going through
  `cmd.exe`. A shim it cannot read is refused with the reason; point a profile in
  `profiles.toml` at the real executable instead.
- **Gemini CLI and Aider** have no per-run notification flag. Fallow uses Gemini CLI's window
  titles, the terminal bell, and "quiet for 5 s" to mark them as needing attention.
- **WSL2:** processes inside WSL2 are not visible to the Windows build.

## Platform support

| Platform | Status |
|---|---|
| Linux | Builds; CI gates pass |
| Windows | Builds; CI gates pass |
| macOS | Builds; CI gates pass. **Beta** until hardware verification passes (below) |

**macOS hardware verification (open)**: macOS releases are labeled "beta" until they have been
verified on a physical Apple Silicon Mac (M1 or later; not a GitHub-hosted runner, not an Intel
Mac). The checks are:
- memory figures vs Activity Monitor with a running local model;
- the command-line read on macOS not retaining environment data;
- the canary privacy test;
- a manual run-through in Terminal.app and iTerm2, including agent sessions.

Details are in [plan.md, Release blockers](specs/001-llm-aware-tui-editor/plan.md#release-blockers).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
