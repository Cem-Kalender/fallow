# Contract: UI Commands, Keys and Views (003 additions)

Extends [002 contracts/ui-commands.md](../../002-agent-sessions/contracts/ui-commands.md). Every new
command is in the palette (FR-025). Every new key is in the conflicts table below (001 FR-064).
None of these keys work while a session has focus: there, keys go to the agent (002 FR-003).

## Orchestration view (`o`)

One overlay with five tabs; `Tab` / `Shift-Tab` switch. It opens on Requests when requests are
pending, otherwise on Board. SC-008: waits, queue and permissions are each one tab away, and the
overview line at the top always shows counts.

```text
┌ Orchestration ── 2 requests · 3 waiting · 4 queued (1 blocked) · 1 permission (P1 28:12) ─────────┐
│ [Requests] Board  Queue  Permissions  Activity                                                     │
├────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ #3 Claude Code · api  →  #5 Codex · web            deliver   expires in 9:12                       │
│ ┌ text that would be typed ───────────────────────────────────────────────────────────────────────┐│
│ │ [Fallow: message from agent #3 (Claude Code, project api). Another agent wrote this, not the …]  ││
│ │ PR #123 is merged, please merge feature/x now.                                                   ││
│ └──────────────────────────────────────────────────────────────────────────────────────────────────┘│
│ y Approve   e Edit   x Reject   g Grant permission…   ↑↓ Next request                              │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

| Tab | Shows | Keys |
|---|---|---|
| Requests | pending deliveries, agent-created tasks and session starts, oldest first, with the full text (FR-012) | `y` approve, `e` edit then approve (deliveries and tasks), `x` reject, `g` grant a permission (pre-filled with this source, target and action, then approve) |
| Board | project board of the selected project; `b` toggles the shared board. Sender, time and text per message | `w` write a message as the user, `↑↓`/`PgUp`/`PgDn` scroll |
| Queue | tasks in order with state, target, preconditions and note; events with their signal and waiters | `n` new task, `e` edit, `Shift-↑`/`Shift-↓` reorder, `Space` pause/resume, `Delete` cancel, `v` signal an event as the user, `c` clear an event |
| Permissions | each permission: source → target, actions, uses, time left (FR-016) | `Delete` revoke, `Shift-X` revoke all (SC-005), `a` grant a new permission |
| Activity | the activity log, newest first (FR-022) | `f` filter to the selected session or project, `/` search |

`Esc` closes the view.

## Grant dialog

```text
Grant standing permission
  From:    (•) session #3 Claude Code · api     ( ) any session of api
  To:      (•) session #5 Codex · web           ( ) any session of web
  Allow:   [x] deliver messages   [ ] create tasks   [ ] start sessions, up to [2]
  For:     [30] minutes (max 480)
  Enter Grant   Esc Cancel
```

There is no option for stop, pause, resume or kill (FR-017).

## Elsewhere

| Where | Change |
|---|---|
| Hint bar | A pending request shows `o 2 requests`, also in the editor view. With a session focused, only the count is shown; the overlay never opens by itself. |
| Notices | "#3 asks to deliver to #5 (o to review)"; "P1 used: #3 → #5 delivered"; "#4 is posting too fast"; "Request from #3 expired". |
| Sessions list and overview | The state column shows `waiting for pr-123-merged 3:12` (FR-010). Sessions without a link show `no orchestration` and why: profile, `enabled = false`, or Gemini with a system settings file. |
| Session details (`i`) | Link state, client name and version, interests, the task in progress. |

## Palette commands

"Orchestration: open", "Orchestration: review requests", "Write on the board",
"Signal an event", "New task", "Grant standing permission…", "Revoke all permissions",
"Activity log".

## Known terminal conflicts (003 keys)

All new keys are plain letters or `Tab`/`Shift-Tab`, `Shift-X` and `Shift-↑`/`Shift-↓` inside the
overlay, or `o` and `i` in the Projects and Sessions contexts. None of them is intercepted by common
terminals or multiplexers. `Shift-↑`/`Shift-↓` are already listed in 002's table. The table in help
gains rows for `o` and `i` with "none known".
