# Implementation Plan: Manual and Auto mode, the same for the lead and the assistant

**Branch**: `main` (the project's working branch since 2026-11-14) | **Date**: 2026-11-18 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/015-auto-mode/spec.md`

## Summary

- **One mode for both chats**: `ChatMode { Manual, Auto }`.
  - It replaces the lead's four modes and the assistant's "Ask for commands" / "Until I switch back".
  - It is shown by one shared switch, **Manual | Auto**, in both composers. Shift+Tab toggles it (research R-1, R-7).
- **Fallow decides, not the runtime**:
  - Both agents always send their commands and out-of-rule edits to Fallow's permission prompt.
  - No allow-all rule and no bypass option, so Auto is identical for every runtime (R-1).
- **Auto allows everything** within the agent's fixed limits:
  - the assistant edits only its working copy;
  - the lead edits only spec documents;
  - neither pushes onto a protected target (R-2, R-3).
- **Refusals are records**, shown at once in both modes, with the reason, **Allow this once** and **Dismiss**.
  - The agent is told the reason.
  - "Needs you" and the tray list the chat.
  - "Allow this once" is a one-shot grant, after which the agent is told to try again (R-4, R-5, R-9).
- **The default mode** is `[defaults] chat_mode` in settings.toml. New chats, and every chat after a restart, start in it (R-6).

## Technical Context

**Language/Version**: Rust, edition 2024, MSRV 1.88 (as `Cargo.toml`). The page stays plain JavaScript and CSS, compiled in.

**Primary Dependencies**: none added.

**Storage**:
- conversation events (`refused`, `refusal_allowed`, `refusal_dismissed`, `chat_mode`);
- a `mode` mark file per conversation;
- `settings.toml` `[defaults] chat_mode`.

**Testing**: `cargo test --workspace --features automation`, with the fake runtime as the lead and as the assistant (R-11). Judge unit tests for `pushes_onto`. Rights unit tests show no allow-all rule.

**Target Platform**: Windows 10 and 11 x64 (release); the tests also run on macOS and Linux for development.

**Project Type**: a desktop app (014): a Rust service with its own window showing a local web UI.

**Performance Goals**: allowing a request in Auto adds no measurable delay (SC-004). A refusal reaches the chat, "Needs you" and the tray within 2 s (SC-005).

**Constraints**: the budgets of constitution VIII, unchanged; no network request of Fallow's own; no runtime bypass option.

**Scale/Scope**:
- engine:
  - a new `studio/mode.rs` of about 300 lines, with the R-2 table, the grants and the refusals;
  - `solo_tool_call` and the new `lead_tool_call` call into it;
  - `rights.rs` loses `Mode` and `any_command`;
  - judge gets `pushes_onto`;
- events: 4 new;
- settings: 1 key;
- `needs_items`, and one notice kind in `notify.rs`;
- page: the shared switch and refusal card in `app.js`, used by `solo.js` and the lead's composer; "Switch to Auto" on both request cards.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Not affected: loops and their verification are unchanged. |
| II. The Engineer Decides | Pass (10.1.0): both chats are in Manual or Auto as the engineer chooses. Auto is their word within the fixed limits. Refusals are never silent and can be allowed once, a push onto a protected target included (R-4, R-5). The merge rule for loops is unchanged. |
| III. Coordination in Code | Pass: the mode is applied in Fallow's code at one decision point (R-1), with no model in the loop. |
| IV. Simplest Setup First | Pass: two modes instead of six; no reviewer; one setting. |
| V. Local and Private | Pass: no new traffic. The tray names the project and the chat, never the command (R-9). The output of commands that show the environment, and of reads of secrets files, is hidden in the activity and records (R-10, FR-010). |
| VI. Least Privilege | Pass (10.1.0): no bypass option and no allow-all rule; Fallow's narrow rules stay (R-1). The fixed limits hold in both modes; only a one-time allow goes past them (R-3, R-5). The runtime's permission files are never editable, and their refusal is shown (R-3). |
| VII. Runs Survive | Pass: grants and the mode are in memory. After a restart, chats start in the default mode and say so. Nothing waits on a refusal (R-6). |
| VIII. Small and Measured | Pass: no dependency, a few hundred lines; no budget affected (SC-004). |
| IX. Runtime-Agnostic | Pass: the decision is made on Fallow's request kinds (`RequestKind`), never on a runtime's tool names. The fake runtime and Claude Code take the same path (SC-003). |
| CI gates (Workflow) | Unchanged. New tests are added to the suite. |

**Re-check after Phase 1 design**: unchanged. The contracts add no transport, no write place outside Fallow's data folder, and no request of Fallow's own.

## Project Structure

### Documentation (this feature)

```text
specs/015-auto-mode/
├── spec.md
├── plan.md              # this file
├── research.md          # R-1 to R-11
├── data-model.md        # the chat mode, refusals, grants, settings, events
├── quickstart.md        # Q1 to Q9
├── contracts/
│   └── web-api.md       # the mode routes, refusals, "Switch to Auto", "Needs you"
├── checklists/
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── agent/rights.rs          # Mode and any_command removed; lead(approved) always prompts
├── agent/claude.rs          # rules(): no allow-all Bash for the lead (follows rights)
├── config/settings.rs       # [defaults] chat_mode
├── conversation/events.rs   # refused, refusal_allowed, refusal_dismissed, chat_mode; old mode_set ignored
├── studio/
│   ├── mode.rs              # NEW: ChatMode, the R-2 table, limits, refusals, grants
│   ├── mod.rs               # tool_call → lead_tool_call / solo_tool_call; Req::Refusal; mode requests
│   ├── solo.rs              # solo_tool_call through mode.rs; until_switch removed; restart note
│   ├── conv.rs              # mode, mode_note, grants, the lead's waiting messages
│   └── judge.rs             # pushes_onto
├── run/engine.rs            # needs_items: refusals
├── service/
│   ├── answer.rs            # the mode routes' new values; the refusals route
│   ├── notify.rs            # the "refused" notice
│   └── web/
│       ├── app.js           # modeSwitch, refusalCard, Shift+Tab on [data-act=chat-mode], the lead's composer
│       ├── screens/solo.js  # the assistant's composer and cards use them; "Switch to Auto"
│       ├── screens/dashboard.js # the "refused" item
│       └── app.css          # the segmented switch, both themes
tests/
├── us15_modes.rs            # NEW: the R-2 table for both agents, the switch, a waiting request
├── us15_refusals.rs         # NEW: refusals, Allow this once, Dismiss, Needs you, restart
└── (existing 012 and 009 mode tests updated to Manual and Auto)
```

**Structure Decision**: one crate, as today. The mode logic is one new module that both agents' request paths call, so the two chats cannot drift apart again.

## Complexity Tracking

No violations.
