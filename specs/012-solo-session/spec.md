# Feature Specification: Solo sessions

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/012-solo-session`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "Solo sessions (012; first numbered 011, renumbered on 2026-11-03 when merge conflicts became 011), asked for by the engineer on 2026-11-02, after 010 (publishing to the remote). A bare coding assistant inside Fallow for vibe coding without Spec Kit. The project page's middle column gets two tabs, 'Plan with the lead' (today's conversation) and 'Code with the assistant', the latter a dropdown of sessions with '+ New session'; a new agent kind Assistant (shipped default `assistant`, any runtime and model); each session has its own worktree and branch from the engineer's branch, Fallow commits every turn; git by asking the assistant (merge, commit, push, pull request) counts as the engineer's decision, Fallow shows every change it observes to the project's branches and remotes; web access on by default for the assistant. First version: undo a turn, screenshots, queued messages while working, the project's instruction files handed as plain text, web access, gates on demand on the session's branch. Files tab shows the session's worktree with changes marked, read-only, conversation beside an open file. Closing is Fallow's own action; a closed session leaves the dropdown, its worktree is removed, its branch too when merged, with unmerged work Fallow asks first; its conversation stays readable under 'Closed sessions…'. Not in the first version: switching models within a session, a terminal, 'turn this into a feature'." Changed on 2026-11-03 by the engineer (this reverses "turn this into a feature" being left out above): the assistant is the project page's only conversation, also for discussing features; when a feature has been discussed enough, a spec session is started with a lead from a brief of the discussion, and specify runs on it; the lead works only behind features. The decisions agreed with the engineer are kept in [decisions.md](decisions.md), the mockup in [mockup/solo.html](mockup/solo.html).

## Context

Today a project in Fallow has one way to get code written: a feature, worked out with the lead in Spec Kit documents, locked, and built by a loop of agents that Fallow verifies. That is right for a feature that deserves a spec. It is too heavy for the rest of an engineer's day: a quick fix, an experiment, a refactor, a question about the code, a dependency to bump. For those the engineer leaves Fallow and opens a coding assistant in a terminal, where Fallow sees nothing.

A **solo session** is that coding assistant inside Fallow: a chat with an agent that reads and edits the project's code and runs commands, in a working copy of its own, while Fallow keeps what it always keeps: every turn committed, every change to the project's branches and remotes shown, the project's gates runnable on the result, and nothing presented as verified that was not.

The assistant is also where features begin. The engineer talks ideas through with it, as with a colleague who knows the code; when something has been discussed enough to become a feature, the assistant suggests it, and on the engineer's word Fallow starts the feature's spec session with a lead, handing it a brief of the discussion and running specify on it. The lead then works only behind features: their spec sessions and the questions of their loops. The project page no longer has a conversation with the lead of its own.

Fallow stays an agentic orchestration tool, not an editor: writing code by hand belongs in the engineer's own editor.

The constitution provides for it (6.3.0, 6.4.0): in a solo session the assistant acts on the engineer's word, and a merge, commit, push or pull request it makes when the engineer asks counts as the engineer's decision (II); its web and git-remote traffic is on by default (V); its rights are fixed by its kind (VI).

**What does not change**: features and their spec sessions with the lead (except that a file opens beside the conversation, FR-029), "+ New feature", loops and their verification, publishing a loop (010), and what any other agent kind may do. **What changes besides**: the project conversation with the lead is no longer started (those already held stay readable, FR-035).

## Clarifications

### Session 2026-11-03

- Q: Which commands may the assistant run without asking? → A: As the lead (constitution VI): the gates and the project's approved commands run; any other command is asked of the engineer in the chat, per command, with "allow until I switch back" for the session. VI is amended to name the assistant.
- Review decisions (2026-11-03, after the independent review): git commands that rewrite or delete a branch other than the session's own, force a push, discard uncommitted changes or change the engineer's folder are asked per command even after "allow until I switch back" (II: destructive actions are confirmed per action); undo can go back to before any turn, as in the mockup; edits the engineer makes in a session's working copy between turns are committed as the engineer's, and named before an undo or a close would drop them.
- Q: Should the lead and the assistant be one agent? → A: No: they stay separate kinds (their rights differ), but the assistant is the only conversation on the project page, also for discussing features. When a feature has been discussed enough, the assistant suggests it (or the engineer types `/feature`); on the engineer's confirmation Fallow starts the feature's spec session with a lead, commits a brief of the discussion in the feature's folder, and runs specify on it. The project conversation with the lead is retired; the lead works only behind features. VI names the assistant (6.4.0, FR-008).
- Q: Should Fallow's per-turn commits on a session's own branch run the project's commit hooks? → A: No: they are Fallow's snapshots for undo, not commits the engineer chose; they are not signed either. A plain merge would carry them to the engineer's branch without the hooks having run on them, so when the engineer asks for a merge, the request says so and the assistant is told to merge as one squashed commit, on which the hooks and signing run as the engineer's git is set up; the gates still check the work.
- Q: What about a project folder that is not a git repository? → A: Git becomes mandatory for every project in Fallow: it is how agents coordinate and how work is handed over. A folder without git is made a repository (`git init` and a first commit of what is there) on the engineer's click before it can be used; plain-folder projects already added get the same question. When git is not installed or too old, Fallow says so on its start page with how to install it, and checks again on request; it never installs git itself.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A coding session with the assistant (Priority: P1)

The engineer opens a project; its middle column is the assistant. They start a new session and type what they want ("the date picker shows the wrong week on Sundays, fix it"). The assistant reads the code, edits files, runs commands, and answers in the chat, showing that it works and what it does, as a conversation with the lead does today. Each turn's result is committed by Fallow on the session's own branch. The engineer's project folder and checked-out branch are untouched while they work.

**Why this priority**: it is the feature; everything else refines it.

**Independent Test**: on a project with a small bug, start a session, ask for the fix in one message, and check that the fix is on the session's branch as a commit for that turn, that the engineer's folder and branch did not change, and that the chat showed the assistant's work.

**Acceptance Scenarios**:

1. **Given** a project, **When** the engineer opens it, **Then** the middle column shows the assistant: the session open last, or a new one ready for a message when there is none; the left column and the top bar stay as they are.
2. **Given** the session dropdown, **When** the engineer picks "+ New session", **Then** a session starts with its own working copy and branch, made from the latest commit of the engineer's current branch, with the project's default assistant (or the one the engineer picks), and is ready for a message; when the engineer's folder has uncommitted changes, the session says they are not in it.
3. **Given** a session, **When** the engineer sends a message, **Then** the assistant works in the session's working copy, its activity (files read and edited, commands, web requests) shows in the chat as it happens, and its answer ends the turn.
4. **Given** a turn that changed files, **When** the turn ends, **Then** Fallow has committed the changes on the session's branch, one commit per turn, named after the turn.
5. **Given** a session at work, **When** the engineer looks at their project folder, **Then** its files and checked-out branch are as they were (until the engineer asks the assistant to change them, User Story 4).
6. **Given** the assistant at work, **When** the engineer stops the turn, **Then** the assistant stops, what it changed so far is committed as the turn's result and marked as cut short, and the engineer can go on.

---

### User Story 2 - Several sessions, picked from one dropdown (Priority: P1)

The engineer keeps more than one session open: a fix in one, an experiment in another. The top of the middle column is a dropdown with the open session's name: clicking it opens the list of the project's open sessions, each named after its first message (renameable in its header), the open one ticked and on top, a dot on those whose assistant is working, and the number of changes in each; "+ New session" starts another. Picking an item opens that session. Sessions work at the same time without touching each other.

**Why this priority**: a session per task is how the engineer works; one shared working copy would mix unrelated changes.

**Independent Test**: start two sessions, give each a task that edits the same file differently, and check that each session's branch holds only its own change, that both show in the dropdown with their state, and that switching between them never changes either.

**Acceptance Scenarios**:

1. **Given** two open sessions, **When** the engineer clicks the session dropdown, **Then** a list opens with both sessions and "+ New session"; clicking it never switches session by itself.
2. **Given** the list open, **When** the engineer picks a session, **Then** that session's conversation, changes and gates show, and the list closes.
3. **Given** one session's assistant working, **When** the engineer opens another session and sends a message, **Then** both work at the same time, each in its own working copy and on its own branch.
4. **Given** a session, **When** the engineer renames it in its header, **Then** the dropdown shows the new name.
5. **Given** sessions open when Fallow stops or restarts, **When** Fallow is back, **Then** every session is in the dropdown with its conversation, working copy and branch; a turn that was at work is shown as cut short, never repeated by itself.

---

### User Story 3 - Seeing and checking what the session changed (Priority: P2)

Beside the conversation, the rail shows the session's **Changes** (the files changed since the session started, with what changed) and **Gates** (the project's gates, runnable on the session's branch). In the left column, the Files tab shows the session's working copy with its changed files marked. Opening a file shows it in the middle, read-only, with the conversation beside it. The engineer can undo back to before any turn. The session's result says "Not verified by Fallow" until the gates ran on it, and again after any later change.

**Why this priority**: a coding assistant whose changes the engineer cannot see, check and undo is one they have to babysit in another tool.

**Independent Test**: in a session, let the assistant change two files over two turns; check the Changes panel and the marked files; run the gates and see their outcome against where the session started; undo the second turn and check the branch is back at the first turn's commit; check the label before and after the gates ran, and after a further change.

**Acceptance Scenarios**:

1. **Given** a session with changes, **When** the engineer looks at the rail, **Then** the Changes panel lists every file changed since the session started (added, modified, deleted) and shows what changed in each.
2. **Given** the Files tab while a session is open, **When** the engineer looks at it, **Then** it shows the open session's working copy, changed files marked, folders with the number of changes in them, and a way to show only changed files; while a feature is open, it shows the project as today.
3. **Given** a file opened from the Files tab or the Changes panel, **When** it shows in the middle, **Then** it is read-only, the conversation stays usable beside it, and "Open in my editor" opens it in the engineer's editor.
4. **Given** the Gates panel, **When** the engineer runs the gates, **Then** Fallow runs the project's gates on the session's branch, compared with the session's base (where it started, its last update from the starting branch, or its last squash into it that Fallow observed), and shows each gate's outcome with its output.
5. **Given** a session whose gates have not run on its latest commit, **When** the engineer looks at it, **Then** it says "Not verified by Fallow" and names what was not checked; after the gates pass it says they passed on that commit, never in the words or colors of a verified loop (no independent checker looked at it); after any later change it says "Not verified by Fallow" again.
6. **Given** a session with turns, **When** the engineer picks "Undo" on a turn, **Then** after confirming (naming the turns undone, when more than the last), the session's branch and working copy are back at the commit before that turn; the undone turns stay visible in the conversation marked as undone, their commits stay recoverable until the session closes, and the assistant is told in its next turn what was undone.
7. **Given** the engineer edited a file in the session's working copy (for example with "Open in my editor"), **When** the next turn starts, or before an undo or a close, **Then** Fallow first commits those edits as the engineer's own on the session's branch, marked so; they are never committed as the assistant's turn, and an undo or a close that would drop them names them first.
8. **Given** the assistant ran the project's tests itself during a turn, **When** the engineer looks at the session, **Then** those runs show in the activity as the assistant's commands, never as gate outcomes.

---

### User Story 4 - Git by asking, every change shown (Priority: P2)

There is no merge button in a session. When the engineer wants the work somewhere, they ask the assistant, as they would in a terminal: "merge this into main", "commit and push", "open a pull request". That is the engineer's decision (constitution II). Fallow watches the project's branches and remotes and shows every change it observes, whoever made it ("main moved: this session's work, squashed into 1 commit"; "pushed fallow/solo/date-picker to origin").

**Why this priority**: it is how the session's work leaves the session; showing every change is what keeps the engineer's word the only authority over their branches.

**Independent Test**: in a session, ask the assistant to merge into the engineer's branch, then to push its branch to a remote; check that each change shows in the session and on the project, naming the branch, the commits and who made it; move a branch outside Fallow and check that it shows too.

**Acceptance Scenarios**:

1. **Given** a session with work, **When** the engineer asks the assistant to merge it into their branch, **Then** the assistant's merge command is asked of the engineer (always, for a command that changes the engineer's folder), the request shows what Fallow finds in the folder (the branch checked out there, uncommitted changes), and after the merge Fallow shows in the session and on the project that the branch moved, by how many commits, from this session.
2. **Given** a project with a remote, **When** the engineer asks the assistant to push or to open a pull request, **Then** the assistant does it with the engineer's own git and tools, and Fallow shows the push (branch, remote) in the session.
3. **Given** a branch of the project moved or a remote branch updated by something else (another session, the engineer's own terminal, a loop), **When** Fallow notices, **Then** it shows the change on the project, naming its source when Fallow knows it and "outside Fallow" when it does not.
4. **Given** a merge into the engineer's branch while the engineer's folder has uncommitted changes, **When** the assistant's merge command is asked of the engineer, **Then** the request names those changes and warns that the merge may touch them; Fallow does not judge the command for the engineer, and afterwards shows the folder's state (merged, conflicted and left half-done, or unchanged) in the session.
5. **Given** a session, **When** the engineer has asked nothing about git, **Then** nothing but the session's own branch has changed.
6. **Given** "allow until I switch back" is on, **When** the assistant runs a command that force-pushes, deletes or rewrites a branch other than the session's own, discards uncommitted changes, or changes the engineer's folder, **Then** it is still asked of the engineer, per command, naming what it affects.

---

### User Story 5 - Everything a coding assistant needs (Priority: P3)

The engineer works as they do in a terminal assistant: they paste or attach a screenshot, they type the next message while the assistant still works (it waits and is sent when the turn ends), the assistant follows the project's own instruction files, and it can look things up on the web.

**Why this priority**: each is small, but without them the engineer goes back to the terminal for everyday work.

**Independent Test**: attach a screenshot to a message and check the assistant receives it; send a message while it works and check it is sent after the turn, in order; put an instruction in the project's instruction file and check the assistant follows it; ask a question that needs the web and see the request in the activity.

**Acceptance Scenarios**:

1. **Given** a message with a pasted or attached image, **When** it is sent, **Then** the assistant receives the image with the text, and the conversation shows it.
2. **Given** the assistant at work, **When** the engineer sends another message, **Then** it shows as waiting, can be edited or removed before it is sent, and is sent when the turn ends, in the order typed.
3. **Given** a project with instruction files for coding agents (such as `CLAUDE.md` or `AGENTS.md`), **When** a session starts, **Then** Fallow hands their content to the assistant as plain text, whatever runtime it runs on, and the session shows which files it handed.
4. **Given** a session, **When** the assistant requests a web page, **Then** the request shows in the activity like any other tool call; requests made by commands it runs (a package install, a push) show only as those commands. Web access is on for the assistant by default; the engineer can turn it off for a session when the assistant's runtime supports that, and otherwise the session says it cannot.

---

### User Story 6 - Closing a session (Priority: P3)

When the work is done, the engineer closes the session, with "Close session" in its header or `/close` in the `/` menu. A closed session leaves the dropdown, its working copy is removed, and its branch too when its work is merged; when it holds work that is not merged (see "Merged" in Assumptions), or uncommitted changes, Fallow asks first whether to keep the work or discard it. Its conversation stays readable under "Closed sessions…" at the bottom of the dropdown.

**Why this priority**: without it sessions pile up, but losing nothing matters more than tidiness.

**Independent Test**: close a merged session and check its working copy and branch are gone and its conversation readable; close an unmerged one, choose "keep the work", and check the branch is still there; close another, choose "discard", and check the confirmation named what is lost.

**Acceptance Scenarios**:

1. **Given** a session whose work is merged (its commits on another branch, or squashed into one as Fallow observed), **When** the engineer closes it, **Then** its working copy and branch are removed and it leaves the dropdown.
2. **Given** a session with commits not on any other branch, or uncommitted changes, **When** the engineer closes it, **Then** Fallow asks: keep the work (what is uncommitted is committed first; the branch stays and is named, and the working copy is removed, or kept and named when that commit fails) or discard the work (naming the commits and files that will be lost); nothing is removed before the engineer answers.
3. **Given** a closed session, **When** the engineer opens "Closed sessions…", **Then** its conversation is readable, read-only, and the engineer can delete its record (conversation and attachments), after confirming.
4. **Given** a runtime's own exit command typed in a session, **When** it is sent, **Then** it means nothing special to Fallow; only Fallow's close closes a session.
5. **Given** a session whose assistant is working, **When** the engineer closes it, **Then** Fallow asks to stop the turn first, naming it.

---

### User Story 7 - From a discussion to a feature (Priority: P2)

The engineer talks a feature through with the assistant ("users should be able to export their data; what would that take here?"). When it has been discussed enough, the assistant suggests making it a feature, with a title and a short brief of what was settled; the engineer can also type `/feature` at any time. The engineer reads the brief, edits it if they like, picks the lead (the project's default is preset) and confirms. Fallow starts the feature, commits the brief in its folder, opens its spec session with that lead, and runs specify with the brief as the description. The lead comes back with the spec and its open questions; from there it goes on as today (clarify, plan, tasks, lock), and that lead answers the builders' questions in the feature's loop. The session stays open.

**Why this priority**: it makes the assistant the one place to start, for a fix or a feature, and gives the lead a concrete start instead of a blank page.

**Independent Test**: on the fake runtime, discuss a feature in a session until the assistant suggests it; confirm with an edited brief; check that the feature exists, its brief is committed in its folder as edited, its spec session runs specify with the brief, the session stays open and shows a link to the feature, and nothing was started before the confirmation.

**Acceptance Scenarios**:

1. **Given** a session in which the assistant suggests a feature, **When** its turn ends, **Then** the session shows "Make this a feature" with the suggested title and brief; nothing is started until the engineer confirms.
2. **Given** `/feature` typed in a session, **When** it is sent, **Then** the assistant is asked for a title and brief from the discussion so far, and the same confirmation shows.
3. **Given** the confirmation, **When** the engineer edits the title or the brief, picks a lead and confirms, **Then** Fallow creates the feature, commits the brief as edited in the feature's folder, opens its spec session with the chosen lead, and runs specify with the brief as the description.
4. **Given** a feature started from a session, **When** the engineer looks at the session, **Then** it shows a link to the feature, and the feature's spec session shows the session it came from.
5. **Given** the confirmation, **When** the engineer cancels, **Then** nothing is created and the discussion goes on.
6. **Given** a feature started from a session, **When** its loop runs, **Then** the questions of its builders go to the lead of its spec session, as for any feature.

---

### User Story 8 - Every project is a git repository (Priority: P1)

Fallow as a whole depends on git: it is how its agents coordinate and how work is handed over. The lead writes a feature's spec on its own branch; builders and checkers work in their own working copies; Fallow verifies their commits, runs the gates against a baseline commit, merges or publishes the verified commit; and solo sessions commit every turn. So every project in Fallow is a git repository. A folder without git is made one on the engineer's click when it is added; when git itself is not installed, Fallow says so plainly and how to install it, instead of quietly treating every project as a plain folder.

**Why this priority**: every agent of Fallow depends on it, not only solo sessions; and today a missing git goes unnoticed, every project quietly looking like a plain folder.

**Independent Test**: add a folder without git and check that nothing opens until "Make it a git repository" is clicked, and that a first commit then holds what was there; start Fallow with git unavailable and check the start page names the problem and how to install git, and that "Check again" finds it once installed.

**Acceptance Scenarios**:

1. **Given** a folder without git, **When** the engineer adds it, **Then** Fallow offers "Make it a git repository", naming what it will do (create the repository and a first commit of what is there), and the project opens only after the engineer clicks it.
2. **Given** a plain-folder project added before this feature, **When** the engineer opens it, **Then** the same question shows, and after it the project's features and their spec documents are as they were, in the first commit.
3. **Given** git is not installed, **When** Fallow starts, **Then** its start page says that Fallow needs git, names the minimum version and how to install it on that system, and offers "Check again"; no project opens until git is found.
4. **Given** git older than the version Fallow needs, **When** Fallow starts, **Then** it says which version it found and which it needs.
5. **Given** a folder inside a bigger git repository, **When** the engineer adds it, **Then** Fallow offers the repository's top folder instead and never makes a repository inside another.

### Edge Cases

- **The engineer's branch moves while a session is open** (another session merged, the engineer committed): the session keeps its own starting point; the session shows that its starting branch moved, and the engineer may ask the assistant to bring its branch up to date. Bringing it up to date moves the session's base to that update, so Changes and the gates' baseline count from there and the engineer's own commits never show as the session's changes; a plain merge of the session into the starting branch does not move the base, and undoing the turn that brought it up to date moves the base back to where it was before that turn. After a squash of the session into its starting branch that Fallow observed (see "Merged" in Assumptions), the base moves to that squash commit, so Changes and the gates count only what came after; bringing the branch up to date later finds the squashed changes on both sides; where later turns changed the same lines again, it may conflict, which the assistant resolves when asked.
- **The engineer's folder is on no branch** (a detached commit): the session starts from that commit and says so.
- **A merge leaves conflicts in the engineer's folder**: Fallow shows the folder as conflicted in the session and on the project; resolving it is the engineer's, or the assistant's when asked.
- **A merge into a branch that is not checked out in the engineer's folder**: Fallow never changes what the engineer has checked out by itself; the assistant's command to do so is asked of the engineer like any command that changes the folder.
- **Files the project's ignore rules exclude** (dependencies, build output, local secrets): Fallow's commits never include them.
- **Fallow's commit at the end of a turn fails** (a full disk, a broken repository; hooks and signing do not run on these commits): the turn shows the failure and its output; the changes stay in the working copy, still counted as that turn's. Fallow tries the commit again before the next turn starts; until it succeeds, the next turn and undo wait, and the session says why and offers two ways on: "Fix it in the next turn" (the next turn starts with the failure handed to the assistant, and its commit holds both turns' changes, marked so) or "Undo these changes" (after confirming, naming the files).
- **The engineer edits a file in the working copy while the assistant works**: Fallow cannot tell those edits from the assistant's; they count as the turn's, and the session says so when the engineer opens a file in their editor during a turn.
- **A command's effect hidden inside a script** (a build script that pushes, an alias): Fallow judges a command as it is written; it cannot see what a script does inside, says so where it asks, and shows afterwards every change to the project's branches and the engineer's folder that it observes (FR-017).
- **A new working copy has no installed dependencies** (packages, a virtual environment): the assistant installs them when the engineer asks; the session says nothing about it by itself in the first version.
- **A turn changes nothing**: no commit is made for it; undoing it removes only its effect on the conversation (it is marked undone).
- **The assistant commits by itself during a turn**: its commits stay; Fallow commits what is left uncommitted at the end of the turn.
- **The engineer asks for a plain merge instead of a squash**: it is the engineer's word; the request names Fallow's snapshot commits that skipped the project's hooks, and the merge then carries them to the engineer's branch as they are.
- **Undo after a merge or push the engineer asked for**: undo resets only the session's own branch and working copy; what was merged or pushed elsewhere stays, and the session says so before undoing.
- **A command the assistant ran wrote outside its working copy** (a global package, a file elsewhere): Fallow cannot undo or see it; the session says that undo and Changes cover only the working copy.
- **The project has no gates declared**: the Gates panel says so, and the session stays "Not verified by Fallow".
- **The project folder is not a git repository**: the project cannot be used until the engineer makes it one (FR-039); nothing in it is touched before that click.
- **The first commit would take in dependencies, build output or likely secrets** (folders such as `node_modules` or a virtual environment, `.env` files): the confirmation names them, with the number and size of the files to be committed, and proposes an ignore file excluding them, which the engineer can edit or decline before the first commit.
- **Git has no identity configured** (no user name or e-mail): the first commit cannot be made; Fallow says so and how to set it, and does not set it itself.
- **A repository without any commit yet**: the same question offers its first commit (FR-039); until then the project is not usable.
- **The folder sits inside a bigger git repository** (a subfolder of one): Fallow says so and offers the repository's top folder as the project instead; it never makes a repository inside another.
- **Git is missing or too old**: Fallow's start page says so, names the minimum version and how to install git on that system, and offers "Check again"; no project opens until git is there (FR-040).
- **The discussion refers to code that exists only on the session's branch**: the feature starts from the engineer's current branch, as any feature; the brief says what the session changed, and the engineer merges the session first when the feature needs it.
- **Spec Kit is not installed in the project** when a feature is made from a session: the feature is created and the brief kept in its folder, and its spec session says specify could not start, as "+ New feature" does today.
- **The assistant suggests a feature again for the same discussion**: the session shows the feature already started from it, and the engineer decides whether to start another.
- **The brief is empty or the assistant gives none**: the confirmation asks the engineer to write one; a feature is not started without a title and a brief.
- **The runtime of the chosen assistant cannot take images**: attaching one is refused before sending, saying so.
- **The assistant cannot start** (runtime missing, not signed in): the session shows why and what to do, keeps its working copy, and the message can be sent again.
- **The session's branch name is already taken**: Fallow picks another and shows it; it never overwrites a branch.
- **The engineer deletes the session's branch or working copy outside Fallow**: the session says so and can only be closed; its conversation stays readable.
- **Many sessions at once**: every session counts as an agent while its assistant works, against the same limits as other agents; a message sent when no slot is free waits and says so.

## Requirements *(mandatory)*

### Functional Requirements

**The page**

- **FR-001**: The project page's middle column MUST be the assistant: the session open last, or a new one when there is none. Opening a feature from the left column shows its spec session there, as today; an earlier project conversation with the lead opens there read-only (FR-035). The left column (Features, Files) and the top bar MUST stay as they are; the top bar's lead picker stays the lead's and applies to "+ New feature".
- **FR-002**: The top of the middle column MUST be a dropdown with the open session's name: clicking it only opens the list; picking an item opens that session; "+ New session" starts one. The list MUST show each open session with its name, the open one ticked and on top, a mark while its assistant works, and its number of changed files; "Closed sessions…" and "Earlier conversations with the lead…" at the bottom. Sessions MUST NOT appear anywhere else (no row of sessions, nothing in the left column).
- **FR-003**: The rail MUST show the panels of what the middle column shows: for a session, its Changes and Gates; for a feature's spec session, the panels of today; for an earlier project conversation, none (its unstarted proposals show in it, FR-037).
- **FR-004**: A session MUST be named after its first message until the engineer renames it in its header. Its header MUST show its branch, the branch and commit it started from, whether web access is on, "Open in my editor" (the session's working copy) and "Close session".

**The assistant**

- **FR-005**: Fallow MUST provide a new agent kind, **Assistant**, and ship a default agent `assistant`. Any runtime that implements the runtime contract, with any model, MUST be usable as an assistant. The engineer MUST be able to pick the assistant when starting a session, with a default per project.
- **FR-006**: The assistant's conversation MUST offer what a conversation with the lead offers: the chat, the `/` menu, the working indicator, the activity as it happens, and stopping a turn.
- **FR-007**: At the start of a session, Fallow MUST hand the content of the project's instruction files for coding agents, as they are in the session's working copy, to the assistant as plain text, whatever the runtime, and show which files it handed; when one of them changes in the working copy during the session, Fallow MUST hand it again at the next turn. It MUST NOT rely on a runtime reading such files by its own convention.
- **FR-008**: The assistant's rights MUST be fixed by Fallow from its kind, whatever its definition says: it edits files only in its session's working copy (and, on the engineer's word, the project's branches and the engineer's folder, FR-016); the gates and the project's approved commands run without asking; any other command MUST be asked of the engineer in the chat, per command, unless the engineer allowed other commands "until I switch back" for that session, which they can switch back at any time. Even then, a command that force-pushes, deletes or rewrites a branch other than the session's own, discards uncommitted changes, or changes the engineer's folder MUST be asked per command, naming what it affects (constitution II). Fallow judges a command as it is written; a command whose working directory or target it cannot tell is asked per command. What a script does inside is not seen; each request MUST say so, and so MUST the session when the engineer turns "until I switch back" on. "Until I switch back" MUST NOT outlive a restart of Fallow. Options that make an agent skip its own permission checks MUST NOT be used.
- **FR-009**: Web access MUST be on for the assistant by default. The engineer MUST be able to turn it off for a session when the assistant's runtime supports that through the runtime contract; otherwise the session MUST say it cannot. Every web request the assistant makes as a tool call MUST show in the activity; requests made by the commands it runs show only as those commands, and the session MUST NOT imply more. No other agent kind gains web access through this feature.
- **FR-010**: A message with a pasted or attached image MUST reach the assistant with the image; when the assistant's runtime cannot take images, Fallow MUST refuse the attachment before sending, saying so.
- **FR-010a**: Taking images and switching web access off MUST be declared by each runtime through the runtime contract, as capabilities every runtime states; Fallow's behaviour without them is the one FR-009 and FR-010 specify, the same for every runtime (constitution IX).
- **FR-011**: A message sent while the assistant works MUST wait, visible, editable and removable, and be sent when the turn ends, in the order typed.

**The working copy and its branch**

- **FR-012**: Each session MUST have its own working copy and its own branch, made from the latest commit of the engineer's current branch (or the commit checked out, when on no branch) at the moment the session starts; uncommitted changes in the engineer's folder are not in it, and the session MUST say so when there are any. Sessions MUST NOT share a working copy, and MUST NOT change the engineer's project folder or checked-out branch unless the engineer asks the assistant to (FR-016).
- **FR-013**: At the end of every turn that changed files, Fallow MUST commit the changes on the session's branch, one commit per turn, named after the turn; a turn cut short MUST be committed and marked so. A turn that changed nothing MUST NOT make a commit. The commit MUST follow the project's ignore rules. Every commit Fallow makes on a session's branch (a turn's, or the engineer's between-turn edits) MUST run none of the project's hooks, from any source, and MUST NOT be signed; these commits are snapshots for undo, not the engineer's commits. When it fails, the turn MUST show the failure, the changes MUST stay in the working copy as that turn's, and Fallow MUST try again before the next turn starts; the next turn and undo MUST wait until it succeeds, unless the engineer chooses to fix it in the next turn (one commit for both turns, marked so) or to undo the uncommitted changes after confirming. Changes made in the working copy between turns (by the engineer) MUST be committed before the next turn, an undo or a close, as the engineer's own, marked so. When that commit fails, the next turn, the undo or the close MUST wait and say why. Any commit that holds the engineer's between-turn edits (including a turn's commit retried after a failure) MUST name them as the engineer's. An undo the engineer confirms after such a failure drops those edits, named in its confirmation.
- **FR-014**: The engineer MUST be able to undo back to before any turn: the session's branch and working copy go back to the commit before that turn. Undoing more than the last turn, or undoing past the engineer's own edits, MUST be confirmed, naming the turns undone and the engineer's edits among them. Turns that share one commit (a turn whose commit failed, fixed in the next) are undone together, and the confirmation MUST name both. Undone turns MUST stay in the conversation marked as undone, their commits MUST stay recoverable until the session closes, and the assistant MUST be told in its next turn what was undone. When the turn merged or pushed elsewhere, the session MUST say before undoing that only the session's branch is reset. Undoing the turn that squashed the session into its starting branch, or a turn before it, moves the base back to where it was before that squash; the squashed work stays on the starting branch, and the undone turns no longer count as the session's merged work.
- **FR-015**: A session's branch name MUST NOT overwrite an existing branch; when its name is taken, Fallow picks another and shows it.

**Git by asking, and what Fallow shows**

- **FR-016**: There MUST be no merge or publish button in a session. Fallow MUST tell the assistant, in plain text, to bring a session's work into another branch as one squashed commit, so that the project's hooks and signing run on it; the request for a merge command MUST say how many of Fallow's snapshot commits on the session's branch did not go through the project's hooks (whether the assistant's own commits did, Fallow does not know, and says so) and whether the command squashes them, and Fallow MUST NOT claim more than the command shows. A squash is one request when written as one command line (squash and commit together). A merge, commit, push or pull request the assistant makes because the engineer asked for it counts as the engineer's decision (constitution II). Before a command that changes the engineer's folder, its request MUST name the folder's uncommitted changes; afterwards Fallow MUST show the folder's state (changed, conflicted and left half-done, staged but not committed when the squash's commit was refused by a hook, or unchanged).
- **FR-017**: Fallow MUST show every change it observes to the project's local branches, and to the remote branches of its remotes as git last saw them: the branch, how it moved (new commits, merged, rewound, created, deleted), and its source. A session or a loop MUST be named as the source only when Fallow's own records show it made the change (the session's activity holds the command, or the loop's own step); otherwise the source MUST be "outside Fallow". A change a session made MUST show in that session; every change MUST show on the project. Fallow observes states, not every step between them: several changes between two observations MAY show as one.
- **FR-018**: Fallow itself MUST NOT push, open pull requests or contact a remote for a session; only the assistant does, with the engineer's own git and tools, on the engineer's word.

**Checking the work**

- **FR-019**: The Changes panel MUST list every file changed on the session's branch since the session's base (where it started, its last update from the starting branch, or its last squash into it that Fallow observed), with what changed in each and the turn that changed it.
- **FR-020**: While a session is open, the Files tab MUST show the open session's working copy, changed files marked (added, modified), folders with the number of changes inside, and a filter showing only changed files. A file MUST open read-only in the middle, with the conversation beside it, with "Open in my editor", and for a changed file a choice between its changes and the whole file, saying which turns changed it and by whom (the assistant or the engineer).
- **FR-021**: The engineer MUST be able to run the project's gates on a session's branch at any time; Fallow runs them, compared with the session's base (where it started, its last update from the starting branch, or its last squash into it that Fallow observed), and shows each gate's outcome and output. The assistant's own runs of tests or other commands MUST NOT be shown as gate outcomes.
- **FR-022**: A session's result MUST be shown as "Not verified by Fallow", naming what was not checked, until the gates ran on its latest commit; after they passed, as passed on that commit without an independent check, never in the words or colors of a verified loop; after any later change, as "Not verified by Fallow" again.

**Closing**

- **FR-023**: Closing a session MUST be Fallow's own action ("Close session" in the session's header, `/close` in the `/` menu). A runtime's own exit command MUST NOT close it.
- **FR-024**: Closing MUST remove the session's working copy, and its branch when its work is merged (see "Merged" in Assumptions). When it holds work that is not merged, or changes in its working copy that are not committed, Fallow MUST ask first: keep the work or discard it (naming the commits and files lost), and remove nothing before the answer. Keeping first commits what is uncommitted and then keeps the branch (named); when that commit fails, the working copy is kept too (named), and the session stays listed under "Closed sessions…" with its working copy until the engineer removes it there ("Remove working copy", confirmed, naming the uncommitted files lost). Closing a session whose assistant works MUST first ask to stop the turn.
- **FR-025**: A closed session MUST leave the dropdown and stay readable, read-only, under "Closed sessions…", until the engineer deletes its record (conversation and attachments), after confirming; Fallow MUST show how much its sessions' records hold.

**Lasting**

- **FR-026**: Sessions, their conversations, waiting messages with their attachments, working copies and branches MUST survive Fallow stopping or restarting. A turn at work when Fallow stopped MUST be shown as cut short, its changes committed as such, its assistant stopped if still running, and never be repeated by itself; a command waiting for the engineer's approval is cancelled and shown as such; waiting messages stay waiting until the engineer sends or removes them.
- **FR-027**: Every session counts as an agent while its assistant works, against the same setting for how many agents may work at once that loops use; a message that has to wait for a free slot MUST say so.
- **FR-028**: A session's state MUST be decided from what Fallow observes (its own records, git, the gates), never from how one runtime is known to behave (constitution IX), and the whole behaviour MUST be testable on the fake runtime.

**A feature's spec session, for consistency**

- **FR-029**: A file opened from the Files tab while a feature's spec session is open MUST also show with the conversation beside it, as in a session.

**From a discussion to a feature**

- **FR-030**: The assistant MUST be able to suggest that the discussion become a feature, with a title and a brief, through a structured part of its answer that the runtime contract defines for every runtime; Fallow tells every assistant, in plain text, when to suggest it. Fallow MUST NOT infer a suggestion from the answer's free text (constitution IX).
- **FR-031**: `/feature` in a session MUST ask the assistant for a title and a brief from the discussion so far, and show the same confirmation.
- **FR-032**: Nothing MUST be started before the engineer confirms. The confirmation MUST open in the middle of the screen, over the page, and MUST show the title and the brief, both editable, and the lead, preset to the project's default and changeable; a feature MUST NOT be started without a title and a brief.
- **FR-033**: A title already used by a feature of the project MUST be refused in the confirmation, naming that feature, and the engineer can change it (as 009 refuses a second feature of the same name). On confirmation, Fallow MUST create the feature as "+ New feature" does (from the engineer's current branch, refused on no branch, saying why), commit the brief as confirmed in the feature's folder, open its spec session with the chosen lead, and run specify there with the brief as the description. If specify cannot start, the feature stays created and its spec session says why.
- **FR-034**: The session MUST stay open after the handover and show a link to each feature started from it; the feature's spec session MUST show the session it came from. The assistant does not write the brief into the feature or create the feature: Fallow does both, on the engineer's confirmation.
- **FR-035**: Fallow MUST NOT start new project conversations with the lead. Those already held MUST stay readable, read-only, under "Earlier conversations with the lead…" at the bottom of the session dropdown. `/specify <title>` in a project conversation is replaced by `/feature` in a session; "+ New feature" stays, and MUST NOT start a project conversation any more.
- **FR-036**: What 009 sent to the project conversation MUST go to the assistant: "Ask about it" from a file (on the project page or a loop page) MUST put the reference to the file and its version in the message box of the session open last, or of a new one when there is none; the pipeline designer's "Ask the lead" becomes "Ask the assistant", and a pipeline the assistant proposes MUST show with "Open in the designer", through a structured part of its answer as for a feature (FR-030).
- **FR-037**: The project conversation's rail panels go with it: "What the lead knows" stays in features' spec sessions; "Proposed features" is replaced by the assistant's suggestions (FR-030). Features proposed in an earlier project conversation and never started MUST stay listed in that conversation, read-only, each with "Make this a feature", which opens the confirmation (FR-032) with the proposal as its brief; a proposal already started shows "Started" with a link to its feature, as in 009.
- **FR-038**: Earlier project conversations MUST open read-only in the middle column from "Earlier conversations with the lead…", with their unstarted proposals (FR-037).

**Every project is a git repository**

- **FR-039**: Every project in Fallow MUST be a git repository. Adding a folder that is not one, or a repository without any commit, MUST offer "Make it a git repository" (`git init` where needed and a first commit of what is there, by the engineer's click); the confirmation MUST name what will be committed (the number and size of the files) and any dependency folders, build output or likely secrets among them, with a proposed ignore file the engineer can edit or decline. Until then the project MUST NOT be opened for anything else, and nothing in the folder is changed. A plain-folder project added before this feature MUST get the same question when opened; its features' spec documents are in the first commit. A folder inside a bigger repository MUST NOT become a repository of its own: Fallow offers the repository's top folder instead; for a project already added at such a folder, its features MUST keep their spec documents where they are, Fallow recording their folders against the new project folder.
- **FR-040**: Fallow MUST check that git is installed, in at least the version it needs, when it starts and when a project is added, and MUST NOT treat a missing git as a folder without a repository. When git is missing or too old, Fallow's start page MUST say so, name the minimum version, show how to install git on that system, and offer "Check again"; no project opens until git is there. Fallow MUST NOT install git itself.
- **FR-041**: Fallow's handling of plain-folder projects MUST be retired: the plain-folder kind of project and what the page shows for it, the instructions to the lead for a plain folder, the exceptions for plain folders and for repositories without a commit in spec sessions (working in the project folder, undo) and in loops.

### Key Entities

- **Solo session**: a project's conversation with an assistant: its name, the assistant (agent, runtime, model), the branch and commit it started from, its base (where it started, its last update from the starting branch, or its last observed squash into it), its own branch and working copy, its turns, its web access setting, open or closed, and how it was closed (merged, branch kept, branch and working copy kept, discarded).
- **Turn**: one message of the engineer and the assistant's work on it: the activity, the commit it made (if any), whether it was cut short or undone.
- **Waiting message**: a message typed while the assistant works, with its attachments, in order.
- **Observed git change**: a change Fallow saw to a branch of the project or of its remotes: the branch, before and after, the kind of change, its source, and when.
- **Gate run (session)**: the gates run on a session's commit against the session's base, with each outcome.
- **Feature brief**: the title and the text of what a session's discussion (or an earlier conversation's proposal, FR-037) settled about a feature, as the engineer confirmed it; committed in the feature's folder, the description specify runs on, linking the feature to the session or conversation it came from.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new session is ready for its first message within 5 seconds of "+ New session" on a project of up to 10,000 files.
- **SC-002**: In 100% of tested sessions, the engineer's project folder and checked-out branch are unchanged by a session unless the engineer asked the assistant to change them.
- **SC-003**: 100% of turns that changed files end with exactly one commit by Fallow on the session's branch (besides the assistant's own), or with the failure of that commit shown and retried before the next turn (or, by the engineer's choice, folded into the next turn's commit, marked so), and an undo restores the session's branch and working copy to exactly the previous turn's commit.
- **SC-004**: Every change to a branch of the project made during the tests, from a session, a loop or outside Fallow, shows on the project within 5 seconds of the change being made, with the right source (never a session or loop that did not make it); changes made in quick succession may show as one.
- **SC-005**: With two sessions working at the same time on the same file, each session's branch holds only its own changes in 100% of runs.
- **SC-006**: No session result is ever shown as verified; "Not verified by Fallow" shows on every session result whose latest commit the gates have not run on.
- **SC-007**: Closing never removes work that is not merged (neither on another branch nor in a squash Fallow observed) without the engineer's explicit choice, in 100% of tests.
- **SC-008**: After Fallow restarts, 100% of open sessions are back with their conversation, working copy and branch, and no turn is repeated.
- **SC-009**: With 4 sessions working at the same time and no other agent, Fallow's own process stays within the constitution's memory budget for 4 concurrent agent steps (200 MB), and an assistant's activity appears on the page within 2 seconds (p95); both measured by benchmarks with scripted sessions that run in CI.
- **SC-010**: On the fake runtime, a small fix goes from "+ New session" to merged into the engineer's branch with two messages (the fix, the merge) and one approval (the squash and its commit, as one command line), and nothing else asked of the engineer; closing the session afterwards asks nothing, its work counting as merged.
- **SC-011**: On the fake runtime, on a project whose constitution is set, a feature goes from a session's suggestion to its spec session running specify on the brief with one confirmation by the engineer and nothing else asked (with the constitution still the template, its step comes first, as for "+ New feature"); the brief committed in the feature's folder is exactly the confirmed text in 100% of runs, and nothing is started in 100% of cancelled confirmations.
- **SC-012**: With git missing or too old, 100% of starts show the start page's message naming the problem, and no project is ever shown as a plain folder because of it; with a folder without git, nothing in it changes before the engineer's click.

## Assumptions

- **Setup of a new working copy**: in the first version, the assistant installs dependencies when the engineer asks; a per-project setup command that Fallow runs when a session starts comes later.
- **A file beside a feature's conversation too** (FR-029): sessions and spec sessions behave alike.
- **The brief's place**: a file in the feature's folder beside its spec, committed by Fallow (constitution VI: Fallow writes the handovers of a project's features); its exact name is a plan decision.
- **The top bar's lead picker** stays the lead's: it picks who leads a feature started with "+ New feature"; a feature started from a session takes the lead picked in its confirmation; the assistant is picked when a session starts.
- **Merged** means every commit of the session's branch is on another branch of the project (local or the remote's as git last saw it), or the session's work up to a turn was squashed into a commit Fallow observed: the session's activity holds the command, and that commit's changes equal the session's changes since its base, up to that turn. Work after that turn counts as not merged. A squash merge made on a remote (a pull request) is not recognized, so Fallow asks before discarding.
- **Undone commits** of the assistant's turns are dropped when the session closes, without asking again: undoing them was the engineer's choice. Undone commits holding the engineer's own edits count as unmerged work at close: Fallow asks first (FR-024).
- **A closed session cannot be reopened** in the first version; its branch, when kept, can be used from a new session or the engineer's own tools.
- **The project's instruction files** are those coding agents commonly read at a project's root (such as `CLAUDE.md`, `AGENTS.md`); the exact list is a plan decision, extendable per project.
- **The fake runtime** gains what a session needs (images, web requests as activity) so the behaviour is tested without a real model.
- **Not in the first version**: switching models within a session, a terminal in Fallow, reopening a closed session, a per-project setup command, continuing an earlier project conversation with the lead.

## Dependencies

- 009 (conversations with the lead, the `/` menu, the working indicator, the Files tab, read-only file view): reused; its project conversation is retired (FR-035, "+ New feature" no longer starts one), its `/specify` from the project conversation becomes `/feature` from a session, its "Ask the lead about it" and the pipeline designer's "Ask the lead" go to the assistant (FR-036), its rail panels are settled by FR-037, and its rule against two features of the same name holds for features made from a session (FR-033).
- 007 and 009 (plain-folder projects, the "git init" offered at the engineer's request): plain folders are retired; the existing offer to make a folder a git repository becomes mandatory (FR-039 to FR-041).
- 010 (the remote and the engineer's own tools): a pull request the engineer asks the assistant for uses the same tools; Fallow's own publishing of loops is unchanged.
- Constitution 6.5.0: III (every project is a git repository) and VI (making a folder one, on the engineer's click, FR-039).
- Constitution 6.4.0: II (the assistant acts on the engineer's word), III (the brief committed as the feature's starting point), V (web and remote traffic on by default for the assistant), VI (the assistant's rights, FR-008, amended in 6.4.0), IX (runtime-agnostic; the feature suggestion in the runtime contract).
