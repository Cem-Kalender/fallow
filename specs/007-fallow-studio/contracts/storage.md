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

010 adds (all optional when read, so earlier logs load unchanged):

| Type | Fields |
|---|---|
| `loop_created` | + when_verified: wait \| pull_request (absent: wait) |
| `published` | remote, service, repo, branch (on the remote), commit (the gated commit), target, pr_number, pr_url, by: engineer \| when_verified |
| `publish_failed` | step: check \| push \| pull_request, why, pushed? [branch, commit], by |
| `pull_request_state` | state: open \| merged \| closed, checks[] (name, state: waiting \| passed \| failed \| skipped, url?), merged_at? (written only when something changed) |
| `steering` | + kind `publish` (detail: the commit and who: engineer or when_verified) |

For a pull request, `merged.commit` (and a record's final commit) is the commit pushed and merged on the remote, not a commit on the local target (a squash or a rebase makes new ones there). A loop's stage goes Ready → Publishing on `published`, Publishing → Done (with `merged`, how: "pull request") when the pull request is merged on the remote, and Publishing → Ready when it is closed. `projects.json` gains `remote_seen` per project (the remote as last read, with `checked_at`) and `when_verified` on each lock record. The pull request's text is kept at `runs/<id>/publish/body.md`.

011 adds (011 data-model.md). Every field added to an existing event is `#[serde(default)]`, so earlier logs load unchanged:

| Type | Fields |
|---|---|
| `loop_created` | + update_limit (absent: 3), run_after? (the loop it waits for, chosen at the lock), switches { auto, line, overlap } (from `.fallow/squad.toml` `[updates]`; absent: all true) |
| `relocked` | + reconciled_target? (the target commit a "Lock again" was reconciled against) |
| `merged` | + unchecked[] (target commits a "Merge without the latest check" was not verified with) |
| `published` | + unchecked[] (the same for "Publish without the latest check") |
| `publish_failed` | + moved_to? (the remote target's commit, when it moved since the loop was verified) |
| `pull_request_state` | + conflicting, behind (absent: false), strict? (whether the target requires branches to be up to date; absent when it cannot be read) |
| `steering` | + kinds `update` (also "Verify again on the latest <target>"), `merge_unchecked`, `publish_unchecked`, `leave_pull_request`, `lock_again`, `wait_for`, `stop_waiting` |
| `updated` | n, target, target_commit, loop_head, remote, in_turn, for_publication (the last three absent: false) |
| `update_conflicted` | n, working (commit), branch (its actual name), conflicts[] (path, kind: text \| non_text) |
| `update_docs_changed` | n, paths[] |
| `update_merged` | n, commit |
| `update_dropped` | n, why |
| `pushed_forward` | from, to |
| `left_pull_request` | unchecked[] |
| `touched` | files[] |
| `overtaken` | n, target_commit |
| `lined` | at_ms |
| `unlined` | why |
| `overlap` | with (run id), files[] |
| `waits_for` | run (run id), from: lock \| engineer |
| `wait_ended` | run, why |
| `started_from` | target_commit, commit |

- `updated`: an update started. It is written before the merge is computed, so a restart can resume it; `remote` says the target was read from the remote, `in_turn` that the loop held its line's turn, `for_publication` that the remote target moved before the first push.
- `update_conflicted`: the merge conflicts. The repair starts on the `working` commit, kept on the loop's own branch `branch`.
- `update_docs_changed`: the target changed the feature's locked documents. Nothing is merged; the engineer locks again.
- `update_merged`: Fallow's two-parent merge commit is on the loop's branch. The stories are checked again from there.
- `update_dropped`: the update was dropped, because the pull request was merged or closed, the loop stopped, it was locked again, or its merge could not be computed or moved onto the loop's branch (`why` says which; the loop then asks).
- `pushed_forward`: the loop's branch on the remote moved forward from `from` to `to`, after an update of a published loop.
- `left_pull_request`: the engineer chose "Leave the pull request as it is" at the limit of overtaken updates; `unchecked` names the target commits not verified with.
- `touched`: the files an accepted step changed. They feed the overlap warnings.
- `overtaken`: the target moved again while the loop held its turn, before its result could be merged or published.
- `lined` and `unlined`: the loop joined or left the merge line of its target. `at_ms` (milliseconds since the epoch) is its place in the line.
- `overlap`: another unfinished loop touches the same files.
- `waits_for` and `wait_ended`: the loop does not start steps until loop `run` is merged, stopped or deleted, and then the wait is over.
- `started_from`: a loop that waited from its lock and built nothing starts from the target at `target_commit`, merged straight onto its branch as `commit`. Its base moves there and no update is left in progress.

`projects.json` gains, on each lock record, `update_limit?`, `run_after?` and `reconciled_target?`, also optional.

006's events (`step_*`, `verified`, `gate_run`, `verdict`, `steering`, `limits_set`, `session_lost`, `needs_cleared`, `merged`, `stopped`) are kept. `run_created` is kept for runs from 006, which stay readable.

**Rules**:
- An event is written and flushed before Fallow acts on it (constitution VII).
- Removing a project from Fallow deletes its conversations and worktrees under Fallow's data, after a confirmation naming them. The project folder and its branches are not touched.

012 adds (012 data-model.md). Every field added to an existing event is `#[serde(default)]`, so earlier logs load unchanged:

| Where | What |
|---|---|
| `conversations/<id>/events.jsonl` | a conversation of kind `solo`: the session's events (`session_creating`, `session_created`, `session_renamed`, `branch_renamed`, `turn_committed`, `fold_next`, `uncommitted_discarded`, `engineer_edits`, `base_moved`, `undo_started`, `turns_undone`, `message_queued`, `queue_edited`, `queue_removed`, `queue_sent`, `web_set`, `git_observed`, `folder_state_seen`, `squashed`, `gates_run`, `feature_suggested`, `feature_starting`, `feature_started`, `close_started`, `session_closed`, `copy_removed`); `turn_started` gains `attachments`, `instructions`, `slot_waited_ms`; `turn_finished` gains `commits`, `commit_failed`, `changed`, `cut`, `folded_into`; `approval_asked` gains `reasons`, `merge`; `approval_answered` gains `auto`, `why`; `conversation_created` gains `agent`, `origin` |
| `conversations/<id>/attachments/<sha>.<ext>` | the images attached to a session's messages |
| `conversations/<id>/turn.pid` | the running turn's root pid and its start time (a restart stops it first) |
| `projects/<p>/git.jsonl`, `git-seen.json` | the changes Fallow observed to the project's branches and remotes, and the last snapshot |
| `<data>/nohooks/` | the empty folder Fallow's own git commands on a session use as `core.hooksPath` |
| in the project's repository | `refs/fallow/solo/<id>/undone/<n>`: a session's undone commits, kept until it closes |
