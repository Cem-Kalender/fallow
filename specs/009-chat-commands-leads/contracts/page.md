# Contract: the page (009)

## The `/` menu (US1)

- **Opens** when the message box's text starts with `/` and has no space yet. It closes on Escape, on a space, when the text no longer starts with `/`, or when the box loses focus.
- **Markup**: the box is a combobox (`role="combobox"`, `aria-expanded`, `aria-controls` pointing at the listbox, `aria-activedescendant` on the highlighted option).
  - The listbox has four labelled groups: "Spec Kit steps", "Fallow actions", "Agents", "Project commands". Empty groups are not shown.
  - Each option shows its name, its description, and, for project commands, its source. An unavailable option shows its reason, has `aria-disabled="true"` and comes last in its group.
  - A `role="status"` line says "<n> commands", or "No command matches".
- **Keys**:

  | Key | Does |
  |---|---|
  | ↑ / ↓ | Move the highlight; it wraps, and skips unavailable options |
  | Tab | Complete the highlighted name (`/<name> `) |
  | Enter | Pick. Spec Kit steps and project commands become `/<name> ` in the box; Fallow actions and agents act at once. With no match, the text is sent as it is |
  | Escape | Close; the text stays |
  | Click | Pick |
- **Fallow actions**:

  | Action | Where | Does | Unavailable when |
  |---|---|---|---|
  | New feature | project conversation | focuses the new feature's name field | — |
  | Lock the spec | spec session | `lock-step` (008) | not ready (its reason), or in the project conversation ("Open a feature's spec session to lock it") |
  | Cancel the answer | both | `cancel-turn` (008) | the lead is not answering |
  | Expand / Narrow the chat | both | `chat-expand` | — |
  | Open spec.md / plan.md / tasks.md | spec session | the document's tab | the document does not exist yet |
  | Go to the loop | spec session | `#/p/<p>/f/<f>/loop` | the feature has no loop |
  | Files | both | the Files tab or drawer (US5) | — |
  | Choose the lead | both | opens the lead picker | the lead is fixed ("A session keeps its lead") |
- **Agents**: every agent of the project's library, with its kind.
  - A lead, before the session's first message: picking it makes it the session's lead (`POST /api/conversations/<c>/lead`).
  - Every other agent, and a lead afterwards: picking it opens `#/agents/<p>/<name>`.

## The lead in the chat (US2)

- The chat head shows "Lead: <name> · <runtime> · <model>".
  - Before the first message, the name is a button that opens the picker: the project's Lead agents, each available one, or greyed out with its reason.
  - After the first message, it is plain text, with the title "A session keeps its lead".
- `lead.changed`: a notice above the message box: "<lead>'s runtime or model changed since this session started: your next message starts a new session with it."
- `lead.problem`: the notice says why, and the message box is disabled.
- "+ New feature" offers the lead next to the name, defaulting to the project's lead.
- On the Agents tab, a Lead agent has "Use for this project" (as pipelines have), and the project's lead shows a "this project's lead" pill.

## The working indicator (US3)

`#working`, a sibling after the chat's message list, has `role="status"`: three dots and "The lead is answering…".
- It is shown while `busy`, and hidden at the end of the turn or a cancel.
- The dots animate only under `prefers-reduced-motion: no-preference`.
- Re-rendering the messages never re-creates it.

## Leaving with unsaved changes (US4)

- A screen may define `dirty()`: the agents and pipelines screens, and the spec session while a document is being edited.
- On any address change that leaves a dirty screen (another screen or project, Back or Forward, a link), the old address is put back. Then "Leave your changes?" asks before going.
- On close or reload, `beforeunload` brings up the browser's prompt while a screen is dirty.

## The project's files (US5)

- **Project page**: the left column's tabs, "Features" and "Files" (`role="tablist"`).
  - Files shows a version picker, a filter and "Only changed".
  - The tree is keyboard-operable (`role="tree"`): ↑ / ↓ move, → / ← unfold and fold, Enter opens.
  - Marks: M (changed), A (added), D (deleted), "spec" for the feature's folder, and a lock for secrets.
- **An open file** takes the chat column's place, with:
  - a header showing the path, its mark, and who changed it in which task;
  - the buttons "Changes vs <base>" / "Whole file", "Ask the lead about it", "Open in my editor" and "← Conversation";
  - line numbers, with added lines highlighted.
- **Loop page and spec session**: a "Files" button in the header.
  - B1, the drawer: it replaces the side column, with "Changed · n" / "All files" and "Open full".
  - B2, a file open: the drawer widens over the agent cards and the feed (spec session: over the documents), as in `mockup/files.html`. The header and the task tree stay in view, and it has "← Back to the loop".
- **Address**: `#/p/<p>/files?v=<version>&f=<path>` opens the project page's Files tab with that file.
- **No file view has an editable element.**
