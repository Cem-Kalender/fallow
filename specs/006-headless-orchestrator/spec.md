# Feature Specification: Headless Orchestrator

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; spec directory `006-headless-orchestrator`)

**Created**: 2026-10-29

**Status**: Draft

**Input**: User description: "Headless orchestrator: replace Fallow's interactive-session coordination (PTY-hosted agent sessions that coordinate by typed messages and MCP, with the engineer routing between them) with a headless orchestrator that owns the build process as code and runs agents as headless calls, plus a local web UI that visualizes the run and lets the engineer steer it." (full description: P1 to P6 as given on 2026-10-29, summarized in the stories below)

## Background

In T050 Q2 (2026-10-29) a squad built a two-part feature under 005's coordination. It got there,
but only with the engineer steering throughout. Agents stayed isolated in interactive sessions,
waiting for typed input. A part that had already reported done could not report follow-up work
after a contract change, because its task was closed. Two parts that both had a criterion `total`
made the review verdict ambiguous. The engineer ended up as the router between sessions.

This feature moves coordination out of the agents and into the orchestrator. The orchestrator owns
the process as an explicit sequence of steps. It runs each step as a non-interactive agent call
and decides the next step from what it has verified itself. A local web page shows the run and
takes the engineer's decisions. The engineer is needed for the plan, the merge, and decisions that
are genuinely theirs.

A clickable mockup of the run view exists ("Fallow Run View", a claude.ai artifact). It shows the
cart task from Q2 with the steering controls described here.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - One task to completion without steering (Priority: P1)

The engineer starts a run for a task in a repository. They agree the plan with a planning agent:
what is built, the acceptance criteria (including cases that must fail), and, for a split, the
parts and contracts. Then they approve it. From then on the orchestrator works through
build → gate → review → repair → ready to merge on its own:
- each step is a fresh non-interactive agent call with a focused instruction, its own working copy
  and fixed rights;
- after each step the orchestrator checks the result itself: the commit exists, the right files
  changed, and the tests pass against a baseline;
- a failed check or review goes back to the builder, a limited number of times.

When the work is verified, the engineer is asked to merge, and merging puts exactly the verified
commit on the target branch. If the orchestrator stops (crash, restart, machine sleep), it resumes
where it was.

**Why this priority**: This is the whole point: a verified result with the engineer involved twice.
It is also the solo mode that T051 compares against a plain session.

**Independent Test**: Run the `search` task from the T050 task set: approve the plan, wait, merge.
Then count interventions (expected: 2), check the merged commit is the gated one, and run the
hidden tests on it.

**Acceptance Scenarios**:

1. **Given** an approved plan, **When** the builder's step ends, **Then** the orchestrator accepts
   its result only after confirming the commit is on the step's branch, the files the part owns
   changed, and the gates pass with no regression against the baseline. Otherwise the step is
   repaired or redone, and the engineer is not asked.
2. **Given** a builder that claims done with a commit that does not exist, or with failing tests,
   **When** the result arrives, **Then** it is rejected with the reason, and the next step is a
   repair.
3. **Given** a review that finds a failing criterion, **When** the repair cap is not reached,
   **Then** a repair goes to the builder with the failed check, followed by the gates and a new
   review. When the cap is reached, the engineer is asked.
4. **Given** a run in any step, **When** the orchestrator is stopped and started again, **Then**
   the run continues from the last completed step without repeating it. The step that was
   interrupted is resumed or redone, and the run's history is intact.
5. **Given** a verified result, **When** the engineer merges, **Then** exactly the verified commit
   lands on the target branch. Nothing lands before that.

---

### User Story 2 - Decisions during a step, without a chat (Priority: P2)

A builder meets a question the plan does not answer: "should Cart get `clear()`?". It asks the
orchestrator and waits. A short planning step decides and commits the decision to the plan or a
contract. The builder continues in the same step with that commit, keeping its context. Agents
never message each other. If the decision changes a contract or criteria, every part it affects is
reopened, including parts already done, and their checks run again.

**Why this priority**: This is the failure that stalled Q2. Without it, autonomy breaks as soon as
the plan is incomplete, which it usually is.

**Independent Test**: In the `cart` task, have the cart-core builder ask about `clear()`.
1. The decision appears as a commit.
2. The builder's result contains that commit.
3. The receipt part, already done, is reopened and checked again.
4. The engineer is not asked, at High autonomy.

**Acceptance Scenarios**:

1. **Given** a builder that asks for a decision, **When** the planning step decides, **Then** the
   decision is committed to the plan or a contract, the builder continues with that commit, and the
   decision appears in the decisions log with who decided and the commit.
2. **Given** a part already reported done, **When** a later decision changes its criteria or a
   contract it builds against, **Then** the part is reopened automatically, takes the change, and
   is verified again before the run can be ready to merge.
3. **Given** two parts whose criteria share an identifier, **When** the reviewer answers, **Then**
   each answer names its part and criterion (`part/id`) and is counted for that part only. Answers
   given in several partial verdicts add up within one review.
4. **Given** a question the planning step cannot decide, or one the autonomy level reserves for
   the engineer, **When** it is raised, **Then** the engineer is asked with options, and only the
   affected part waits.

---

### User Story 3 - See what the agents are doing (Priority: P3)

While a run goes on, the engineer opens a local page and sees:
- where the run is: plan, build, integrate, gate, review, merge;
- what each agent is doing right now, from its own output: files read and edited, commands run and
  their results;
- the decisions taken and their commits;
- the gate results against the baseline;
- the criteria checklist per part, with the evidence for each;
- repairs, time and cost.

**Why this priority**: The engineer gives up watching agents type. They need another way to
understand and trust the run.

**Independent Test**: During a `cart` run, ask someone who did not watch it to explain from the
page alone what each agent did and why the result can be trusted.

**Acceptance Scenarios**:

1. **Given** a running step, **When** the agent reads a file, edits one or runs a command,
   **Then** the page shows it within a few seconds, under that agent.
2. **Given** a finished review, **When** the engineer opens the criteria, **Then** each `part/id`
   shows pass or fail and what the reviewer ran.
3. **Given** the page is closed during a run, **When** it is opened again, **Then** it shows the
   whole run so far. The run itself never depends on the page being open.

---

### User Story 4 - Steer the run, not the agents (Priority: P4)

The engineer can steer at the level of the plan and the run, without typing into an agent:
- set the **autonomy level**:
  - High: only the plan, the merge and what the planner cannot decide come to them;
  - Medium, the default: also changes to contracts or criteria;
  - Low: also each repair round;
- answer questions;
- overrule a decision;
- add or change a criterion;
- leave a note for an agent's next step;
- pause, resume or stop a part or the whole run;
- take over a part by opening its agent session interactively, then hand it back;
- at the end, merge, request changes, or reject the run.

**Why this priority**: Autonomy is only acceptable if the engineer can intervene precisely when they
want to.

**Independent Test**: In one `cart` run, do each action once and check its effect. Overruling
reopens the affected parts, a new criterion gets checked, a note reaches the next step, and handing
back triggers a verification. Then check that each action appears in the run's history.

**Acceptance Scenarios**:

1. **Given** autonomy Medium, **When** a decision would change a contract, **Then** the engineer is
   asked, with the planner's recommendation, before it is committed.
2. **Given** an answered decision, **When** the engineer overrules it, **Then** the reversal is
   committed and every part that acted on the old decision is reopened.
3. **Given** a part already done or a finished review, **When** the engineer adds a criterion,
   **Then** it is committed to the plan, the part is reopened if it needs work, and the reviewer
   checks the new criterion before the merge is offered.
4. **Given** the engineer takes over a part, **When** they hand it back, **Then** the orchestrator
   verifies that part's branch as it would an agent's result.
5. **Given** a run ready to merge, **When** the engineer requests changes with a comment, **Then**
   a repair round goes to the named part with that comment, within the repair cap, followed by the
   gates and a review.
6. **Given** any steering action, **When** it is taken, **Then** the run's history records it with
   its time, and the run record counts it as an intervention.

---

### User Story 5 - Parallel parts where the plan declares them (Priority: P5)

When the approved plan splits the work into parts with owned files, no overlap and a committed
contract for each boundary, the orchestrator does the rest:
- it runs those builders at the same time, within a cap on sessions and memory;
- it integrates their branches itself;
- a merge conflict or an integration failure becomes a repair for the part that caused it.

**Why this priority**: Splitting pays only for some work, so it comes after the single loop is
proven.

**Independent Test**: Run the `cart` task with a two-part plan. Both builders run at once, the
orchestrator integrates their branches, and a deliberate conflict comes back as a repair for one
part.

**Acceptance Scenarios**:

1. **Given** a plan with parts but no committed contract for a boundary, or with overlapping owned
   files, **When** approval is requested, **Then** the problem is named and the split cannot be
   approved as it is.
2. **Given** two parts done, **When** integrating their branches conflicts, **Then** a repair goes
   to the part whose change conflicts, and the engineer is not asked unless the repair cap is
   reached.
3. **Given** the session or memory cap is reached, **When** another part could start, **Then** it
   waits and starts by itself when capacity frees up, and the page says why it waits.

---

### User Story 6 - Publishing that respects the remote (Priority: P6)

> Delivered for a feature loop by 010 (`specs/010-publish-remote`): FR-024 and FR-027 (push the
> loop's branch, open a pull request with the run's summary, never push to the target, force,
> bypass or approve). Not yet: FR-025 and FR-026 (learning required checks, waiting for them,
> repairing a failing check, bringing a branch that fell behind up to date); 010 shows the
> remote's checks and leaves what follows to the engineer.

**Without a remote, or with an unprotected target branch:** the engineer merges locally.

**With a protected target branch:**
1. The orchestrator pushes the run's branch and opens a pull request. Its description is the run
   summary: the plan, the criteria with their evidence, the gates, the decisions with their
   commits, the repairs and the cost.
2. It learns which checks and reviews the branch rules require, and follows the checks that
   actually report.
3. A failing check becomes a repair round.
4. A branch that has fallen behind is brought up to date and checked again.

It never bypasses protection.

**Why this priority**: Many real repositories protect their main branch, and the run has to fit
that workflow.

**Independent Test**: On a test repository with a protected main that requires one status check
and one review:
1. the run opens a pull request;
2. it waits for the check;
3. it repairs a deliberately failing check;
4. it shows "needs 1 approving review";
5. after the engineer's approval, merging goes through the normal pull request merge.

**Acceptance Scenarios**:

1. **Given** a protected target branch, **When** the run is verified, **Then** a pull request is
   opened, and nothing is pushed to the target branch directly.
2. **Given** the branch rules require named checks, **When** they report, **Then** the run waits
   for exactly those. A failure in a required check becomes a repair round, and a failure in a
   check that is not required is shown as a warning.
3. **Given** no checks are required and none report within a waiting period, **When** that period
   ends, **Then** the page says the repository has no CI, and the run continues to the merge.
4. **Given** any situation, **When** the run publishes, **Then** it never force-pushes, never
   approves its own pull request, and never uses rights beyond the engineer's own.

---

### User Story 7 - Measure it on the engineer's own tasks (Priority: P7)

The engineer runs their task set with hidden acceptance tests. Each task runs as **plain** (one
non-interactive session, no orchestration), **solo** (this feature with one builder) and
**squad** (with parts where the plan splits). Every run records:
- its steps, time and cost (as agents report it);
- its repairs, gate and review outcomes and decisions;
- the engineer's interventions;
- the hidden-test results.

Components can be switched off one at a time.

**Why this priority**: It answers whether the orchestrator beats a plain session, on evidence.

**Independent Test**: Run the T050 task set in the three modes. Records group by task and mode, with
pass rate, time, cost and interventions.

**Acceptance Scenarios**:

1. **Given** a recorded run, **When** it ends, **Then** its hidden tests run on the final result
   only after the agents are finished, and the counts are recorded.
2. **Given** a component switched off (reviewer, gates, decisions, autonomy prompts), **When** the
   run is recorded, **Then** the record names the components switched off, and the run is grouped
   apart from runs with every component on.

---

### Edge Cases

**Agent results**
- An agent step ends without a valid structured result, or exceeds its turn or time limit: it
  counts as a failed step. It is redone once with the reason, then repaired or escalated.
- An agent claims a commit that does not exist on its branch, or files it did not change: the claim
  is rejected and recorded, and the step is repaired.
- An agent's command writes outside its working copy: the orchestrator cannot prevent this. It
  checks the working copy and the branch. Reviewers whose working copy changes stop counting.

**Decisions and plan changes**
- A decision changes a contract while a part is mid-step: that step's result is not accepted until
  it contains the decision commit, and the step is told which commit to take.
- The engineer does not answer a question for a long time: only the affected part waits. A
  reminder is sent, and nothing is decided on the engineer's behalf.

**Integration**
- An integration gate fails and no single part is clearly the cause: the engineer is asked, with
  the failing tests and the parts involved.

**Engineer actions**
- The engineer takes over a part and hands it back with uncommitted changes: the hand-back check
  reports them, and the part is not accepted until they are committed or discarded.
- The engineer closes the page, or the machine sleeps: the run continues or resumes. Questions
  wait.

**Setup and limits**
- The repository has uncommitted changes when a run starts: the run starts from the chosen commit,
  and the engineer is told what is left out.
- The agent tool is missing or not signed in: the run does not start, and the page names what is
  missing.
- A cost or time limit is reached: the run pauses before its next step and asks the engineer
  (FR-038).

**Publishing**
- A push is rejected by branch protection: it is never forced. The page shows the rule.
- CI never reports on a pull request: after a waiting period the engineer is asked.

**Concurrency**
- Two runs on the same repository: each has its own working copies and branches. Runs never
  share a part or a branch.

## Requirements *(mandatory)*

### Functional Requirements

**The run (US1)**

- **FR-001**: A run MUST follow an explicit sequence of stages: plan, build, integrate, gate,
  review, repair, ready to merge, done (or stopped). Its state MUST be kept on disk after every
  change, so that a restarted orchestrator resumes it without repeating completed steps.
- **FR-002**: The plan MUST be agreed in a conversation between the engineer and a planning agent,
  held in the run's page. The plan MUST be committed to the repository before approval. It lists:
  - what is built;
  - acceptance criteria per part, each with an identifier unique within its part, and at least one
    case that must fail;
  - for a split: owned files per part and a contract for each boundary.
- **FR-003**: Nothing after the plan MUST start before the engineer approves the plan.
- **FR-004**: Each step after the plan MUST be one non-interactive agent call with:
  - a focused instruction (the step's goal, the relevant plan sections, the criteria);
  - its own working copy and branch;
  - rights fixed by its role (FR-036): builders edit and run the commands on their allowlist,
    reviewers run those commands but do not edit;
  - a turn limit and a time limit;
  - a required structured result: done, blocked or failed, the commit, its claims, and an optional
    question.
- **FR-005**: The orchestrator MUST accept a step's result only after verifying it itself:
  - the commit is on the step's branch;
  - the part's owned files changed, and nothing outside them except shared files;
  - the gates pass with no regression against a baseline taken on the starting commit.

  A claim that fails verification MUST be rejected, with the reason recorded.
- **FR-006**: The gates MUST be the build and test commands the project declares. They MUST be run
  by the orchestrator itself, never reported by an agent.
- **FR-007**: A review MUST run in a fresh call by a reviewer that did not build the work, against
  the integrated result. Its verdict MUST answer every criterion as `part/id`, with what was run and
  the result, including every case that must fail. An incomplete verdict MUST cause the review step
  to run again, not a question to the engineer. Partial verdicts within one review add up.
- **FR-008**: A failed gate or review MUST go to the builder of the failing part as a repair, with
  the failed checks. Repairs MUST be capped per part (default 2); at the cap the engineer is asked.
- **FR-009**: Without a protected remote target, the run MUST end at "ready to merge". Only the
  engineer's merge puts exactly the verified commit on the target branch.

**Decisions (US2)**

- **FR-010**: During a step, an agent MUST be able to read the plan, its criteria and the contracts
  from the orchestrator. It MUST be able to report progress for the page, and to ask for a decision.
  Asking blocks only that step.
- **FR-011**: A decision MUST be taken by a short planning step, or by the engineer when the
  autonomy level or the planner says so, and MUST be committed to the plan or a contract. The asking
  step MUST continue with that commit. Every decision MUST appear in the run's decisions log, with
  the question, who decided, the answer and the commit.
- **FR-012**: Agents MUST NOT be able to message each other or the engineer. All coordination goes
  through the orchestrator and the repository.
- **FR-013**: When a commit changes a contract or criteria, the orchestrator MUST reopen every part
  that builds against it, including parts already done. Each such part MUST be verified again
  before the run can be ready to merge. A result that lacks the commit MUST be refused, naming it.

**The run's page (US3)**

- **FR-014**: The orchestrator MUST serve each run's page locally, to the engineer only, reached
  from the list of runs (FR-035). A run's page MUST show:
  - the pipeline and the current stage;
  - per agent: its role, state, current activity, branch and cost;
  - a live activity feed taken from the agents' own output, filterable by agent;
  - the decisions log;
  - gate results against the baseline;
  - the criteria checklist per part, with the evidence;
  - repairs, time and cost.
- **FR-015**: Activity MUST reach the page within 2 seconds of the agent producing it. The page
  MUST show the full history of a run when opened at any time. A run MUST NOT depend on the page
  being open.
- **FR-016**: When something needs the engineer, the page MUST show it first, with the options, and
  the engineer MUST be notified (a desktop notification where available).

**Steering (US4)**

- **FR-017**: The engineer MUST be able to set the autonomy level at any time. It takes effect from
  the next decision.
  - **High**: the plan, the merge, and questions the planner cannot decide.
  - **Medium** (default): also changes to contracts or criteria.
  - **Low**: also each repair round.
- **FR-018**: The engineer MUST be able to:
  - answer a question;
  - overrule an answered decision: the reversal is committed, and affected parts are reopened;
  - add or change a criterion: it is committed, parts that need work are reopened, and the reviewer
    checks it;
  - add a note to an agent's next step;
  - pause, resume or stop a part or the run;
  - take over a part, by opening its agent session interactively by its session identifier, and
    hand it back, after which the part is verified as in FR-005;
  - at the end, merge, request changes (a repair round with their comment), or reject the run.
- **FR-019**: No steering action MUST type into a running agent. Each one changes the plan or the
  run's state, and the orchestrator applies it to the steps it affects.
- **FR-020**: Every steering action MUST be recorded in the run's history with its time, and
  counted as an intervention.

**Parallel parts (US5)**

- **FR-021**: The orchestrator MUST run parts in parallel only when the approved plan declares
  them, with owned files (code and its tests), no overlap, and a committed contract for each
  boundary. A plan that lacks any of these MUST NOT be approvable as a split, and the missing item
  is named.
- **FR-022**: Parallel steps MUST stay within a cap on concurrent agent sessions and on memory
  (carried over from 005: measured use plus expected size against a budget). A step that does not
  fit MUST wait and start by itself when capacity frees up, with the reason shown.
- **FR-023**: The orchestrator MUST integrate the parts' branches itself. A conflict or an
  integration-gate failure MUST become a repair for the part that caused it. When the cause is
  unclear, the engineer is asked.

**Publishing (US6)**

- **FR-024**: With a remote and a protected target branch, the orchestrator MUST push the run's
  branch and open a pull request whose description is the run summary. It MUST NOT push to the
  target branch.
- **FR-025**: The orchestrator MUST learn what a pull request needs:
  - first from the branch rules the remote reports (required checks by name, required reviews,
    up-to-date requirement);
  - then from the checks that actually report on the commit;
  - with the repository's CI configuration files used only as a hint.

  The page MUST show where that knowledge comes from.
- **FR-026**: The run MUST wait for the required checks. A failing required check MUST become a
  repair round on the same branch. A failing check that is not required is shown as a warning. A
  branch behind the target MUST be brought up to date and gated again. With nothing required and
  nothing reporting within a waiting period (default 5 minutes), the page MUST say so, and the run
  continues.
- **FR-027**: The orchestrator MUST NOT force-push, bypass protection, approve its own pull request,
  or act with rights beyond the engineer's own. The merge of a pull request MUST be the engineer's
  action, through the remote's normal merge.

**Measurement (US7)**

- **FR-028**: Every run MUST record its steps, durations, cost and tokens as agents report them,
  repair rounds, gate and review outcomes, decisions (counts) and interventions.
- **FR-029**: The task sets from 005 MUST run in modes plain, solo and squad, in fresh copies of the
  project, with hidden tests applied only after the run's agents are finished. Results MUST be
  grouped by task and mode, with pass rate, median time, cost and interventions.
- **FR-030**: Each orchestration component MUST be switchable off for a recorded run: the reviewer,
  the gates, decisions by the planner (every question goes to the engineer) and parallel parts. The
  record names the components switched off.

**Across the feature**

- **FR-031**: The orchestrator MUST support Claude Code, and MUST allow other agents (Codex, Gemini)
  through an adapter that provides a non-interactive call, streamed output, a structured result
  and resume by session.
- **FR-032**: Before a run starts, the orchestrator MUST check that the agent tool is present and
  signed in, that the gates' commands exist, and whether a remote and branch rules apply. It MUST
  report anything missing.
- **FR-033**: The orchestrator MUST NOT start agent calls before the plan is approved, except the
  planning conversation. The approval of the plan covers the steps of that run, within its caps.
- **FR-034**: This feature replaces Fallow. The terminal app is retired: the editor, the process
  monitor and hosting agents in terminals. Fallow becomes the orchestrator with its run page.
  Fallow's constitution MUST be replaced by one written for the orchestrator before planning
  starts (proposal: `constitution-proposal.md`). Code from earlier features is kept only where the
  orchestrator uses it.
- **FR-035**: Fallow MUST run as one local service per engineer:
  - started once, by command or, if the engineer chooses, at login;
  - one page that lists every run across repositories with its stage and what needs the engineer,
    and opens each run's view;
  - runs started from that page or from the command line in a repository (with the task as text);
  - unfinished runs resumed by themselves when the service starts again, after a crash or a
    reboot.

  Stopping the service MUST NOT lose a run. Each run continues where it was the next time the
  service starts.
- **FR-036**: A builder MUST be able to run only the commands on its allowlist, without asking:
  - by default: git within its own working copy, the build and test commands the project declares,
    and the usual read-only shell tools (listing, reading and searching files);
  - plus any commands the project adds in its squad file.

  A command outside the list MUST be refused by the agent's own permission settings. The step then
  reports itself blocked, naming the command, and the run treats that as a question for the
  planner or the engineer under the autonomy level. Reviewers get the same list without edits.
  The planner may edit only the plan, the criteria and the contracts.
- **FR-037**: The engineer MUST be able to choose the agent and the model for each role (planner,
  builder, reviewer):
  - as a default in Fallow's settings;
  - per project in the squad file;
  - per run when starting it.

  The more specific choice wins. Without a choice, every role uses the default agent and its
  default model. When the reviewer uses the same agent and model as the builder, the run's page
  MUST say so. The run record keeps the agent and model of each step.
- **FR-038**: Every run MUST have a cost limit and a time limit, by default **$10** and **2 hours**,
  changeable per project and per run. When either is reached, the run MUST pause before starting
  another step and ask the engineer to raise the limit, continue once, or stop. Cost counts only
  what agents report. Where an agent reports none, only the time limit applies, and the page says
  so.

### Key Entities

- **Run**:
  - one task in one repository, from plan to merge or stop;
  - its stage, autonomy level, caps, steps, decisions, steering actions and outcome;
  - kept on disk so it survives a restart.
- **Plan**:
  - committed to the repository;
  - the work, its parts (owned files), the acceptance criteria per part (`part/id`, text,
    must-fail), and the contracts between parts.
- **Step**:
  - one non-interactive agent call;
  - its role, part, working copy and branch, instruction, limits, session identifier, streamed
    activity, structured result and verification outcome.
- **Decision**:
  - a question raised during a step;
  - who decided (planner or engineer), the answer, the commit that records it, and whether it was
    overruled.
- **Gate result**:
  - a declared command run by the orchestrator on a commit;
  - its outcome classified against the baseline.
- **Verdict**: a reviewer's answer per `part/id`, with what was run, the result and its
  completeness.
- **Steering action**: the engineer's action, its time, what it changed, and the steps it affected.
- **Publication**:
  - the pull request, the rules the remote reports, the checks that reported, and their outcomes;
  - or the local merge.
- **Run record**: the counts, durations and outcomes kept for comparison, in the limits the
  constitution allows.
- **Task set**: as in 005, tasks with a starting commit, a fixed description and hidden tests.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the engineer's task set, at least 80% of solo runs reach "ready to merge" with no
  intervention other than approving the plan. The merge is the only intervention after that.
- **SC-002**: On the same task set, solo's pass rate on the hidden tests is at least plain's pass
  rate, over at least 3 repeats per task.
- **SC-003**: A run interrupted at any stage, with the orchestrator stopped and started again,
  resumes and completes without repeating a completed step: 100% of the stages tested.
- **SC-004**: None of the false claims injected in testing is accepted: a missing commit, files not
  changed, failing tests, or an incomplete verdict.
- **SC-005**: After a contract or criteria change, every affected part, including parts already
  done, is verified again before the merge is offered: 100% of tested cases, with no question to
  the engineer at High autonomy.
- **SC-006**: Agent activity appears on the page within 2 seconds of the agent producing it.
- **SC-007**: Someone who did not watch a run can explain, from the page alone, what each agent did
  and why the result can be trusted, within 3 minutes.
- **SC-008**: Where a single loop is right, solo's median time is no more than 2 times plain's, and
  its median cost no more than 2.5 times, on the same tasks.
- **SC-009**: In tests against a protected branch, no run pushes to the target branch, force-pushes
  or approves its own pull request: zero occurrences.

## Assumptions

**Product shape**
- The first version runs for one engineer on their own Windows machine. Linux and macOS, the
  Linux VPS, reaching the page from another machine, multi-user operation and running agents on
  another machine are out of scope for now. Nothing in the design may rule them out later.
- The planning conversation takes place on the run's page. Each turn is a non-interactive call that
  resumes the planner's session. The engineer can also open that session interactively.
- One run per repository at a time is the default. Several runs on different repositories may run
  at once, within the caps.

**Carried over and dropped**
- The task sets and hidden tests from 005 carry over unchanged.
- Gates, baselines, manifest and contract checks, verdict completeness, run records and the memory
  cap carry over from 005 as behaviour. They are not necessarily the same code.
- Dropped from 005 for this feature:
  - hosting agents in terminals;
  - the editor;
  - typing into sessions;
  - detecting idle sessions;
  - messages between agents.

  The terminal app does not remain (FR-034).

**Agents and the remote**
- Cost and tokens are recorded only as agents report them; nothing is estimated.
- Claude Code provides the non-interactive call, streamed structured output, resume by session and
  per-call rights this feature relies on. Other agents join only where an adapter can provide the
  same.
- A remote's branch rules are read with the engineer's own credentials, through the remote's
  standard tools. Without access to the rules, the orchestrator relies on the checks that report.

**Data**
- Run state, step activity and decisions are kept on disk locally, readable by the engineer only,
  until the engineer deletes the run, as the new constitution allows.

## Clarifications

### Session 2026-10-29

- Q: Fallow's constitution forbids this feature as specified. Should it become Fallow under a new
  constitution, a new product, or a second program alongside the terminal app? → A: It completely
  overwrites Fallow; this is the new product, and the constitution is recreated for it (FR-034).
- Q: Should Fallow run as one background service that manages all runs, or as a separate program
  per run? → A: One local service, started once (optionally at login), with one page listing all
  runs across repositories; runs start from the page or the command line, and unfinished runs
  resume after a restart (FR-035).
- Q: Which machines must the first version work on, and how is the page reached on the Linux VPS?
  → A: Windows only for the first version; the Linux VPS (and Linux and macOS generally) is left
  for later, so remote access to the page is out of scope for now.
- Q: Which commands may a builder run on its own in a non-interactive step? → A: A default
  allowlist (git in its own working copy, the project's declared build and test commands, usual
  read-only shell tools), which each project extends in its squad file; anything else is refused
  and the step reports itself blocked (FR-036).
- Q: Should the reviewer use a different model or agent from the builder by default? → A: It is
  configurable by the engineer: the agent and model of each role (FR-037).
- Q: What should a run's default limits on cost and time be before it pauses and asks? → A: $10
  and 2 hours per run by default, changeable per project and per run (FR-038).
