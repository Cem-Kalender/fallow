# Contract: what Fallow keeps on disk

Everything is under Fallow's own directories, readable by the current user only (ACL set at
creation). Nothing is ever sent anywhere (constitution V).

```text
%LOCALAPPDATA%\fallow\
├── service.json            # pid, port, start time (no secret)
├── runs.json               # index for the runs page; rebuilt from runs\ if missing
├── runs\<id>\
│   ├── events.jsonl        # the authoritative log (append-only, fsync per event)
│   ├── steps\<n>.jsonl     # activity of step n
│   ├── steps\<n>.prompt    # the instruction given to step n
│   └── worktrees\          # the run's working copies (git worktrees of the repository)
├── records\<id>.json       # run records of measured runs (005's format and field rules)
└── copies\                 # measured runs' copies of the project (removed after the run)
```

## Events (`events.jsonl`)

Each line is `{"seq": n, "at": "<RFC 3339>", "type": "<type>", ...}`. `seq` increases by 1 and is
the page's `Last-Event-ID`.

| Type | Fields |
|---|---|
| `run_created` | repo, target, start_commit, task, mode, autonomy, limits, roles, repair_cap, switched_off, recorded |
| `plan_committed` | commit, approvable, problems[] |
| `plan_approved` | commit |
| `stage` | from, to, why |
| `step_queued` | n, kind, role, part?, reason |
| `step_waiting` | n, why (capacity, a decision, the engineer) |
| `step_started` | n, agent, model, session, worktree, base, limits |
| `step_finished` | n, result \| failure, cost_usd, duration_ms, permission_denials[] |
| `verified` | n, outcome (accepted \| rejected), checks[] with reasons |
| `gate_run` | gate, commit, kind, outcome, failing[], class |
| `decision` | id, question, from_step, part, state, decided_by?, answer?, commit?, recommendation?, affected_parts[] |
| `part` | id, state, result_commit?, repairs, takes[] |
| `verdict` | step, answers[], complete, approved |
| `steering` | kind, target?, detail?, stage |
| `limit_reached` | which (cost \| time), used |
| `publication` | kind, rules_source, required_checks[], required_reviews, pr?, checks[] |
| `merged` | commit, how (local \| pull_request) |
| `stopped` | reason? |
| `hidden_tests` | passed, failed (measured runs) |

**Rules:**
- An event is written and flushed before Fallow acts on it. On resume, the state is the fold of all
  events (research R-2).
- Deleting a run removes its directory and worktrees after a confirmation naming them
  (constitution II). Branches in the repository are kept unless the engineer chooses to remove
  them.
- The runs page shows how many runs Fallow holds and their total size on disk (constitution V).
