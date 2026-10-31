# Contract: what Fallow keeps on disk

006's layout (006 contracts/storage.md) is kept, with these changes. Everything stays readable by the current user only, and nothing is sent anywhere (constitution V).

```text
%USERPROFILE%\.fallow\               # the engineer's own configuration (research S-16)
├── settings.toml                    # moved from %APPDATA%\fallow\ on first start
├── agents\<name>.md                 # the defaults, copied on first start, and the engineer's own
└── pipelines\<name>.toml

%LOCALAPPDATA%\fallow\               # Fallow's data
├── service.json
├── projects.json                    # NEW: the projects (data-model.md, Project)
├── conversations\<id>\              # NEW: project conversations and spec sessions
│   ├── events.jsonl
│   └── turns\<n>.jsonl              # the activity of each turn
├── runs\<id>\                       # loops (006's runs)
│   ├── events.jsonl
│   ├── steps\<n>.jsonl, steps\<n>.prompt
│   └── wt\                          # worktrees
├── worktrees\<project>\<feature>\   # NEW: the spec sessions' and project conversations' worktrees
├── records\v1\loop-<id>.json        # NEW: a loop's run record, written once when it ends (S-20):
│                                    #   numbers, identifiers and fixed values only
├── notified.json                    # NEW: the "Needs you" items already notified (S-14)
└── notifications-off                # NEW: present while the tray menu's switch is off

<project>\
├── .specify\, .claude\skills\speckit-*   # installed on add, only where missing (S-15)
├── .fallow\squad.toml                    # gates (006)
├── .fallow\agents\, .fallow\pipelines\   # the project's overrides
└── specs\NNN-name\handover\phase-N.md    # committed by Fallow
```

## Conversation events

| Type | Fields |
|---|---|
| `conversation_created` | kind, project, feature?, forked_from? |
| `turn_started` | n, said, edits[], mode, session |
| `turn_finished` | n, reply, documents_changed[], commit?, discarded[], cost_usd, failure? |
| `approval_asked` | id, command |
| `approval_answered` | id, answer, prefix? |
| `mode_set` | mode |
| `step_set` | step (Spec Kit) |
| `doc_edited` | path, by: engineer, version |

## New loop events

| Type | Fields |
|---|---|
| `loop_created` | project, feature, lock, lock_commit, base_branch, build_branch, pipeline (resolved, as TOML), level, budget_usd, budget_minutes, phases[] (from `tasks.md`; each with n, label, title, story, tasks) |
| `task` | id, state, commit?, repairs (its gate run is the `gate_run` event that follows) |
| `phase` | n, state, session?, handover? |
| `question` | id, step, task, text, options[], state, by?, answer?, commit?, affects, recommended?, cost_usd?, reopened[]?, why? (relayed: why it went to the engineer) |
| `unlocked` | (always the engineer's action; recorded with a `steering` event) |
| `relocked` | lock, commit, affected[], phases[] (the new lock's `tasks.md`) |
| `outcome` | ready \| done_unverified, missing[] |

006's events (`step_*`, `verified`, `gate_run`, `verdict`, `steering`, `limits_set`, `session_lost`, `needs_cleared`, `merged`, `stopped`) are kept. `run_created` is kept for runs from 006, which stay readable.

**Rules**:
- An event is written and flushed before Fallow acts on it (constitution VII).
- Removing a project from Fallow deletes its conversations and worktrees under Fallow's data, after a confirmation naming them. The project folder and its branches are not touched.
