# Fallow

Fallow is a desktop app for spec-driven development with coding agents. You work out each
feature with a lead agent, following [Spec Kit](https://github.com/github/spec-kit): constitution,
specify, clarify, plan, tasks, analyze. When the documents are clear you lock them. A loop then
builds the whole locked `tasks.md`, task by task, and Fallow checks the work itself: it commits
each task, runs your gates against a baseline, and has a reviewer check every acceptance scenario
by running something. You merge once, for the whole feature.

**Status: pre-release.** It builds from source on Windows, its release target, and passes its CI
gates there; there are no release binaries yet. The specifications, plans and research are in
[`specs/`](specs/) (this version is [`specs/007-fallow-studio/`](specs/007-fallow-studio/)), and
the project's principles are in [`.specify/memory/constitution.md`](.specify/memory/constitution.md).

## What it does

- **Projects.** Add any local folder, git repository or not. Fallow installs Spec Kit's agent
  skills where they are missing and lists the project's features (`specs/NNN-*`) with their stage:
  spec session, locked, building, ready to merge, merged.
- **Spec sessions.** A chat with the lead next to `spec.md`, `plan.md`, `tasks.md` and the
  constitution, which update live and which you can edit yourself; the lead sees your edits on its
  next turn. Clarify questions come as choices with a recommendation, open `[NEEDS CLARIFICATION]`
  markers are highlighted, and "Lock & start the loop" opens once tasks and analyze are done.
- **The loop.** One builder works through the locked tasks, phase by phase, in one session per
  phase. After each task Fallow commits it and runs the gates against the baseline; at each user
  story's checkpoint a reviewer answers every criterion with what it ran; checkers a pipeline
  marks to run alongside each other run at the same time and are judged together. Failures go back
  to the builder for one repair, capped, then to you. A result a pipeline left unverified (for example
  without gates) ends as "Done, not verified by Fallow", never as ready to merge.
- **Watching and steering.** The loop page shows the pipeline and its current stage, what Fallow
  verified per user story (each criterion with what was run and what came out; a failure opens
  by itself), the task tree, one card per agent, one activity feed (what each agent says, reads,
  edits and runs, with output), Fallow's own checks, time and cost against the budget. From its
  header you pause, stop, leave a note for the next step and change limits.
- **Questions through the lead.** Agents ask through Fallow's `ask` tool. When the spec or the
  spec session settles it, the lead answers, Fallow commits the answer to the spec, and the agent
  goes on; you can overrule it. Otherwise the lead relays the question to you with options and a
  recommendation, and the loop waits for your answer.
- **The dashboard.** A "Needs you" inbox (questions, features ready to merge, stopped loops), the
  running loops with their phase progress, and desktop notifications from Fallow's tray icon,
  which name the project, the feature and what is needed, and never code or output.
- **Agents and pipelines.** Agents are markdown files with frontmatter in `~/.fallow/agents/`,
  overridden per project in `.fallow/agents/`: makers change files, checkers only run and verify.
  The Agents tab edits them as form fields and instructions; save for all your projects or this
  project only. The Pipelines tab draws a pipeline as a graph of Fallow's fixed steps, where you
  add, change and remove steps; Standard, Test first and Solo ship with Fallow, and the lead can
  propose one, which is used only once you save it.
- **Light and dark.** The page follows your system's light or dark setting, switching with it
  without a reload. The control at the right of the top bar picks Light, Dark or System; Fallow
  remembers the choice across restarts. Text and controls meet WCAG AA contrast in both themes.
- **Runtimes.** Every agent step goes through one runtime interface. Claude Code (`claude -p`) is
  the first; a scripted `fake` runtime drives the tests.

## Getting started

### What you need

- Windows 10 or 11 (x64), with the Microsoft Edge WebView2 Runtime. Windows 11 has it; on Windows
  10 Fallow tells you if it is missing and offers Microsoft's download page.
- [Git](https://git-scm.com/download/win).
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code), for the agents (below).

### Install

There is no published release yet. Until there is, either:

- **take the installer CI builds**: on GitHub, open *Actions*, pick the latest green run of `ci` on
  `main`, and download the `fallow-windows-x64` artifact (you need to be signed in to GitHub; it is
  kept 90 days). Unzip it, check the file against its `.sha256`, and run
  `Fallow-<version>-x64-setup.exe`. It installs for you only, with no administrator rights, and
  adds Fallow to the Start menu. It is not signed yet, so Windows SmartScreen warns about it: choose
  *More info*, then *Run anyway*. Uninstall from Windows' *Installed apps*; your projects are never
  touched, and you choose whether Fallow's own data goes too.
- **or build it** with Rust (`rustup` installs the toolchain the repository names):

  ```sh
  git clone https://github.com/Cem-Kalender/fallow
  cd fallow
  cargo build --release
  target/release/fallow          # starts Fallow and opens its window
  ```

### Connect Claude Code

There is nothing to link: Fallow runs the `claude` command itself, as you, whenever an agent works,
and uses the sign-in Claude Code already has. So:

1. Install Claude Code, so that `claude` runs in a terminal.
2. Sign in once, in a terminal: run `claude` and log in as it asks. Fallow never sees your password
   or keys.
3. Open Fallow. It finds `claude` on your `PATH`; where it is missing or not signed in, Fallow says
   so before any work starts.

To use another `claude` (one not on your `PATH`), set it in Fallow's `settings.toml`, in its
config folder (`%APPDATA%\fallow`):

```toml
[agents.claude]
command = "C:/path/to/claude.exe"
```

### Use it

Open Fallow from the Start menu, or with `fallow` in a terminal. Add a project folder with **+**,
work out a feature with the lead, lock it, and let its loop build it.

```sh
fallow            # opens Fallow's window (starts it if needed)
fallow quit       # quits, asking first when loops or sessions are working
fallow status     # lists the loops
fallow stop       # stops Fallow at once; loops resume at the next start
```

The Studio is shown only in Fallow's own window, through the system's web view; nothing listens on
any port, and no browser can reach it. Closing the window keeps Fallow and its loops running in
the background, with its icon in the notification area; Quit (the icon's menu, the window's system
menu, or `fallow quit`) stops it after naming the work that would pause. On other platforms the
Studio is still a page on `127.0.0.1` with a secret per launch, until their own window comes.

Fallow's settings are in its config folder (`settings.toml`), its data (loops, conversations,
worktrees, run records) in its data folder, readable by you only. The dashboard's "What Fallow
keeps on disk" shows how large each part is and deletes what you no longer need. On Windows, Fallow
also registers its name and icon for its notifications, for you only, under
`HKCU\Software\Classes\AppUserModelId\Fallow`; deleting that key undoes it.

## Budgets

Measured by `cargo xtask bench` in CI (constitution VIII), on Fallow's process and its window's web
view processes, as each one's own memory (private working set) summed:
- the window shows the Studio within 1 s of `fallow` (p95 of 30 launches after 3 warm-ups);
- under 250 MB of memory when idle with the window open (60 MB with it closed), and under 400 MB
  with 4 agent steps running at once;
- an agent's activity appears within 2 s.

Fallow makes no network requests of its own; the traffic of a loop is the agents' own traffic
to their providers.

What the window's web view (Microsoft Edge WebView2) does that Fallow cannot switch off: Windows'
required diagnostic data from the runtime (optional data follows Windows' own setting), and
updates of the WebView2 runtime through Microsoft Edge Update. Fallow switches off SmartScreen
lookups and keeps the web view's crash reports on the machine.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --features automation   # the fake agent: no network, no cost
cargo build --features automation && cargo xtask bench   # the budgets above
```

Licensed under MIT or Apache-2.0, at your option.
