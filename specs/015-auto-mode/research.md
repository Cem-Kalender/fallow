# Research: Manual and Auto mode, the same for the lead and the assistant (015)

Decisions taken during `/speckit-plan`, 2026-11-18, from the code on `main` at `dbafdd1`. Code
references are to that commit.

## R-1 Where the mode lives: Fallow's answer to the runtime, not the runtime

**Decision**: one mode type for both chats, `ChatMode { Manual, Auto }`, in a new module
`src/studio/mode.rs`. It replaces two things:
- for the assistant, `Conv::allow_until_switch` (`src/studio/conv.rs:71`);
- for the lead, `agent::rights::Mode` (ReadOnly, Approved, Ask, RunAnything; `src/agent/rights.rs:53`) as the chat's mode.

Both agents are always started the same way, whatever the mode:
- with Fallow's permission prompt tool;
- without an allow-all rule: `Rights::any_command` and `Mode::RunAnything` go, and `rights::lead` loses its mode parameter;
- without a bypass option.

So every command, and every edit the runtime does not allow by rule, comes to Fallow as a
`PermissionRequest` (`Studio::tool_call`, `src/studio/mod.rs:2196`). There the mode is applied:
- in `solo_tool_call` for the assistant (`src/studio/solo.rs:885`);
- in a new `lead_tool_call` for the lead, moved out of `tool_call`.

Narrow allow rules stay, because they are Fallow's own rights, not the mode:
- the assistant's gates and safe approved prefixes;
- the lead's approved commands;
- `Edit` and `Write` inside the spec documents for the lead;
- reads.

The lead answering an agent's question during a loop keeps its approved-commands-only rights
(`rights::lead_in_loop`). It is not a chat, so it has no mode.

**Rationale**:
- One decision point per request, in Fallow, makes Auto identical for every runtime: the fake runtime exercises the same path as Claude Code (SC-003).
- Switching mode needs no restart of the agent's process: the next request is answered under the new mode. A runtime-level rule (today's Run anything) would need one, and Fallow would not see the commands, so it could not check the protected-target limit (R-3).

**Alternatives rejected**:
- *The runtime's own auto or bypass mode*: Claude-only, forbidden by VI.
- *Keeping Run anything's allow-all rule for the lead's Auto*: Fallow would not see the commands (constitution 10.0.0, VI forbids it).

## R-2 What each mode answers, per request kind

| Request | Manual | Auto |
|---|---|---|
| Read | Assistant: inside the copy allowed; outside it or a secrets file asked (012 FR-043). Lead: allowed (its runtime rule, as today). | Allowed. |
| Edit inside the limit (assistant: its working copy; lead: spec documents) | Allowed. | Allowed. |
| Edit outside the limit (including `.git`, Fallow's and the runtime's control files) | **Refused, visible, can be allowed once** (R-4). Today it is refused silently. | The same. |
| Run: gates and approved commands | Allowed. | Allowed. |
| Run: a command that changes the engineer's folder (`refwatch::in_folder`), assistant only | Asked, as today, with 012's folder facts. | Refused at the copy limit, visible, can be allowed once. |
| Run: a push onto the protected target (R-3) | Asked, with the reason "pushes onto the protected target". | Refused at the target limit, visible, can be allowed once. |
| Run: any other command | Asked. For the assistant, with the judge's reasons (012). | **Allowed**, including the commands the judge "cannot tell", force-pushes of the session's own branch, and `gh`. |
| Web (assistant) | As the web switch says: allowed when on, refused (visible) when off. | The same. The web switch is the engineer's explicit setting, not a question. Its refusal offers "Allow this once" and "Turn web on". |
| Other (a tool Fallow cannot judge, such as an MCP tool) | Refused (visible). | Allowed. |

**Rationale**:
- "Auto mode is auto mode" (the engineer, 2026-11-18). The judge's ask reasons (`judge::assess`) are not consulted in Auto, except for the limits.
- Manual is the current Ask behaviour for both agents.

When Auto allows a request, the runtime gets its ordinary allow reply (`permission_reply(runtime, true, …)`) and nothing more.

## R-3 The limits, exactly

1. **The edit limit**:
   - For the assistant: an Edit request that `judge::edit_problem` refuses, or a Run request that `refwatch::in_folder` says changes the engineer's folder.
   - For the lead: an Edit request outside `SPEC_GLOBS`.

   The runtime's own fixed denials stay at the runtime, because an agent that can edit its runtime's permission files could switch Fallow's checks off (VI). These are Claude Code's control files (`CONTROL_FILES`, `CONTROL_GLOBS`). Fallow never sees those requests. The Auto note (FR-006) names those files as never editable, and the activity shows the failed call. They are the only refusals Fallow does not record itself.
2. **The protected-target limit**, for both agents: a Run request that pushes onto the project's target branch on its remote, when 010's record of the remote (`publish::Remote`) says `protected` is `yes`, `unknown` or `checking`.
   - A new judge function, `pushes_onto(cmd, target) -> bool`, reads the push forms: `git push <remote> <target>`, `…:<target>`, `HEAD:<target>`, `refs/heads/<target>`, `--all` and `--mirror`.
   - A plain `git push`, or a push of the session's own branch, is not onto the target.
   - A command the judge cannot split (`judge::split` fails) that contains `push` and the target's name counts as onto the target.

**Not limits, by decision**: `gh pr merge`, because the remote's protection applies and II counts the agent's merge on the engineer's word as theirs.

**"Unless the engineer asked for the change elsewhere"** is not inferred from the chat, because Fallow does not read intent. The engineer's word is "Allow this once" (R-5).

## R-4 A refusal is a record, shown at once, in both modes and both chats

**Decision**: a new conversation event, `Refused { id, kind, what, reason, facts, limit }`, appended when Fallow refuses a request.
- `limit` is one of `edit`, `target`, `web` or `tool`.
- `facts` holds 012's folder facts, when there are any.

The runtime gets the deny reply at once, so the turn does not wait. Its message is: "Fallow refused this: <reason>. The engineer can allow it once from the chat; if they do, you will be told."

The page shows a refusal card in the turn, with the reason, "Allow this once" and "Dismiss". The lead's and the assistant's chats share the card (R-7).

**Alternatives rejected**:
- *Hold the request open until the engineer answers*: that is asking, and Auto asks nothing.
- *Show it only in the activity*: the engineer would not notice it.

## R-5 "Allow this once"

**Decision**: "Allow this once" records `RefusalAllowed { id }` and puts a one-shot grant on the chat, in memory, for the same kind and the same command or path, exactly. Then Fallow tells the agent:
- If a turn is running, the message waits and is sent when the turn ends. The assistant's waiting messages already work this way (012); the lead gets the same queue.
- If the chat is idle, the message is sent now.

The message is: "The engineer allowed, once: `<what>`. Do it again if it is still needed."

The next request that matches the grant uses it up and is allowed, whatever the limit, including a push onto a protected target (II, 10.0.0). Whether the remote accepts the push is up to the remote.

Grants do not survive a restart: the card then says "Fallow restarted; ask the agent to try again". "Dismiss" records `RefusalDismissed { id }`.

**Alternatives rejected**:
- *Fallow runs the action itself*: Fallow would act as an agent, and the agent would not know the outcome.
- *A grant for the whole kind*: that is a rule, not "this once".

## R-6 The default mode and restarts

**Decision**:
- `settings.toml` gains `[defaults] chat_mode = "manual" | "auto"`, default `"manual"`. It goes in `config::settings::Defaults`, which has `deny_unknown_fields` and `default`.
- A new chat of either kind starts in it, and after a restart every chat starts in it.
- The chat's last mode is kept in a mark file, `<conv dir>/mode`, only to word a note when it differs from the default: "Fallow restarted: this chat is in Manual, your default." This replaces 012's `until_switch` mark.

The lead's `ModeSet { mode }` events in existing conversations hold the old values. They are read and ignored (`#[serde(other)]` on a compatibility variant). A chat that was in Run anything opens in the default mode, without a note, because it predates 015.

The picker's description line names the setting: "New chats start in Manual (`chat_mode` in settings.toml)". The Studio has no settings screen for `[defaults]`, and one is out of scope.

## R-7 The control and the shortcut, shared by both composers

**Decision**: `Fallow.modeSwitch(mode)` and `Fallow.refusalCard(r)` in `src/service/web/app.js`, next to the lead's composer, which renders the lead's chip today (`app.js:743`). The assistant's composer (`screens/solo.js:253`) renders the same two helpers.
- It is a two-option segmented switch, **Manual | Auto**: `role="radiogroup"` labelled "Mode", with two `role="radio"` buttons.
- Each option has a `title`: "Manual: Fallow asks you before commands" and "Auto: every action runs without asking you".
- The selected option has a filled background and a check glyph, so it does not rely on colour alone (013).

Shift+Tab in either composer toggles it. Shift+Tab already toggles the lead's chip (`app.js:955` clicks `[data-act=mode]`); it misses the assistant's (`data-act="solo-mode"`). Both become `[data-act=chat-mode]`, so one handler serves both composers.

Manual's request card, in both chats, gets "Switch to Auto", which allows that request and sets Auto (FR-007).

## R-8 A request waiting when the engineer switches to Auto

**Decision**: switching to Auto answers the waiting request (`Conv::pending`) under Auto's table (R-2).
- If it is within the limits, it is allowed: `ApprovalAnswered` is recorded with `auto: true` and `why: "switched to Auto"`.
- If it is outside them, it becomes a refusal (R-4).

Switching to Manual leaves nothing waiting.

## R-9 "Needs you" and the tray

**Decision**:
- A chat with a refusal that is neither allowed nor dismissed is a "Needs you" item: `kind: "refused"` and `key: "<conv>:refused:<id>"`, with the text "<chat name> — Fallow refused: <what, shortened to 80 characters>".
- `Engine::needs_items` (`src/run/engine.rs:1230`) gains the studio's items. The tray's notice (`service::notify::notice`) maps `refused` to "An agent was refused something". It names the project and the chat only, never the command (008).

## R-10 Tests without a real model

The fake runtime already scripts permission requests for every kind, for the assistant (`fake_agent_012`) and the lead (007/009 scenarios). The new tests cover:
- the R-2 table, for both agents and both modes;
- the limits;
- the refusal record, the deny message, and "Allow this once" with its grant and message;
- the switch with a request waiting;
- the restart rule, including an old lead conversation with `run_anything`;
- "Needs you";
- that no agent is started with an allow-all rule (`rights` unit tests).

`judge::pushes_onto` gets unit tests for each push form, plus a variant that pushes the chat's own branch. One manual check with Claude Code is in the quickstart.
