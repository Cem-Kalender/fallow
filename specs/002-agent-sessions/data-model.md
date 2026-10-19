# Data Model: Agent Sessions and Multiple Projects

Entities from [spec.md](./spec.md) "Key Entities", with fields, rules and state machines. 001 types
(`Project`, `Buffer`, `Snapshot`, `ProcessKey`) are reused where named.

## ProjectList

The remembered list of projects (FR-014, FR-015). Persisted in `projects.toml` in Fallow's config
directory ([contracts/config.md](./contracts/config.md)).

| Field | Type | Rules |
|---|---|---|
| projects | Vec<ProjectEntry> | Order is the user's order. No two entries with the same canonical path. |
| selected | Option<usize> | Index of the selected project; restored at start (FR-015). |

### ProjectEntry

| Field | Type | Rules |
|---|---|---|
| id | ProjectId (u32) | Unique per run; not persisted. |
| path | PathBuf | Absolute. Compared after `std::path::absolute`, case-insensitively on Windows and macOS. |
| name | CompactString | Folder name unless the user renamed it. 1..=64 chars. |
| present | bool | False when the folder is missing or unreadable (FR-018). Checked at start (after the first frame) and when selected. |
| tree | Option<001 `Project`> | Lazy tree for the editor view; created when the project is first shown in the editor view. |
| sessions | Vec<SessionId> | Sessions started in this project, in start order. |

Operations: add (path → validated readable folder, appended and selected), remove (sessions running →
confirmation; never touches the folder), move up/down, rename, relocate (missing → new path).
Every change rewrites `projects.toml` (FR-015); a write failure is shown as a notice and does not
change the in-memory list.

## AgentProfile

What can be started (FR-023, FR-024). Built-ins are compiled in; `profiles.toml` overrides or
extends them by `id`, with the same merge rules as 001's `signatures.toml`.

| Field | Type | Rules |
|---|---|---|
| id | CompactString | `[a-z0-9_-]{1,32}`, unique. |
| name | CompactString | Shown in menus and the overview. |
| command | CompactString | Program name (looked up on `PATH`, including `PATHEXT` on Windows) or absolute path. |
| args | Vec<CompactString> | Passed as given, no shell expansion. |
| env | Vec<(name, value)> | Additions to the inherited environment. Names are shown; values are never displayed or logged (FR-027). |
| kind | Builtin \| User | |
| available | bool | Resolved command exists and is executable. Checked when the start menu opens, not at startup. |

## Session

One agent in a terminal pane (FR-001 to FR-013).

| Field | Type | Rules |
|---|---|---|
| id | SessionId (u32) | Unique per run, shown as `#n`. |
| project | ProjectId | Fixed at start. |
| profile | profile id | Fixed at start; restart uses the same profile. |
| child | Option<ChildInfo> | PID and start time of the agent (a 001 `ProcessKey`), set after spawn. |
| term | TermState | Screen grid, scrollback (cap `sessions.scrollback_lines`, default 10,000), modes the agent set (bracketed paste, mouse reporting, application cursor keys, alternate screen). |
| size | (cols, rows) | Always the pane's real size; resizes are forwarded to the agent (FR-002). |
| state | SessionState | See below. |
| state_since | Instant | For "time in state" (FR-008). |
| last_output | Instant | For the attention rule. |
| bell | bool | Set by BEL from the agent; cleared when the session is shown with focus. |
| scroll | usize | Lines scrolled back from the bottom; 0 = following output. |
| selection | Option<range> | For copy (FR-005). |
| workdir | ProjectFolder or Worktree { path, branch } | Where the agent runs (US6). Fixed for the session's life; restart reuses it. |

### SessionState

```text
Starting ──spawn ok──▶ Working ◀──────────── output or busy CPU ───┐
   │                     │                                          │
   │ spawn failed        │ bell, or quiet ≥ attention delay         │
   ▼                     │ while group CPU < busy threshold         │
Failed(reason)           ▼                                          │
                     NeedsAttention ───────────────────────────────┘
                         │
 any state ── child exited ──▶ Exited(status)
 Exited ── restart ──▶ Starting
 Working/NeedsAttention ── user stops (FR-012) ──▶ Stopping ──▶ Exited
```

- "Quiet" means no bytes from the agent for `sessions.attention_delay_ms` (default 5,000).
- The busy threshold is 001 FR-012's (2% of one core averaged over 10 s), applied to the agent's
  process group from the latest procmon snapshot.
- Entering `NeedsAttention` while the session is not visible with focus raises a notice (FR-009).
- `Stopping` follows 001's ControlRequest lifecycle (confirmation, revalidation, graceful stop, and a
  separately confirmed force stop on Linux and macOS).

## Worktree (US6)

A git worktree Fallow created for a project (research R-11).

| Field | Type | Rules |
|---|---|---|
| path | PathBuf | `<parent>/<project>.worktrees/<profile>-<n>` unless `sessions.worktree_root` is set. |
| branch | String | `fallow/<profile id>-<n>`, first free `n`; never an existing branch. |
| project | ProjectId | |
| present | bool | The folder exists. |

Lifecycle:
1. Created at session start (the default for git projects).
2. Used by the session and its restarts.
3. On close: kept (the default), or removed, only when `git status --porcelain` is empty. The
   branch is always kept.

While it exists, it is listed in the start menu.

## Workspace

What is on screen (FR-019, FR-020).

| Field | Type | Rules |
|---|---|---|
| view | Agents \| Editor | Starts as `Agents`. The view key switches; each view keeps its own focus. |
| agents_focus | ProjectList \| Sessions \| Session(SessionId) \| Processes | Starts at `ProjectList`. |
| layout | Single(SessionId) \| Split(SessionId, SessionId) \| Overview | |
| process_filter | Option<ProjectId> | When set, the LLM panel shows only processes whose cwd is inside the project (FR-016). |

## Limits

| Item | Limit | Source |
|---|---|---|
| Concurrent sessions | 8 by default, configurable up to 16 | FR-007; the memory budget is measured at 4 with full scrollback (FR-029), and 8 is checked for responsiveness (SC-006). |
| Scrollback per session | 10,000 lines default, 1,000..=100,000 | FR-004 |
| Projects | 200 | keeps `projects.toml` and the list cheap |
