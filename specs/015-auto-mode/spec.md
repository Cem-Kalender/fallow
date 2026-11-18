# Feature Specification: Manual and Auto mode for the assistant

**Feature Branch**: `main` (the project's working branch since 2026-11-14; the spec lives in `specs/015-auto-mode`)

**Created**: 2026-11-18

**Status**: Draft

**Input**: User description:

> Feedback from a user of Fallow on 2026-11-18, with a screenshot of the solo session's composer (the chip "Until I switch back" next to "Attach image"): "'Until I switch back' is a somewhat unintuitive description of what the button does" and "How do I turn on run without permission? It keeps asking."
>
> The engineer's decision (2026-11-18): "If a user chooses auto mode, the user should get auto mode. It should work exactly like Claude Code, while remaining model and LLM supplier agnostic." And: "Auto mode is auto mode. Manual mode is manual mode. No relaying actions to a second AI model."

## Context

Today a solo session (012) asks the engineer before most commands. The only relief is the composer's chip, which toggles between "Ask for commands" and "Until I switch back" (012 FR-008). The chip shows the current mode, not what clicking it does.

Even with "Until I switch back" on, Fallow keeps asking about:
- every command its rules deem risky;
- every command whose effect it cannot tell from the text, such as `python -c`, `npx`, `docker`, pipes and redirections.

So a user who turned it on is still asked all the time.

**What the engineer wants**: two plain modes.
- **Manual**: Fallow asks before commands, as today.
- **Auto**: Fallow asks nothing. Every action the assistant requests runs.

There is no second model and no reviewer. Fallow keeps no list of exceptions that are still asked.

**Model and supplier agnostic.** The mode is Fallow's own, not a runtime's flag:
- Fallow already receives each runtime's tool requests as its own kinds (run, edit, web, read, other) and answers them itself (012 FR-008, FR-043). In Auto it answers "allow" to every request.
- A runtime is never started with its own bypass option.
- So any runtime Fallow supports gets the same Auto. Today Claude Code is the only real one.

**Constitution amendment 9.0.0 (MAJOR: it removes guarantees)**, made on 2026-11-18:
- **VI**: in a solo session the engineer may choose Auto, in which every action the assistant requests runs without asking.
- **II**: the per-command asks that 012 FR-008 requires become Manual mode's only. In Auto, the engineer's choice of Auto is their word for every action.
- **Unchanged**:
  - "Options that make an agent skip its own permission checks MUST NOT be used": Fallow still answers every request; it answers yes.
  - Loops and the lead are untouched.
  - The merge rule of II: nothing reaches the target branch without the engineer's merge, or through a pull request on a protected remote.
  - Fallow still shows every change to branches and remotes it observes (II).

**Out of scope**:
- A reviewer model or classifier (rejected by the engineer).
- Accept-edits or plan modes.
- Auto for loops and for the lead's conversation.
- New runtimes.

## Clarifications

### Session 2026-11-18

- Q: What should the engineer get when they choose auto mode? → A: Auto mode: Auto is Auto, Manual is Manual. In Auto nothing is asked; no second model reviews actions. Model and supplier agnostic.
- Q: Do Fallow's fixed limits (own working copy only, no push to a protected target) hold in Auto? → A: They hold, but a refusal is never silent: the assistant is told why, and the engineer sees it at once with the reason and a button to allow it once, for every limit, a push to a protected target included, in case the engineer wants it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The engineer picks Manual or Auto (Priority: P1)

The composer shows a switch with two named modes, **Manual** and **Auto**, and the current one is always visible. A keyboard shortcut toggles them. Choosing Auto shows one line: "Auto: the assistant runs every action without asking you."

**Why this priority**: the user could not find or understand today's control.

**Independent Test**: ask an engineer new to Fallow to "let it run without asking me". Check that they switch to Auto, without help.

**Acceptance Scenarios**:

1. **Given** a solo session, **When** the engineer looks at the composer, **Then** they see "Manual" or "Auto" by name, as a control that switches between them.
2. **Given** Manual, **When** the engineer switches to Auto, by click or shortcut, **Then** the control shows Auto and the session adds the one-line note.
3. **Given** a default mode set in Fallow's settings, **When** the engineer starts a new solo session, **Then** it starts in that mode. Without the setting, new sessions start in Manual.

---

### User Story 2 - Auto asks nothing (Priority: P1)

In Auto, every request the assistant makes runs at once, with no card: commands, edits, reads and web requests. The activity shows each action as usual.

**Why this priority**: this is the mode the user asked for: "it keeps asking".

**Independent Test**: in Auto, drive the fake runtime through a turn that asks for:
- a read;
- an edit;
- `python -c …`;
- `npx …`;
- a redirection to a file;
- `git push --force` of the session's branch.

Check that all of them run with no card and appear in the activity.

**Acceptance Scenarios**:

1. **Given** Auto, **When** the assistant requests any command, edit, read or web request within Fallow's two fixed limits (FR-009), **Then** Fallow allows it without asking.
4. **Given** Auto, **When** the assistant requests an action outside the fixed limits (an edit outside its working copy that the engineer did not ask for, or a push to a protected target branch), **Then** the action does not run, and the refusal is never silent:
   - the assistant is told it was refused and why, so it can do it another way or tell the engineer;
   - the session shows the refusal at once, with the reason and "Allow this once", which runs it, a push to a protected target branch included, in case the engineer does want it;
   - the session appears in "Needs you", and the tray notifies, naming the project and session.
2. **Given** Auto, **When** a request waits for the engineer from before the switch, **Then** it is allowed at the switch.
3. **Given** Auto, **When** the engineer switches back to Manual, **Then** the next request is asked as today.

---

### User Story 3 - Manual asks clearly (Priority: P2)

Manual is today's asking. The request card says the session is in Manual and offers "Switch to Auto", next to Allow once, Always in this project, and Deny.

**Why this priority**: an engineer annoyed by asks should find Auto from the card that annoys them.

**Independent Test**: in Manual, ask for a command. Check that the card offers "Switch to Auto". Choosing it allows that command and puts the session in Auto.

**Acceptance Scenarios**:

1. **Given** Manual, **When** a card appears, **Then** it offers "Switch to Auto" alongside the existing choices.
2. **Given** the engineer chooses "Switch to Auto" on a card, **When** it takes effect, **Then** the command runs and the session is in Auto.

---

### Edge Cases

- **Fallow restarts while a session is in Auto.** Whether Auto comes back is decided by the default mode in settings, the same rule as for new sessions. 012's "never after a restart" is dropped. This is how Claude Code's `defaultMode` behaves.
- **A turn is running when the engineer switches mode.** The new mode applies from the next request.
- **Two windows or tabs** (automation builds). Both show the mode change within the usual refresh.
- **Dark mode (013).** Manual and Auto are distinguishable without relying on colour alone, at WCAG AA contrast in both themes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A solo session MUST be in one of two modes:
  - **Manual**: today's per-command asking;
  - **Auto**: no asking at all.

  "Ask for commands" and "Until I switch back" MUST be removed.
- **FR-002**: The composer MUST show the current mode by name, as a switch between Manual and Auto. A keyboard shortcut MUST toggle it. The switch MUST work with the keyboard and screen readers, with its name and state announced.
- **FR-003**: In Auto, Fallow MUST allow every request from the assistant without asking: commands, edits, reads and web requests. This includes commands its judge cannot read and commands 012 FR-008 asked per command. Fallow uses no reviewer, classifier or second model. Limits: FR-009.
- **FR-004**: Fallow MUST NOT start any runtime with that runtime's own option for skipping permission checks. Auto works by Fallow answering the runtime's requests, so it behaves the same for every runtime.
- **FR-005**: Fallow's settings MUST offer a default mode for solo sessions: Manual unless set. A new session, and a session after a restart, starts in it.
- **FR-006**: Switching modes MUST add a one-line note to the session. Switching to Auto says that every action now runs without asking.
- **FR-007**: In Manual, a request card MUST offer "Switch to Auto", which allows that request and switches the session.
- **FR-008**: The activity MUST show every action in Auto as it does today, so the engineer can see what ran. Fallow MUST keep showing the changes to branches and remotes it observes (II).
- **FR-009**: Auto MUST keep Fallow's two fixed limits on the assistant:
  - edits only inside the session's working copy, unless the engineer asked for the change elsewhere (012 FR-016);
  - no push to a protected target branch (II).

  A request outside them MUST NOT run, and its refusal MUST NOT be silent. The assistant MUST receive the refusal with its reason. The session MUST show it immediately with the reason and "Allow this once", which runs that one request, a push to a protected target branch included, as the engineer's decision (constitution II, 9.0.0). Whether the remote itself accepts the push is up to its protection. The session MUST be listed in "Needs you" and notified through the tray, naming the project and session only.

### Key Entities

- **Session mode**: Manual or Auto, per solo session. The default for new sessions and after a restart comes from settings.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a test with at least 3 engineers new to Fallow, every one switches to Auto within 30 seconds of being asked to "let it run without asking", without help.
- **SC-002**: In Auto, 0 request cards appear in the scripted test turn covering every request kind and every command shape the judge asks about today.
- **SC-003**: Auto behaves identically with the fake runtime and with Claude Code, with no runtime-specific code on the path, as the scripted tests show.
- **SC-004**: Allowing a request in Auto adds no measurable delay compared with an approved command today.
- **SC-005**: 100% of refusals in Auto reach both the assistant (with the reason) and the session, "Needs you" and the tray, within 2 s, as the scripted tests show.

## Assumptions

- "Exactly like Claude Code" means: choose the mode, and the agent stops asking; a default mode in settings; a shortcut to switch. The engineer explicitly rejected a reviewer model.
- Constitution 9.0.0 (2026-11-18) allows Auto mode and the override (II, VI).
- Builds on 012 (solo sessions, FR-008 and FR-043), 013 (themes) and 014 (the window), as on `main` at `5110626`.
