# Fallow

Fallow is a local dashboard for spec-driven development with coding agents. You work out each
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
  story's checkpoint a reviewer answers every criterion with what it ran. Failures go back to the
  builder for a repair, capped, then to you. A result a pipeline left unverified (for example
  without gates) ends as "Done, not verified by Fallow", never as ready to merge.
- **Watching and steering.** The loop page shows the pipeline and its current stage, the task tree,
  one card per agent, one activity feed (what each agent says, reads, edits and runs, with
  output), Fallow's own checks, time and cost against the budget. You can pause, stop, leave a
  note for the next step and change limits.
- **Questions through the lead.** Agents ask through Fallow's `ask` tool. When the spec or the
  spec session settles it, the lead answers, Fallow commits the answer to the spec, and the agent
  goes on; you can overrule it. Otherwise the lead relays the question to you with options and a
  recommendation, and only the affected story waits.
- **The dashboard.** A "Needs you" inbox (questions, features ready to merge, stopped loops), the
  running loops with their phase progress, and desktop notifications from Fallow's tray icon,
  which name the project, the feature and what is needed, and never code or output.
- **Agents and pipelines.** Agents are markdown files with frontmatter in `~/.fallow/agents/`,
  overridden per project in `.fallow/agents/`: makers change files, checkers only run and verify.
  Pipelines are built from fixed steps in the pipeline designer; Standard, Test first and Solo
  ship with Fallow, and the lead can propose one, which is used only once you save it.
- **Runtimes.** Every agent step goes through one runtime interface. Claude Code (`claude -p`) is
  the first; a scripted `fake` runtime drives the tests.

## Getting started

Fallow needs `git`, and `claude` signed in for real agents.

```sh
cargo build
target/debug/fallow            # starts the service if needed and opens the dashboard
target/debug/fallow status     # lists the loops
target/debug/fallow stop       # stops the service; loops resume at the next start
```

The dashboard is served on `127.0.0.1` only, with a secret per launch; `fallow` opens it signed
in. Fallow's settings are in its config folder (`settings.toml`), its data (loops, conversations,
worktrees, run records) in its data folder, readable by you only. The dashboard's "What Fallow
keeps on disk" shows how large each part is and deletes what you no longer need.

## Budgets

Measured by `cargo xtask bench` in CI (constitution VIII), on Fallow's own process:
- the page answers within 500 ms of Fallow starting (p95 of 30 launches);
- under 60 MB of memory when idle, and under 200 MB with 4 agent steps running at once.

Fallow makes no network requests of its own; the traffic of a loop is the agents' own traffic
to their providers.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # agents are the scripted fake agent: no network, no cost
cargo xtask bench               # the budgets above
```

Licensed under MIT or Apache-2.0, at your option.
