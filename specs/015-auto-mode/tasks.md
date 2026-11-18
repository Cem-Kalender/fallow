---
description: "Task list for 015 Manual and Auto mode, the same for the lead and the assistant"
---

# Tasks: Manual and Auto mode, the same for the lead and the assistant

**Input**: design documents from `specs/015-auto-mode/`:
- spec;
- plan;
- research R-1 to R-11;
- data-model;
- contracts/web-api.md;
- quickstart Q1 to Q8.

**Tests**: the spec's success criteria are checked by scripted tests (SC-002, SC-003, SC-005), so tests are included. They use the fake runtime only. Claude Code is checked by hand (quickstart Q8).

**Build and test**: `cargo test --workspace --features automation`, one test file per command while iterating. Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` before each commit. On Linux the 22 known failures of 2026-11-14 are not this feature's; compare against `main` before claiming a regression.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

- [X] T001 [P] The default mode setting (research R-6), in `src/config/settings.rs`:
  - add `chat_mode: ChatModeSetting` (`manual` | `auto`, default `manual`) to `Defaults`;
  - an unknown value is a settings error naming `defaults.chat_mode`;
  - add unit tests: the default, `auto`, and `"sometimes"` refused.
- [X] T002 [P] `judge::pushes_onto(cmd, target) -> bool` (R-3) in `src/studio/judge.rs`. It covers:
  - `git push <remote> <target>`;
  - `…:<target>`, `HEAD:<target>` and `refs/heads/<target>`;
  - `--all` and `--mirror`;
  - Git Bash and PowerShell quoting, as `assess` reads them;
  - an unsplittable command containing `push` and the target's name.

  Add unit tests for each form, plus negatives: a plain `git push`, `git push origin HEAD` from the chat's own branch, and `git push origin fallow/solo/x`.

---

## Phase 2: Foundational (one decision point for both agents)

**⚠️ No user story can start before this phase is done.**

- [X] T003 `ChatMode` and the decision table (R-1, R-2), in the new `src/studio/mode.rs`:
  - `enum ChatMode { Manual, Auto }`, with serde `manual` and `auto`, and `parse` refusing old values with `no mode "<v>": manual or auto`;
  - `enum Agent { Lead, Assistant }`;
  - `fn decide(agent, mode, req, facts) -> Decision`, where `Decision` is `Allow`, `Ask(reasons)` or `Refuse { limit, reason, facts }`. It implements R-2's table exactly. The judge's reasons are consulted only in Manual. In both modes, the limits come from `edit_problem` and `refwatch::in_folder` (assistant), `SPEC_GLOBS` (lead), and `pushes_onto` with `publish::Remote` (protected `yes`, `unknown` or `checking`);
  - unit tests for every row of the R-2 table, for both agents and both modes.
- [X] T004 Events (data-model), in `src/conversation/events.rs`:
  - `Refused { id, kind, what, reason, limit, facts }`, `RefusalAllowed { id }`, `RefusalDismissed { id }` and `ChatMode { mode }`;
  - `State` keeps the refusals per turn, with their state (refused, allowed, dismissed or expired);
  - the old `ModeSet` with the lead's values is read and ignored (a test loads a `run_anything` log);
  - `State.mode` is removed, because the mode is not restored from events. Its use in `src/studio/solo.rs:678` moves to `Conv::mode` (T006).
- [X] T005 Rights (R-1, data-model "Agent rights"), in `src/agent/rights.rs` and `src/agent/claude.rs`:
  - remove `Mode`, `Mode::next`, `Mode::label` and `Rights::any_command`;
  - `lead(approved)` always sets `prompt_tool`;
  - `lead_in_loop` stays approved-only, without a prompt tool;
  - update `claude.rs`'s tests (lines 923 and 1068-1082): no rights set allows plain `Bash` or `PowerShell` (FR-004).
- [X] T006 Conversation state (data-model), in `src/studio/conv.rs`:
  - `mode: ChatMode`, `mode_note: Option<String>`, `grants: Vec<Grant>`, and a `waiting` queue of Fallow's messages for the lead;
  - remove `allow_until_switch` and `mode_reset`;
  - the `<conv dir>/mode` mark (R-6) replaces the `until_switch` mark.
- [X] T007 One request path (R-1, R-4), in `src/studio/mod.rs` and `src/studio/solo.rs`:
  - `tool_call` sends the lead's requests to a new `lead_tool_call` and the assistant's to `solo_tool_call`. Both call `mode::decide`, use up a matching grant first (R-5), and act on the decision:
    - `Allow`: the runtime's allow reply, and for a Run request `open_call` (`solo.rs:1221`), so 012's ref watch still attributes branch changes to the command (II, FR-008);
    - `Ask`: today's `ask_engineer` path (the lead's ask is moved from `tool_call` unchanged);
    - `Refuse`: append `Refused`, then send the deny reply with R-4's message at once;
  - a grant matches only the exact same kind and command or path;
  - remove the old "Only Ask mode asks the engineer" and "the lead changes only spec documents" silent denials;
  - the lead's turn start (`mod.rs:1233`, `1369`) uses `rights::lead(approved)`.
- [X] T008 The runtime's own refusals (research R-3, spec FR-009), in `src/run/engine.rs` and `src/studio/mode.rs`. A `ToolResult { refused: true }` for an edit of a control file (`CONTROL_FILES` and `CONTROL_GLOBS`, matched on the `ToolCall`'s path) appends `Refused { limit: "control" }`, which has no "Allow this once". Add a unit test with the fake runtime's refused result.
- [X] T009 Hidden output (research R-10, spec FR-010), in `src/run/engine.rs` where a `ToolResult` becomes an activity line:
  - for a call whose command `judge::assess` flags as showing the environment, or a read `judge::read_problem` flags as a possible secrets file, the output becomes "Output hidden: it may hold secret values" before it is shown or written;
  - add unit tests for `env`, `printenv`, `cat .env` and a harmless `ls`.
- [X] T010 The conversation views of both chats, in `src/studio/solo.rs` (`solo_view`) and `src/studio/mod.rs` (the lead's view): `mode`, `mode_note`, `default_mode`, and `refusals` per turn, as data-model "Views".

**Checkpoint**: both agents' requests are decided in one place. The existing tests still compile, with the old mode tests marked for T026.

---

## Phase 3: User Story 1 - One switch, the same in both chats (P1) 🎯 MVP

**Goal**: Manual | Auto in both composers, the same control, Shift+Tab, notes, and the default mode.

**Independent test**: quickstart Q1 and Q7.

- [ ] T011 [US1] Mode requests (contracts/web-api.md), in `src/service/answer.rs` and `src/studio/mod.rs`. `POST /api/sessions/<s>/mode` and `POST /api/conversations/<c>/mode` both:
  - take `manual` or `auto`, and refuse the old values;
  - set `Conv::mode`, append `ChatMode`, add the chat note (FR-006), and write the mark.

  `Req::SessionMode` and `Req::SetMode` take `ChatMode`.
- [ ] T012 [US1] The default mode and restarts (R-6), in `src/studio/solo.rs` (`solo_on_load`, `new_session`) and `src/studio/mod.rs` (new lead conversation, load):
  - new chats and loaded chats start in `settings.defaults.chat_mode`;
  - when the mark differs from the default, `mode_note` reads "Fallow restarted: this chat is in <Mode>, your default.";
  - grants are not restored, and allowed-but-unused refusals become expired.
- [ ] T013 [P] [US1] The shared switch (R-7), in `src/service/web/app.js`:
  - `Fallow.modeSwitch(mode)`: `role="radiogroup"` labelled "Mode", two `role="radio"` buttons with `data-act="chat-mode"` and the titles of R-7, and a check glyph on the selected one;
  - replace the lead's chip (`app.js:489-490`, `696-704`, `743`, `812`) with it;
  - Shift+Tab in either composer (`app.js:955`) toggles `[data-act=chat-mode]`;
  - the description line names the default: "New chats start in <Mode> (`chat_mode` in settings.toml)".
- [ ] T014 [P] [US1] The assistant's composer uses it, in `src/service/web/screens/solo.js` (`composer`, `setMode`, the notices at lines 97 and 110) and `src/service/web/screens/project.js` (the `solo-mode` action becomes `chat-mode`). Remove `MODES`, "Until I switch back" and its notices; show `mode_note`.
- [ ] T015 [P] [US1] Styles for the segmented switch in both themes (013): selected and unselected states with WCAG AA contrast, and not by colour alone. In `src/service/web/app.css`.
- [ ] T016 [US1] Test `tests/us15_modes.rs::one_switch_in_both_chats`:
  - for the lead and the assistant, the mode route sets Manual and Auto, adds the notes, and refuses `until_switch` and `run_anything`;
  - a new chat starts in the default;
  - after a restart with `chat_mode = "auto"`, a chat that was in Manual is in Auto with the note;
  - an old `run_anything` lead log opens in the default.

**Checkpoint**: the switch works in both chats. US2's behaviour is foundational (T003, T007), so Auto already asks nothing at this point.

---

## Phase 4: User Story 2 - Auto asks nothing, refusals are visible (P1)

**Goal**: Auto allows everything within the limits. A refusal reaches the agent, the chat, "Needs you" and the tray. Allow this once works.

**Independent test**: quickstart Q2, Q3, Q4 and Q6.

- [ ] T017 [US2] "Allow this once" and "Dismiss" (R-5, contracts), in `src/studio/mode.rs`, `src/studio/mod.rs` and `src/service/answer.rs`:
  - `POST /api/conversations/<c>/refusals/<id>`, as `Req::Refusal`;
  - `allow_once` appends `RefusalAllowed`, adds the grant, and tells the agent: the assistant through 012's waiting queue, the lead through its new `waiting` queue, sent when its turn ends or at once when idle (in `src/studio/mod.rs`, at the lead turn's end);
  - `dismiss` appends `RefusalDismissed`;
  - "already answered" is refused.
- [ ] T018 [US2] A waiting request at the switch to Auto (R-8), in `src/studio/mod.rs`. It is answered by `mode::decide` under Auto:
  - allowed, with `ApprovalAnswered { auto: true, why: "switched to Auto" }`;
  - or turned into a refusal.
- [ ] T019 [P] [US2] "Needs you" and the tray (R-9):
  - `Studio::refusals_waiting()` in `src/studio/mod.rs`;
  - `Engine::needs_items` in `src/run/engine.rs` adds `kind: "refused"` items;
  - `src/service/notify.rs` adds `what()` for `refused`: "An agent was refused something", naming the project and the chat only;
  - `src/service/web/screens/dashboard.js` renders the item and opens the chat at the refusal.
- [ ] T020 [P] [US2] The refusal card (R-4, R-7): `Fallow.refusalCard(r)` in `src/service/web/app.js`. It shows the reason, 012's folder facts when present, **Allow this once** (not for `limit: "control"`, which says "This file is never editable") and **Dismiss**, and the allowed, dismissed and expired states. Both the lead's chat (`app.js` messages) and `screens/solo.js` (`list`) render it in the turn.
- [ ] T021 [US2] Test `tests/us15_modes.rs::auto_asks_nothing`. In Auto, the fake assistant requests:
  - a read outside the copy;
  - an edit;
  - `python -c`;
  - `npx`;
  - a redirection;
  - `git push --force origin HEAD`;
  - an `Other` tool.

  The fake lead requests `cargo test` and an edit of `specs/…/spec.md`. Check:
  - no `ApprovalAsked`, and every action allowed (SC-002);
  - every action appears in the activity (FR-008);
  - the same turn with Claude Code's request shape (`agent::registry::permission_request("claude", …)`) gives the same decisions (SC-003);
  - the median time from request to allow is within 10 ms of an approved command's (SC-004);
  - with `env` and a read of `.env` added, neither the activity nor the records contain the values set in the test's environment and `.env` file (SC-006).
- [ ] T022 [US2] Test `tests/us15_refusals.rs::refusals_are_never_silent`. In both modes:
  - the assistant edits the engineer's folder;
  - the lead edits `src/main.rs`;
  - the assistant pushes `HEAD:<target>` with the remote marked protected.

  Check that each gives `Refused` with its limit, the deny reply with R-4's message, a "Needs you" item and a notice without the path, all within 2 s (SC-005). Then check:
  - `allow_once` delivers the message, which names the exact command; the next identical request is allowed (the push included), while a different command and a third identical one are refused again;
  - a control-file edit refused by the runtime becomes a refusal without "Allow this once", and `allow_once` on it is refused;
  - `dismiss` clears the "Needs you" item;
  - after a restart, an allowed-but-unused refusal is expired.
- [ ] T023 [US2] Test `tests/us15_modes.rs::switching_to_auto_answers_the_waiting_request` (R-8): in Manual, a waiting request runs at the switch, and an out-of-limit one becomes a refusal.

**Checkpoint**: the user's complaint is solved: Auto asks nothing, and every refusal can be seen and overridden.

---

## Phase 5: User Story 3 - Manual asks clearly (P2)

**Goal**: Manual's cards offer "Switch to Auto". Refusals in Manual look as they do in Auto.

**Independent test**: quickstart Q5.

- [ ] T024 [US3] "Switch to Auto" (FR-007, contracts):
  - the approve route accepts `answer: "auto"`, which allows the request once and sets the chat to Auto (with the note), in `src/studio/mod.rs` and `src/service/answer.rs`;
  - the button is added to the lead's card (`app.js:564-571`) and the assistant's (`screens/solo.js` `approval`).
- [ ] T025 [US3] Test `tests/us15_modes.rs::manual_offers_auto`: in Manual, a card for each agent, and `answer: "auto"` runs the command and leaves the chat in Auto. A refusal in Manual has the same record and card as in Auto (covered by T022).

---

## Phase 6: Polish & cross-cutting

- [ ] T026 Update the tests of the old modes:
  - `tests/us1_modes.rs` (007's four modes): rewrite it for Manual and Auto, keeping its once, always, deny and time-out cases under Manual;
  - `tests/us12_many.rs:239-278` (`until_switch` and `mode_reset`): replace it with the default-mode restart;
  - any other test that names `Mode::`, `until_switch`, `run_anything` or `mode_reset` (`grep -rn` over `tests/`).
- [ ] T027 [P] README (`README.md`, "What it does" and "Use it"): name the two modes, Auto's limits and the refusal card, and `chat_mode` in settings.toml.
- [ ] T028 [P] Earlier specs, with notes only and no rewrites: add "superseded by 015" notes at 012 FR-008's "until I switch back" and at 007/009 FR-007a's four modes, in `specs/012-solo-session/spec.md` and `specs/007-fallow-studio/spec.md`.
- [ ] T029 Check SC-001 by hand: at least 3 people new to Fallow (the person who gave the feedback among them, if willing) are asked to "let it run without asking" in both chats. Record the time taken and whether help was needed in `specs/015-auto-mode/checks/usability.md`. This is for the engineer to arrange.
- [ ] T030 Run quickstart Q1 to Q7 with the `automation` build. Record the results in `specs/015-auto-mode/checks/quickstart.md`. Q8 (Claude Code) is run by the engineer, or on the Windows machine.
- [ ] T031 `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the whole test suite. Push, and check that CI on Windows and macOS is green apart from the known benchmark lines.

---

## Dependencies & Execution Order

- **Setup** (T001, T002): independent of each other, and needed by T003.
- **Foundational** (T003 to T010): in order (T003 → T004 → T005 → T006 → T007 → T008 → T009 → T010; T008 and T009 can run in parallel after T007). It blocks every story.
- **US1** (T011 to T016): after Foundational. T013 to T015 can run in parallel with T011 and T012.
- **US2** (T017 to T023): after Foundational. T017 needs T011's routes in place. T019 and T020 can run in parallel.
- **US3** (T024, T025): after US2's card (T020).
- **Polish**: last (T029 is the engineer's). T026 can start as soon as T005 to T007 break the old tests.

## Parallel Opportunities

- T001 ∥ T002.
- T013 ∥ T014 ∥ T015 (page files), alongside T011 and T012 (engine).
- T019 ∥ T020.
- T027 ∥ T028.

## Implementation Strategy

1. **MVP**: Phases 1 to 3. Both chats have Manual | Auto, and Auto already allows everything within the limits through the foundational decision point. Refusals are recorded and sent to the agent, but have no card yet.
2. **Then US2**: the refusal card, Allow this once, "Needs you" and the tray. This completes the engineer's requirement that refusals are never silent.
3. **Then US3 and Polish**.

Each phase ends green (fmt, clippy, tests) and in its own commit.
