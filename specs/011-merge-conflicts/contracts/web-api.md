# Contract: page and API additions (011)

Additions to the page API of 007 to 010 (same secret, same origin rules, JSON). Every request
below is a steering action, recorded in the loop's log (constitution II).

## The loop

`GET /api/loops/<id>` gains:

```json
"target": {
  "moved": true,                        // the target holds commits the verified commit does not (FR-001)
  "commits": [{ "sha": "a1c09e2", "subject": "…", "from": "010-receipt" }],   // from: a feature, or null = outside Fallow
  "conflicts": ["shop/receipt.py"],     // empty: it would merge without conflict
  "docs_changed": [],                   // this feature's locked documents changed on the target
  "remote": false,                      // true: read from the remote target (a published loop)
  "pr": { "conflicting": false, "behind": true, "required": false },   // a published loop only
  "read_at": "2026-11-03T14:02:11Z"
},
"update": {                             // null when no update is in progress
  "n": 2, "target_commit": "a1c09e2", "conflicts": ["shop/receipt.py"],
  "step": "resolving the conflicts"     // merging | resolving the conflicts | gates | checking stories | pushing
},
"updates": 2, "update_limit": 3, "overtaken": 1,
"line": { "position": 2, "after": "010-receipt" },     // null when not in a line
"waits_for": { "run": 14, "feature": "010-receipt", "from": "lock" },   // null
"overlaps": [{ "run": 14, "feature": "010-receipt", "files": ["shop/receipt.py"] }],
"offer": ["bring_up_to_date", "stop"]
```

`offer` replaces "merge" by what can succeed (FR-003): one or more of `merge`, `publish`,
`bring_up_to_date`, `verify_again`, `merge_unchecked`, `publish_unchecked`, `leave_pr`,
`lock_again`, `wait_for`, `stop_waiting`, `stop`. `stop`, `wait_for` and `lock_again` are never offered while
the loop's pull request is open (FR-016b; unlock is refused while publishing): the page says to close
the pull request on the remote first.

The page's labels: `bring_up_to_date` "Bring up to date"; `verify_again` "Verify again on the latest
<target>"; `merge_unchecked` "Merge without the latest check"; `publish_unchecked` "Publish without
the latest check"; `leave_pr` "Leave the pull request as it is"; `lock_again` "Lock again";
`wait_for` "Wait for <feature>"; `stop_waiting` "Stop waiting". Under `merge_unchecked`, `publish_unchecked` and `leave_pr`: "not
checked with: <n> commits" and the commits.

## Requests

| Request | Body | Effect |
|---|---|---|
| `POST /api/loops/<id>/update` | `{}` | Bring up to date (also "Verify again on the latest <target>"); refused when not offered. |
| `POST /api/loops/<id>/merge` | `{ "without_latest_check": true }` (optional) | Merge; with the flag, only when `merge_unchecked` is offered; refused when the merge would conflict. While another loop holds the turn, accepted and queued: the reply is `{ "in_line_after": "<feature>" }`, and the merge runs (rechecked) when the turn comes. |
| `POST /api/loops/<id>/publish` | `{ "without_latest_check": true }` (optional) | As 010; with the flag only when `publish_unchecked` is offered. |
| `POST /api/loops/<id>/leave-pr` | `{}` | Leave the pull request as it is (recorded with the newer commits not verified with, and labelled so). |
| `POST /api/loops/<id>/wait` | `{ "for": <run id> }` | Wait for that loop (after the current step); refused when it would close a cycle (the reply names the loop already waiting) or the pull request is open. |
| `DELETE /api/loops/<id>/wait` | | Stop waiting. |
| `POST /api/loops/<id>/lock-again` | `{}` | Opens the feature's spec session with Fallow's message showing the target's version of the changed documents; the lock there is the existing relock, which ends the held update. Refused while the pull request is open (FR-016b). |

## The lock

`GET /api/projects/<id>/features/<f>/overlap` (read before locking):

```json
{ "overlaps": [{ "run": 14, "feature": "010-receipt", "stage": "building",
                  "files": ["shop/receipt.py"], "named_in_tasks": true }],
  "forecast": "compared with the files this feature's tasks name and the files the other loops changed so far or name in their tasks" }
```

`POST …/lock` gains `run_after` (a run id, optional) and `update_limit` (1 to 20, default 3). The lock
form shows "Updates overtaken before I decide" (default 3) under "When verified", and the overlaps
above "Lock" with "Run after <feature>" and "Run now anyway".

## Dashboard and notifications

- "Needs you" gains `update` (bring up to date, at Normal/Strict), `update_limit` (the limit
  reached), `docs_changed` (lock again or stop), `overlap` (a new overlap while running).
- Notifications (Fallow's own): "<feature>: <target> moved; bring it up to date", "<feature>: updated
  <n> times and overtaken; your decision", "<feature> merged without the latest check (<n> commits)".
  They name the project and feature, never code or output (V).
