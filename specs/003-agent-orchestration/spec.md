# Feature Specification: Agent Orchestration

**Feature Branch**: `001-llm-aware-tui-editor` (no separate branch; see Assumptions)

**Created**: 2026-10-19

**Status**: Draft. The constitution amendment it needed
([constitution-amendment.md](./constitution-amendment.md)) was ratified on 2026-10-19 (v4.0.0).

**Input**: User description: "Agent orchestration in Fallow, building on 002 agent sessions.
Agents running in Fallow sessions can coordinate with each other: post and read messages on a
shared board, wait for events (for example a PR being merged, signalled by the agent that checked
it), hand tasks to each other, and run a queue of tasks handed to agents in order. Fallow exposes
this to agents through the Model Context Protocol (MCP) over a local, non-network channel, so agent
CLIs need no changes; no new protocol is invented. The user stays in control: nothing is typed into
a session without the user's approval unless the user granted a scoped standing permission; every
cross-agent action is visible and logged in memory; messages are labelled with their sender to
limit prompt-injection between agents. Requires a constitution amendment (Principles IV and VII)."

**Depends on**: feature 002 (agent sessions, projects, profiles). Everything here acts only on
sessions that Fallow started. Agents started outside Fallow are never contacted.

**Protocol**: Fallow does not define a protocol. It offers its coordination functions to agents as
tools over the Model Context Protocol, which the agent CLIs already support. The agent decides when
to call a tool, as with any other tool it has.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Agents leave messages for each other on a board (Priority: P1)

Two agents work on related parts of a system. Agent A finishes the API change and posts "API
v2 endpoint is in, schema in docs/api.md". Agent B, working on the client, reads the board when it
next checks and picks up the change. The engineer sees every message on the board in Fallow, with
who sent it and when.

**Why this priority**: A shared board is the smallest useful form of coordination. Agents can
leave information for each other without anything being typed into a session, so it needs no
approval and carries the least risk.

**Independent Test**: Start two sessions with orchestration. Ask agent A to post a message and
agent B to read the board. Verify B reports A's message, and that the board view shows the message
with A's session, project and profile as sender.

**Acceptance Scenarios**:

1. **Given** two sessions are connected to Fallow, **When** agent A posts a message, **Then** it
   appears on the board with A's session name, project, profile and time as sender, set by Fallow.
2. **Given** a message is on the board, **When** agent B reads the board, **Then** B receives the
   message with its sender label and a marker saying it comes from another agent, not from the user.
3. **Given** the board is open in Fallow, **When** any message is posted, **Then** the engineer sees
   it within one second.
4. **Given** an agent tries to post as another sender, **When** the message is stored, **Then** the
   sender is still the posting session; agents cannot choose or change the sender.
5. **Given** the engineer writes on the board from Fallow, **When** agents read it, **Then** the
   message is labelled as coming from the user.
6. **Given** a project has its own board and there is a board shared by all projects, **When** an
   agent posts, **Then** it chooses one of the two and the message appears only there.

---

### User Story 2 - An agent waits for an event that another agent signals (Priority: P2)

Agent B must merge a feature branch only after PR #123 is merged. Agent A watches that PR with its
own tools. B tells Fallow it waits for "pr-123-merged". When A sees the PR merged, it signals
"pr-123-merged". B is told and continues. The engineer sees which agents wait for which events.

**Why this priority**: This is the engineer's example of agents checking in on each other. It
builds on the board, and it is the first story where one agent's action changes what another
agent does.

**Independent Test**: Session B waits for an event; session A signals it. Verify B learns of it,
that the waits and signals are shown in Fallow, and that a wait with a time limit ends with
"timed out" when nothing is signalled.

**Acceptance Scenarios**:

1. **Given** agent B waits for an event that has not been signalled, **When** agent A signals it,
   **Then** B's wait ends with the event, its sender and any message attached.
2. **Given** an event was already signalled, **When** an agent starts waiting for it, **Then** the
   wait ends at once with the earlier signal.
3. **Given** an agent waits with a time limit, **When** the limit passes with no signal, **Then** the
   wait ends with "timed out".
4. **Given** an agent is waiting, **When** the engineer looks at the session overview (002), **Then**
   the session shows "waiting for <event>" and how long it has waited.
5. **Given** an agent is idle at its prompt (not inside a tool call) when the event it registered
   interest in is signalled, **When** the event arrives, **Then** Fallow offers to deliver a wake-up
   message into that session, under the delivery rules of User Story 3.
6. **Given** the engineer signals an event from Fallow, **When** agents wait for it, **Then** they
   are told it came from the user.

---

### User Story 3 - Deliver a message into another agent's session, with the engineer's approval (Priority: P3)

An agent asks Fallow to deliver a message into another session, for example "PR #123 is merged,
please merge feature/x now". Because this types text into another agent's input, Fallow asks the
engineer first, showing sender, target and the full text. The engineer can approve once, edit,
reject, or grant a standing permission for this kind of delivery for a limited time.

**Why this priority**: Agents idle at their prompt only act on new input, so "check in on each
other" sometimes means typing into another session. This is the riskiest capability, so it comes
after the safe ones and is always under the engineer's control.

**Independent Test**: Session A requests a delivery to session B. Verify the approval prompt shows
sender, target and text; that rejecting delivers nothing; that approving types exactly the shown
text (with its sender label) into B; and that a standing permission for A→B lets the next delivery
through without a prompt until it expires or is revoked.

**Acceptance Scenarios**:

1. **Given** agent A requests a delivery into session B and no standing permission covers it,
   **When** the request arrives, **Then** Fallow shows an approval prompt with sender, target, and
   the exact text that would be typed, and nothing is typed before the engineer approves.
2. **Given** the approval prompt is shown, **When** the engineer rejects it or it is left
   unanswered for the request's time limit, **Then** nothing is typed and A is told the delivery was
   rejected or expired.
3. **Given** the engineer approves, **When** the text is delivered, **Then** session B receives it as
   a paste, starting with a sender label naming A, its project and profile, and stating that it comes
   from another agent via Fallow.
4. **Given** the engineer grants a standing permission (for example "A may deliver to B for 30
   minutes"), **When** A requests another delivery to B within that time, **Then** it is delivered
   without a prompt, and the delivery is shown in the activity log and as a notice.
5. **Given** a standing permission exists, **When** the engineer opens the permissions view, **Then**
   every permission is listed with source, target, allowed actions and time left, and each can be
   revoked at once.
6. **Given** a request asks to stop, pause or kill another session, **When** it arrives, **Then** it
   always needs a per-action confirmation (001 FR-032); no standing permission can cover it.
7. **Given** session B has focus and the engineer is typing in it, **When** an approved delivery for
   B arrives, **Then** it waits until the engineer leaves the session or pauses typing for the
   delivery delay, so it never mixes with the engineer's keystrokes.

---

### User Story 4 - Hand tasks to agents and run a queue (Priority: P4)

The engineer (or an agent, with approval) writes tasks such as "merge feature/x into main once
pr-123-merged" and assigns them to a session, or to "the next free agent of profile Claude Code in
project api". Tasks wait in an ordered queue. When the target is free, and the task's preconditions
are met, the task is handed over under the delivery rules of User Story 3. The agent marks the task
done or failed, with a note; the next task follows.

**Why this priority**: A queue turns individual hand-offs into a plan the engineer can set up once
and watch. It builds on all three earlier stories.

**Independent Test**: Queue three tasks for one session, the second with a precondition event.
Verify the first is handed over, that the second waits for its event even after the first is done,
that marking tasks done moves the queue, and that the queue view shows every task's state.

**Acceptance Scenarios**:

1. **Given** the engineer creates a task with a target, **When** it is saved, **Then** it appears in
   the queue with state "queued".
2. **Given** a queued task's target is free (its session needs attention or is idle at its prompt)
   and its preconditions are met, **When** the queue advances, **Then** the task is handed over under
   the delivery rules of User Story 3 and its state becomes "in progress".
3. **Given** a task has a precondition event, **When** the event has not been signalled, **Then** the
   task stays "blocked" and shows which event it waits for.
4. **Given** an agent reports a task done or failed, **When** the report arrives, **Then** the task's
   state, note and time are shown and the next task for that target becomes eligible.
5. **Given** an agent creates a task for another session, **When** it is created, **Then** it needs the
   engineer's approval before it enters the queue unless a standing permission covers task creation
   for that target.
6. **Given** tasks are queued, **When** the engineer reorders, edits, pauses or cancels them, **Then**
   the queue reflects it at once and cancelled tasks are never handed over.
7. **Given** a task is assigned to "next free agent of profile P in project X", **When** no such
   session is free, **Then** it waits; Fallow does not start a new session for it unless the engineer
   approves (see User Story 5).

---

### User Story 5 - An agent asks Fallow to start another agent (Priority: P5)

An orchestrating agent asks Fallow to start a new session (profile, project, first task). Fallow
asks the engineer, who approves or rejects; a standing permission can allow it up to a set number of
sessions.

**Why this priority**: It completes self-organising workflows but is the furthest from the core and
carries resource risk, so it comes last.

**Independent Test**: An agent requests a new session. Verify the approval prompt, that approval
starts the session as in 002 with the first task queued for it, and that the session limit of a
standing permission is enforced.

**Acceptance Scenarios**:

1. **Given** an agent requests a new session, **When** no standing permission covers it, **Then**
   the engineer sees profile, project and first task, and nothing starts before approval.
2. **Given** a standing permission allows up to N new sessions, **When** an agent requests one more
   than N, **Then** the request needs approval again.
3. **Given** the session limit of 002 (FR-007) or the memory budget would be exceeded, **When** a
   start is requested, **Then** Fallow refuses it and says why.

### Edge Cases

- **A message contains instructions aimed at another agent** (prompt injection from a malicious
  file the sender read): Fallow cannot judge content. Mitigations: every message carries a sender
  label set by Fallow; nothing is typed into a session without approval or a scoped, time-limited,
  visible permission; destructive actions always need per-action confirmation; message size and rate
  are limited; the engineer can revoke all permissions at once.
- **The target session exits while a delivery or task is pending**: the request fails with "target
  exited"; a task returns to "queued" if its target was "next free agent", otherwise "failed".
- **Fallow quits**: board, events, tasks, permissions and the activity log are kept in memory only and
  are gone; sessions stop as in 002.
- **An agent does not support the tool connection**: its session works as in 002 but cannot take part
  in orchestration; the overview says so.
- **A process that is not a Fallow session tries to connect**: it is refused; only sessions Fallow
  started can connect, each only as itself.
- **Flooding**: an agent posting or requesting faster than the rate limit is slowed and the engineer is
  told; the board keeps the newest messages up to its limit.
- **Two agents wait for each other** (deadlock): Fallow shows both waits in the overview; it does not
  break them itself. Waits with time limits end on their own.
- **Engineer is away**: approval prompts queue up; each request has a time limit after which it
  expires and the requesting agent is told.

## Requirements *(mandatory)*

### Functional Requirements

**Connection**

- **FR-001**: Fallow MUST offer its coordination functions to agents in its sessions as tools over the
  Model Context Protocol, so that agent CLIs which support that protocol can use them without code
  changes.
- **FR-002**: The connection between an agent and Fallow MUST be local to the machine and MUST NOT use
  any network interface, including the loopback address.
- **FR-003**: Only sessions that Fallow started MAY connect, each only as itself. Fallow MUST refuse any
  other process and MUST set the sender of everything an agent does from the session's identity.
- **FR-004**: For built-in profiles that support it, Fallow MUST connect the agent automatically when
  the session starts, without changing the user's own agent configuration files. For other agents,
  help MUST show the one-step setup.
- **FR-005**: The coordination functions MUST start only when the first session that uses them
  starts. They MUST NOT start with Fallow or delay its first frame.

**Board**

- **FR-006**: Agents and the engineer MUST be able to post and read messages on a board per project
  and on one board shared by all projects.
- **FR-007**: Every message MUST carry a sender label (session, project, profile, or "user") and a
  time, both set by Fallow. Agents reading messages MUST receive the label and a marker that the
  message comes from another agent or the user.
- **FR-008**: Messages MUST be limited in size (default 8 KB) and in rate per session (default 30 per
  minute). Each board keeps at most its newest 1,000 messages.

**Events**

- **FR-009**: Agents and the engineer MUST be able to signal named events with an optional message,
  and agents MUST be able to wait for an event with an optional time limit. A wait for an event that
  was already signalled ends at once.
- **FR-010**: The session overview (002 FR-008) MUST show each waiting session's event and wait time.

**Delivery into sessions**

- **FR-011**: An agent MUST be able to request that text be delivered into another session. Nothing
  MAY be typed into a session unless the engineer approved that delivery or a standing permission
  covers it.
- **FR-012**: The approval prompt MUST show sender, target and the exact text, and offer approve,
  edit then approve, reject, and grant a standing permission. Unanswered requests expire after a time
  limit (default 10 minutes) and the requester is told.
- **FR-013**: Delivered text MUST start with a sender label naming the sending session, its project
  and profile, and stating it comes from another agent via Fallow. It MUST arrive as a paste.
- **FR-014**: A delivery MUST NOT be typed while the engineer is typing in the target session; it waits
  until the engineer leaves the session or stops typing for the delivery delay (default 3 s).

**Standing permissions**

- **FR-015**: The engineer MUST be able to grant standing permissions scoped by source (a session, or
  any session of a project), target (a session, or any session of a project), action (deliver
  messages, create tasks, start sessions up to N) and duration (at most 8 hours; default 30 minutes).
- **FR-016**: All standing permissions MUST be visible in one view with time left and MUST be
  revocable individually and all at once. A notice MUST be shown whenever a permission is used.
- **FR-017**: Stopping, pausing, resuming or killing a session MUST always need per-action
  confirmation (001 FR-032, FR-033). No standing permission MAY cover it.

**Tasks and queue**

- **FR-018**: The engineer, and agents subject to FR-011 and FR-015, MUST be able to create tasks with
  text, a target (a session, or "next free session of profile P in project X"), optional preconditions
  (events), and an order.
- **FR-019**: A task MUST be handed over only when its target is free and its preconditions are met,
  through the delivery rules (FR-011 to FR-014). Task states are queued, blocked, in progress, done,
  failed and cancelled.
- **FR-020**: Agents MUST be able to report a task done or failed with a note. The engineer MUST be
  able to reorder, edit, pause and cancel tasks.
- **FR-021**: An agent MUST be able to request a new session (profile, project, first task); it starts
  only after approval or under a standing permission with a session limit, and never beyond the limits
  of 002 FR-007 and FR-029.

**Visibility and privacy**

- **FR-022**: Every cross-agent action (post, signal, wait, delivery request and outcome, task change,
  permission grant, use and revoke, session start request) MUST be recorded in an activity log that
  the engineer can view and filter by session and project.
- **FR-023**: The board, events, tasks, permissions and activity log MUST be kept in memory only,
  bounded in size (activity log default 5,000 entries, oldest dropped first), never written to disk,
  logged elsewhere or sent anywhere (Principle VII).
- **FR-024**: Fallow MUST NOT make network requests on an agent's behalf. Checks such as "is the PR
  merged" are done by agents with their own tools; Fallow only carries the signal.
- **FR-025**: Every new command MUST be in the command palette and every new key checked against the
  known terminal conflicts (001 FR-062, FR-064).
- **FR-026**: Orchestration MUST work on Linux, macOS and Windows.

### Key Entities

- **Board message**: board (project or shared), sender label (set by Fallow), time, text.
- **Event**: name, scope (project or shared), signalled or not, signal sender and time, optional
  message; the sessions waiting for it.
- **Delivery request**: sender, target session, text, time limit, state (pending, approved, edited,
  rejected, expired, delivered, failed), the permission that covered it if any.
- **Standing permission**: source scope, target scope, allowed actions, session limit (for starts),
  expiry, uses so far.
- **Task**: text, target, preconditions, position, state, assigned session, note, times, creator.
- **Activity entry**: time, actor, action, subject, outcome.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The PR scenario works end to end: agent B waits for "pr-123-merged", agent A signals it,
  and B continues, with the engineer approving at most one delivery, in each of the built-in profiles
  that support the tool connection.
- **SC-002**: A message posted by one agent can be read by another within 1 second, and is visible to
  the engineer within 1 second.
- **SC-003**: In 100% of test cases, nothing is typed into a session without an approval or a matching,
  unexpired standing permission; a CI test drives every request path and checks this.
- **SC-004**: No sender label can be forged: in a test where an agent claims another identity, 100% of
  stored and delivered messages carry the true sender.
- **SC-005**: Revoking all permissions takes one command and takes effect before the next request is
  processed.
- **SC-006**: In a CI test, no listening network socket is opened by Fallow at any time, and the
  canary test of 001 SC-007 finds no board, task or delivery content in any file Fallow writes.
- **SC-007**: With orchestration in use (4 sessions, full board, full activity log), Fallow stays under
  the 150 MB loaded budget, and startup stays under 100 ms because nothing starts before the first
  session that uses it.
- **SC-008**: An engineer can see, in under 10 seconds, which agents wait for what, which tasks are
  queued or blocked, and which permissions are active, from one overview.

## Assumptions

- Specified on branch `001-llm-aware-tui-editor`, where all work happens for now (user instruction).
- The constitution amendment in [constitution-amendment.md](./constitution-amendment.md) is ratified
  before planning. Without it, standing permissions (User Stories 3 to 5) and the local connection
  (FR-001, FR-002) conflict with Principles IV and VII as written.
- The built-in agent CLIs of 002 support the Model Context Protocol and can be pointed at an extra tool
  server for one run without editing their global settings. Planning confirms this per profile.
- Agent-to-agent protocols beyond tool calls (for example A2A) are out of scope; agent CLIs do not
  speak them natively.
- Fallow does not interpret message content, judge whether a message is safe, or run checks for agents
  (no polling of external services; Principle II and VII).
- Orchestration state lives as long as the Fallow process, matching 002 (sessions never outlive
  Fallow).
- Desktop notifications remain out of scope; notices are inside Fallow.
