# Quickstart & Validation: Agent Sessions and Multiple Projects

How to prove 002 works end to end. Contracts: [config](./contracts/config.md),
[ui-commands](./contracts/ui-commands.md). Data model: [data-model.md](./data-model.md).

## Prerequisites

- A Fallow build from this branch (`cargo build --release`).
- For the manual scenarios: at least one agent CLI installed and signed in (Claude Code, Codex CLI,
  Gemini CLI or Aider), and two or three scratch folders to use as projects.

## Automated gates (CI on all three platforms)

```sh
cargo test --workspace            # projects, profiles, keys, replies, attention, transcripts
cargo xtask bench-startup         # p95 < 100 ms, with 50 projects in projects.toml
cargo xtask bench-memory --idle   # < 50 MB with the project list loaded
cargo xtask bench-memory --loaded # < 150 MB: reference project + 4 sessions with full scrollback
cargo xtask bench-sessions        # echo p95 < 50 ms; switch < 100 ms; echo < 100 ms with 8 flooding
cargo xtask canary                # also with a session printing the canary (SC-008)
```

## Manual scenarios

| # | Scenario | Steps | Expected |
|---|---|---|---|
| S1 | Default screen (US3) | Start `fallow` with no arguments. | Projects, sessions and LLM processes are shown; no editor; focus on Projects; the hint bar shows how to start a session, switch, open the editor and get help. |
| S2 | Add projects (US4) | `a`, enter a folder path, Enter; repeat for a second folder; enter a file path once. | Both folders are listed and the last is selected; the file path is rejected with the reason. |
| S3 | Start an agent (US1) | Select a project, `n`, pick the agent's profile. | The agent starts in the project folder, fills the pane and has focus; its screen looks as in a normal terminal of the same size. |
| S4 | Keys go to the agent (FR-003) | In the session press `Esc`, `Ctrl-C`, `Tab`, arrows, `Ctrl-P`, `?`. | The agent reacts to each as it would in a normal terminal; Fallow's palette and help do not open. |
| S5 | Leave and return | `Ctrl-]`; move around Fallow; `Enter` on the session. | Focus returns to Fallow at once; the agent kept running; `Ctrl-]` twice inside the session sends one `Ctrl-]` to the agent. |
| S6 | Scrollback, select, copy | Ask the agent for long output; leave; scroll with the wheel or `Shift-PgUp`; drag to select; `Ctrl-C`. | Earlier output is reachable up to 10,000 lines; the selection is copied to the system clipboard. |
| S7 | Paste | Paste a multi-line block into the focused session with the terminal's paste. | The agent receives it as one paste. |
| S8 | Several sessions (US2) | Start sessions in both projects; give one a task ending in a question; open the overview (`Ctrl-T`). | Each session shows project, profile, state and time; the one waiting shows "needs attention" within ~7 s; its project row shows `●1`; a notice names it. |
| S9 | Switch and split | `Ctrl-N` / `Ctrl-B` from Fallow; `s` on a second session. | Switching takes one key and draws at once; split shows both side by side. |
| S10 | Stop | Select a session, `Shift-K`, `Esc`; then `Shift-K`, `y`. | The first time nothing happens; then the agent stops (graceful first; force stop needs its own confirmation on Linux and macOS). The pane shows the exit status; `R` restarts it. |
| S11 | Processes linked (FR-013) | Open the LLM panel. | The session's agent appears as a group labelled `#n project`; `j` jumps to its session. `f` on a project filters the panel to it. |
| S12 | Editor view (FR-020) | `Ctrl-E`; open a file; `Ctrl-E`. | The 001 editor for the selected project; back to the workspace with the previous selection. |
| S13 | Restart Fallow (FR-015) | `Ctrl-Q` with sessions running; confirm; start Fallow again. | The quit dialog lists the running sessions; agents stop; after restart the projects are listed in the same order with the same selection, and no session is started. |
| S14 | Custom profile (US5) | Add a `[[profile]]` to `profiles.toml`; "Reload configuration"; start it. | The profile is offered and starts with exactly its command and arguments. |
| S15 | Missing project (FR-018) | Rename a listed project's folder outside Fallow; select it. | The row shows "missing"; other projects and sessions are unaffected. |
| S16 | Worktree by default (US6) | In a git project, `n`, pick an agent, `Enter` without touching "Where". Repeat for a second session. | Each session's title shows its own branch (`fallow/claude-1`, `fallow/claude-2`); `<project>.worktrees/` next to the project holds both folders. |
| S17 | Isolation | Ask one agent to change a file; look at the same file in the other worktree and in the project folder. | Only the first worktree has the change. |
| S18 | Close and remove | Exit an agent whose worktree has a new untracked file; `w`. Then delete the file, `w` again, choose Remove. | First: Remove is not offered and the dialog says why. Second: the worktree folder is gone; `git branch` still lists its branch. |
