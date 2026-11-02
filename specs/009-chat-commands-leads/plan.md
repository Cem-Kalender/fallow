# Implementation Plan: Chat commands and lead choice

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is `009-chat-commands-leads`) | **Date**: 2026-11-02 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/009-chat-commands-leads/spec.md`

## Summary

009 adds five things to 008's page and service:

- **A `/` menu** in the chat with the lead (research R-9). Spec Kit steps reach the lead as "read and follow this skill file" (R-1). The project's own commands, from the command folders of Claude Code, Gemini CLI, Copilot and Cursor, reach it as their text (R-2). The server decides what is a command (R-3), and `/specify` in the project conversation starts a feature.
- **The lead per session** (R-4). It comes from the project's default lead, or the engineer's pick before the first message. It is fixed from then on, and recorded in the conversation's log with the runtime and model it ran on. A changed runtime or model starts a new agent session. The loop's questions go to the feature's session lead (R-5).
- **A working indicator**, kept outside the re-rendered chat so it does not restart, and still under reduced motion (R-6).
- **One unsaved-changes guard in the router**, plus `beforeunload` (R-7).
- **Read-only file views**: a Files tab on the project page and a Files drawer on the loop page and spec session (R-8). They have:
  - git-based versions and marks, and "who changed it in which task" from the loop's commits;
  - a secrets list that is never read;
  - "Ask the lead about it", which adds the file to the lead's prompt;
  - "Open in my editor", with an editor found and confirmed once (R-10).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as 007 and 008).

**Primary Dependencies**: no new crates, no JavaScript libraries. git is run as a subprocess (`src/verify/git.rs`), as everywhere in Fallow. Frontmatter is read with the existing agent-definition reader's approach, and TOML with the `toml` crate already used for pipelines.

**Storage**:
- `projects.json` gains `lead` (optional).
- A conversation's log gains `lead` on `conversation_created`, the new `lead_set` event, and `runtime`/`model` on `turn_started`. All are optional with defaults, so logs from before 009 load unchanged.
- New: `<data>/prefs.json` (`editor: {program, args}`).
- Older builds truncate a log at `lead_set` (R-4 note).

**Testing**: `cargo test` with the fake runtime: `tests/us9_leads.rs`, `tests/us9_commands.rs` and `tests/us9_files.rs`, plus unit tests for the catalogue, the frontmatter, input substitution, path safety, the secrets list and parsing commit subjects. The page is checked with headless Chrome over CDP, as 008: the menu by keyboard, the indicator under reduced motion, the leave prompt on every route, and the file views against `mockup/files.html` at 1440 px and 390 px. Every test from 007 and 008 stays green.

**Target Platform**: Windows 10/11 first (the editor's `.cmd` launchers are handled, R-10); paths and commands also work on Linux and macOS.

**Project Type**: local service with a browser page (one crate plus `xtask`).

**Performance Goals**:
- SC-002: the catalogue of 200 commands is answered within 100 ms (a CI test); narrowing runs in the page over that list, without a request.
- SC-008: the tree of 5,000 files within 2 s. That is one `git ls-files` and one `git diff --name-status`, about 200 KB of JSON.
- FR-034: nothing kept from before (a CI fingerprint test); the page asks every 3 s while visible (a page check, Complexity Tracking).
- 007's budgets hold: idle < 60 MB, page ready < 500 ms. File listings are not cached in memory.

**Constraints**:
- No outbound connections.
- The file views never write.
- No secret file's content is read (constitution V).
- Agent definitions, pipelines and Spec Kit's files are unchanged.
- The `/` commands never use a runtime's native slash commands.

**Scale/Scope**:
- Up to ~200 commands per project.
- Projects of up to 5,000 tracked files (larger ones work, slower).
- Files shown up to 512 KiB.
- A handful of Lead agents.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.1.0.

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Nothing changes in what Fallow checks. A step from the menu ends like any lead turn: the step the lead reports is still checked against the documents, and analyze must run again after a change. The file views show what is on disk and in git, never an agent's claim. | PASS |
| II. The Engineer Decides | The engineer picks the lead, and the session keeps it. A changed runtime or model is shown before the next message. Actions from the menu keep their buttons' confirmations. The editor is used only after the engineer confirms it. | PASS |
| III. Coordination in Code | The loop's questions go to the session's lead, found by Fallow from the feature's record, not by agents. No new messages between agents. | PASS |
| IV. Simplest Setup First | The default stays one lead named `lead`. Nothing changes for an engineer who never picks another. No new step kinds. | PASS |
| V. Local and Private | No network path. File views read only the project and Fallow's copies of it. Files that may hold secrets are listed but never read, shown or sent to a lead (FR-036a). Prefs are stored in Fallow's data directory. | PASS |
| VI. Least Privilege | Project commands cannot widen a lead's rights: their `allowed-tools` and similar settings are not applied (FR-009). Spec Kit's `implement` and `taskstoissues` are not offered. The file views write nothing. "Open in my editor" starts the engineer's own program, with their rights, on one file path, never "open with the associated program". Fallow writes only `prefs.json` in its own data directory. | PASS |
| VII. Runs Survive | The session's lead and its runtime and model are in the conversation's log, so a restart keeps them. The loop finds its lead from the feature's record on every question, as today. | PASS |
| VIII. Small and Measured | No dependencies. Listings are computed on request and not kept. Every timed claim has a `cargo test` that CI runs (research R-12): the catalogue of 200 commands within 100 ms (SC-002), the conversation `busy` when read right after `say` returns (SC-006, with the page showing the indicator before the reply), a new fingerprint right after a file changes (FR-034's "nothing kept from before"), and 5,000 files listed within 2 s (SC-008). The idle-memory benchmark still runs. | PASS |

**Re-check after Phase 1 design**: PASS. The contracts add endpoints and optional fields, and only `prefs.json` and the conversation log's optional fields are new on disk.

## Project Structure

### Documentation (this feature)

```text
specs/009-chat-commands-leads/
├── plan.md          # this file
├── research.md      # R-1 to R-12
├── data-model.md
├── quickstart.md    # Q1 to Q6
├── contracts/
│   ├── web-api.md   # leads, commands, files, open in editor
│   └── page.md      # the menu, the lead in the chat, the indicator, the guard, the file views
├── mockup/          # files.html (US5's look)
├── checklists/requirements.md
└── tasks.md         # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── conversation/events.rs  # ConversationCreated.lead, LeadSet, TurnStarted.runtime/model/new_session; State.lead, lead_runtime/model
├── project/mod.rs          # Project.lead
├── studio/
│   ├── mod.rs              # say(): the session's lead, new session on a changed runtime/model, fork only with the same lead,
│   │                       # commands (R-3), /specify in the project conversation, @ references; set_lead; patch_project
│   │                       # checks before changing; new_feature(lead); lead_of(): the agent
│   ├── conv.rs             # view(): lead{…}, leads[]
│   ├── commands.rs         # NEW: the catalogue (Spec Kit steps, project commands), frontmatter, input substitution
│   └── files.rs            # NEW: versions, entries, file, who changed it, path safety, secrets list, prefs and the editor
├── agent/prompts.rs        # LeadTurn.command and .refs: the step / command / file blocks
├── run/
│   ├── questions.rs        # Lead.agent; start_lead uses it; by: <agent>
│   ├── activity.rs         # "answered by a lead" no longer means by == "lead"
│   └── engine.rs           # Req dispatch for the new requests
├── verify/git.rs           # listed, name_status(base), last_change(base, path), added_ranges(base, path), diff_worktree
├── service/
│   ├── http.rs             # the new routes; SCREENS += files.js
│   └── web/
│       ├── index.html      # files.js
│       ├── app.js          # router guard + beforeunload; the / menu (combobox); #working; lead in chat heads
│       ├── app.css         # menu, indicator (+ reduced motion), lead picker, tree, file view, drawer B1/B2
│       └── screens/
│           ├── files.js    # NEW: the tree and the file view, shared by the three places
│           ├── project.js  # Features | Files tabs; the file in place of the chat; lead picker at + New feature
│           ├── spec.js     # dirty(); Files drawer; lead in the chat head
│           ├── loop.js     # Files button and drawer (B1/B2); "careful-lead answered" (by_lead)
│           ├── pipelines.js # dirty() for the router's guard
│           └── agents.js   # "Use for this project" on a Lead; "this project's lead"; dirty()
└── bin/fallow-fake-agent.rs  # unchanged (scenarios match on prompt text)
tests/
├── us9_leads.rs     # NEW (US2)
├── us9_commands.rs  # NEW (US1)
├── us9_files.rs     # NEW (US5)
├── common/mod.rs    # second_lead, fake_editor
└── (007/008 tests adjusted where `by == "lead"` or a conversation view's keys are asserted)
```

**Structure Decision**: the same crate and page. Two new Rust modules under `src/studio`, one new screen script, and three new test files.

The order of the work follows the priorities, with what they share first:
1. Foundation: the events and state for the session lead, `Project.lead`, and the git helpers.
2. US2, the lead, and US1, the menu (both P1). US1's step prompt does not depend on US2, but the menu's "Agents" group uses US2's picker.
3. US3, the indicator, and US4, the guard: small page-only changes.
4. US5, the files: its own module, endpoints and screen.
5. The visual check and a real-agent check: a `/plan` and a project command with `claude`, and a question in a loop answered by a second lead.

## Complexity Tracking

| Exception | Why it is needed | Simpler alternative rejected because |
|---|---|---|
| The page's own timing (the indicator shown before the reply, the 3-second refresh of the file views) is checked with headless Chrome, which CI does not run | CI has no browser; adding one (Chrome in CI and a CDP driver) is a dependency against VIII | The server-side half of each claim is a CI test (R-12): for SC-008 the listing of 5,000 files within 2 s, for which the page's drawing of the tree adds no request; the page half is a fixed interval, an order of statements or the drawing of an answer already received, checked at every phase and at the visual check (T038) |
| An older Fallow build truncates a conversation log at `lead_set` (VII: no history lost) | A lead picked before the first message must survive restarts, and can only be recorded as an event | Recording the lead only on `conversation_created` would leave no way to record a pick made before the first message. The loss happens only when the engineer goes back to a build older than 009, as with 008's `cancel_asked`, and is stated in the release note. |
