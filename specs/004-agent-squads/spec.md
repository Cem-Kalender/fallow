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

## Clarifications

### Session 2026-10-26

- Q: When all checks have passed, who performs the final merge? → A: Both ways are supported per squad. Publishing as a draft pull request (the lead opens it; the engineer merges on the platform, enforced there by branch protection) is the built-in default when the project has a remote; otherwise no agent merges and Fallow merges the exact gated commit locally when the engineer presses merge.
- Q: When a gate fails or a reviewer asks for changes, how does Fallow know which builder gets the repair task? → A: The owner of a part is the session its task was handed to; Fallow records it, and repairs for that part go back to that session. Failures not tied to one part go to the lead.
- Q: How does a squad say which command a gate runs across Linux, macOS and Windows? → A: One command per gate, with optional overrides for Windows, macOS or Linux.
- Q: How are a role's permissions given for agents other than Claude Code? → A: Roles use permission levels (read-only, edit, edit and run commands); Fallow ships flags for each built-in agent checked against its documentation; a squad may override them; an agent without a mapping cannot take a restricted role until the squad gives its flags.
- Q: How many sessions may a burst start? → A: One number set when starting the burst: how many sessions the lead may start (default 3, at most the session limit); the existing session limit and memory guard still apply.

### Session 2026-10-26 (after the first manual run)

- Q: Who is told what happens in a squad, and who do agents ask when they need a decision? → A: Everything goes through the lead. Fallow reports every event that needs action to the lead, not to the engineer. The other sessions work autonomously; when they need clarification they ask the lead, never the engineer, and only the lead consults the engineer. Autonomy comes first: briefings make asking possible without prompting it. The engineer keeps the decisions that are theirs (the merge, confirming a role that bypasses an agent's checks, a repair loop past its cap), and the lead brings those to them.
- Q: How autonomous is the squad? → A: Fully, once the spec is agreed. The engineer defines the spec with the lead; from then on the squad builds it on its own, and the lead manages and monitors the squad until the work is actually finished: every task done with evidence, the builders' work integrated in the lead's commit, the gates passing on that commit, and the reviewers' approval. Only then does the lead report to the engineer, with the merge ready.

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
the engineer sees both opinions. No agent merges: with a remote, the lead publishes the work as a
draft pull request and the engineer merges it on the platform; without one, Fallow shows "ready to
merge" with the evidence and merges the exact commit the gates passed when the engineer presses
merge.

**Why this priority**: Independent reviewers catch different mistakes, and the engineer stays the
one who ships.

**Independent Test**: Run a squad with two reviewers; have one approve and one ask for changes;
verify the merge stays blocked and the engineer sees both verdicts; then approve both and verify the
merge waits for the engineer.

**Acceptance Scenarios**:

1. **Given** a squad has two reviewer roles, **When** one approves and the other asks for changes,
   **Then** the merge stays blocked and the engineer is shown both verdicts with their notes.
2. **Given** all gates are open and the squad publishes as a draft pull request, **When** the lead
   publishes, **Then** it opens a draft pull request for the gated commit and reports its address;
   the engineer merges on the platform.
3. **Given** all gates are open and the squad merges locally, **When** the engineer presses merge in
   Fallow, **Then** Fallow merges exactly the commit the gates passed into the target branch; no
   agent is handed a merge task.
4. **Given** a squad merges locally, **When** a commit appears on the target branch during the burst
   that the engineer did not merge through Fallow, **Then** the engineer is warned.

### User Story 7 - The lead drives the squad (Priority: P7)

The engineer defines the spec with the lead (with Spec Kit: specify and clarify). From then on the
squad builds it on its own, and the lead manages it: Fallow tells the lead about everything that needs action (a task reported, a
review verdict, a gate result, a repair, a session stuck waiting for a permission or asking a
question, everyone idle), so the lead can merge the builders' work into its own branch, hand out the
next task and ask for another review. Builders and reviewers work on their own; when they need
clarification they ask the lead, and only the lead consults the engineer. The lead
keeps going until the work is finished by evidence Fallow can check, then tells the engineer that the
merge is ready.

**Why this priority**: Without it the engineer has to route every result between sessions by hand,
which is the babysitting a squad should remove. Found in the first manual run of US1 to US6.

**Independent Test**: Agree a small spec with the lead for two builders and a reviewer, then do
nothing until Fallow says the merge is ready. Count the engineer's interventions (target: none
besides the spec, the merge, and decisions brought to them by the lead).

**Acceptance Scenarios**:

1. **Given** a burst with a lead, two builders and a reviewer, **When** a builder reports its task
   done, **Then** Fallow tells the lead at once (who, which part, the evidence, what the merge still
   needs), even if the lead is idle, and whoever created the task.
2. **Given** Fallow created a repair task, **When** its owner reports it, **Then** the lead is told
   the same way, so the repaired work is integrated.
3. **Given** a reviewer gives a verdict, a gate finishes, a repair loop stops at its cap, or every
   session waits, **When** it happens, **Then** the lead is told with what it means for the merge,
   and the engineer is told only when a decision is theirs.
4. **Given** a builder or reviewer needs clarification, **When** it asks the lead, **Then** the question goes to the lead as a message labelled as a question from
   that session, and the lead's answer goes back to it; it cannot ask the engineer directly.
5. **Given** a builder or reviewer is stuck at its agent's own permission prompt, **When** Fallow
   sees the prompt, **Then** the lead is told which session is stuck and on what; roles that edit
   and run commands are started so that their agent does not stop to ask for the actions their level
   allows.
6. **Given** the spec is agreed and work is open, **When** no event has reached the lead for a set
   time, **Then** Fallow sends the lead a short status of what is open, so the lead never waits
   forever.
7. **Given** every merge condition holds on the lead's integrated commit, **When** that happens,
   **Then** Fallow tells the lead and the engineer that the merge is ready, naming the commit.

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
- **The baseline finds no tests** (a new project): it is shown as "no tests ran: nothing to
  compare", builders begin, and final results are classified only as pass or fail.
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
- **Many events at once** (two builders report while a gate finishes): updates to the lead are
  grouped into one message, at most one every few seconds, so the lead is not flooded.
- **The lead is busy or its session exited**: updates wait and are typed when the lead is idle;
  without a running lead, they go to the engineer as notices, who can name a new lead.
- **A question with no lead to answer it**: it goes to the engineer.

## Requirements *(mandatory)*

### Functional Requirements

**Coordination burst (P1)**

- **FR-001**: The engineer MUST be able to start a burst for one project in one step, from a running
  session, with a duration of at most 8 hours and a number of sessions it may start (default 3, at
  most the session limit; the dialog states that each session has its own usage costs). The global
  session limit and memory guard of 003 still apply on top. Three ways in:
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

- **FR-008**: A role MUST have a name, a description, a permission level (read-only, edit, or edit and
  run commands), an optional model, and the Fallow tools and events it may use. Fallow MUST ship, for
  each built-in agent profile, the agent CLI flags for each level, checked against that agent's
  documentation; a squad MAY give its own flags per agent instead. An agent with no flags for a
  restricted level MUST NOT be started in such a role, and Fallow MUST say why. Built-in roles: lead (all tools;
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
- **FR-016a**: Fallow MUST offer a command that creates a commented squad file in the project, on the
  engineer's explicit request only: the built-in squad written out, a gate with the test command
  guessed from the project's files (or a commented placeholder), and a baseline only when the
  project already has code. It opens the file in the editor, never overwrites an existing one and
  never commits it. (Added after the first manual run: without a starting point the format had to
  be known by heart.)
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
- **FR-022a**: A gate MUST have one command, with optional overrides for Windows, macOS and Linux; the
  override for the running OS is used when present. The command actually run MUST be shown in the gate
  session's details.

**Evidence and repair (P5)**

- **FR-023**: Reporting a task done during a burst MUST include evidence: the commit, commands run with
  their results, and files changed. Fallow MUST refuse a report whose commit is not on the reporting
  session's branch, and compare the listed files with the changes since that session started (not
  since the burst started). Read-only roles report through their verdict and need no evidence; a
  verdict completes the reviewer's open review task.
- **FR-024**: A task handed to the next role MUST carry the evidence of the reports it depends on.
- **FR-025**: A failed gate or a review asking for changes MUST count a repair round for its part and
  be told to the lead with the part's owner; the lead rules on anything left open and hands the
  repair out (a task for that part is the round's repair). After the squad's repair cap (default 2
  rounds), Fallow MUST stop the loop, refuse tasks for that part, and ask the engineer. (Changed
  after the second manual run: repair tasks Fallow created itself crossed the lead's rulings.)
- **FR-025a**: The owner of a part MUST be the session its task was handed to, as Fallow recorded it at
  hand-over; a review verdict names the part (task) it concerns. A failure not tied to one part (for
  example a final gate failing) MUST go to the lead, which decides whom to hand the repair to.
- **FR-026**: When every session in a burst waits on another for longer than a set time (default 10
  minutes), Fallow MUST tell the lead which sessions wait for what (US7; the engineer when there
  is no running lead).

**Review and publishing (P6)**

- **FR-027**: A squad MUST be able to require approval from two reviewer roles; disagreement MUST keep
  the merge blocked and show the engineer both verdicts with their notes. Reviewers on the same model
  MUST produce a warning.
- **FR-028**: No agent MUST be handed the final merge. A squad chooses how work is published:
  "draft pull request" (the lead opens a draft pull request for the gated commit; the engineer
  merges on the platform; the built-in default when the project has a remote) or "local merge"
  (Fallow shows "ready to merge" with the evidence and merges exactly the gated commit into the
  target branch when the engineer presses merge; the default without a remote). A squad MAY set
  local merging to automatic.
- **FR-028a**: With local merging, Fallow MUST warn the engineer when the target branch gains a
  commit during the burst that was not merged through Fallow. With draft pull requests, Fallow MUST
  state in the briefing and in the squad view that enforcement depends on the platform's branch
  protection, which Fallow cannot check.

**The lead drives the squad (P7)**

- **FR-032**: During a burst Fallow MUST tell the lead, as a message typed into its session, about
  every event that needs action: a task reported done or failed (including tasks Fallow created), a
  review verdict, a final gate result, a repair task created or a loop stopped at its cap, a session
  waiting at its agent's permission prompt, a question from a session, everyone waiting (FR-026),
  and the merge becoming ready. Each message MUST say what it means for the merge.
- **FR-033**: Messages to the lead MUST be grouped (at most one every 10 seconds) and typed only while
  the lead is idle, in the order the events happened; none may be lost while the lead is busy.
- **FR-034**: Sessions other than the lead MUST consult the lead, not the engineer: Fallow MUST offer
  them a tool to consult the lead, deliver the question to the lead labelled as a question from that
  session, and deliver the lead's answer back; their agents MUST be started without their own "ask
  the user" tool where the agent CLI allows it. Briefings MUST put autonomy first: they say to work
  on their own and that clarifying questions go to the lead, without prompting questions.
- **FR-035**: Roles that edit or run commands MUST be started with flags under which their agent does
  not stop to ask for the actions their level allows, and denies the rest without asking, where the
  agent CLI supports it. When Fallow sees an agent's permission prompt in a session other than the
  lead's, it MUST tell the lead.
- **FR-036**: The engineer's notices during a burst MUST be limited to decisions that are theirs (the
  merge, confirming a role that bypasses an agent's checks, a repair loop past its cap) and to the
  burst's own state (start, ten minutes left, end); the rest goes to the lead.
- **FR-037**: When the lead has been told nothing for a set time (default 10 minutes) while tasks are
  open or the merge is not ready, Fallow MUST send it a short status of what is open.
- **FR-038**: When every merge condition holds, Fallow MUST tell the lead and the engineer that the
  merge is ready, with the commit. The lead's briefing MUST define finished as that state and say
  that, once the spec is agreed, the lead manages the squad on its own until then, consulting the
  engineer only for decisions that are theirs.
- **FR-039**: A burst MUST keep a start for each role its merge conditions need until that role has
  a session; a start that would use a kept start, or one beyond the allowance, MUST wait for the
  engineer's approval instead of being refused. A burst whose allowance is smaller than the roles
  the merge needs MUST be flagged to the engineer when it starts.
- **FR-040**: When the squad waits for something only the engineer can clear (a start waiting for
  approval, or a role the merge needs with no session and no starts left), Fallow MUST tell the
  engineer once and the lead once, instead of repeated status or waiting notices. A quiet-time
  status (FR-037) MUST NOT be repeated unchanged.
- **FR-041**: A `signal:` merge condition MUST stop waiting for a session of its role that exited
  without giving the signal, and the lead MUST be told once; a signal given before the session
  exited still counts, and so does a request for changes: it keeps the merge waiting, and the
  engineer and the lead are told, until the engineer clears it. When no session of the role
  counts any more, the condition is unmet until a new one gives the signal, so exiting never
  skips it. A restarted session counts again.
- **FR-042**: When a member other than the lead exits with unfinished work (a task in progress at
  it, which fails, or one queued for it, which never starts), the lead MUST be told once, naming
  the tasks and their parts; a task for the next free session goes back to the queue by itself.
  A withdrawn task MUST NOT count for a `done:` merge condition, and the condition's reason MUST
  say how many were withdrawn; a failed one counts until it is withdrawn, and with every task
  withdrawn the condition is unmet. Besides its own tasks, the lead MUST be able to withdraw
  Fallow's tasks of its burst and any task of it that failed or waits for a session that exited;
  the engineer's other tasks stay theirs. A session whose task in progress is withdrawn is told.
  A part belongs to the builder it was last handed to (never the lead or a read-only role), so a
  repair goes there; a repair of a part whose owner exited MUST tell the lead to hand it to
  another session.

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
- **Role**: name, description, permission level (read-only, edit, edit and run commands), optional
  model, allowed Fallow tools, events it may signal; the CLI flags come from the agent's mapping for
  that level or from the squad's override.
- **Squad**: roles, gates, parallel groups, repair cap, gate commands with time limits, merge
  conditions, publishing (draft pull request or local merge; local merge on approval or automatic),
  target branch, source file.
- **Gate**: name, command, optional per-OS command overrides, time limit, baseline result, final
  result, classification.
- **Evidence**: task, commit, commands with results, files changed, verified facts (commit on branch).
- **Repair loop**: part (the task it concerns), owning session (recorded at hand-over), rounds so far,
  cap.

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
- **SC-009**: A squad end-to-end test (lead, two builders, reviewer, gate) completes with the local
  merge waiting for the engineer, using only scripted agents, in CI on all three platforms. No agent
  is ever handed a merge task.
- **SC-010**: In the end-to-end test, the lead learns of every report, verdict, gate result and
  repair only from Fallow's messages (it never polls), and the run reaches "merge ready" with no
  input from the engineer besides the goal and the merge.
- **SC-011**: In a manual run with Claude Code (one goal, two builders, a reviewer, a gate), the
  engineer intervenes only for the goal, the merge, and decisions Fallow or the lead brings to them;
  every intervention is counted and recorded.

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
