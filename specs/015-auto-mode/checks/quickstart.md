# Quickstart results: 015 (2026-11-18, Linux x64, the `automation` build)

Run on the build server (Ubuntu, Linux 7.0), not on Windows. Fallow's tests pass on Linux and
macOS for development (the release target is Windows; CI runs there after the push). There is no
browser on this machine, so the page parts were checked by `node --check`, the page tests
(`the_page_has_one_switch_for_both_chats`, `manual_offers_auto`) and three independent reviews,
not by eye.

| Check | How | Result |
|---|---|---|
| Q1 One switch in both chats | `tests/us15_modes.rs::one_switch_in_both_chats`, `the_page_has_one_switch_for_both_chats`; reviews of phase 3 (roles, keyboard, contrast computed: selected 5.87:1 light, 6.15:1 dark) | Pass (engine and page code). The look in the real window: not seen, no browser here |
| Q2 Auto asks nothing | `us15_modes.rs::auto_asks_nothing` (a read outside the copy, an edit, `python3 -c`, `bash -c`, a redirection, a variable, a force-push of its own branch, and the lead's command); `us15_refusals.rs::auto_asks_nothing_refuses_at_the_limits_visibly_and_hides_secrets`; `npx`, web and other-tool requests in the unit test `studio::mode::tests::auto_allows_the_assistant_everything_within_its_limits` | Pass |
| Q3 Refusals visible, both modes | `us15_refusals.rs::refusals_are_never_silent`, `allowing_while_the_agent_works_and_a_cancelled_answer` (the latter asserts the refusal shows within 2 s while the turn works); shell commands beyond the copy (`echo > folder/x`, `rm -rf folder/src`, writes to `.claude/settings*.json`) in `auto_asks_nothing_refuses_…` and the unit test `the_assistant_s_limits_hold_in_both_modes` | Pass |
| Q4 Allow this once | the same two tests, and `a_note_behind_the_lead_s_question_waits_for_the_engineer_s_answer` | Pass |
| Q5 Manual offers Auto | `us15_modes.rs::manual_offers_auto` | Pass |
| Q6 A waiting request at the switch | `us15_refusals.rs::switching_to_auto_answers_the_waiting_request_as_auto_would` | Pass |
| Q7 Default mode and restart | `us15_modes.rs::one_switch_in_both_chats` (restart with `chat_mode = "auto"`, a pre-015 chat, a new chat); `config::settings` unit test for a bad value | Pass |
| Q8 With Claude Code | `tests/us15_claude.rs` (ignored; `FALLOW_REAL_CLAUDE=1 … -- --ignored`), both agents on `claude-sonnet-5-5`, Claude Code 2.1.327 | Pass, see below |
| Q9 Secrets hidden, control files closed | `us15_refusals.rs::auto_asks_nothing_…` (a real `env` and a `.env` read: hidden in the activity and Fallow's data folder; `.claude/settings.json` refused without "Allow this once") | Pass |

## Q8 with Claude Code (Sonnet 5.5), 2026-11-18

- The assistant in Auto ran `python3 -c "print(6*7)"` (42) and `echo hello > note.txt` without a card; its `git push origin HEAD:main` was refused at the target limit (no remote read yet, so the checked-out branch counts as protected) and it was told why. Its reply: "The Python command printed `42` and `echo hello > note.txt` ran, but `git push origin HEAD:main` was refused because `main` is protected, so I didn't try another way."
- Switched to Manual, `python3 -c "print(7*8)"` came as a card; "Switch to Auto" ran it (56) and left the session in Auto.
- The lead in Auto ran `python3 -c "print(9*9)"` (81) without a card; its write of `src/not-a-spec.txt` was refused at the edit limit. Its reply: "The shell command ran and printed 81, but Fallow refused the write to `src/not-a-spec.txt` because the lead may only change spec documents, so I didn't try another way."
- The whole run took 16 s.
- The agent's start command (prompt tool, no bypass, no plain `Bash` allow rule) is checked by the unit test `agent::claude::tests::chat_rights_become_claude_options_and_never_allow_everything`, not by this run.

## Not done here

- T029 (SC-001): the check by hand with three people new to Fallow is for the engineer to arrange.
- T031's CI on Windows and macOS: after the push.
