# Data model: Publishing that respects the remote (010)

Additions only; everything else is as in 007 to 009. New fields on recorded events are optional
(`#[serde(default)]`), so logs written by an earlier Fallow still read.

## Remote of a project (not stored; known in memory, refreshed)

| Field | Meaning |
|---|---|
| `state` | `none` (no remote for the target), `no_target` (remotes, but the target branch is on none of them), `unsupported` (a service Fallow cannot publish to), `known` |
| `remote` | the remote's name (`origin`) |
| `service` | `github`, or the host's name when unsupported (`gitlab.com`) |
| `repo` | `owner/name` |
| `target` | the target branch's name on the remote |
| `protected` | `yes`, `no`, or `unknown` |
| `why` | for `unknown` or `no_target`: the reason in plain words (not signed in, no access, offline, the branch is not on the remote yet) |
| `checked_at` | when it was last read |

- Computed from git at once (R-1), and from the hosting adapter on a thread (R-2, R-3): the project
  shows the git part at once and `protected` when the adapter has answered ("checking…" until then).
- Refreshed when the project is opened (at most every 60 s) and when a loop of it reaches Ready
  (FR-003). Kept in the engine's memory; the last known value per project is also kept in
  `projects.json` (`remote_seen`) so a restart shows it at once, marked with its time.

## Loop: "when verified" (recorded)

- `LoopCreated.when_verified`: `wait` (default) | `pull_request`. Also in `LockRec` and
  `LoopState`. Set at the lock; preset by the level (Free → `pull_request`); shown on the loop page.

## Publication of a loop (recorded, in the loop's events)

New events:

| Event | Fields | When |
|---|---|---|
| `Published` | `remote`, `service`, `repo`, `branch` (on the remote), `commit`, `target`, `pr_number`, `pr_url`, `by` (`engineer` or `when_verified`) | the push and the pull request both succeeded |
| `PublishFailed` | `step` (`check`, `push`, `pull_request`), `why`, `pushed` (the branch and commit if the push had succeeded) | any failure; the loop stays Ready (FR-012) |
| `PullRequestState` | `state` (`open`, `merged`, `closed`), `checks` [{`name`, `state`: `waiting`, `passed`, `failed`, `skipped`; `url`}], `merged_at` | recorded only when something changed (R-6) |

- Stages: Ready → **Publishing** on `Published`; Publishing → **Done** with `Merged { how: "pull
  request" }` when merged; Publishing → **Ready** when closed (a new transition, added to the
  allowed ones). Steering gets `Publish` (the engineer's action, or Fallow's when verified).
- `LoopState.publication`: the last `Published` with its latest `PullRequestState`; at most one
  open pull request per loop (FR-011).
- A restart during publishing reconciles from the remote (R-8).

## Settings (`settings.toml`, optional)

```toml
[hosting.github]
command = "gh"   # the default; tests name a fake here
args = []         # leading arguments (tests: ["--state", "<file>"])
```

## Validation rules

- The push's destination ref MUST differ from the target branch; checked before every push.
- A remote branch with another commit is never overwritten: a suffix is added (R-4).
- Publishing on its own (`when_verified = pull_request`) only from a verified outcome
  (`Outcome { verified: true }`), with `Remote.state = known` and `protected` read (`yes` or
  `no`); with `protected = unknown` the loop waits for the engineer and says why (FR-020).
