# Feature Specification: Chat commands and lead choice

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/009-chat-commands-leads`)

**Created**: 2026-11-02

**Status**: Draft

**Input**: User description: "The engineer's second round of remarks on Fallow Studio (008), as a feature of its own, built on 008 as it is: a '/' menu in the chat with the lead, Fallow's own and model-agnostic, with Spec Kit steps, Fallow actions, the project's agents and the project's own commands and skills; choosing the lead (a default per project, another per spec session, a session keeps its lead, the loop's lead is the spec session's); a loading animation while the lead answers; every way of leaving an agent or pipeline with unsaved changes asks first. Out of scope: supplier adapters for other runtimes, the standalone app, changes to the loop engine beyond using the session's lead." (The full description is kept in the conversation of 2026-11-02.)

## Context

The engineer ran a full feature through Fallow Studio (008) and came back with four remarks:

- **No commands.** In Claude Code, typing `/` lists the commands, skills and agents at hand. In Fallow's chat with the lead, `/` does nothing: the engineer has to know the Spec Kit steps by name and write them out, and Fallow's own actions are only buttons. Fallow is model-agnostic, so the list cannot be one runtime's built-in slash commands: it has to be Fallow's own, and work with any lead.
- **The lead cannot be chosen.** The Agents tab lets the engineer create an agent of kind Lead, but Fallow always talks to the agent named `lead`. A second lead is saved and never used.
- **A waiting chat looks stuck.** While the lead answers, the chat shows only the italic text "The lead is answering…".
- **Unsaved work is lost.** Fallow asks "Leave your changes?" when the engineer picks another agent or pipeline, or presses Back or Forward, but not when the engineer goes to another tab or screen, or closes or reloads the window: the edits are lost without a word.

The engineer agreed the answers on 2026-11-02: a lead per project with another choice per session, and all four groups in the `/` menu.

**What does not change**: the spec session's steps and documents, the loop and its verification, the pipelines, and the agent definitions' format (a lead is an agent of kind Lead, as today).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Commands from the keyboard (Priority: P1)

In the chat with the lead (a spec session or the project conversation), the engineer types `/` at the start of the message box. A menu opens above the box with what can be done here, in groups: Spec Kit steps, Fallow actions, agents, and the project's own commands. Typing more narrows the list; the arrow keys move, Enter picks, Escape closes the menu and leaves the text as typed. Picking a Spec Kit step or one of the project's commands sends it to the lead as a message, with whatever the engineer typed after the command; picking a Fallow action does that action, as its button would.

**Why this priority**: It is what the engineer asked for first and uses at every turn; it makes the Spec Kit steps and Fallow's actions discoverable without knowing their names.

**Independent Test**: In a spec session, type `/pl`, pick "plan" with Enter: the lead receives a request to run the plan step, following that step's instruction file, and answers as for a written request. Type `/` and pick "Expand": the chat takes the full width.

**Acceptance Scenarios**:

1. **Given** an empty message box, **When** the engineer types `/`, **Then** the menu opens with the groups Spec Kit steps, Fallow actions, Agents and Project commands, each entry with its name and one line saying what it does.
2. **Given** the menu is open, **When** the engineer types `cla`, **Then** only entries whose name or description match remain (here "clarify"), and the first match is highlighted.
3. **Given** "clarify" is highlighted, **When** the engineer presses Enter, **Then** the box reads `/clarify ` and the menu closes; **When** the engineer then sends, **Then** the lead is asked to run Spec Kit's clarify step following its instruction file, and the chat shows the engineer's message as `/clarify`.
4. **Given** the engineer typed `/specify A cart for the shop`, **When** it is sent, **Then** the lead runs the specify step with "A cart for the shop" as its input.
5. **Given** the menu is open, **When** the engineer presses Escape, **Then** the menu closes and the text stays as typed, and a message that starts with `/` but names no command is sent as plain text. (must fail)
6. **Given** the project has `.claude/commands/review-copy.md`, **When** the menu opens, **Then** "review-copy" is listed under Project commands with where it came from; **When** it is sent, **Then** the lead receives the command's text as its instructions, whatever runtime the lead uses.
7. **Given** the lead is answering, **When** the engineer opens the menu, **Then** "Cancel the answer" is listed and does what Stop does; actions that are not possible now (Lock before the spec is ready, say) are not listed, or are listed with the reason they are not available.
8. **Given** a screen reader user, **When** the menu opens and the selection moves, **Then** the highlighted entry and the number of matches are announced.

---

### User Story 2 - Choosing the lead (Priority: P1)

Each project has a default lead: the agent named `lead` unless the engineer chooses another agent of kind Lead for the project, the way a pipeline is chosen with "Use for this project". When the engineer starts a spec session (a new feature or the project conversation), the default lead is chosen and another lead can be picked before the first message. From the first message on, the session keeps its lead, because an agent session cannot move to another runtime or model. The chat shows which lead the session talks to. When the feature's loop runs, the lead that answers the builders' questions is the lead of that feature's spec session.

**Why this priority**: A lead the engineer creates and cannot use is a broken promise of the Agents tab; the choice also lets the engineer try a lead with other instructions or another model on one feature.

**Independent Test**: Create a second lead, `careful-lead`, and make it the project's default. Start a new feature: its chat says it talks to careful-lead, and careful-lead answers. Start another feature, pick `lead` before the first message: `lead` answers there. Lock a feature started with careful-lead and run its loop: a builder's question to the lead is answered by careful-lead.

**Acceptance Scenarios**:

1. **Given** a project with no lead chosen, **When** the engineer starts a spec session, **Then** the session's lead is the agent named `lead`, and the chat shows "Lead: lead".
2. **Given** the Agents tab with a lead `careful-lead` and a project selected, **When** the engineer chooses "Use for this project" on it, **Then** the project's new spec sessions start with careful-lead, and sessions already started keep theirs.
3. **Given** a new spec session without a message yet, **When** the engineer picks another lead (from the chat's lead control or the `/` menu), **Then** the session's lead changes and the chat shows it.
4. **Given** a session with at least one message, **When** the engineer looks for another lead, **Then** the lead cannot be changed there and Fallow says why ("A session keeps its lead; start a new feature or session to use another"). (must fail)
5. **Given** a session whose lead definition was deleted, **When** the engineer opens it, **Then** Fallow says the lead is gone and the session cannot go on with it; the documents and the conversation stay readable.
6. **Given** a feature locked from a session with careful-lead, **When** a builder in its loop asks a question that the lead answers, **Then** careful-lead answers it, and the loop's feed says so.
7. **Given** a session started before this feature existed, **When** it is opened, **Then** its lead is `lead`, as it was.

---

### User Story 3 - A chat that shows it is working (Priority: P2)

While the lead answers, the chat shows a small moving indicator next to "The lead is answering…", like Claude's, until the lead's first words or its answer arrive. An engineer who asked the system for reduced motion sees a still indicator.

**Why this priority**: Small, but a still line of text made the engineer wonder whether Fallow was stuck.

**Independent Test**: Send a message: the indicator moves until the answer arrives. With reduced motion asked for, it does not move.

**Acceptance Scenarios**:

1. **Given** the engineer sent a message, **When** the lead has not answered yet, **Then** the chat shows the moving indicator with "The lead is answering…".
2. **Given** the lead's live text starts to arrive, **When** it is shown, **Then** the indicator stays at the end of that text until the answer is complete.
3. **Given** the answer is complete or cancelled, **When** the chat updates, **Then** the indicator is gone.
4. **Given** the system asks for reduced motion, **When** the lead answers, **Then** the indicator is shown without animation. (must fail)

---

### User Story 4 - Unsaved work is never lost silently (Priority: P2)

The engineer edits an agent or a pipeline and, without saving, goes elsewhere: another tab (Agents to Pipelines, say), the dashboard, a project, a spec session, or closes or reloads the window. Fallow asks first, as it already does when the engineer picks another agent or pipeline: "Leave your changes?". Leaving drops the changes; staying keeps the screen and the edits exactly as they were.

**Why this priority**: Losing edits is the worst kind of surprise in an editor; it is quick to fix.

**Independent Test**: Change an agent's instructions, click Pipelines: Fallow asks; choose to stay: the agent and the changed text are still there. Click Pipelines again and leave: Pipelines opens. Change a pipeline and reload the window: the browser asks before reloading.

**Acceptance Scenarios**:

1. **Given** unsaved changes in an agent, **When** the engineer clicks another tab or a link to another screen, **Then** Fallow asks "Leave your changes?" before going.
2. **Given** that question, **When** the engineer chooses to stay, **Then** the screen, the selected agent or pipeline, the edits and the address stay as they were.
3. **Given** that question, **When** the engineer chooses to leave, **Then** Fallow goes where the engineer clicked and the changes are dropped.
4. **Given** unsaved changes in an agent or a pipeline, **When** the engineer closes or reloads the window, **Then** the browser's own leave prompt appears.
5. **Given** no unsaved changes, **When** the engineer goes anywhere, **Then** nothing is asked. (must fail)
6. **Given** an unsaved edit of a document in a spec session, **When** the engineer goes to another screen or closes the window, **Then** Fallow asks in the same way.

---

### Edge Cases

- **A step's instruction file is missing** (Spec Kit not installed in the project, or a file deleted): the step is listed with "Spec Kit is not installed here" and cannot be sent; the lead is never pointed at a file that is not there.
- **Two commands with the same name** (a project command and a Spec Kit step, or the same name in two runtimes' folders): both are listed, each with where it came from; typing the name picks the Spec Kit step first, then the project's commands in a fixed order.
- **Spec Kit's own skills in the project's folders** (`.claude/skills/speckit-*`): listed once, as Spec Kit steps, not again as project commands.
- **A project command written for one runtime** (for example with Claude-specific settings at its top): its text is sent as instructions; settings Fallow does not understand are not applied, and the lead's permissions stay those Fallow gives every lead (the engineer's approvals, the project's approved commands).
- **A very large project command file**: it is not sent; the menu says it is too large.
- **`/` typed in the middle of a message**: no menu; only `/` at the start of the box opens it.
- **No matches**: the menu says "No command matches" and Enter sends the text as typed.
- **The default lead is deleted**: the project falls back to `lead` and the project's screen says so; sessions that used the deleted lead behave as in Story 2, scenario 5.
- **A lead whose runtime is not available** (not installed, not signed in): it is listed but cannot be picked for a new session; the reason is shown, as the Agents tab already shows it.
- **The lead's definition is edited during a session** (its instructions, say): the session keeps the lead by name and the next turn uses the definition as it is then; if its runtime or model changed, Fallow tells the engineer that the session cannot resume with the new runtime or model, and the next message starts a new agent session with the same lead, which reads the feature's documents as they are (FR-019a).
- **A loop whose feature has no recorded session lead** (locked before this feature): its questions go to `lead`, as today.
- **Several unsaved editors** (an agent and, in another window, a pipeline): each window asks for its own changes.
- **Leaving during a save**: the save completes or fails first; Fallow does not ask about changes that were saved.

## Requirements *(mandatory)*

### Functional Requirements

**The `/` menu**

- **FR-001**: In the chat with the lead, typing `/` as the first character of the message box MUST open a menu of commands; `/` anywhere else MUST NOT.
- **FR-002**: The menu MUST list four groups, in this order: Spec Kit steps, Fallow actions, Agents, Project commands. Each entry MUST show its name and one line saying what it does; project commands MUST also show where they came from.
- **FR-003**: Typing after `/` MUST narrow the list to entries whose name or description match; the arrow keys MUST move the highlight, Enter or a click MUST pick, Tab MUST complete the highlighted name, and Escape MUST close the menu leaving the text as typed.
- **FR-004**: The menu MUST be usable by keyboard alone and announced to screen readers (the highlighted entry and the number of matches).
- **FR-005**: Picking a Spec Kit step or a project command MUST put `/<name> ` in the box for the engineer to complete and send; sending `/<name>` followed by text MUST send that command with the text as its input. A message whose first word names no command MUST be sent as plain text.
- **FR-006**: A sent Spec Kit step MUST reach the lead as a request to run that step following its instruction file in the project, with the engineer's text as the step's input. It MUST NOT depend on any runtime's built-in slash commands, so that a lead of any runtime can follow it.
- **FR-007**: The Spec Kit steps listed MUST be those whose instruction files are installed in the project's working copy for the session; a step whose file is missing MUST be shown as unavailable with the reason.
- **FR-008**: Project commands MUST be found in the project's command and skill folders of the runtimes Fallow knows (at least Claude Code's commands and skills); a sent project command MUST reach the lead as the command's text, with the engineer's text as its input, whatever runtime the lead uses. Spec Kit's own skills MUST NOT be listed twice.
- **FR-009**: A project command MUST NOT widen what the lead may do: settings in the command's file that grant tools or permissions MUST NOT be applied; the lead's approvals stay those of every lead turn.
- **FR-010**: Fallow actions MUST do exactly what their button does (including its confirmations), and only actions possible at that moment MUST be offered; actions not possible now MUST either be left out or shown as unavailable with the reason. The list MUST include at least: start a new feature, lock the spec, cancel the answer, expand or narrow the chat, open a document of the feature, and go to the feature's loop when it has one.
- **FR-011**: Picking an agent MUST open it in the Agents tab; picking a lead MUST, in a session without a message yet, make it the session's lead (FR-013), and otherwise open it in the Agents tab.
- **FR-012**: The chat MUST show a sent command as the engineer typed it (`/plan …`), not as the instructions Fallow passed to the lead.

**Choosing the lead**

- **FR-013**: Each project MUST have a default lead: the agent named `lead` unless the engineer chose another agent of kind Lead for the project. The choice MUST be made the way a pipeline is chosen for a project, and shown on the project and in the Agents tab.
- **FR-014**: A new spec session (a new feature or the project conversation) MUST start with the project's default lead, and the engineer MUST be able to pick another available lead until the first message is sent.
- **FR-015**: From the first message on, a session MUST keep its lead; Fallow MUST NOT offer to change it there and MUST say why when the engineer looks for it.
- **FR-016**: The chat MUST show which lead the session talks to, with its runtime and model.
- **FR-017**: The lead that answers questions raised in a feature's loop MUST be the lead of that feature's spec session; a feature whose session has no recorded lead MUST use `lead`.
- **FR-018**: Sessions and loops created before this feature MUST keep working, with `lead` as their lead.
- **FR-019**: When a session's lead no longer exists or its runtime is not available, Fallow MUST say so in the session and MUST NOT send messages to another lead without the engineer choosing it; the session's documents and conversation MUST stay readable.
- **FR-019a**: When a session's lead changed runtime or model after the session started, Fallow MUST say so before the next message, and that message MUST start a new agent session with the same lead; the conversation and the documents stay.
- **FR-020**: Leads whose runtime is not available MUST be listed as unavailable with the reason, and MUST NOT be choosable for a new session or as a project's default.

**Working indicator**

- **FR-021**: While the lead is answering, the chat MUST show a moving indicator with "The lead is answering…" until the answer is complete or cancelled; it MUST stay visible while the lead's live text arrives.
- **FR-022**: When the system asks for reduced motion, the indicator MUST NOT animate.

**Unsaved changes**

- **FR-023**: With unsaved changes in an agent, a pipeline or a spec document, every way of leaving it within Fallow (another agent or pipeline, another tab, another screen, Back or Forward, a link) MUST ask "Leave your changes?" first.
- **FR-024**: Choosing to stay MUST keep the screen, the selection, the edits and the address exactly as they were; choosing to leave MUST go where the engineer asked and drop the changes.
- **FR-025**: With unsaved changes, closing or reloading the window MUST bring up the browser's leave prompt.
- **FR-026**: Without unsaved changes, nothing MUST be asked.

### Key Entities

- **Command**: an entry of the `/` menu: its name, its group (Spec Kit step, Fallow action, agent, project command), one line of description, where it came from (for project commands), whether it is available now and, if not, why.
- **Session lead**: the lead agent a spec session talks to, recorded when the session starts and fixed from its first message: the agent's name, with its runtime and model as they were at the first message.
- **Project's default lead**: the lead a project's new sessions start with; `lead` unless chosen.
- **Unsaved change**: an edit of an agent, a pipeline or a spec document not yet saved.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From an empty message box, the engineer can send any Spec Kit step in at most 3 typed characters and two presses of Enter (for example `/pl`, Enter to pick, Enter to send).
- **SC-002**: The `/` menu opens and narrows as the engineer types with no noticeable delay (under 100 ms for a project with up to 200 commands).
- **SC-003**: A Spec Kit step sent from the menu leads to the same documents as the same step asked for in words, for a lead of any runtime Fallow supports (checked with Fallow's test agent and one real lead).
- **SC-004**: In a project with two leads, every session and every loop question is answered by the lead the session shows, in 100% of the checked cases.
- **SC-005**: In every way of leaving an unsaved agent, pipeline or document listed in Story 4, no edit is lost without the engineer confirming (0 silent losses in the checked cases).
- **SC-006**: The engineer sees that the lead is working within 1 second of sending, every time.

## Assumptions

- The `/` menu is in the chat with the lead only (spec sessions and the project conversation); the loop page's steering keeps its buttons.
- "Fallow knows" the command folders of Claude Code (`.claude/commands`, `.claude/skills`) now; other runtimes' folders (for example Codex's prompts, Gemini's commands, Copilot's prompt files) are added as Fallow learns those runtimes. With only Claude available to the engineer, these are checked with files laid down in a test project, not with those runtimes.
- A Spec Kit step's instruction file is the one Spec Kit installed for the project (today its Claude skills); the lead is told the file's path in its working copy and reads it. No runtime's native slash commands are used.
- The project's default lead is kept with the project's other settings in Fallow, not in the repository; a team that wants to share it uses an agent saved for the project.
- A session's lead is fixed by the first message because the agent session Fallow resumes belongs to one runtime and model; changing the lead's instructions does not need a new session.
- The loading indicator replaces nothing else: Stop, the live text and the cancel behaviour of 008 stay as they are.
- The leave prompt on closing or reloading is the browser's own, whose text Fallow cannot set.
