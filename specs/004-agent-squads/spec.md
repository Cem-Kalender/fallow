# Feature Specification: Agent Squads and Coordination Mode

**Feature Branch**: `001-llm-aware-tui-editor` (no separate branch; see Assumptions)

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "Agent squads and coordination mode: let a project's agent sessions
work together as a squad in a time-limited burst, led by the engineer's own session, under a
declared workflow that Fallow enforces instead of trusting agents. Builds on 003. Principle: any
step can be incomplete or wrong, so progress needs independent evidence; governance is enforced by
the workflow, not by agents' good faith." (Full description: P1 coordination burst, P2 roles
enforced by Fallow, P3 squad definition, P4 gate sessions, P5 evidence and repair, P6 review and
publishing; ideas adapted from "Building Reliable Coding-Agent Squads", antaoalmada.dev.)

**Depends on**: feature 003 (board, events, deliveries, task queue, session starts, standing
permissions, verified session identity) and 002 (sessions, worktrees, profiles).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A coordination burst led by the engineer's session (Priority: P1)

The engineer asks their Claude Code session to build a login page with a few helper agents.
Instead of approving every hand-off, they start a coordination burst for the project, for four
hours, from that session. That session becomes the lead. Every session in the burst receives a
briefing from Fallow for its role: which tools exist, how to report work, how to check in, and the
rules. The agents start helpers, hand out tasks and message each other without prompts. The
engineer watches, can step in anywhere, and the burst ends by itself after four hours.

**Why this priority**: It removes the approval friction found in the 003 manual run while keeping
the limits of constitution IV. It is useful on its own, before roles or squads exist.

**Independent Test**: Start a burst from a session for one hour; have it start a second session and
hand it a task; verify nothing prompted, every use was announced, the Permissions view shows the
burst with its time left, and after the burst ends the next request prompts again.

**Acceptance Scenarios**:

1. **Given** a session in a project is selected, **When** the engineer runs "Coordinate this
   project" with a duration, **Then** Fallow grants the project-wide standing permission (deliver,
   create tasks, start sessions up to the set number) for that duration, marks the session as the
   lead, and shows the burst in the Permissions view and the session list.
2. **Given** the engineer starts the first session of a project, **When** the session starts,
   **Then** Fallow offers to start a burst (once per project per run), with a choice not to ask again
   for this project.
3. **Given** Fallow is launched with a coordinate option and a duration, **When** the first session
   of that project starts, **Then** the burst starts with that session as the lead.
4. **Given** a duration over 8 hours is asked for, **When** the burst is started, **Then** it is
   refused with the limit stated; no shorter burst is started silently.
5. **Given** a burst runs, **When** any session joins it (already running or started later),
   **Then** it receives a briefing for its role from Fallow.
6. **Given** a burst runs, **When** it reaches its end or the engineer ends it, **Then** requests
   prompt again, the engineer is told, and a note that coordination ended appears on the project
   board.
7. **Given** a burst runs, **When** the engineer extends it, **Then** a new grant of at most 8 hours
   replaces the old one.
8. **Given** a project uses Spec Kit, **When** the briefing is made, **Then** it describes the Spec
   Kit pipeline (the lead specifies, clarifies, plans and splits the work once; builders implement
   assigned task ranges; reviewers check against the spec; the lead merges after the gates open).
9. **Given** a project does not use Spec Kit, **When** a burst starts, **Then** Fallow offers to let
   the lead install it with the official installer in the lead's own session after the engineer
   agrees; if declined, the briefing describes the same pipeline in plain steps.

---

### User Story 2 - Roles that Fallow enforces (Priority: P2)

The lead starts a backend builder, a frontend builder and a read-only reviewer. Each session has a
role fixed by Fallow: a builder may edit and build, a reviewer may only read; a builder cannot start
sessions; only a reviewer can give the verdict that opens the merge. Every message an agent receives
says which session and role it came from, as set by Fallow.

**Why this priority**: The engineer does not want to rely on agents' good faith. Roles turn
"please don't" into "cannot".

**Independent Test**: Start a builder and a reviewer; have the builder signal the review verdict and
call the session-start tool; verify both are refused; have the reviewer change a file in its
worktree; verify the engineer is warned and the reviewer's verdict no longer counts.

**Acceptance Scenarios**:

1. **Given** a session is started in a role, **When** anything it posts, signals, delivers or reports
   reaches another session, **Then** it carries the session, role, profile and project as set by
   Fallow, and the session list shows the role.
2. **Given** a role does not allow a tool, **When** a session in that role lists or calls it,
   **Then** the tool is not listed and a call is refused.
3. **Given** an event is restricted to a role, **When** a session in another role signals it,
   **Then** the signal is refused.
4. **Given** a task precondition requires a signal from a role, **When** the event is signalled by
   another role, **Then** the precondition stays unmet.
5. **Given** a read-only session's worktree gains changes or commits, **When** Fallow checks it,
   **Then** the engineer is warned and that session's signals stop counting for gates.
6. **Given** a role passes flags that bypass the agent's own permission checks, **When** a session in
   that role starts, **Then** the engineer sees a warning before it starts.
7. **Given** a session is running, **When** the engineer opens its details, **Then** the exact
   command line it was started with is shown.
8. **Given** an agent asks to change its own role, **When** it tries, **Then** there is no way to do
   so; a role is fixed for the session's life.

---

### User Story 3 - Squads declared in the project (Priority: P3)

The engineer keeps a squad file in the project, versioned with the code: the roles, which signals
from which roles open which tasks, what may run in parallel, and what must hold before merging.
During a burst, Fallow enforces it; agents cannot skip a gate or change the rules. Without a squad
file, a built-in squad for the Spec Kit pipeline is used.

**Why this priority**: Explicit, reviewable workflows are what make the squad repeatable; they build
on enforced roles.

**Independent Test**: Write a squad whose merge needs "backend done", "frontend done" and "review
approved"; run a burst; verify the merge task stays blocked until all three are signalled by the
right roles, and that an agent cannot alter the squad during the burst.

**Acceptance Scenarios**:

1. **Given** a project has a squad file, **When** a burst starts, **Then** Fallow reads it, shows the
   roles and gates, and reports problems in it by file and line before the burst starts.
2. **Given** a squad's gate needs signals from given roles, **When** a task depends on that gate,
   **Then** it is handed over only after those roles signalled.
3. **Given** a burst runs, **When** the squad file changes on disk, **Then** the running burst keeps
   the rules it started with; the change applies to the next burst.
4. **Given** no squad file exists, **When** a burst starts, **Then** the built-in squad (lead,
   builders, reviewer) is used and named in the briefing.

---

### User Story 4 - Gates whose result Fallow observes itself (Priority: P4)

Before builders start, a gate runs the project's build and tests on the starting commit (the
baseline). After the builders are done, the same gate runs again. Fallow records the results itself
and signals "gate passed" or "gate failed", so no agent can claim the build is green.

**Why this priority**: Mechanical evidence that no agent can fake is the core of the squad's
reliability.

**Independent Test**: Run a burst on a project whose tests already fail; verify the baseline records
the failure, the final gate is classified as "failures already in the baseline only" when nothing
new breaks, and as "regression" when a builder breaks another test.

**Acceptance Scenarios**:

1. **Given** a squad defines a gate command, **When** the gate runs, **Then** it runs as a visible
   session under the burst's permission, and Fallow signals the outcome from its exit status.
2. **Given** a gate ran on the starting commit, **When** the final gate finishes, **Then** its result
   is classified as pass, failures already in the baseline only, or regression.
3. **Given** an agent signals a gate's event itself, **When** it tries, **Then** it is refused; only
   Fallow signals gates.

---

### User Story 5 - Evidence with every report, and capped repair loops (Priority: P5)

A builder reporting a task as done must include evidence: the commit, the commands it ran with their
results, and the files it changed. Fallow checks what it can (the commit exists on that session's
branch). A failed gate or a review asking for changes becomes a task for the builder who owns that
part, and after a set number of rounds Fallow stops and asks the engineer.

**Why this priority**: It turns a chain of claims into a chain of evidence and prevents endless
loops.

**Independent Test**: Report a task with a commit that is not on the session's branch and verify it
is refused; make the review ask for changes three times with a cap of two and verify the third round
goes to the engineer instead.

**Acceptance Scenarios**:

1. **Given** a builder reports a task done, **When** the report has no evidence, or names a commit
   not on its own branch, **Then** the report is refused with the reason.
2. **Given** a task is handed to the next role, **When** it is typed, **Then** it carries the evidence
   of the reports it depends on.
3. **Given** a gate failed or a review asked for changes, **When** that happens, **Then** a repair
   task goes to the builder who owns the part, without a prompt during the burst.
4. **Given** the repair cap is reached, **When** another repair would start, **Then** Fallow stops the
   loop and asks the engineer.
5. **Given** every session in the burst is waiting on another, **When** that persists, **Then**
   Fallow tells the engineer which sessions wait for what.

---

### User Story 6 - Independent review and a human checkpoint before merging (Priority: P6)

The squad has two reviewers on different models. The merge needs both to approve; if they disagree,
the engineer sees both opinions. Even during a burst, the final merge or pull request waits for the
engineer's approval unless the squad says otherwise.

**Why this priority**: Independent reviewers catch different mistakes, and the engineer stays the
one who ships.

**Independent Test**: Run a squad with two reviewers; have one approve and one ask for changes;
verify the merge stays blocked and the engineer sees both verdicts; then approve both and verify the
merge waits for the engineer.

**Acceptance Scenarios**:

1. **Given** a squad has two reviewer roles, **When** one approves and the other asks for changes,
   **Then** the merge stays blocked and the engineer is shown both verdicts with their notes.
2. **Given** all gates are open, **When** the lead asks to merge or open a pull request, **Then** the
   request waits for the engineer's approval, even during a burst, unless the squad sets merging to
   automatic.

### Edge Cases

- **A burst is started from a session in another project's folder**: the burst covers the lead's
  project only; sessions of other projects are untouched.
- **The lead exits during a burst**: the burst continues until it ends or the engineer ends it; no
  other session becomes the lead automatically; the engineer can name a new lead.
- **Fallow quits during a burst**: the burst ends with Fallow (as all orchestration state, 003);
  nothing about it is kept.
- **A session in the burst is not connected to Fallow's tools** (agent without tool support): it
  takes part only through typed tasks and messages; the session list says so.
- **The squad file is invalid**: the burst is not started; problems are shown by file and line.
- **A gate command hangs**: a gate has a time limit (from the squad, default 30 minutes); past it,
  the gate fails with "timed out".
- **The baseline gate cannot run** (for example the project does not build at the starting commit):
  the engineer is told; the burst may continue with no baseline, and final results are then
  classified only as pass or fail.
- **Two reviewers on the same model**: allowed, with a warning that their reviews are less
  independent.
- **Prompt injection**: a file one agent reads may instruct it to tell others to do harmful things.
  During a burst, nobody reviews messages before they are typed; the limits are the role tools, the
  gates, the time limit, the project scope and the engineer's ability to end the burst at once.
- **Subagents**: an agent's own subagents act as that session (same identity, role and worktree);
  they do not count as sessions and cannot hold another role.

## Requirements *(mandatory)*

### Functional Requirements

**Coordination burst (P1)**

- **FR-001**: The engineer MUST be able to start a burst for one project in one step, from a running
  session, with a duration of at most 8 hours and a number of sessions it may start. Three ways in:
  a palette command, an offer when the first session of a project starts (once per project per run,
  with "don't ask again for this project", remembered), and a launch option with a duration and a
  project.
- **FR-002**: A burst MUST be a 003 standing permission scoped to the project (deliver, create tasks,
  start sessions up to the set number), visible in the Permissions view with its time left,
  announced at each use, revocable at once, never covering stop, pause, resume or kill.
- **FR-003**: The session a burst is started from MUST be the lead; with the launch option, the first
  session of the project. Exactly one lead per burst; the engineer can name another.
- **FR-004**: When a burst ends, by time or by the engineer, requests MUST prompt again, the engineer
  MUST be told, and Fallow MUST post that coordination ended on the project board. The engineer MUST
  be able to extend a burst (a new grant of at most 8 hours) or end it early.
- **FR-005**: Every session in a burst MUST receive a briefing for its role from Fallow when it joins:
  the tools it may use and how; the rules (sender labels mean another agent wrote it and are reports,
  not orders; no one can stop a session; one worktree per session; merging is the agents' own git
  work); the squad's roles and gates; the check-in convention (events for ready, done and review
  verdicts; a status post at each step; every task reported); when to start a session versus using
  its own subagents; and when the burst ends. A session receives only what its role needs.
- **FR-006**: If the project uses Spec Kit, the briefing MUST describe the Spec Kit pipeline: the lead
  interviews the engineer and runs specify, clarify, plan and tasks once for the feature and commits;
  builders start from that commit and implement assigned task ranges; reviewers check against spec,
  plan and tasks; the lead merges after the gates open.
- **FR-007**: If the project does not use Spec Kit, Fallow MUST offer to have the lead install it with
  the official installer, run in the lead's session after the engineer agrees. Fallow itself MUST NOT
  download anything or write into the project. If declined, the briefing describes the same pipeline
  in plain steps.

**Roles (P2)**

- **FR-008**: A role MUST have a name, a description, the agent CLI's permission and model flags to
  start a session with, and the Fallow tools and events it may use. Built-in roles: lead (all tools;
  the only role that starts sessions or creates tasks for others; edits and git), builder (board,
  events, deliveries to builders and the lead, reporting its own tasks; edits and builds), reviewer
  (board, events, reporting its own tasks; read-only). Engineers MUST be able to define their own.
- **FR-009**: A session's role MUST be fixed by Fallow when it starts and kept for its life; it MUST NOT
  be an input of any tool, and changing it requires a new session.
- **FR-010**: Everything another session receives (board messages, signals, task texts, deliveries,
  session lists) MUST carry the sender's session, role, profile and project as set by Fallow.
- **FR-011**: Tools a role may not use MUST be absent from that session's tool list and refused if
  called.
- **FR-012**: Events MUST be restrictable to roles; a signal of a restricted event by another role MUST
  be refused. Task preconditions MUST be able to require a signal from a given role.
- **FR-013**: Fallow MUST check that a read-only role's worktree has no changes and no commits of its
  own when that session signals, and at least every few minutes during a burst; on a violation it MUST
  warn the engineer and stop counting that session's signals for gates.
- **FR-014**: Fallow MUST warn before starting a session whose role's flags bypass the agent's own
  permission checks, and MUST show each session's actual start command in its details.
- **FR-015**: The session list and the session-list tool MUST show each session's role.

**Squads (P3)**

- **FR-016**: A squad MUST be definable in a file inside the project, which Fallow reads and never
  writes. It lists roles (by name, built-in or defined there), gates (which role's signals open which
  tasks), what may run in parallel, the repair cap, gate commands and time limits, and the merge
  conditions.
- **FR-017**: Problems in a squad file MUST be reported by file and line, and a burst with an invalid
  squad MUST NOT start.
- **FR-018**: A running burst MUST keep the squad rules it started with; agents MUST NOT be able to
  change gates, roles or merge conditions during a burst.
- **FR-019**: Without a squad file, a built-in squad implementing the Spec Kit pipeline (lead,
  builders, reviewer; merge after builders' done signals and the reviewer's approval) MUST be used.

**Gates (P4)**

- **FR-020**: A gate MUST run as a visible session (a command, not an agent) started under the burst's
  permission. Fallow MUST signal its outcome itself from the command's exit status; agents MUST NOT be
  able to signal gate events.
- **FR-021**: A baseline gate MUST be able to run on the starting commit before builders begin; the
  final gate's result MUST be classified against it as pass, failures already in the baseline only, or
  regression.
- **FR-022**: Gates MUST have a time limit (default 30 minutes); past it, the gate fails as "timed
  out".

**Evidence and repair (P5)**

- **FR-023**: Reporting a task done during a burst MUST include evidence: the commit, commands run with
  their results, and files changed. Fallow MUST refuse a report whose commit is not on the reporting
  session's branch.
- **FR-024**: A task handed to the next role MUST carry the evidence of the reports it depends on.
- **FR-025**: A failed gate or a review asking for changes MUST create a repair task for the builder
  who owns the part; after the squad's repair cap (default 2 rounds), Fallow MUST stop the loop and ask
  the engineer.
- **FR-026**: When every session in a burst waits on another for longer than a set time (default 10
  minutes), Fallow MUST tell the engineer which sessions wait for what.

**Review and publishing (P6)**

- **FR-027**: A squad MUST be able to require approval from two reviewer roles; disagreement MUST keep
  the merge blocked and show the engineer both verdicts with their notes. Reviewers on the same model
  MUST produce a warning.
- **FR-028**: By default, the final merge or pull request MUST wait for the engineer's approval even
  during a burst; a squad MAY set merging to automatic.

**Across the feature**

- **FR-029**: Everything about a burst (grants, roles, gate results, evidence, repair counts) MUST be
  recorded in the 003 activity log and kept in memory only (constitution VII).
- **FR-030**: Fallow MUST NOT run any command, start any session or touch the repository except as the
  engineer's burst allows (sessions, gate sessions) or for read-only git checks of the burst's own
  worktrees (FR-013, FR-023).
- **FR-031**: Every new command MUST be in the palette and every new key checked against the known
  terminal conflicts; the feature MUST work on Linux, macOS and Windows.

### Key Entities

- **Burst**: project, lead session, start, end, session allowance, squad in force, state (running,
  ended, ended early).
- **Role**: name, description, agent profile, permission and model flags, allowed Fallow tools,
  events it may signal, read-only or not.
- **Squad**: roles, gates, parallel groups, repair cap, gate commands with time limits, merge
  conditions, merge mode (approval or automatic), source file.
- **Gate**: name, command, time limit, baseline result, final result, classification.
- **Evidence**: task, commit, commands with results, files changed, verified facts (commit on branch).
- **Repair loop**: part, owning builder, rounds so far, cap.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An engineer can start a burst for a project in at most 2 steps from a running session,
  and agents can then start up to the allowed sessions and hand off tasks with zero approval prompts.
- **SC-002**: In 100% of test cases, a session cannot use a tool its role does not allow, signal an
  event restricted to another role, change its role, or signal a gate event.
- **SC-003**: In 100% of test cases, the merge task is not handed over before every gate it depends on
  has been opened by the required roles or by Fallow's own gate result.
- **SC-004**: A gate's result is always the command's real outcome: no test case lets an agent make a
  failing gate count as passed.
- **SC-005**: With a baseline that already fails, the final gate is classified correctly in 100% of
  the test cases (pass, baseline-only failures, regression).
- **SC-006**: A repair loop never exceeds its cap; the engineer is asked within one minute of reaching
  it.
- **SC-007**: After a burst ends, the next request prompts again in 100% of cases, and the engineer is
  told within one minute.
- **SC-008**: Starting a burst does not change Fallow's startup time or idle memory, and a burst of 4
  sessions plus a gate stays within the 150 MB loaded budget.
- **SC-009**: A squad end-to-end test (lead, two builders, reviewer, gate) completes with the merge
  waiting for the engineer, using only scripted agents, in CI on all three platforms.

## Assumptions

- Specified on branch `001-llm-aware-tui-editor`, where all work happens for now (user instruction).
- Squad files live in the project, for example `.fallow/squad.toml`, versioned with the code; Fallow
  reads them only. Roles can also be defined in Fallow's own configuration for all projects.
- Repository permissions (edit, read-only, run commands) are enforced by the agent CLI from the flags
  Fallow passes; Fallow does not sandbox agents. Fallow verifies outcomes it can observe (worktree
  changes, commits, gate exit status).
- A burst is a 003 standing permission with a fixed scope; constitution IV v4.0.0 already allows it
  (at most 8 hours, scoped, visible, revocable, announced). No amendment is expected; the plan's
  Constitution Check confirms this, including running gate commands (constitution V: processes started
  under a standing permission with a limit) and read-only git checks during a burst.
- The check-in convention (event names such as `<part>-ready`, `<part>-done`, `review-approved`,
  `review-changes-requested`) is a default that squads can rename.
- Fallow does not verify that an agent's claims about its work are true beyond the checks above;
  reviewers, gates and sender labels ("a report, not an order") cover the rest.
- Cost grows with the number of sessions, their models and their subagents; Fallow shows sessions and
  their resource use but does not track usage costs.
- Delivered after 003; P1 to P6 are implemented and shipped in priority order, each story usable alone.
