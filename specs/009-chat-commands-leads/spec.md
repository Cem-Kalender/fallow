# Feature Specification: Chat commands and lead choice

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/009-chat-commands-leads`)

**Created**: 2026-11-02

**Status**: Draft

**Input**: User description: "The engineer's second round of remarks on Fallow Studio (008), as a feature of its own, built on 008 as it is: a '/' menu in the chat with the lead, Fallow's own and model-agnostic, with Spec Kit steps, Fallow actions, the project's agents and the project's own commands and skills; choosing the lead (a default per project, another per spec session, a session keeps its lead, the loop's lead is the spec session's); a loading animation while the lead answers; every way of leaving an agent or pipeline with unsaved changes asks first; and (added the same day) a read-only view of the project's files: a Files tab on the project page and a Files drawer on a feature's loop page and spec session. Out of scope: supplier adapters for other runtimes, the standalone app, changes to the loop engine beyond using the session's lead." (The full description is kept in the conversation of 2026-11-02.)

## Context

The engineer ran a full feature through Fallow Studio (008) and came back with four remarks, and a fifth wish the same day:

- **No commands.** In Claude Code, typing `/` lists the commands, skills and agents at hand. In Fallow's chat with the lead, `/` does nothing: the engineer has to know the Spec Kit steps by name and write them out, and Fallow's own actions are only buttons. Fallow is model-agnostic, so the list cannot be one runtime's built-in slash commands: it has to be Fallow's own, and work with any lead.
- **The lead cannot be chosen.** The Agents tab lets the engineer create an agent of kind Lead, but Fallow always talks to the agent named `lead`. A second lead is saved and never used.
- **A waiting chat looks stuck.** While the lead answers, the chat shows only the italic text "The lead is answering…".
- **Unsaved work is lost.** Fallow asks "Leave your changes?" when the engineer picks another agent or pipeline, or presses Back or Forward, but not when the engineer goes to another tab or screen, or closes or reloads the window: the edits are lost without a word.
- **The project's files are out of sight.** Everything is built by agents, but the engineer still wants to see the project's files and folders, and what the agents changed in them. Fallow shows the spec documents and diffs per task, but no tree of the project.

The engineer agreed the answers on 2026-11-02: a lead per project with another choice per session, and all four groups in the `/` menu. For the files, from a mockup (`mockup/files.html`): a Files tab on the project page and a Files drawer on a feature's loop page and spec session, both read-only.

**What does not change**: how files are changed (by agents, the lead, or the engineer's own editor; Fallow's file views only read), the spec session's steps and documents, the loop and its verification, the pipelines, and the agent definitions' format (a lead is an agent of kind Lead, as today).

## Clarifications

### Session 2026-11-02

- Q: Should the Spec Kit steps also appear in the `/` menu of the project conversation, or only in a feature's spec session? → A: In a spec session, all steps (except `implement` and `taskstoissues`, which a lead may not run); in the project conversation, only `/specify <title>`, which starts a new feature as "+ New feature" does (FR-007).
- Q: When the engineer asks the lead about a file from a feature's loop page, which conversation gets the question? → A: The project conversation, which is always open and whose lead only reads; the reference names the file and the feature's branch (FR-031). From a spec session's drawer, it goes into that spec session's own chat, which is open and whose lead knows the feature (decided at analyze, 2026-11-02).
- Q: What does "Open in my editor" do when no editor is set yet? → A: Fallow looks for known editors on the computer, proposes the one it finds, and saves the engineer's confirmation as the setting; if it finds none, it asks for the editor's command (FR-032).
- Q: Should the `/` menu leave out Fallow actions that are not possible now, or show them greyed out with the reason? → A: Show them greyed out at the end of their group, with the reason; they cannot be picked (FR-010).

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
7. **Given** the lead is answering, **When** the engineer opens the menu, **Then** "Cancel the answer" is listed and does what Stop does; actions that are not possible now (Lock before the spec is ready, say) are listed greyed out at the end of their group with the reason, and cannot be picked.
8. **Given** a screen reader user, **When** the menu opens and the selection moves, **Then** the highlighted entry and the number of matches are announced.
9. **Given** the project conversation, **When** the engineer opens the menu, **Then** the only Spec Kit step listed is "specify"; **When** the engineer sends `/specify A cart for the shop`, **Then** a new feature "A cart for the shop" starts as with "+ New feature", its spec session opens, and the lead runs specify there.

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

While the lead answers, the chat shows a small moving indicator next to "The lead is answering…", like Claude's, from the moment the message is sent until the answer is complete, also while the lead's first words stream in. An engineer who asked the system for reduced motion sees a still indicator.

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

### User Story 5 - The project's files, read-only (Priority: P3)

On the project page, the left column has two tabs, Features and Files. Files shows the project's tree for a version the engineer chooses: the project folder as it is, or a feature's branch as its agents leave it (a running loop's work included). Files the chosen feature changed are marked (changed, added, deleted), and a filter narrows the tree by name or to the changed files only. A file opens in the middle of the page, in place of the chat, read-only, with the lines added on that branch highlighted and who changed it in which task. From there the engineer can see its changes against the base, ask the lead about it, open it in their own editor, or go back to the conversation. On a feature's loop page and in its spec session, a Files button opens a drawer in place of the side column, scoped to that feature, listing its changed files first; opening a file widens the drawer over the agent cards and the feed so the code reads at full width, while the header and the task tree stay in view; "Open full" goes to the project page with that file and version chosen.

**Why this priority**: The engineer wants to keep an eye on the code agents write, but the loop already verifies the work and shows diffs per task; this is for orientation, not for the loop to function.

**Independent Test**: With a loop running on `001-shopping-cart`, open the project page's Files tab and choose that feature's version: `shop/cart.py` is marked changed; open it: the builder's lines are highlighted with the task that added them. On the loop page, Files lists the changed files first; "Open full" shows the same file on the project page. Nothing in either view can change a file.

**Acceptance Scenarios**:

1. **Given** the project page, **When** the engineer chooses the Files tab, **Then** the project's tree shows for the project folder as it is, with folders folded except the top level, and files the project ignores (its ignore rules, version-control internals) left out.
2. **Given** a feature with a branch, **When** the engineer chooses it as the version, **Then** the tree is that branch as its agents leave it, files it changed are marked changed, added or deleted against the version it started from, and "only changed" narrows the tree to them.
3. **Given** a file in the tree, **When** the engineer opens it, **Then** it shows in place of the chat, read-only, with line numbers; on a feature's version the lines it added are highlighted, and the file's header says who last changed it on that branch and in which task.
4. **Given** an open file, **When** the engineer chooses "Ask the lead about it", **Then** the project conversation shows with a reference to the file in the message box; when sent, the lead gets the file's text, its path and its version (the project folder, or the feature's branch) with the message. From a loop page's drawer this goes to the project conversation too, since the feature's spec session is closed while it is locked; from a spec session's drawer it goes into that spec session's own chat.
5. **Given** an open file, **When** the engineer chooses "Open in my editor", **Then** the file opens in the editor set in Fallow's settings; the first time, without one, Fallow proposes an editor it found on the computer (or asks for the editor's command if it found none) and keeps the engineer's confirmed choice. It never runs the file.
6. **Given** a running loop, **When** its builder changes a file, **Then** the tree and an open file show the change at the page's next refresh (every 3 seconds), without closing the open file or moving the reading position.
7. **Given** a feature's loop page or spec session, **When** the engineer chooses Files, **Then** a drawer opens in place of the side column listing that feature's changed files first, with who changed each and in which task; **When** the engineer opens one, **Then** the drawer widens over the agent cards and the feed and shows the file's changes (or the whole file) at full width, with the header and the task tree still in view; "Back to the loop" brings the cards and the feed back, and "Open full" opens the project page's Files tab with that file and version.
8. **Given** any file view, **When** the engineer tries to type in it, **Then** nothing changes: there is no way to edit, save, rename or delete a file there. (must fail)

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
- **A binary or very large file**: a binary file shows its kind and size, not its bytes; a large text file shows its beginning with a note and "Open in my editor".
- **A path outside the project** (a link pointing out of the folder, `..` in an address): not shown and not opened; the file views never read outside the project and Fallow's copies of it.
- **A feature whose copy is gone** (merged and cleaned up, or deleted): it is no longer a version to choose; an open file of it says so and offers the project folder's version.
- **A file that may hold secrets** (`.env`, private keys, credential files): listed with a lock mark, never opened, previewed or sent to the lead; the view says Fallow does not read files that may hold secrets and offers "Open in my editor" (constitution V).
- **A plain folder without git**: only "the project folder as it is" is offered, with no marks.
- **A file deleted on the chosen branch**: listed with a deleted mark when "only changed" is on; opening it shows its last content before the deletion.

## Requirements *(mandatory)*

### Functional Requirements

**The `/` menu**

- **FR-001**: In the chat with the lead, typing `/` as the first character of the message box MUST open a menu of commands; `/` anywhere else MUST NOT.
- **FR-002**: The menu MUST list four groups, in this order: Spec Kit steps, Fallow actions, Agents, Project commands. Each entry MUST show its name and one line saying what it does; project commands MUST also show where they came from.
- **FR-003**: Typing after `/` MUST narrow the list to entries whose name or description match; the arrow keys MUST move the highlight, Enter or a click MUST pick, Tab MUST complete the highlighted name, and Escape MUST close the menu leaving the text as typed.
- **FR-004**: The menu MUST be usable by keyboard alone and announced to screen readers (the highlighted entry and the number of matches).
- **FR-005**: Picking a Spec Kit step or a project command MUST put `/<name> ` in the box for the engineer to complete and send; sending `/<name>` followed by text MUST send that command with the text as its input. A message whose first word names no command MUST be sent as plain text.
- **FR-006**: A sent Spec Kit step MUST reach the lead as a request to run that step following its instruction file in the project, with the engineer's text as the step's input. It MUST NOT depend on any runtime's built-in slash commands, so that a lead of any runtime can follow it.
- **FR-007**: The Spec Kit steps listed MUST be those whose instruction files are installed in the project's working copy for the session; a step whose file is missing MUST be shown as unavailable with the reason. In a spec session every step MUST be offered except `implement` (the loop builds, and a lead may not write code) and `taskstoissues` (it publishes to GitHub), constitution VI; in the project conversation only `/specify <title>`, which MUST start a new feature with that title exactly as "+ New feature" does: its spec session opens with the project's default lead, and that lead runs specify there.
- **FR-008**: Project commands MUST be found in the project's command and skill folders of the runtimes Fallow knows (at least Claude Code's commands and skills); a sent project command MUST reach the lead as the command's text, with the engineer's text as its input, whatever runtime the lead uses. Spec Kit's own skills MUST NOT be listed twice.
- **FR-009**: A project command MUST NOT widen what the lead may do: settings in the command's file that grant tools or permissions MUST NOT be applied; the lead's approvals stay those of every lead turn.
- **FR-010**: Fallow actions MUST do exactly what their button does (including its confirmations), and only actions possible at that moment MUST be choosable; actions not possible now MUST be shown greyed out at the end of their group with the reason (for example "Not ready to lock: …", or "Open a feature's spec session to lock it"), MUST NOT be choosable, and MUST be announced as unavailable to screen readers. The list MUST include at least: start a new feature, lock the spec, cancel the answer, expand or narrow the chat, open a document of the feature, and go to the feature's loop when it has one.
- **FR-011**: Picking an agent MUST open it in the Agents tab; picking a lead MUST, in a session without a message yet, make it the session's lead (FR-013), and otherwise open it in the Agents tab.
- **FR-012**: The chat MUST show a sent command as the engineer typed it (`/plan …`), not as the instructions Fallow passed to the lead.

**Choosing the lead**

- **FR-013**: Each project MUST have a default lead: the agent named `lead` unless the engineer chose another agent of kind Lead for the project. The choice MUST be made the way a pipeline is chosen for a project, and shown on the project and in the Agents tab.
- **FR-014**: A new spec session (a new feature or the project conversation) MUST start with the project's default lead, and the engineer MUST be able to pick another available lead until the first message is sent.
- **FR-015**: From the first message on, a session MUST keep its lead. The first message counts from the moment it is sent, while the lead is still answering it too, and also when its turn failed; only a message the engineer cancelled (withdrawn) does not count. Changing the lead while the lead is answering MUST be refused; Fallow MUST NOT offer to change it there and MUST say why when the engineer looks for it.
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

**The project's files**

- **FR-027**: The project page's left column MUST offer two tabs, Features and Files; Files MUST show the project's tree, leaving out what the project ignores and version-control internals.
- **FR-028**: The engineer MUST be able to choose the version shown: the project folder as it is, or any feature with a branch or a working copy of its own, including one whose loop is running.
- **FR-029**: On a feature's version, files MUST be marked changed, added or deleted against the version the feature started from, and the engineer MUST be able to narrow the tree to changed files and filter it by name.
- **FR-030**: Opening a file MUST show it in place of the chat, read-only, with line numbers; on a feature's version the lines it added MUST be highlighted, and the view MUST say who last changed the file on that branch and in which task, when Fallow knows.
- **FR-031**: An open file MUST offer: its changes against the base, "Ask the lead about it" (a reference to the file and its version in the message box, whose text the lead receives with the message, without changing the file; from the project page and from a loop page's drawer it goes to the project conversation, from a spec session's drawer into that spec session's chat), "Open in my editor", and the way back to the conversation.
- **FR-032**: "Open in my editor" MUST open the file in the editor set in Fallow's settings. Without one, Fallow MUST look for known code editors on the computer, propose the one it found, and keep it as the setting once the engineer confirms; if none is found, it MUST ask for the editor's command (and show the file's full path to copy if the engineer declines). The setting is a program with optional fixed arguments (for example `code -r`); Fallow MUST start it without a shell (a Windows `.cmd` launcher excepted, with the path checked for characters the shell would read), with the file's path as the last argument. The setting MUST be changeable and removable from the file view ("Editor: <program> · change"). It MUST NOT run the file or open it with whatever program the system associates with it.
- **FR-033**: A feature's loop page and spec session MUST offer a Files drawer in place of the side column, scoped to that feature: changed files first, with who changed each and in which task. Opening a file MUST widen the drawer so that code is read at full width (over the agent cards and the feed, never in the narrow column), keeping the header's controls and the task tree in view, with a way back to the loop and "Open full" to the project page's Files tab with that file and version chosen.
- **FR-034**: The tree and an open file MUST follow changes made while they are shown (a running loop's included): Fallow MUST answer each request from the files as they are (nothing kept from before), and the page MUST ask again every 3 seconds while it is visible, keeping the open file, the folded folders and the reading position.
- **FR-035**: The file views MUST NOT change any file: no editing, saving, renaming, creating or deleting.
- **FR-036**: The file views MUST NOT read or show anything outside the project folder and Fallow's copies of it for its features; links that lead outside MUST NOT be followed.
- **FR-036a**: Files that may hold secrets (environment files such as `.env`, private keys and certificates, credential and token files) MUST NOT be read: the tree lists them with a mark, and opening one, its changes or a reference to it for the lead MUST say that Fallow does not read files that may hold secrets, offering only "Open in my editor".
- **FR-037**: Binary files MUST show their kind and size, not their content; text files over a size limit MUST show their beginning with a note that the rest is in the editor.
- **FR-038**: The Files tab, the tree and the drawer MUST be usable by keyboard alone (move, fold, open) and announced to screen readers, as every control in Fallow.

#### Follow-up: the project page's side rail (the engineer, 2026-11-02; mockup/project-proposals.html, option 1)

- **FR-039**: The project page MUST NOT have a fixed right column. A rail on its right edge MUST offer two panels, opened at will: "Proposed features" and "What the lead knows". The "Ask the lead to" shortcuts MUST be removed. The Features/Files column MUST take the room freed (340 px on a wide window).
- **FR-040**: "Proposed features" MUST list the features the lead proposed in the project conversation that are not started (no feature of the project has that name), the latest proposal of a name only, each with the lead's summary, "Start spec session" and "Where it was proposed" (which brings the proposal in the chat into view). A started proposal MUST leave the list. The rail's icon MUST show how many there are.
- **FR-041**: A panel MUST open and close by its icon, close by Close, by Escape while the keyboard is in the panel or on the rail, and by a click elsewhere; the keyboard's focus MUST go to the panel's heading when it opens and back to its icon when it is closed from the keyboard. On a wide window the panel MUST float over the chat and end above the message box, however tall it grows; on a narrow one it MUST sit in the page above the chat. An open panel and the icon's count MUST follow the conversation without closing or moving the focus. Both panels MUST also be reachable from the `/` menu.
- **FR-042**: A project MUST NOT get two features of the same name (compared without case, spaces collapsed): Fallow MUST refuse the second, naming the feature that exists. A proposal already started MUST show "Started" with the way to its feature in the chat, in place of "Start spec session", and starting it again MUST open that feature; a second click while a feature is being created MUST do nothing (the engineer, 2026-11-02: a proposal could be started any number of times).

### Key Entities

- **Command**: an entry of the `/` menu: its name, its group (Spec Kit step, Fallow action, agent, project command), one line of description, where it came from (for project commands), whether it is available now and, if not, why.
- **Session lead**: the lead agent a spec session talks to, recorded when the session starts and fixed from its first message: the agent's name, with its runtime and model as they were at the first message.
- **Project's default lead**: the lead a project's new sessions start with; `lead` unless chosen.
- **File view**: a read-only look at the project's tree and files for a version (the project folder as it is, or a feature's branch), with each file's mark against the feature's base and, when known, who changed it in which task.
- **Unsaved change**: an edit of an agent, a pipeline or a spec document not yet saved.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From an empty message box, the engineer can send any Spec Kit step in at most 3 typed characters and two presses of Enter (for example `/pl`, Enter to pick, Enter to send).
- **SC-002**: The `/` menu opens and narrows as the engineer types with no noticeable delay: the list of a project with 200 commands is ready within 100 ms, and narrowing happens in the page without asking Fallow again.
- **SC-003**: A Spec Kit step sent from the menu leads to the same documents as the same step asked for in words, for a lead of any runtime Fallow supports (checked with Fallow's test agent and one real lead).
- **SC-004**: In a project with two leads, every session and every loop question is answered by the lead the session shows, in 100% of the checked cases.
- **SC-005**: In every way of leaving an unsaved agent, pipeline or document listed in Story 4, no edit is lost without the engineer confirming (0 silent losses in the checked cases).
- **SC-006**: The engineer sees that the lead is working as soon as the message is sent, before Fallow has answered the request, every time.
- **SC-007**: From a loop page, the engineer reaches the content of a file the builder just changed in at most 2 clicks.
- **SC-008**: Fallow lists the tree of a project of up to 5,000 files within 2 seconds of the engineer choosing the Files tab or another version; the page draws it from that one answer.
- **SC-009**: No file is changed through the file views in any checked case (they only read).

## Assumptions

- The `/` menu is in the chat with the lead only (spec sessions and the project conversation); the loop page's steering keeps its buttons.
- "Fallow knows" the command folders of Claude Code (`.claude/commands`, `.claude/skills`) now; and also the project command folders of Gemini CLI (`.gemini/commands`), GitHub Copilot (`.github/prompts`) and Cursor (`.cursor/commands`); further runtimes' folders are added as Fallow learns those runtimes. With only Claude available to the engineer, the other folders are checked with files laid down in a test project, not with those runtimes.
- A Spec Kit step's instruction file is the one Spec Kit installed for the project (today its Claude skills); the lead is told the file's path in its working copy and reads it. No runtime's native slash commands are used.
- The project's default lead is kept with the project's other settings in Fallow, not in the repository; a team that wants to share it uses an agent saved for the project.
- A session's lead is fixed by the first message because the agent session Fallow resumes belongs to one runtime and model; changing the lead's instructions does not need a new session.
- The loading indicator replaces nothing else: Stop, the live text and the cancel behaviour of 008 stay as they are.
- The leave prompt on closing or reloading is the browser's own, whose text Fallow cannot set.
- The file views are for reading because agents build and Fallow verifies: the engineer's own changes go through the lead or their own editor, where Fallow's lock and loop already account for them.
- "The editor set in Fallow's settings" is a command (for example VS Code's `code`), proposed by Fallow from the editors it finds or typed by the engineer, confirmed once; Fallow passes it the file's path only. The editors Fallow looks for are common code editors installed in their usual places or on the command path.
- The mockup `mockup/files.html` (with `files-1440.png`) shows both places; it is the reference for the look, and the visual check compares against it at 1440 px and 390 px.
