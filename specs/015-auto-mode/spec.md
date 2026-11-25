# Feature Specification: Manual and Auto mode, the same for the lead and the assistant

**Feature Branch**: `main` (the project's working branch since 2026-11-14; the spec lives in `specs/015-auto-mode`)

**Created**: 2026-11-18

**Status**: Draft

**Input**: User description:

> Feedback from a user of Fallow on 2026-11-18, with a screenshot of the assistant's composer (the chip "Until I switch back" next to "Attach image"): "'Until I switch back' is a somewhat unintuitive description of what the button does" and "How do I turn on run without permission? It keeps asking."
>
> The engineer's decisions (2026-11-18):
> - "If a user chooses auto mode, the user should get auto mode. It should work exactly like Claude Code, while remaining model and LLM supplier agnostic."
> - "Auto mode is auto mode. Manual mode is manual mode. No relaying actions to a second AI model."
> - Refusals are never silent, with a button to override them.
> - "Why would the modes differ between the lead and the solo session?" The lead and the assistant get the same two modes.

## Context

Fallow has two chats with an agent, each with its own permission switch, designed in separate features.

- **The assistant**: the chat in the middle of a project's page, a solo session (012). Its chip toggles between "Ask for commands" and "Until I switch back" (012 FR-008). The chip shows the current mode, not what clicking it does. Even with "Until I switch back" on, Fallow keeps asking about every command its rules deem risky, and every command whose effect it cannot tell from the text (`python -c`, `npx`, `docker`, pipes, redirections). So a user who turned it on is still asked all the time.
- **The lead**: the chat of a feature's spec session (007, 009). Its chip cycles four modes with Shift+Tab: Read only, Approved commands, Ask and Run anything. In Run anything, the runtime itself is told to allow every command, so Fallow never sees them. An edit outside the spec documents is refused by the runtime, without the engineer being told.

**What the engineer wants**: one switch, the same in both chats, with two plain modes.
- **Manual**: Fallow asks before commands that are not approved.
- **Auto**: Fallow asks nothing. Every action the agent requests runs, apart from the agent's fixed limits.
- A refusal at a fixed limit is never silent: the agent is told why, and the engineer sees it at once with a button to allow it once.

There is no second model and no reviewer. Fallow keeps no list of exceptions that are still asked.

**Model and supplier agnostic.** The mode is Fallow's own, not a runtime's flag:
- Fallow receives each runtime's tool requests as its own kinds (run, edit, web, read, other) and answers them itself (012 FR-008, FR-043). In Auto it answers "allow", except at the fixed limits.
- A runtime is never started with its own bypass option, nor with an allow-everything rule that would keep requests from reaching Fallow.
- So every runtime Fallow supports gets the same Auto. Today Claude Code is the only real one.

**Constitution amendments 10.0.0 (MAJOR) and 10.1.0 (MINOR, after /speckit-analyze)**, made with this spec on 2026-11-18. It replaces 9.0.0's wording and the uncommitted 9.0.1 patch.
- **II**: both chats are in Manual or Auto. In Auto, the engineer's choice of Auto is their word for every action within the agent's fixed limits. A refusal at a limit is never silent, and allowing it once is the engineer's decision, a push onto a protected target branch included.
- **VI**: the lead's fixed limit is that it edits only spec documents; the assistant's is that it edits only its session's working copy. Outside that limit, only what the engineer allowed once from a refusal is allowed.
- **Unchanged**:
  - "Options that make an agent skip its own permission checks MUST NOT be used."
  - Loops are untouched.
  - Fallow shows every change to branches and remotes it observes (II).

**Out of scope**:
- A reviewer model or classifier (rejected by the engineer).
- Accept-edits, plan, read-only or approved-only modes.
- Auto for loops: they run unattended under the gates and the locked plan.
- New runtimes.

## Clarifications

### Session 2026-11-18

- Q: What should the engineer get when they choose auto mode? → A: Auto mode: Auto is Auto, Manual is Manual. In Auto nothing is asked; no second model reviews actions. Model and supplier agnostic.
- Q: Do the fixed limits hold in Auto? → A: They hold, but a refusal is never silent: the agent is told why, and the engineer sees it at once with the reason and a button to allow it once, for every limit, a push onto a protected target included.
- Q: Which chat does this cover, the lead's or the assistant's? → A: Both, with the same modes: there is no reason for them to differ. The lead's four modes (Read only, Approved commands, Ask, Run anything) and the assistant's two ("Ask for commands", "Until I switch back") are replaced by Manual and Auto.
- Q (/speckit-analyze): Can the runtime's own permission files be allowed once? → A: No. They are never editable, in any mode, because an agent that could edit them could switch Fallow's checks off. Their refusal is shown like any other, without "Allow this once" (constitution 10.1.0).
- Q (/speckit-analyze): Auto runs `env` and reads `.env` files, but constitution V forbids Fallow to display environment or secret values. → A: The output is hidden in the activity and in Fallow's records (FR-010); the agent still gets it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - One switch, the same in both chats (Priority: P1)

In both the lead's and the assistant's composer, the same switch shows two named modes, **Manual** and **Auto**, in the same place, with the current one always visible. Shift+Tab in the composer toggles them. Choosing Auto adds one line to the chat: "Auto: every action runs without asking you."

**Why this priority**: the user could not find or understand today's control, and the two chats work differently for no reason.

**Independent Test**: ask an engineer new to Fallow to "let it run without asking me", once in a spec session and once in a solo session. Check that they find the switch in both, without help.

**Acceptance Scenarios**:

1. **Given** a spec session or a solo session, **When** the engineer looks at the composer, **Then** they see "Manual" or "Auto" by name, as one control that switches between them, the same in both.
2. **Given** Manual, **When** the engineer switches to Auto by click or Shift+Tab, **Then** the control shows Auto and the chat adds the one-line note.
3. **Given** a default mode in Fallow's settings, **When** the engineer opens a new chat of either kind, **Then** it starts in that mode. Without the setting it starts in Manual.

---

### User Story 2 - Auto asks nothing (Priority: P1)

In Auto, every request the agent makes runs at once, with no card: commands, edits, reads and web requests. The exception is a request outside the agent's fixed limits. The activity shows each action as usual.

**Why this priority**: this is what the user asked for: "it keeps asking".

**Independent Test**: in Auto, drive the fake runtime as the assistant through a turn that asks for:
- a read;
- an edit;
- `python -c …`;
- `npx …`;
- a redirection to a file;
- `git push --force` of the session's own branch.

Check that all of them run with no card and appear in the activity. Do the same as the lead with a command and an edit of `spec.md`.

**Acceptance Scenarios**:

1. **Given** Auto, **When** the agent requests any command, edit, read or web request within its fixed limits (FR-009), **Then** Fallow allows it without asking.
2. **Given** Auto, **When** the agent requests an action outside its fixed limits, **Then** the action does not run, and the refusal is never silent. Outside the limits means: for the assistant, an edit outside its working copy; for the lead, an edit outside the spec documents; for both, a push onto a protected target branch.
   - The agent is told it was refused and why, so it can do it another way or tell the engineer.
   - Allowing it once allows exactly that command or path once, and the agent is told so.
   - The chat shows the refusal at once, with the reason and "Allow this once". This runs the action, a push onto a protected target included, in case the engineer does want it.
   - The chat appears in "Needs you", and the tray notifies, naming the project and the chat.
3. **Given** a request waiting for the engineer, **When** they switch to Auto, **Then** it is answered as Auto would answer it.
4. **Given** Auto, **When** the engineer switches back to Manual, **Then** the next request is asked as in Manual.

---

### User Story 3 - Manual asks clearly (Priority: P2)

Manual asks before commands that are not the gates or the project's approved commands. The assistant's reads outside its working copy are asked too, as today. The request card offers "Switch to Auto", next to Allow once, Always in this project, and Deny.

**Why this priority**: an engineer annoyed by asks should find Auto from the card that annoys them.

**Independent Test**: in Manual, ask for a command in each chat. Check that the card offers "Switch to Auto", and that choosing it allows that command and puts the chat in Auto.

**Acceptance Scenarios**:

1. **Given** Manual, **When** a card appears, **Then** it offers "Switch to Auto" alongside the existing choices.
2. **Given** the engineer chooses "Switch to Auto" on a card, **When** it takes effect, **Then** the command runs and the chat is in Auto.
3. **Given** Manual, **When** the agent requests an action outside its fixed limits, **Then** the refusal is shown just as in Auto (US2 AS2).

---

### Edge Cases

- **Fallow restarts.** Every chat starts in the default mode from settings, as new chats do. 012's "never after a restart" is dropped; this matches Claude Code's default mode setting. When the chat was in another mode before, it says so.
- **A turn is running when the engineer switches mode.** The new mode applies from the next request.
- **A lead chat that was in Read only, Approved commands or Run anything before the upgrade.** It opens in the default mode.
- **Two windows or tabs** (automation builds). Both show the mode change within the usual refresh.
- **Dark mode (013).** Manual and Auto are distinguishable without relying on colour alone, at WCAG AA contrast in both themes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The lead's spec sessions and the assistant's solo sessions MUST each be in one of two modes:
  - **Manual**: asks before commands that are not the gates or approved commands;
  - **Auto**: asks nothing.

  The lead's Read only, Approved commands, Ask and Run anything, and the assistant's "Ask for commands" and "Until I switch back", MUST be removed.
- **FR-002**: Both composers MUST show the current mode by name, in the same control in the same place: the composer's bottom bar, left of the send button, where both chips are today. Shift+Tab in the composer MUST toggle it. The control MUST work with the keyboard and screen readers, with its name and state announced.
- **FR-003**: In Auto, Fallow MUST allow every request from the agent without asking, within its fixed limits (FR-009): commands, edits, reads and web requests, including the commands Fallow's judge cannot read. Fallow uses no reviewer, classifier or second model. The assistant's web switch (012) stays its own setting, beside the mode switch. With web off the agent has no web tools at all, so there is nothing to allow or refuse.
- **FR-004**: Fallow MUST NOT start any runtime with that runtime's own option for skipping permission checks, nor with a rule that allows every command or edit without the request reaching Fallow. Auto works by Fallow answering the runtime's requests, so it behaves the same for every runtime.
- **FR-005**: Fallow's settings MUST offer a default mode for both chats: Manual unless set. A new chat, and every chat after a restart, starts in it.
- **FR-006**: Switching modes MUST add a one-line note to the chat. Switching to Auto says that every action now runs without asking.
- **FR-007**: In Manual, a request card MUST offer "Switch to Auto", which allows that request and switches the chat.
- **FR-008**: The activity MUST show every action in Auto as it does today. Fallow MUST keep showing the changes to branches and remotes it observes (II).
- **FR-009**: Each agent's fixed limits MUST hold in both modes:
  - the assistant edits only inside its session's working copy (012 FR-008);
  - the lead edits only spec documents (`specs/`, the constitution in `.specify/memory/`, `.fallow/drafts/`);
  - neither pushes onto a protected target branch (II). "Protected" includes a target whose protection Fallow could not read, or is still reading: only a target known to be unprotected is not a limit;
  - in Manual only, a tool Fallow cannot judge (for example an MCP tool) is refused. Auto allows it.

  A request outside them MUST NOT run, and its refusal MUST NOT be silent:
  - The agent MUST receive the refusal with its reason.
  - "Allow this once" MUST allow exactly that command or path once. The agent MUST be told so in those words, so that a retry with a different command is understood to be refused again.
  - An edit of a runtime's own permission or settings files (for Claude Code, `.claude/settings.json` and `.claude/settings.local.json`) MUST be refused in every mode, shown like any refusal, and MUST NOT offer "Allow this once" (constitution II and VI, 10.1.0). The note on switching to Auto names these files.
  - The chat MUST show it at once with the reason and "Allow this once", which runs that one request, a push onto a protected target included, as the engineer's decision (constitution II, 10.1.0). Whether the remote itself accepts the push is up to its protection.
  - The chat MUST be listed in "Needs you" and notified through the tray, naming the project and the chat only.

- **FR-009a** (2026-11-24 amendment, the engineer): At the edit limit and the protected-target limit, the agent MUST wait for the engineer, in both modes, instead of being refused at once.
  - The request's card MUST offer **Allow once** (it runs then), **Always allow** and **Deny** (the agent receives the refusal with its reason).
  - Always allow records, for the project, the limit and a command prefix, or a path or folder, that the engineer can edit in the card's confirm. The next matching request MUST then run without a card, in both modes. The project page MUST list these entries and let the engineer remove them.
  - In Auto, a command reaches the edit limit only when it changes the engineer's folder. Reads anywhere, `cd` anywhere, and work in the system temp folder (the assistant's edits there included) run without asking.
  - Control files (the runtime's settings files, Fallow's control files, `.git` internals) MUST be refused with no Allow and no Always.
  - No message about an allowed request is queued for the agent.

- **FR-010**: Fallow MUST NOT display or store environment variable values or secret values (constitution V). When Auto lets through a command that Fallow's judge flags as showing the environment (for example `env`, `printenv`, `set`), or a read of a file that may hold secrets (the test of 012 FR-043), the activity and Fallow's records MUST show "Output hidden: it may hold secret values" instead of the output. The agent itself still receives the output from its runtime.

### Key Entities

- **Chat mode**: Manual or Auto, per spec session and per solo session. The default, for new chats and after a restart, comes from settings.
- **Refusal**: a request Fallow refused at a fixed limit. It records the action, the reason, the limit, and whether the engineer allowed it once or dismissed it.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a check by hand with at least 3 people new to Fallow (the person who gave the feedback among them, if willing), every one switches to Auto within 30 seconds of being asked to "let it run without asking", in both chats, without help.
- **SC-002**: In Auto, 0 request cards appear in the scripted test turns. The turns cover every request kind and every command shape the judge asks about today, for both agents.
- **SC-003**: Auto behaves identically with the fake runtime and with Claude Code, with no runtime-specific code on the path, as the scripted tests show.
- **SC-004**: Allowing a request in Auto takes no longer than allowing an approved command (within 10 ms at the median of the scripted turn).
- **SC-005**: 100% of refusals reach both the agent (with the reason) and the chat, "Needs you" and the tray, within 2 s, as the scripted tests show.
- **SC-006**: In Auto, 0 environment or secret values appear in the activity or in Fallow's records for the scripted turn that runs `env` and reads a `.env` file.

## Assumptions

- "Exactly like Claude Code" means: choose the mode, and the agent stops asking; a default mode in settings; Shift+Tab to switch. The engineer explicitly rejected a reviewer model.
- The lead's former Read only and Approved commands modes are dropped without a replacement. An engineer who wants a stricter lead stays in Manual and denies.
- Constitution 10.1.0 (2026-11-18) is made with this spec (II, V, VI).
- Builds on 007 and 009 (the lead's spec sessions), 012 (solo sessions, FR-008 and FR-043), 013 (themes) and 014 (the window), as on `main` at `dbafdd1`.
