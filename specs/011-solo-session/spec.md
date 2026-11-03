# Feature Specification: Solo sessions

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/011-solo-session`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "Solo sessions (011), asked for by the engineer on 2026-11-02, after 010 (publishing to the remote). A bare coding assistant inside Fallow for vibe coding without Spec Kit. The project page's middle column gets two tabs, 'Plan with the lead' (today's conversation) and 'Code with the assistant', the latter a dropdown of sessions with '+ New session'; a new agent kind Assistant (shipped default `assistant`, any runtime and model); each session has its own worktree and branch from the engineer's branch, Fallow commits every turn; git by asking the assistant (merge, commit, push, pull request) counts as the engineer's decision, Fallow shows every change it observes to the project's branches and remotes; web access on by default for the assistant. First version: undo a turn, screenshots, queued messages while working, the project's instruction files handed as plain text, web access, gates on demand on the session's branch. Files tab shows the session's worktree with changes marked, read-only, conversation beside an open file. Closing is Fallow's own action; a closed session leaves the dropdown, its worktree is removed, its branch too when merged, with unmerged work Fallow asks first; its conversation stays readable under 'Closed sessions…'. Not in the first version: switching models within a session, a terminal, 'turn this into a feature'." The decisions agreed with the engineer are kept in [decisions.md](decisions.md), the approved mockup in [mockup/solo.html](mockup/solo.html).

## Context

Today a project in Fallow has one way to get code written: a feature, worked out with the lead in Spec Kit documents, locked, and built by a loop of agents that Fallow verifies. That is right for a feature that deserves a spec. It is too heavy for the rest of an engineer's day: a quick fix, an experiment, a refactor, a question about the code, a dependency to bump. For those the engineer leaves Fallow and opens a coding assistant in a terminal, where Fallow sees nothing.

A **solo session** is that coding assistant inside Fallow: a chat with an agent that reads and edits the project's code and runs commands, in a working copy of its own, while Fallow keeps what it always keeps: every turn committed, every change to the project's branches and remotes shown, the project's gates runnable on the result, and nothing presented as verified that was not.

Fallow stays an agentic orchestration tool, not an editor: writing code by hand belongs in the engineer's own editor.

The constitution already provides for it (6.3.0): in a solo session the assistant acts on the engineer's word, and a merge, commit, push or pull request it makes when the engineer asks counts as the engineer's decision (II); its web and git-remote traffic is on by default (V).

**What does not change**: the lead's conversation and features, loops and their verification, publishing a loop (010), and what any other agent kind may do.

## Clarifications

### Session 2026-11-03

- Q: Which commands may the assistant run without asking? → A: As the lead (constitution VI): the gates and the project's approved commands run; any other command is asked of the engineer in the chat, per command, with "allow until I switch back" for the session. VI is amended to name the assistant.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A coding session with the assistant (Priority: P1)

The engineer opens a project and switches the middle column from **Plan with the lead** to **Code with the assistant**. They start a new session and type what they want ("the date picker shows the wrong week on Sundays, fix it"). The assistant reads the code, edits files, runs commands, and answers in the chat, showing that it works and what it does, as the lead's conversation does today. Each turn's result is committed by Fallow on the session's own branch. The engineer's project folder and checked-out branch are untouched while they work.

**Why this priority**: it is the feature; everything else refines it.

**Independent Test**: on a project with a small bug, start a session, ask for the fix in one message, and check that the fix is on the session's branch as a commit for that turn, that the engineer's folder and branch did not change, and that the chat showed the assistant's work.

**Acceptance Scenarios**:

1. **Given** a project, **When** the engineer opens it, **Then** the middle column shows two tabs, "Plan with the lead" (today's conversation, unchanged) and "Code with the assistant", and the left column and the top bar stay as they are.
2. **Given** the assistant's tab with no session yet, **When** the engineer picks "+ New session", **Then** a session starts with its own working copy and branch, made from the engineer's current branch as it is then, with the project's default assistant (or the one the engineer picks), and is ready for a message.
3. **Given** a session, **When** the engineer sends a message, **Then** the assistant works in the session's working copy, its activity (files read and edited, commands, web requests) shows in the chat as it happens, and its answer ends the turn.
4. **Given** a turn that changed files, **When** the turn ends, **Then** Fallow has committed the changes on the session's branch, one commit per turn, named after the turn.
5. **Given** a session at work, **When** the engineer looks at their project folder, **Then** its files and checked-out branch are as they were (until the engineer asks the assistant to change them, User Story 4).
6. **Given** the assistant at work, **When** the engineer stops the turn, **Then** the assistant stops, what it changed so far is committed as the turn's result and marked as cut short, and the engineer can go on.

---

### User Story 2 - Several sessions, picked from one dropdown (Priority: P1)

The engineer keeps more than one session open: a fix in one, an experiment in another. "Code with the assistant" is a dropdown: clicking it opens the list of the project's open sessions, each named after its first message (renameable in its header), the open one ticked and on top, a dot on those whose assistant is working, and the number of changes in each; "+ New session" starts another. Picking an item opens that session. Sessions work at the same time without touching each other.

**Why this priority**: a session per task is how the engineer works; one shared working copy would mix unrelated changes.

**Independent Test**: start two sessions, give each a task that edits the same file differently, and check that each session's branch holds only its own change, that both show in the dropdown with their state, and that switching between them never changes either.

**Acceptance Scenarios**:

1. **Given** two open sessions, **When** the engineer clicks "Code with the assistant", **Then** a list opens with both sessions and "+ New session"; clicking the tab never switches session by itself.
2. **Given** the list open, **When** the engineer picks a session, **Then** that session's conversation, changes and gates show, and the list closes.
3. **Given** one session's assistant working, **When** the engineer opens another session and sends a message, **Then** both work at the same time, each in its own working copy and on its own branch.
4. **Given** a session, **When** the engineer renames it in its header, **Then** the dropdown shows the new name.
5. **Given** sessions open when Fallow stops or restarts, **When** Fallow is back, **Then** every session is in the dropdown with its conversation, working copy and branch; a turn that was at work is shown as cut short, never repeated by itself.

---

### User Story 3 - Seeing and checking what the session changed (Priority: P2)

Beside the conversation, the assistant's tab shows the session's **Changes** (the files changed since the session started, with what changed) and **Gates** (the project's gates, runnable on the session's branch). In the left column, the Files tab shows the session's working copy with its changed files marked. Opening a file shows it in the middle, read-only, with the conversation beside it. The engineer can undo the last turn. The session's result says "Not verified by Fallow" until the gates ran on it, and again after any later change.

**Why this priority**: a coding assistant whose changes the engineer cannot see, check and undo is one they have to babysit in another tool.

**Independent Test**: in a session, let the assistant change two files over two turns; check the Changes panel and the marked files; run the gates and see their outcome against where the session started; undo the second turn and check the branch is back at the first turn's commit; check the label before and after the gates ran, and after a further change.

**Acceptance Scenarios**:

1. **Given** a session with changes, **When** the engineer looks at the assistant's tab, **Then** the Changes panel lists every file changed since the session started (added, modified, deleted) and shows what changed in each.
2. **Given** the Files tab while the assistant's tab is on, **When** the engineer looks at it, **Then** it shows the open session's working copy, changed files marked, folders with the number of changes in them, and a way to show only changed files; with the lead's tab on, it shows the project as today.
3. **Given** a file opened from the Files tab or the Changes panel, **When** it shows in the middle, **Then** it is read-only, the conversation stays usable beside it, and "Open in my editor" opens it in the engineer's editor.
4. **Given** the Gates panel, **When** the engineer runs the gates, **Then** Fallow runs the project's gates on the session's branch, compared with the commit the session started from, and shows each gate's outcome with its output.
5. **Given** a session whose gates have not run on its latest commit, **When** the engineer looks at it, **Then** it says "Not verified by Fallow" and names what was not checked; after the gates pass it says they passed on that commit, never in the words or colors of a verified loop (no independent checker looked at it); after any later change it says "Not verified by Fallow" again.
6. **Given** a session with at least one turn, **When** the engineer undoes the last turn, **Then** the session's branch and working copy are back at the previous turn's commit, the undone turn stays visible in the conversation marked as undone, and the assistant is told in its next turn that it was undone.

---

### User Story 4 - Git by asking, every change shown (Priority: P2)

There is no merge button in a session. When the engineer wants the work somewhere, they ask the assistant, as they would in a terminal: "merge this into main", "commit and push", "open a pull request". That is the engineer's decision (constitution II). Fallow watches the project's branches and remotes and shows every change it observes, whoever made it ("main moved: 1 commit merged from this session"; "pushed fallow/solo/date-picker to origin").

**Why this priority**: it is how the session's work leaves the session; showing every change is what keeps the engineer's word the only authority over their branches.

**Independent Test**: in a session, ask the assistant to merge into the engineer's branch, then to push its branch to a remote; check that each change shows in the session and on the project, naming the branch, the commits and who made it; move a branch outside Fallow and check that it shows too.

**Acceptance Scenarios**:

1. **Given** a session with work, **When** the engineer asks the assistant to merge it into their branch, **Then** the assistant merges, and Fallow shows in the session and on the project that the branch moved, by how many commits, from this session.
2. **Given** a project with a remote, **When** the engineer asks the assistant to push or to open a pull request, **Then** the assistant does it with the engineer's own git and tools, and Fallow shows the push (branch, remote) in the session.
3. **Given** a branch of the project moved or a remote branch updated by something else (another session, the engineer's own terminal, a loop), **When** Fallow notices, **Then** it shows the change on the project, naming its source when Fallow knows it and "outside Fallow" when it does not.
4. **Given** a merge into the engineer's branch while that branch is checked out in the engineer's folder with uncommitted changes the merge would touch, **When** the assistant tries, **Then** the engineer's uncommitted work is never overwritten: the merge does not happen and the session says why.
5. **Given** a session, **When** the engineer has asked nothing about git, **Then** nothing but the session's own branch has changed.

---

### User Story 5 - Everything a coding assistant needs (Priority: P3)

The engineer works as they do in a terminal assistant: they paste or attach a screenshot, they type the next message while the assistant still works (it waits and is sent when the turn ends), the assistant follows the project's own instruction files, and it can look things up on the web.

**Why this priority**: each is small, but without them the engineer goes back to the terminal for everyday work.

**Independent Test**: attach a screenshot to a message and check the assistant receives it; send a message while it works and check it is sent after the turn, in order; put an instruction in the project's instruction file and check the assistant follows it; ask a question that needs the web and see the request in the activity.

**Acceptance Scenarios**:

1. **Given** a message with a pasted or attached image, **When** it is sent, **Then** the assistant receives the image with the text, and the conversation shows it.
2. **Given** the assistant at work, **When** the engineer sends another message, **Then** it shows as waiting, can be removed before it is sent, and is sent when the turn ends, in the order typed.
3. **Given** a project with instruction files for coding agents (such as `CLAUDE.md` or `AGENTS.md`), **When** a session starts, **Then** Fallow hands their content to the assistant as plain text, whatever runtime it runs on, and the session shows which files it handed.
4. **Given** a session, **When** the assistant requests a web page, **Then** the request shows in the activity like any other tool call; web access is on for the assistant by default and the engineer can turn it off for a session.

---

### User Story 6 - Closing a session (Priority: P3)

When the work is done, the engineer closes the session, with "Close session" in its header or `/close` in the `/` menu. A closed session leaves the dropdown, its working copy is removed, and its branch too when its work is merged; when it holds work that is not merged anywhere, Fallow asks first whether to keep the branch or discard the work. Its conversation stays readable under "Closed sessions…" at the bottom of the dropdown.

**Why this priority**: without it sessions pile up, but losing nothing matters more than tidiness.

**Independent Test**: close a merged session and check its working copy and branch are gone and its conversation readable; close an unmerged one, choose "keep the branch", and check the branch is still there; close another, choose "discard", and check the confirmation named what is lost.

**Acceptance Scenarios**:

1. **Given** a session whose commits are all on another branch of the project (merged), **When** the engineer closes it, **Then** its working copy and branch are removed and it leaves the dropdown.
2. **Given** a session with commits not on any other branch, **When** the engineer closes it, **Then** Fallow asks: keep the branch (the working copy is removed, the branch stays and is named) or discard the work (naming the commits and files that will be lost); nothing is removed before the engineer answers.
3. **Given** a closed session, **When** the engineer opens "Closed sessions…", **Then** its conversation is readable, read-only.
4. **Given** a runtime's own exit command typed in a session, **When** it is sent, **Then** it means nothing special to Fallow; only Fallow's close closes a session.
5. **Given** a session whose assistant is working, **When** the engineer closes it, **Then** Fallow asks to stop the turn first, naming it.

### Edge Cases

- **The engineer's branch moves while a session is open** (another session merged, the engineer committed): the session keeps its own starting point; the session shows that its starting branch moved, and the engineer may ask the assistant to bring its branch up to date.
- **A new working copy has no installed dependencies** (packages, a virtual environment): the assistant installs them when the engineer asks; the session says nothing about it by itself in the first version.
- **A turn changes nothing**: no commit is made for it; undo then goes back to the last turn that changed something.
- **The assistant commits by itself during a turn**: its commits stay; Fallow commits what is left uncommitted at the end of the turn.
- **Undo after a merge or push the engineer asked for**: undo resets only the session's own branch and working copy; what was merged or pushed elsewhere stays, and the session says so before undoing.
- **A command the assistant ran wrote outside its working copy** (a global package, a file elsewhere): Fallow cannot undo or see it; the session says that undo and Changes cover only the working copy.
- **The project has no gates declared**: the Gates panel says so, and the session stays "Not verified by Fallow".
- **The project folder is not a git repository**: the assistant's tab says a session needs one, and offers nothing else.
- **The runtime of the chosen assistant cannot take images**: attaching one is refused before sending, saying so.
- **The assistant cannot start** (runtime missing, not signed in): the session shows why and what to do, keeps its working copy, and the message can be sent again.
- **The session's branch name is already taken**: Fallow picks another and shows it; it never overwrites a branch.
- **The engineer deletes the session's branch or working copy outside Fallow**: the session says so and can only be closed; its conversation stays readable.
- **Many sessions at once**: every session counts as an agent while its assistant works, against the same limits as other agents; a message sent when no slot is free waits and says so.

## Requirements *(mandatory)*

### Functional Requirements

**The page**

- **FR-001**: The project page's middle column MUST have two tabs, "Plan with the lead" (today's project conversation, unchanged) and "Code with the assistant". The left column (Features, Files) and the top bar MUST stay as they are; the top bar's lead picker stays the lead's.
- **FR-002**: "Code with the assistant" MUST be a dropdown: clicking it only opens the list; picking an item opens that session; "+ New session" starts one. The list MUST show each open session with its name, the open one ticked and on top, a mark while its assistant works, and its number of changed files; "Closed sessions…" at the bottom. Sessions MUST NOT appear anywhere else (no row of sessions, nothing in the left column).
- **FR-003**: The rail MUST show the panels of the tab the engineer is on: for the assistant, Changes and Gates of the open session; for the lead, the panels of today.
- **FR-004**: A session MUST be named after its first message until the engineer renames it in its header.

**The assistant**

- **FR-005**: Fallow MUST provide a new agent kind, **Assistant**, and ship a default agent `assistant`. Any runtime that implements the runtime contract, with any model, MUST be usable as an assistant. The engineer MUST be able to pick the assistant when starting a session, with a default per project.
- **FR-006**: The assistant's conversation MUST offer what the lead's conversation offers: the chat, the `/` menu, the working indicator, the activity as it happens, and stopping a turn.
- **FR-007**: At the start of a session, Fallow MUST hand the content of the project's instruction files for coding agents to the assistant as plain text, whatever the runtime, and show which files it handed. It MUST NOT rely on a runtime reading such files by its own convention.
- **FR-008**: The assistant's rights MUST be fixed by Fallow from its kind, whatever its definition says: it edits files only in its session's working copy (and, on the engineer's word, the project's branches, FR-016); the gates, the plan's commands and the project's approved commands run without asking; any other command MUST be asked of the engineer in the chat, per command, unless the engineer allowed other commands "until I switch back" for that session, which they can switch back at any time. Options that make an agent skip its own permission checks MUST NOT be used.
- **FR-009**: Web access MUST be on for the assistant by default, and the engineer MUST be able to turn it off for a session. Every web or remote request the assistant makes MUST show in the activity. No other agent kind gains web access through this feature.
- **FR-010**: A message with a pasted or attached image MUST reach the assistant with the image; when the assistant's runtime cannot take images, Fallow MUST refuse the attachment before sending, saying so.
- **FR-011**: A message sent while the assistant works MUST wait, visible and removable, and be sent when the turn ends, in the order typed.

**The working copy and its branch**

- **FR-012**: Each session MUST have its own working copy and its own branch, made from the engineer's current branch at the moment the session starts. Sessions MUST NOT share a working copy, and MUST NOT change the engineer's project folder or checked-out branch unless the engineer asks the assistant to (FR-016).
- **FR-013**: At the end of every turn that changed files, Fallow MUST commit the changes on the session's branch, one commit per turn, named after the turn; a turn cut short MUST be committed and marked so. A turn that changed nothing MUST NOT make a commit.
- **FR-014**: The engineer MUST be able to undo the last turn: the session's branch and working copy go back to the previous turn's commit. The undone turn MUST stay in the conversation marked as undone, and the assistant MUST be told in its next turn. When the turn merged or pushed elsewhere, the session MUST say before undoing that only the session's branch is reset.
- **FR-015**: A session's branch name MUST NOT overwrite an existing branch; when its name is taken, Fallow picks another and shows it.

**Git by asking, and what Fallow shows**

- **FR-016**: There MUST be no merge or publish button in a session. A merge, commit, push or pull request the assistant makes because the engineer asked for it counts as the engineer's decision (constitution II). A merge MUST NOT overwrite the engineer's uncommitted work in their folder.
- **FR-017**: Fallow MUST show every change it observes to the project's local branches, and to the remote branches of its remotes as git last saw them: the branch, how it moved (new commits, merged, rewound, created, deleted), and its source (this session, another session, a loop, or "outside Fallow"). A change made during a session's turn MUST show in that session; every change MUST show on the project.
- **FR-018**: Fallow itself MUST NOT push, open pull requests or contact a remote for a session; only the assistant does, with the engineer's own git and tools, on the engineer's word.

**Checking the work**

- **FR-019**: The Changes panel MUST list every file changed on the session's branch since the session started, with what changed in each.
- **FR-020**: With the assistant's tab on, the Files tab MUST show the open session's working copy, changed files marked (added, modified), folders with the number of changes inside, and a filter showing only changed files. A file MUST open read-only in the middle, with the conversation beside it, and with "Open in my editor".
- **FR-021**: The engineer MUST be able to run the project's gates on a session's branch at any time; Fallow runs them, compared with the commit the session started from, and shows each gate's outcome and output.
- **FR-022**: A session's result MUST be shown as "Not verified by Fallow", naming what was not checked, until the gates ran on its latest commit; after they passed, as passed on that commit without an independent check, never in the words or colors of a verified loop; after any later change, as "Not verified by Fallow" again.

**Closing**

- **FR-023**: Closing a session MUST be Fallow's own action ("Close session" in the session's header, `/close` in the `/` menu). A runtime's own exit command MUST NOT close it.
- **FR-024**: Closing MUST remove the session's working copy, and its branch when every commit of it is on another branch of the project. When it holds commits on no other branch, Fallow MUST ask first: keep the branch (named) or discard the work (naming the commits and files lost), and remove nothing before the answer. Closing a session whose assistant works MUST first ask to stop the turn.
- **FR-025**: A closed session MUST leave the dropdown and stay readable, read-only, under "Closed sessions…".

**Lasting**

- **FR-026**: Sessions, their conversations, working copies and branches MUST survive Fallow stopping or restarting; a turn at work when Fallow stopped MUST be shown as cut short and never be repeated by itself.
- **FR-027**: Every session counts as an agent while its assistant works, within the same limits as other agents; a message that has to wait for a free slot MUST say so.
- **FR-028**: A session's state MUST be decided from what Fallow observes (its own records, git, the gates), never from how one runtime is known to behave (constitution IX), and the whole behaviour MUST be testable on the fake runtime.

**The lead's tab, for consistency**

- **FR-029**: A file opened from the Files tab with the lead's tab on MUST also show with the conversation beside it, as in the assistant's tab.

### Key Entities

- **Solo session**: a project's conversation with an assistant: its name, the assistant (agent, runtime, model), the branch and commit it started from, its own branch and working copy, its turns, its web access setting, open or closed, and how it was closed (merged, branch kept, discarded).
- **Turn**: one message of the engineer and the assistant's work on it: the activity, the commit it made (if any), whether it was cut short or undone.
- **Waiting message**: a message typed while the assistant works, with its attachments, in order.
- **Observed git change**: a change Fallow saw to a branch of the project or of its remotes: the branch, before and after, the kind of change, its source, and when.
- **Gate run (session)**: the gates run on a session's commit against its starting commit, with each outcome.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new session is ready for its first message within 5 seconds of "+ New session" on a project of up to 10,000 files.
- **SC-002**: In 100% of tested sessions, the engineer's project folder and checked-out branch are unchanged by a session unless the engineer asked the assistant to change them.
- **SC-003**: 100% of turns that changed files end with exactly one commit by Fallow on the session's branch (besides the assistant's own), and an undo restores the session's branch and working copy to exactly the previous turn's commit.
- **SC-004**: Every change to a branch of the project made during the tests, from a session, a loop or outside Fallow, shows on the project within 5 seconds of Fallow noticing it, with its source, and none is missed.
- **SC-005**: With two sessions working at the same time on the same file, each session's branch holds only its own changes in 100% of runs.
- **SC-006**: No session result is ever shown as verified; "Not verified by Fallow" shows on every session result whose latest commit the gates have not run on.
- **SC-007**: Closing never removes a commit that is on no other branch without the engineer's explicit choice, in 100% of tests.
- **SC-008**: After Fallow restarts, 100% of open sessions are back with their conversation, working copy and branch, and no turn is repeated.
- **SC-009**: Fallow's own process stays within the constitution's memory budgets with 4 sessions working at the same time.
- **SC-010**: An engineer used to a terminal coding assistant can fix a small bug from a new session to a merge into their branch without leaving Fallow, in at most one message to start and one to merge.

## Assumptions

- **Tab names**: "Plan with the lead" and "Code with the assistant", as in the approved mockup; they say what each is for.
- **Setup of a new working copy**: in the first version, the assistant installs dependencies when the engineer asks; a per-project setup command that Fallow runs when a session starts comes later.
- **A file beside the lead's conversation too** (FR-029): both tabs behave alike, as the mockup suggested.
- **The top bar's lead picker** stays the lead's: it picks who leads a new feature; the assistant is picked when a session starts.
- **Merged** means every commit of the session's branch is on another branch of the project (local or the remote's as git last saw it); a squash merge on a remote is not recognized as merged, so Fallow asks before discarding.
- **A closed session cannot be reopened** in the first version; its branch, when kept, can be used from a new session or the engineer's own tools.
- **The project's instruction files** are those coding agents commonly read at a project's root (such as `CLAUDE.md`, `AGENTS.md`); the exact list is a plan decision, extendable per project.
- **The fake runtime** gains what a session needs (images, web requests as activity) so the behaviour is tested without a real model.
- **Not in the first version**: switching models within a session, a terminal in Fallow, turning a session into a feature, reopening a closed session, a per-project setup command.

## Dependencies

- 009 (the lead's conversation, the `/` menu, the working indicator, the Files tab, read-only file view): reused.
- 010 (the remote and the engineer's own tools): a pull request the engineer asks the assistant for uses the same tools; Fallow's own publishing of loops is unchanged.
- Constitution 6.3.0/6.3.1: II (the assistant acts on the engineer's word), V (web and remote traffic on by default for the assistant), VI (the assistant's rights, FR-008; to be amended to name the assistant), IX (runtime-agnostic).
