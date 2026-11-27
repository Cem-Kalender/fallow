# Fallow

Fallow is a desktop app for spec-driven development with coding agents. You work out each
feature with a lead agent, following [Spec Kit](https://github.com/github/spec-kit): constitution,
specify, clarify, plan, tasks, analyze. When the documents are clear you lock them. A loop then
builds the whole locked `tasks.md`, task by task, and Fallow checks the work itself: it commits
each task, runs your gates against a baseline, and has a reviewer check every acceptance scenario
by running something. You merge once, for the whole feature.

**Status: pre-release.** It runs on Windows (x64) and on macOS (Apple silicon), each in its own
window, and passes its CI gates on both; releases are published on GitHub for both. The specifications, plans and research are in
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
  phase (a pipeline may allow more on `[P]` tasks, see below). After each task Fallow commits it and runs the gates against the baseline; at each user
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
- **Manual and Auto.** The assistant's chat and the lead's spec session have the same switch,
  **Manual | Auto**, left of the send button (Shift+Tab switches it). In Manual, Fallow asks you
  before commands that are not approved; every request card also offers **Switch to Auto**. In
  Auto, nothing is asked: every action runs, apart from each agent's fixed limits (the assistant
  changes only its own copy of the project, the lead only the spec documents, and neither pushes
  onto a protected target branch). What Fallow refuses at a limit is never silent: the agent is
  told why, and you see it in the chat and in "Needs you", with **Allow this once** to let exactly
  that request through. An agent never edits its own runtime's settings files, in any mode, and
  output that may show environment or secret values is hidden from the activity. New chats, and
  every chat after a restart, start in `chat_mode` from `settings.toml` (Manual unless set).
- **The dashboard.** A "Needs you" inbox (questions, features ready to merge, stopped loops,
  refused requests in a chat), the
  running loops with their phase progress, and desktop notifications from Fallow's tray icon,
  which name the project, the feature and what is needed, and never code or output.
- **Agents and pipelines.** Agents are markdown files with frontmatter in `~/.fallow/agents/`,
  overridden per project in `.fallow/agents/`: makers change files, checkers only run and verify.
  Every agent file names its `runtime`, the model's full name (for Claude, as listed in
  Anthropic's [models overview](https://platform.claude.com/docs/en/models/overview), such as
  `claude-sonnet-5-5`), its `kind`, the commands it `may_run` and what it `returns`; a file
  missing one, or naming a model by a short name, is listed with its problems and not used. The
  Agents tab shows the selected agent's whole file in one editor; save for all your projects or
  this project only. A new agent starts from a template whose comments say what to fill in. The Pipelines tab draws a pipeline as a graph of Fallow's fixed steps, where you
  add, change and remove steps with the + at each point, and says in plain sentences what the
  pipeline checks; Standard, Test first, Solo and Parallel ship with Fallow, and the lead can
  propose one, which is used only once you save it.
  - *Points.* A step can run before or after each task, at the start or end of each phase, at
    the start or end of each user story's checkpoint, and once at the start or end of the loop.
    The builder (the one maker per task) and the gates are the task itself. A checker at a start
    asks you when it fails (`on_fail = "engineer"`), since nothing of that point is built yet.
  - *Checkers alongside each other.* Checkers at one point marked `parallel_with` each other run
    at the same time at every point, are judged when all have finished, and pass only when all
    passed; their findings go to one repair, after which all of them run again.
  - *Builders on `[P]` tasks.* `[caps] builders` (1 to 8, 1 when not written) is the most
    builders at once on a run of `[P]` tasks in one phase of `tasks.md`, each in its own working
    copy; it is a maximum, not a promise. With no `[P]` tasks, or one in a row, one builder
    works as before. Fallow merges the tasks in `tasks.md` order, never merges two that changed
    the same file without a repair, and gates the merged result. The lock form says what the
    builders will do for the feature's `tasks.md` before you lock.
  - *The `parallel` default.* Up to 3 builders on `[P]` tasks, the gates after every task, and
    the reviewer alongside the security reviewer at each user story's checkpoint.
  - *Pipelines from 0.4.0* load unchanged and mean what they meant: one builder, the same steps
    and the same result; Fallow 0.5.0 rewrites none of them, and loops locked with 0.4.0 go on
    with the pipeline they recorded. The other way round, Fallow 0.4.0 cannot use a pipeline
    that uses 0.5.0's additions (`builders` other than 1, `at` on a task step, a checker per task
    or at a start): it lists such a file with its problem. One saved with one builder and none
    of them still works in 0.4.0.
- **Light and dark.** The page follows your system's light or dark setting, switching with it
  without a reload. The control at the right of the top bar picks Light, Dark or System; Fallow
  remembers the choice across restarts. Text and controls meet WCAG AA contrast in both themes.
- **Runtimes.** Every agent step goes through one runtime interface. Claude Code (`claude -p`) is
  the first; a scripted `fake` runtime drives the tests.

## Getting started

### What you need

- Windows 10 or 11 (x64), with the Microsoft Edge WebView2 Runtime. Windows 11 has it; on Windows
  10 Fallow tells you if it is missing and offers Microsoft's download page.
- Or macOS 14 (Sonoma) or newer on Apple silicon. On an Intel Mac there is no download:
  [build it from source](#build-from-source).
- [Git](https://git-scm.com/download/win) (on a Mac, the Xcode Command Line Tools have it:
  `xcode-select --install`).
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code), for the agents (below).

### Install

- **Download it**: from the repository's
  [Releases](https://github.com/Cem-Kalender/fallow/releases), download
  `Fallow-<version>-x64-setup.exe` (its SHA-256 is beside it) and run it. It installs for you only,
  with no administrator rights, and adds Fallow to the Start menu. It is not signed (Fallow is open
  source, with no paid certificate), so Windows SmartScreen warns about it: choose *More info*, then
  *Run anyway*. Uninstall from Windows' *Installed apps*; your projects are never touched, and you
  choose whether Fallow's own data goes too.
- **On a Mac, with one command** (for you only, no administrator password, no warning):

  ```sh
  curl -fsSL https://raw.githubusercontent.com/Cem-Kalender/fallow/main/installer/macos/install.sh | sh
  ```

  It downloads the latest release's disk image, checks it against its published SHA-256, installs
  Fallow into `~/Applications` and links `fallow` into `~/.local/bin` (adding that folder to your
  `PATH` in `~/.zprofile` if it is not there). Add `-s -- --version 0.2.0` after `sh` for a given
  version.
- **Or download the disk image**: `Fallow-<version>-macos-arm64.dmg` from
  [Releases](https://github.com/Cem-Kalender/fallow/releases) (its SHA-256 beside it), open it and
  drag Fallow into Applications. Fallow is signed but not notarized by Apple (that needs Apple's
  paid membership), so the first time macOS refuses it as from an unidentified developer: try to
  open it, then go to **System Settings → Privacy & Security**, scroll to *"Fallow" was blocked*,
  choose **Open Anyway** and confirm; later launches open without a warning. (Right-click → Open
  no longer does this on macOS 15 and later.) Move Fallow to Applications before you open it: run
  from the disk image or Downloads, macOS may run it from a temporary place. To use `fallow` in a
  terminal with this install, link it yourself:
  `mkdir -p ~/.local/bin && ln -s /Applications/Fallow.app/Contents/MacOS/fallow ~/.local/bin/fallow`
  (as a standard user, without an administrator's password, drag Fallow into the Applications
  folder of your home instead, `~/Applications`, and link
  `~/Applications/Fallow.app/Contents/MacOS/fallow`).
- **If macOS says Fallow "is damaged"**: the app was changed after it was signed, usually by a
  download tool or by copying it in a way that drops its signature. Delete it and install again
  with the command above, or check the disk image's SHA-256 against the release's.
- **Upgrading**: run the install command again, or quit Fallow (its menu, or `fallow quit`) and
  drag the new version over the old one; macOS does not replace an app that is open. A newer
  `fallow` that finds an older Fallow running offers to quit it first, with its Quit question;
  loops resume in the new version. After an upgrade macOS may ask again for access to Documents,
  Desktop or Downloads for a project kept there (each version is a new app to macOS until Fallow
  is notarized); notifications keep their setting.
- **Uninstalling on a Mac**: `curl -fsSL …/install.sh | sh -s -- --uninstall` (the URL above)
  quits Fallow, removes the app, the `fallow` link and the line it added to `~/.zprofile`, and
  tells you which folders it kept: your data in `~/Library/Application Support/Fallow` and your
  settings in `~/.fallow`. By hand: quit Fallow, then delete `Fallow.app` and the link.
- <a id="build-from-source"></a>**Or build it** with Rust (`rustup` installs the toolchain the
  repository names), on Windows, a Mac (Apple silicon or Intel) or Linux:

  ```sh
  git clone https://github.com/Cem-Kalender/fallow
  cd fallow
  cargo build --release
  target/release/fallow          # starts Fallow and opens its window
  ```

### Upgrading to 0.4.0

From 0.3.x, every agent file must name the model's full name and `may_run` (0.4.0 has no
default model). Agent files with a short model name (`model: opus`) or without `model` or
`may_run` are listed in the Agents tab under "Files Fallow could not use", each with what to
change at its line; Fallow never changes them. Open one there, fix it and save. Where such a file
has a shipped agent's name (an edited `reviewer`, say), the shipped default runs meanwhile, and
the new default is beside your copy as `<name>.md.new`; copies you never edited are simply
replaced. A spec session that was open starts a new agent session at its next message.

For the `claude` runtime only names of the form `claude-…` are accepted, so Claude Code set up
for Amazon Bedrock model names (`anthropic.claude-…`) is not supported by 0.4.0.

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

Open Fallow from the Start menu (on a Mac: Applications, Spotlight or the Dock), or with `fallow`
in a terminal. Add a project folder with **+**,
work out a feature with the lead, lock it, and let its loop build it.

To start every chat in Auto, set it in `settings.toml` (in `%APPDATA%\fallow`):

```toml
[defaults]
chat_mode = "auto"   # or "manual", the default
```

```sh
fallow            # opens Fallow's window (starts it if needed)
fallow quit       # quits, asking first when loops or sessions are working
fallow status     # lists the loops
fallow stop       # stops Fallow at once; loops resume at the next start
```

The Studio is shown only in Fallow's own window, through the system's web view; nothing listens on
any port, and no browser can reach it. Closing the window keeps Fallow and its loops running in
the background, with its icon in the notification area (on a Mac, in the menu bar, with a dot when
something needs you, and in the Dock); Quit (the icon's menu, the window's system menu, ⌘Q, the
Dock, or `fallow quit`) stops it after naming the work that would pause. On a Mac, logging out or
shutting down is never held up: loops pause and resume at the next start. On Linux the Studio is
still a page on `127.0.0.1` with a secret per launch, until its own window comes.

On a Mac, notifications arrive quietly in Notification Center at first (macOS gives an app that is
not notarized that much without asking); a project's notifications setting offers **Show banners**,
which asks macOS for banners. Adding a project kept in Documents, Desktop, Downloads or iCloud
Drive needs macOS's permission once (System Settings → Privacy & Security → Files and Folders →
Fallow).

Fallow's settings are in its config folder (`settings.toml`), its data (loops, conversations,
worktrees, run records) in its data folder, readable by you only: `%LOCALAPPDATA%\fallow` on
Windows, `~/Library/Application Support/Fallow` on a Mac (a Fallow from before moved it there once
from `~/Library/Caches`, which macOS may empty). The dashboard's "What Fallow
keeps on disk" shows how large each part is and deletes what you no longer need. On Windows, Fallow
also registers its name and icon for its notifications, for you only, under
`HKCU\Software\Classes\AppUserModelId\Fallow`; deleting that key undoes it.

## Budgets

Measured by `cargo xtask bench` in CI (constitution VIII), on Windows and on a Mac, on Fallow's
process and its window's web view processes, as each one's own memory summed (the private working
set on Windows, the physical footprint, Activity Monitor's "Memory", on a Mac):
- the window shows the Studio within 1 s of `fallow` (p95 of 30 launches after 3 warm-ups), and on
  a Mac within 1 s of opening Fallow when it is not running (p95 of 10);
- under 250 MB of memory when idle with the window open (60 MB with it closed), and under 400 MB
  with 4 agent steps running at once;
- an agent's activity appears within 2 s.

Fallow makes no network requests of its own; the traffic of a loop is the agents' own traffic
to their providers.

What the window's web view (Microsoft Edge WebView2) does that Fallow cannot switch off: Windows'
required diagnostic data from the runtime (optional data follows Windows' own setting), and
updates of the WebView2 runtime through Microsoft Edge Update. Fallow switches off SmartScreen
lookups and keeps the web view's crash reports on the machine.

On a Mac the window uses WebKit, which is part of macOS and updated by Apple with it. Fallow
switches off WebKit's fraudulent-website checks and link previews and keeps nothing of the page on
disk; what WebKit's own processes still connect to, if anything, is measured in CI and listed in
each release's notes.

## Releasing

A release is built by CI from a tagged commit on `main` (`.github/workflows/release.yml`): set the
version in `Cargo.toml`, commit it on `main`, then `git tag v<version>` and push the tag. CI builds
the Windows installer and the Mac disk image, each with its SHA-256, and publishes them on one
GitHub release. The Mac app is signed ad hoc with the hardened runtime; with an Apple Developer ID
in the repository's secrets (`MAC_DEVELOPER_ID`, `MAC_CERT_P12_BASE64`, `MAC_CERT_PASSWORD`,
`MAC_NOTARY_APPLE_ID`, `MAC_NOTARY_TEAM_ID`, `MAC_NOTARY_PASSWORD`) the same job signs with it and
notarizes, and nothing else changes. Fallow does not update itself:
a new version is downloaded and installed over the old one, which quits Fallow first.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --features automation   # the fake agent: no network, no cost
cargo build --features automation && cargo xtask bench   # the budgets above
```

On macOS, on Apple silicon or Intel, the commands above work; `cargo xtask bench` opens and closes
Fallow's window many times, and `cargo xtask screens` walks the Studio's screens in it in light and
dark (both with a throwaway `HOME`, on a Mac nobody is using). The tests need `git` and `python3`,
and the Xcode Command Line Tools have both. For the browser page while working on Fallow:
`cargo build --features automation`, then `FALLOW_AUTOMATION=1 target/debug/fallow` (a release, and
a build without that feature, have no page). `sh installer/macos/make-app.sh <version> <folder>` makes
`Fallow.app` and its disk image from `target/release/fallow`; a `cargo build` binary outside the
app shows its window too, without the app's own notifications.

Licensed under MIT or Apache-2.0, at your option.
