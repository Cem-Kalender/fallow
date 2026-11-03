# Solo sessions: decisions so far (2026-11-02)

Agreed with the engineer before this feature's Spec Kit session, to start `/speckit-specify` from.
Mockup: `mockup/solo.html`. Comes after 010 (the remote and pull requests).

## What it is

A bare coding assistant inside Fallow, for vibe coding without Spec Kit: a chat with an agent
that edits the code and runs commands. Fallow stays an agentic orchestration tool, not an editor:
writing code by hand belongs in the engineer's editor (VS Code).

## Changed on 2026-11-03

The engineer chose to use the assistant for everything, including what the project conversation
with the lead was for:

- **The assistant is the project page's only conversation.** No tabs: the middle column is the
  open session, with the session dropdown at its top. Opening a feature shows its spec session
  there, as today.
- **From a discussion to a feature.** When a feature has been discussed enough, the assistant
  suggests it (a structured part of its answer, defined in the runtime contract; IX), or the
  engineer types `/feature`. The engineer sees the title and a brief of the discussion, can edit
  both and pick the lead, and confirms. Fallow then creates the feature, commits the brief in its
  folder, opens its spec session with the lead, and runs specify on the brief. The engineer goes on
  with the lead as today (clarify, plan, tasks, lock); that lead answers the builders' questions.
  The session stays open.
- **The lead works only behind features.** The project conversation with the lead is no longer
  started; earlier ones stay readable. The lead and the assistant stay separate kinds: their rights
  differ (constitution VI, amended in 6.4.0 to name the assistant).
- **The assistant's commands** follow the lead's rule (VI): approved commands run; others are
  asked per command, or allowed "until I switch back". Destructive git (force, deleting or
  rewriting another branch, discarding uncommitted changes, changing the engineer's folder) is
  asked per command always.
- **Undo** goes back to before any turn (confirmed when more than the last).

The bullets below are the decisions of 2026-11-02; where they speak of two tabs, the change above
applies.

## Decided

- **The project page.** The middle column gets two tabs: **Plan with the lead** (today's project
  conversation, unchanged) and **Code with the assistant**. The left column (Features, Files) and
  the top bar stay as they are. The rail shows the panels of the tab you are on: for the
  assistant, **Changes** and **Gates**.
- **Choosing a session.** "Code with the assistant" is a dropdown: clicking it only opens the
  list (it never switches by itself); picking an item opens that session; "+ New session" starts
  one. The list shows each session (named after its first message, renamed in its header), the
  open one ticked and on top, a dot while its assistant works, its number of changes. No row of
  sessions, no sessions in the left column.
- **A new agent kind, Assistant**, with a shipped default agent `assistant`; not the lead (its
  rights and job differ) and not the builder. It reuses the lead conversation's machinery (chat,
  `/` menu, working indicator, activity, picking an agent, a default per project). Any runtime and
  model (principle IX).
- **Each session has its own worktree and branch**, started from the engineer's branch as it is
  then, so several sessions can be open at once and the engineer's project folder is never
  touched by them. Fallow commits every turn on the session's branch.
- **Git by asking.** No merge button: the engineer asks the assistant to merge into their
  branch, commit, push or open a pull request. That counts as the engineer's decision
  (constitution 6.3.0, II). Fallow watches git itself and shows every change it observes to the
  project's branches and remotes ("master moved: 1 commit merged from this session").
- **Web access on by default** for the assistant (constitution 6.3.0, V); off for every other
  kind unless its definition allows it; each request shows in the activity.
- **In the first version** (plus the handover to a feature, 2026-11-03):
  1. Undo this turn (back to the previous turn's commit on the session's branch);
  2. pasting or attaching a screenshot;
  3. a message typed while the assistant works waits and is sent when the turn ends;
  4. the project's own instruction files (such as `CLAUDE.md`, `AGENTS.md`) handed to the
     assistant by Fallow as plain text, whatever the runtime (IX);
  5. web access (above);
  6. gates on demand, on the session's branch, against where it started; "Not verified by
     Fallow" until they ran, and again after any change (principle I).
- **Files.** In the assistant's tab, the Files tab shows the open session's worktree with its
  changes marked (M, A, counts on folders, "only changed"). A file opens in the middle and the
  conversation moves beside it on the right. Read-only, as in 009; "Open in my editor".
- **Closing a session** is Fallow's own action, `/close` in the `/` menu and "Close session" in
  the session's header (a runtime's own `/exit` means nothing in Fallow, IX). A closed session
  leaves the dropdown; its worktree is removed, and its branch too when its work is merged;
  with unmerged work Fallow asks first (keep the branch, or discard the work). Its conversation
  stays readable under "Closed sessions…" at the bottom of the dropdown.
- **Not in the first version:** switching models within a session, a terminal in Fallow.

## Settled in the spec (2026-11-03)

- The tab names: no tabs any more.
- Setup in a new worktree: the assistant installs dependencies when asked; a per-project setup
  command later.
- A file beside a feature's conversation too, so both behave alike.
- The top bar's "lead ▾" picker stays the lead's, for "+ New feature".

## Formerly open (see the mockup's "To settle")

- The tab names: "Plan with the lead" / "Code with the assistant", or "Lead" / "Assistant".
- Setup in a new worktree (no installed dependencies): the assistant installs them when asked;
  later a per-project setup command.
- A file beside the lead's conversation too, so both tabs behave alike.
- The top bar's "lead ▾" picker stays the lead's.
