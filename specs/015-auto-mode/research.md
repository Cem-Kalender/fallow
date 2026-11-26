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
- the assistant's gates and safe approved prefixes (its whole-tool ask rules still send every
  command to Fallow);
- not the lead's approved commands, nor `cd` (hardening): a rule for an approved `git` would let
  `git push origin main` past the target limit, so `mode::lead` applies them after the limits;
- `Edit` and `Write` inside the spec documents for the lead;
- reads.

The lead answering an agent's question during a loop keeps its approved-commands-only rights
(`rights::lead_in_loop`). It is not a chat, so it has no mode.

**Rationale**:
- One decision point per request, in Fallow, makes Auto identical for every runtime: the fake runtime exercises the same path as Claude Code (SC-003).
- Switching mode needs no restart of the agent's process: the next request is answered under the new mode. A runtime-level rule (today's Run anything) would need one, and Fallow would not see the commands, so it could not check the protected-target limit (R-3).

**Alternatives rejected**:
- *The runtime's own auto or bypass mode*: Claude-only, forbidden by VI.
- *Keeping Run anything's allow-all rule for the lead's Auto*: Fallow would not see the commands (constitution 10.1.0, VI forbids it).

## R-2 What each mode answers, per request kind

| Request | Manual | Auto |
|---|---|---|
| Read | Assistant: inside the copy allowed; outside it or a secrets file asked (012 FR-043). Lead: allowed (its runtime rule, as today). | Allowed. |
| Edit inside the limit (assistant: its working copy; lead: spec documents) | Allowed. | Allowed. |
| Edit outside the limit (including `.git`, Fallow's and the runtime's control files) | **Refused, visible, can be allowed once** (R-4). Today it is refused silently. | The same. |
| Run: gates and approved commands | Allowed. | Allowed. |
| Run: a command that changes the engineer's folder (`refwatch::in_folder`), assistant only | Asked, as today, with 012's folder facts. | Refused at the copy limit, visible, can be allowed once. |
| Run: a push onto the protected target (R-3) | Refused at the target limit, visible, can be allowed once (a fixed limit holds in both modes, spec FR-009, US3 AS3). | The same. |
| Run: any other command | Asked. For the assistant, with the judge's reasons (012). | **Allowed**, including the commands the judge "cannot tell", force-pushes of the session's own branch, and `gh`. |
| Web (assistant) | Allowed when the web switch is on. When it is off, the agent has no web tools (`--tools` leaves them out, `claude.rs`), so there is no request and no refusal. | The same. The web switch stays the engineer's own setting, beside the mode switch (spec FR-003). |
| Other (a tool Fallow cannot judge, such as an MCP tool) | Refused (visible). | Allowed. |

**Rationale**:
- "Auto mode is auto mode" (the engineer, 2026-11-18). The judge's ask reasons (`judge::assess`) are not consulted in Auto, except for the limits.
- Manual is the current Ask behaviour for both agents.

When Auto allows a request, the runtime gets its ordinary allow reply (`permission_reply(runtime, true, …)`) and nothing more.

## R-3 The limits, exactly

1. **The edit limit**:
   - For the assistant: an Edit request that `judge::edit_problem` refuses, or a Run request that `refwatch::in_folder` says changes the engineer's folder.
   - For the lead: an Edit request outside `SPEC_GLOBS`.
   - **2026-11-24 amendment:** In Auto, a Run request reaches the edit limit only when, read as written, it writes to, deletes in or otherwise changes the engineer's folder (`refwatch::in_folder`, or `judge::folder_change`, which follows each `cd`, `~` and `$HOME` included). Reading or naming paths elsewhere, `cd` elsewhere, and writing elsewhere (the system temp folder included) are not limits. Git's own record and Fallow's control files, written, are refused at `control` (`judge::protected_write`). A command Fallow cannot read that names one of them is refused at `control` too (`judge::protected_unread`; 0.3.2, by the engineer's rule that a control file is never allowed, not even always). These control checks run before the target limit, so a control write beside a push is refused, not held. A here-document is read without its body only when it feeds a program that reads it as data and its output is not piped onward; its tag is read as the whole shell word (`E"O"F` is `EOF`); and the control-file and push checks also read such a command's raw text, so taking a body out can only spare a false alarm, never hide a write or a push (0.3.2 review, 1–2). The assistant's Edit requests in the system temp folder (`/tmp`, `$TMPDIR`, `std::env::temp_dir()`, `/private/tmp`, `/var/folders/…`) are allowed, by decision: it is scratch space, not the engineer's project. Any other path outside the copy, and the engineer's folder even when it lies in the temp folder, stays at the limit. The engineer's chat had ten refusals of reads and `/tmp` work, all now allowed (`tests/us15_auto_replay.rs`).

   The runtime's own permission and settings files stay denied at the runtime, in every mode, because an agent that can edit them could switch Fallow's checks off (constitution VI, 10.1.0). These are Claude Code's `CONTROL_FILES` and `CONTROL_GLOBS`. Fallow does not receive these requests, but it sees their result: the runtime reports the call as `ToolResult { refused: true }` (`src/agent/mod.rs:172`). On such a result for an edit of a control file, Fallow appends the same `Refused` record, with `limit: "control"` and no "Allow this once" (spec FR-009). The note on switching to Auto names these files.
2. **The protected-target limit**, for both agents: a Run request that pushes onto the project's target branch on its remote, when 010's record of the remote (`publish::Remote`) says `protected` is `yes`, `unknown` or `checking`. Without a record (not read yet), or with a remote but no target read, the folder's checked-out branch counts as a protected target; only a project known to have no remote has none (review of phase 2).
   - A new judge function, `pushes_onto(cmd, target) -> bool`, reads the push forms: `git push <remote> <target>`, `…:<target>`, `HEAD:<target>`, `refs/heads/<target>`, `--all` and `--mirror`.
   - A plain `git push`, or a push of the session's own branch, is not onto the target.
   - Read conservatively (review of phase 2): a destination holding a variable, a substitution or a wildcard, `heads/<target>`, a git alias or unknown subcommand naming the target, and a command string given to a shell, `eval` or PowerShell (read the same way) all count as onto the target. A command the judge cannot split (`judge::split` fails) counts when it contains `push`.

**Commands, read as written** (final review, 1): `judge::limit_problem` reads a command's paths as the judge does. In both modes a command writing a runtime settings file is refused at `control`. In Auto, a command writing or using the engineer's folder or git's own record, working in another folder or repository, or writing (a redirection, or a program that changes files) outside the copy is refused at `edit`; Manual asks about these, as 012 does. Reads outside the copy, and commands Fallow cannot read as written, are not limits (R-2). For the lead, what a command writes inside its own copy outside the spec documents is thrown away at the turn's end and listed in the chat ("Thrown away, outside the spec documents"); what it writes beyond its copy is refused like the assistant's.

A runtime settings file is found conservatively (`judge::control_write`): any word of a command, after its `cd`s, split at `=` and inside strings handed to a shell or a wrapper, that names one or its folder counts unless the program only reads; so a command that only copies or stages it (`cp .claude/settings.json backup.json`, `git add …`) is refused too, visibly. A command that does not name the file (`find . -name settings.json -delete`, `git clean -fdx`, `xargs rm < list`) is the "cannot be read as written" case that Auto allows (spec FR-003); what limits its effect is that Fallow never lets a runtime load settings from the working copy (`--setting-sources`, `src/agent/claude.rs`), so such a write cannot switch Fallow's checks off. `limit_problem` reads the judge's reason texts; the unit tests pin the shapes, and a structured reason would be sturdier later.

**Not limits, by decision**: `gh pr merge`, because the remote's protection applies and II counts the agent's merge on the engineer's word as theirs.

**"Unless the engineer asked for the change elsewhere"** is not inferred from the chat, because Fallow does not read intent. The engineer's word is "Allow this once" (R-5).

## R-4 A refusal is a record, shown at once, in both modes and both chats

**Decision**: a new conversation event, `Refused { id, kind, what, reason, facts, limit }`, appended when Fallow refuses a request.
- `limit` is one of `edit`, `target`, `tool` (Manual only) or `control` (the runtime's own files, R-3; never "Allow this once").
- `facts` holds 012's folder facts, when there are any.

The runtime gets the deny reply at once, so the turn does not wait. Its message is: "Fallow refused this: <reason>. The engineer can allow exactly this once from the chat; if they do, you will be told."

The page shows a refusal card in the turn, with the reason, "Allow this once" and "Dismiss". The lead's and the assistant's chats share the card (R-7).

**Alternatives rejected**:
- *Hold the request open until the engineer answers*: that is asking, and Auto asks nothing.
- *Show it only in the activity*: the engineer would not notice it.

**2026-11-24 amendment (the engineer):** at a limit the agent waits; it is no longer refused at once.
- In both modes, a request at the edit limit (the assistant's copy or the engineer's folder; the lead's spec documents) or the protected-target limit holds the agent's permission request open. It shows Manual's request card (`approval_asked` with its `limit`) with the reason, and adds a "Needs you" item (`kind: "limit"`) and a tray notice naming the chat only.
- **Allow once** answers the held request with allow, so it runs then. **Deny** answers it with deny, and the agent gets R-4's reason: "Fallow refused this: <reason>. The engineer denied it". The 10-minute wait denies it as Manual's does.
- **Always allow** stores an entry in the project's `allowed_past_limits` (beside `approved_commands`): its limit (`edit` or `target`), its kind, and a command prefix chosen in the card's confirm. An Edit records an exact path, or a folder ending in `/`, compared as paths (never a path with `..` in it). A command matches exactly, unless the engineer shortened it in the confirm (`starts`, 0.3.2; entries stored before have none and match exactly): then it matches the prefix and a space with no further command chained after it. Both modes check the list at that limit before asking. The project page lists the entries and removes them.
- A tool Fallow cannot judge, in Manual, waits the same way, with Allow once and Deny only.
- **Control files are never allowed:** the runtime's settings files, Fallow's control files and `.git` internals. They are refused at once (`limit: "control"`) with no Allow and no Always; the card shows only Dismiss.
- Nothing is queued for the agent any more: "The engineer allowed, once, exactly…" and the follow-up turns it started are gone. Old `refused`, `refusal_allowed`, `refusal_used` and `refusal_dismissed` events still load and render as before. "Allow this once" on an old refusal card works as it did.

## R-5 "Allow this once"

**Decision**: "Allow this once" records `RefusalAllowed { id }` (and `RefusalUsed { id }` when the grant is used) and puts a one-shot grant on the chat, in memory, for the same kind and the same command or path, exactly. Then Fallow tells the agent:
- If a turn is running, the message waits and is sent when the turn ends. The assistant's waiting messages already work this way (012); the lead gets the same queue.
- If the chat is idle, the message is sent now.

The message is: "The engineer allowed, once, exactly: `<what>`. Run exactly that again if it is still needed; anything else is decided as before."

The next request that matches the grant uses it up and is allowed, whatever the limit except `control`, including a push onto a protected target (II, 10.1.0). Whether the remote accepts the push is up to the remote.

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

## R-10 Hidden output for environment and secrets (constitution V, spec FR-010)

**Decision**: the activity line of a tool result is built in `run/engine.rs` from `agent::Event::ToolResult`. For a call whose command `judge::assess` flags with "shows the environment's values" (or "sets environment variables" with no command after it), or a read that `judge::read_problem` flags as a file that may hold secrets, the line's output becomes "Output hidden: it may hold secret values". This happens before the activity is shown or written. The check uses the call's own command or path, kept from its `ToolCall` by `id`.

In Manual this applies to the calls the engineer allowed. In Auto it applies to every such call. A search is decided from its path and glob, which the runtime adapter puts in the call's detail (`path: …`, `glob: …`), with `judge::search_problem`; the assistant's reads and searches are also marked when Fallow decides them (`Conv::hidden`). A refused call's text is Fallow's reason and stays. The runtime's own transcript is the runtime's, not Fallow's (constitution V concerns what Fallow displays and stores).

**Alternatives rejected**:
- *Keep asking for these commands in Auto*: Auto asks nothing.
- *Refuse them*: an agent running tests often needs `env`, and refusing it would make Auto refuse ordinary work.

## R-11 Tests without a real model

The fake runtime already scripts permission requests for every kind, for the assistant (`fake_agent_012`) and the lead (007/009 scenarios). The new tests cover:
- the R-2 table, for both agents and both modes;
- the limits, including a runtime-refused control-file edit (a `ToolResult { refused: true }` from the fake runtime) becoming a refusal without "Allow this once";
- hidden output for `env` and a `.env` read (SC-006);
- the refusal record, the deny message, and "Allow this once" with its grant and message;
- the switch with a request waiting;
- the restart rule, including an old lead conversation with `run_anything`;
- "Needs you";
- the time of an Auto allow against an approved command's (SC-004);
- that no agent is started with an allow-all rule (`rights` unit tests).

`judge::pushes_onto` gets unit tests for each push form, plus a variant that pushes the chat's own branch. One manual check with Claude Code is in the quickstart.
