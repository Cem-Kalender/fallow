# Data Model: Agent Orchestration

Entities from [spec.md](./spec.md) "Key Entities", with fields, rules and state machines. 002 types
(`SessionId`, `ProjectId`, `Session`, `AgentProfile`) are reused where named. Everything here lives
in one `Hub` inside the Fallow process. It is created when the first session with orchestration
starts (FR-005), is never written to disk (FR-023) and is gone when Fallow quits.

## Hub

| Field | Type | Rules |
|---|---|---|
| channel | Channel | The local listener ([research R-2](./research.md)); one per Fallow process. |
| links | Map<SessionId, Link> | One per session started with orchestration. |
| boards | Map<BoardId, Board> | `Shared` exists from the start; a project board appears on first post. |
| events | Map<(Scope, name), Event> | |
| deliveries | Vec<Delivery> | Pending and recently finished (newest 200 finished kept for the log view). |
| permissions | Vec<Permission> | Active ones; expired or revoked ones move to the activity log. |
| tasks | Vec<Task> | Queue order is the vector order. |
| starts | Vec<StartRequest> | Pending session-start requests (US5). |
| activity | Ring<ActivityEntry> | Newest 5,000 (FR-022, FR-023). |
| bytes | usize | Total text stored (messages, notes, task text, delivery text); see Limits. |

## Link

The connection of one session to the hub (FR-003).

| Field | Type | Rules |
|---|---|---|
| session | SessionId | Fixed. The only identity the link can act as. |
| token | [u8; 32] | Random, made at session start, handed only to that session's bridge ([R-3](./research.md)). Compared in constant time. Never shown, logged or written. |
| state | Offered \| Connected \| Unsupported \| Closed | `Offered`: the agent was started with the bridge configured. `Unsupported`: the profile has no way to add a tool server (overview says so, spec Edge Cases). |
| client | Option<(name, version)> | From the MCP `initialize` request; shown in the session details. |
| waiting | Option<Wait> | The event wait in progress, if any (FR-010). |
| interests | Vec<(Scope, name)> | Events this session asked to be woken for when idle (US2 scenario 5); max 16. |
| rate | TokenBucket | 30 posts/requests per minute (FR-008); excess calls are refused with "slow down" and the engineer gets one notice per minute. |

## Sender

Set by Fallow from the link, never from tool arguments (FR-007, SC-004).

```text
Sender = User
       | Agent { session: SessionId, project: String, profile: String }   # snapshot at send time
```

Rendered to agents as `[from agent #3 · Claude Code · api, via Fallow]` or `[from the user, via
Fallow]` ([contracts/mcp-tools.md](./contracts/mcp-tools.md)).

## Board and BoardMessage

| Field | Type | Rules |
|---|---|---|
| BoardId | Shared \| Project(ProjectId) | An agent's "project" board is its session's project. |
| messages | VecDeque<BoardMessage> | Newest 1,000 per board (FR-008). |

| BoardMessage field | Type | Rules |
|---|---|---|
| id | u64 | Increasing across all boards; agents read "since id". |
| sender | Sender | |
| at | SystemTime | Shown as its age ("42s", "3m") in Fallow; sent to agents as RFC 3339 UTC. |
| text | String | 1..=8 KiB UTF-8 (FR-008); control characters other than `\n` and `\t` are replaced with `�` before storing. |

## Event

| Field | Type | Rules |
|---|---|---|
| scope | Shared \| Project(ProjectId) | |
| name | String | `[A-Za-z0-9._:/#-]{1,64}`, case-sensitive. |
| signal | Option<Signal> | Sticky: once signalled, later waits end at once (FR-009). Signalling again replaces it (newest sender, time and message). |
| waiters | Vec<SessionId> | Sessions in a wait now. |

`Signal { sender: Sender, at: SystemTime, message: Option<String> }`, message ≤ 2 KiB. The engineer
can clear an event from the events view; at most 500 events per scope (oldest signalled first out).

### Wait

| Field | Type | Rules |
|---|---|---|
| event | (Scope, name) | |
| since | Instant | For "waiting for X · 3:12" in the overview (FR-010). |
| deadline | Option<Instant> | The agent's time limit; "timed out" when it passes (US2 scenario 3). |
| call | request id | The MCP request to answer. A wait answers at the latest after the block limit and the agent calls again ([R-4](./research.md)); `since` survives the repeat. |

## Delivery

Text to be typed into a session for someone else (FR-011 to FR-014).

| Field | Type | Rules |
|---|---|---|
| id | u64 | |
| from | Sender | |
| to | SessionId | Must be a running session other than the sender's. |
| text | String | ≤ 8 KiB, as shown in the approval prompt. The label is added at typing time, outside the editable text. |
| purpose | Message \| Task(TaskId) \| Wake(event) | |
| created / expires | Instant | Expires 10 minutes after creation by default (`orchestration.request_timeout_s`). |
| covered_by | Option<PermissionId> | Set when a standing permission let it through. |
| state | DeliveryState | Below. |

```text
Pending ──approve──▶ Approved ──target free of typing (FR-014)──▶ Delivered
   │  └─edit+approve─▶ Approved(edited)                                 
   │  (permission matched at creation) ─────────────────▶ Approved
   ├──reject──▶ Rejected
   ├──time limit──▶ Expired
   └──target exited / closed──▶ Failed("target exited")
Approved ──target exited before typing──▶ Failed("target exited")
```

- Only `Approved` leads to typing. The single function that writes delivery bytes into a session
  takes a `DeliveryGrant` that only `approve()` and `Permission::cover()` can construct (the 001
  `ConfirmedToken` pattern; SC-003).
- "Free of typing": the session is not focused, or no key was sent to it for
  `orchestration.delivery_delay_ms` (default 3,000).
- The requester's tool call returns at once with the delivery id and "waiting for approval"; the
  outcome is reported on its next `inbox` read and in the activity log.

## Permission

A standing permission (FR-015, FR-016; constitution IV).

| Field | Type | Rules |
|---|---|---|
| id | u32 | Shown as `P1`, `P2`. |
| source | Scope | `Session(id)` or `Project(id)` (any session of the project). |
| target | Scope | Same forms. |
| actions | set of Deliver \| CreateTask \| StartSession { limit: u8 } | Never stop, pause, resume, kill or re-prioritize: those have no variant (FR-017). |
| granted / expires | Instant | Duration 1 minute ..= 8 hours, default 30 minutes. |
| uses | u32 | Shown in the permissions view. |
| starts_used | u8 | For `StartSession`; past `limit` a start needs approval again (US5 scenario 2). |

A permission covers a request when source and target match the requester and the target, the
action is in `actions`, and `now < expires`. Every use raises a notice and an activity entry. "Revoke
all" empties the vector before the next request is handled (SC-005).

## Task

| Field | Type | Rules |
|---|---|---|
| id | u32 | Shown as `T1`. |
| text | String | 1..=8 KiB. |
| target | Session(SessionId) \| NextFree { profile, project } | |
| preconditions | Vec<(Scope, event name)> | All must be signalled; max 8. |
| state | TaskState | Below. |
| paused | bool | A paused task is skipped by the queue. |
| assigned | Option<SessionId> | Set when handed over. |
| note | Option<String> | ≤ 2 KiB, from the agent's done/failed report. |
| created_by | Sender | Agent-created tasks needed approval or a `CreateTask` permission (US4 scenario 5). |
| times | created, started, finished | |

```text
            preconditions unmet
Queued ◀───────────────────────▶ Blocked
  │ target free + preconditions met → Delivery(purpose = Task) approved and typed
  ▼
InProgress ──agent reports done──▶ Done
    │      ──agent reports failed──▶ Failed(note)
    │      ──target exited──▶ Queued (NextFree) | Failed("target exited") (Session)
any non-final ──engineer cancels──▶ Cancelled (never handed over)
```

- "Free" for a target session: running, link connected, state `NeedsAttention` (002), not waiting
  for an event, and no task of its own `InProgress`.
- The queue advances on every hub change and on 002 attention changes, in vector order; one task per
  target at a time.

## StartRequest (US5)

| Field | Type | Rules |
|---|---|---|
| from | Sender (agent) | |
| profile, project | ids | Must exist; the project must be present. |
| first_task | Option<String> | Queued for the new session (target `Session(new id)`) when it starts. |
| state | Pending \| Approved \| Rejected \| Expired \| Refused(reason) | `Refused` when 002 FR-007's session limit or the memory guard (FR-029) would be exceeded; checked again at approval time. |

## ActivityEntry

| Field | Type | Rules |
|---|---|---|
| at | SystemTime | |
| actor | Sender | |
| action | Post \| Read \| Signal \| Wait \| WaitEnded \| DeliveryRequested \| DeliveryOutcome \| TaskChanged \| PermissionGranted \| PermissionUsed \| PermissionRevoked \| StartRequested \| StartOutcome \| Refused | FR-022 |
| subject | String | ≤ 200 chars: board, event, target or task, plus the first line of text. |
| outcome | Option<String> | |

Filterable by session and project. Memory only (FR-023).

## Limits

| Item | Limit | Source |
|---|---|---|
| Message, task, delivery text | 8 KiB | FR-008 |
| Signal message, task note | 2 KiB | |
| Messages per board | 1,000 | FR-008 |
| All stored text in the hub | 24 MiB; oldest board messages are dropped first | keeps SC-007 inside 150 MB with every board full |
| Requests per session | 30 per minute | FR-008 |
| Events per scope | 500 | |
| Pending deliveries | 64 in total, 8 per sender | approval prompts queue; beyond this requests are refused with "too many pending" |
| Tasks | 500 | |
| Activity log | 5,000 entries | FR-023 |
| Permission duration | 1 min ..= 8 h, default 30 min | FR-015, constitution IV |
| Request time limit | default 10 min, 1 ..= 60 min | FR-012 |
