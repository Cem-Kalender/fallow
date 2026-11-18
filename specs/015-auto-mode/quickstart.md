# Quickstart: checking 015

Prerequisites:
- a build with the `automation` feature, for the scripted checks (`cargo build --features automation`);
- the fake agent (`fallow-fake-agent`);
- for Q8 only, Claude Code signed in;
- a test project that is a git repository with a remote whose target branch is protected (or marked protected in the 010 record).

The scripted equivalents are `tests/us15_modes.rs` and `tests/us15_refusals.rs` (`cargo test --features automation --test us15_modes --test us15_refusals`).

## Q1 One switch in both chats (US1, FR-001, FR-002)

Open a project: the assistant's composer shows **Manual | Auto**, with Manual selected. Open a feature's spec session: the lead's composer shows the same switch in the same place.

In each composer:
1. Press Shift+Tab. Expect: Auto is selected, and the chat adds "Auto: every action runs without asking you."
2. Press Shift+Tab again. Expect: back to Manual, with a note.
3. With a screen reader, expect "Mode, Manual, selected" or the equivalent.

Repeat in the dark theme: the selected option shows a check, not only a colour.

## Q2 Auto asks nothing (US2 AS1, FR-003, SC-002)

In Auto, have the fake assistant request:
- a read outside the working copy;
- an edit;
- `python -c "print(1)"`;
- `npx prettier --check .`;
- `echo x > out.txt`;
- `git push --force origin HEAD` (its own branch).

Expect: no card, each action in the activity.

In Auto, have the fake lead request `cargo test` and an edit of `specs/<feature>/spec.md`. Expect: no card.

## Q3 Refusals are visible, in both modes (US2 AS2, US3 AS3, FR-009, SC-005)

Have the fake assistant request an edit of a file in the engineer's project folder, and the fake lead request an edit of `src/main.rs`. In Auto and in Manual, expect within 2 s:
- a refusal card in the turn, with the reason, **Allow this once** and **Dismiss**;
- the agent's reply containing "Fallow refused this: …";
- a "Needs you" item naming the chat;
- a tray notification naming the project and the chat, and not the path.

## Q4 Allow this once (R-5)

On a Q3 refusal, choose **Allow this once**. Expect:
- the agent receives "The engineer allowed, once: …" (after its turn ends, if one was running);
- when it asks again, the edit runs;
- a third identical request is refused again.

Then have the fake assistant request `git push origin HEAD:<target>`. Expect a refusal at the target limit. Allow it once, and expect the push to be attempted. Whether the remote accepts it is up to its protection.

## Q5 Manual asks, and offers Auto (US3, FR-007)

In Manual, have the fake assistant request `python -c "print(1)"`. Expect a card with **Switch to Auto** next to Allow once, Always in this project, and Deny. Choose **Switch to Auto**: the command runs and the switch shows Auto. Repeat with the lead.

## Q6 A waiting request at the switch (US2 AS3, R-8)

In Manual, leave a card waiting, then switch to Auto with the switch. Expect: the command runs. If it was outside a limit, it becomes a refusal instead.

## Q7 Default mode and restart (FR-005, R-6)

Set `[defaults] chat_mode = "auto"` in settings.toml and restart Fallow. Expect:
- new chats of both kinds start in Auto;
- a chat that was in Manual before the restart is now in Auto, with the note "Fallow restarted: this chat is in Auto, your default."

Open a lead conversation recorded in `run_anything` before 015. Expect: it opens in the default mode.

Set `chat_mode = "sometimes"`. Expect the settings error naming the key.

## Q8 With Claude Code (SC-003)

Repeat Q2 and Q3 with Claude Code as the assistant and as the lead, on a throwaway project. Expect the same results. Check the agent's start command in the activity's details: it has `--permission-prompt-tool`, no `bypassPermissions` or `--dangerously-skip-permissions`, and no plain `Bash` allow rule.
