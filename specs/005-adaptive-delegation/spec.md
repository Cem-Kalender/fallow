# Feature Specification: Adaptive Delegation

**Feature Branch**: `001-llm-aware-tui-editor` (no separate branch; see Assumptions)

**Created**: 2026-10-28

**Status**: Draft

**Input**: User description: "Adaptive delegation: redesign Fallow's multi-agent orchestration so
that a squad beats a single agent session with skills, by delegating only where it pays, following
Anthropic's published lessons. It replaces 004's default workflow, which splits one piece of work by
role, defaults to parallel builders and spends much of its effort on coordination. 004's enforcement
stays." (Full description: P1 one loop by default, delegation decided per piece of work; P2
decisions live in the repo; P3 verification Fallow can trust; P4 a lead that can be replaced; P5
measurement built in; P6 resources.)

**Depends on**: feature 004 (bursts, roles fixed by Fallow, squad files, Fallow-run gates with
baselines in temporary checkouts, evidence, capped repair, review verdicts, the engineer's merge) and
003 (board, events, deliveries, task queue, session starts, standing permissions, verified identity).

**Sources**: Anthropic, "Building multi-agent systems: when and how to use them"; "How we built our
multi-agent research system"; "Harness design for long-running application development";
"Effective harnesses for long-running agents"; Claude Code documentation on agent teams, subagents
and best practices. The lessons this feature applies:

- Start with one agent and add agents only where evidence shows they pay; multi-agent work costs
  3 to 10 times the tokens of one agent.
- Divide work by context boundary, not by type of work: splitting one feature into planner,
  implementer, tester and reviewer agents loses information at every hand-off, and the agents spend
  more on coordinating than on the work. The agent that builds a feature also writes its tests.
  Writing a plan to a file and building from it in a fresh session is a hand-off through the
  repository, which Anthropic's harnesses use.
- Parallel work pays for independent components with clean interfaces, for review from distinct
  angles, and for competing hypotheses; it does not pay for sequential or tightly coupled work.
- A verifier that did not write the work, with concrete criteria, catches what self-review misses;
  rules that must always hold are enforced by the harness, not requested in a prompt.
- Agents communicate through files in the repository, so a fresh session can continue from them.
- Every part of a harness encodes an assumption about what the model cannot do alone; remove parts
  one at a time and keep only those that change the outcome.

## Clarifications

### Session 2026-10-28

- Q: Where do run records live, given Principle VII keeps orchestration data in memory only? → A: Amend the constitution: Fallow may keep run records on disk, numbers and outcomes only, never message content (option A; proposal in `constitution-amendment.md`, v5.0.0).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - One loop by default, delegation decided per piece of work (Priority: P1)

The engineer asks their lead session to build a feature. The lead plans it with them and decides,
for each piece of work, how it is built. By default the lead builds it itself, in one loop, and a
fresh reviewer checks it. The lead may instead hand the whole build to one fresh builder session that
starts from the committed plan, build independent features in separate worktrees, or, only when the
work has clean boundaries, split it into parallel parts whose builders each own their files and build
against a committed contract. The lead records the choice and its reason in the work manifest. Fallow
refuses a split that is not prepared and shows the engineer every decision. A session investigating
something uses its own subagents instead of asking for another session.

**Why this priority**: Anthropic's evidence is that a single loop with independent verification wins
most coding tasks and that a wrong split costs more than none. An orchestrator that knows when not to
delegate is the core of this feature.

**Independent Test**: Start a burst for a small feature with no recorded decision; verify the lead is
briefed to build it itself, that a request for a builder is refused with the reason, and that the
gates and a reviewer still apply. Record two parallel parts with owned files and a committed contract;
verify two builders may start.

**Acceptance Scenarios**:

1. **Given** a burst starts, **When** the manifest records no decision for a piece of work, **Then**
   the lead builds it in its own session, and its briefing says so.
2. **Given** a piece of work is recorded as built by the lead or by one builder, **When** the lead asks
   for another builder for it, **Then** the start is refused with the reason and what a parallel
   split needs.
3. **Given** a piece of work is recorded as parallel parts, **When** the parts' owned files or a
   contract for each boundary between them are not committed, or owned files overlap, **Then**
   builders for those parts cannot start, and the lead is told what is missing.
4. **Given** the manifest lists independent features, **When** the lead starts a builder for each,
   **Then** each builds in its own worktree without a contract between them.
5. **Given** the lead records a decision, **When** the engineer opens the Squad tab, **Then** they see
   each piece of work, how it is built and the lead's reason.
6. **Given** a session needs to investigate (search the code, read documentation), **When** its
   briefing is made, **Then** it says to use its own subagents for that, not to ask for a session.
7. **Given** a review or a debugging task, **When** the lead wants parallel sessions for it, **Then**
   it may start several reviewers with distinct focuses, or several investigators with competing
   hypotheses, within the burst's limits.

---

### User Story 2 - Measure, and compare against a single session (Priority: P2)

The engineer wants to know whether orchestration was worth it. They keep a task set outside the
project: task descriptions and, for each, hidden acceptance tests. Fallow runs a task in one of three
modes from the same starting commit and the same fixed description: plain (one session with its
skills and no orchestration), solo (the lead builds alone, with Fallow's gates and a reviewer) and
squad (the lead decides how to delegate). Each run is recorded: time working and blocked, the
engineer's interventions, gate and review outcomes, repair rounds, signals, tokens and cost where the
agent reports them, and whether the hidden tests pass on what the run left. Each run works in its own
copy of the project, so runs cannot see each other's work. Runs are repeated and
compared side by side. The engineer can switch components off one at a time to see which matter.

**Why this priority**: The feature exists to beat a single session. Without a fair, repeatable
comparison no design choice in it can be judged, and Anthropic's practice is to measure before adding
complexity.

**Independent Test**: Run one task plain and as a squad, twice each, with the same hidden tests;
verify all four runs are recorded with mode, outcome, time, interventions and cost (where reported),
that the hidden tests were placed only after the run's sessions were closed, and that the comparison
view shows the runs side by side with their spread.

**Acceptance Scenarios**:

1. **Given** a task in the task set, **When** the engineer starts a run, **Then** they choose the mode
   and the components switched off, and the run starts from the task's starting commit with its fixed
   description, in its own copy of the project; no session can ask the engineer anything.
2. **Given** a run is recorded, **When** it starts, **Then** Fallow notes fingerprints of the task
   description and the hidden tests, and the agent, model and version of each session.
3. **Given** a run ends (its sessions idle, its time limit, or the engineer), **When** the engineer
   confirms closing the run's sessions, **Then** Fallow runs the hidden tests in the lead's worktree (the plain session's in plain mode) as the run left it and records the counts passed and failed.
4. **Given** a burst ends, **When** the engineer opens its record, **Then** it shows per part and per
   role the time working, time blocked, signals, gate and review outcomes, repair rounds, the
   engineer's interventions, and tokens and cost where the agent reported them.
5. **Given** several runs of one task, **When** the engineer opens the comparison, **Then** runs are
   grouped by mode with pass rate, median time, interventions and cost, and the spread across repeats.
6. **Given** the engineer switches off a component for a run, **When** the run is recorded, **Then**
   the component is named as off.
7. **Given** an agent does not report its token use, **When** the record is shown, **Then** cost is
   marked unknown for that session rather than estimated.
8. **Given** a comparison is planned, **When** the engineer starts a series of runs, **Then** Fallow
   runs them one at a time in a shuffled order of task and mode.

---

### User Story 3 - Decisions live in the repo (Priority: P3)

Two builders work on separate parts against a contract. One finds the contract unclear and raises a
question to the lead. The lead answers by committing a change to the contract, and its reply only
points to that commit. Meanwhile the builder carries on with work the question does not block. When
it reports the part done, Fallow checks that its work includes the latest change to its part's
criteria and contracts. Only the lead changes the manifest, plan and contracts, and a builder that
edits files outside its part is flagged.

**Why this priority**: Parallel builders only pay off when they do not drift apart. Answers kept in
messages get paraphrased and lost; answers in the repo are versioned, reviewable and readable by a
fresh session.

**Independent Test**: Run two parts with a contract; have a builder raise a question, the lead commit
an answer to the contract, and the builder report done from a commit that does not contain it; verify
the report is refused; have the lead change only the other part's criteria; verify the first builder's
report is not refused for it; have a builder edit a file the other part owns; verify the lead is told.

**Acceptance Scenarios**:

1. **Given** a builder needs a decision, **When** it raises it, **Then** it does so as a structured
   signal to the lead (a question, or a proposed change to the plan or a contract with its diff), and
   builders cannot message each other.
2. **Given** the lead answers a signal that needs a decision, **When** it replies, **Then** the reply
   refers to a commit that changed the plan, the manifest or a contract; a reply that only says
   "carry on" or "no change" needs no commit.
3. **Given** a commit changed a part's acceptance criteria or a contract it builds against after its
   builder started, **When** the builder reports the part done, **Then** the report is refused unless
   the reported commit contains that change, and the builder is told which commit to take in.
4. **Given** a session other than the lead commits a change to the manifest, plan or a contract,
   **When** Fallow checks its work, **Then** the lead and the engineer are told and that work does not
   count until the lead accepts it.
5. **Given** a builder commits a change to a file its part does not own, **When** Fallow checks its
   work, **Then** the lead is told, naming the file and its owner.
6. **Given** a builder raised a question, **When** other work in its part is not blocked by it,
   **Then** its briefing and the signal's reply tell it to carry on with that work.

---

### User Story 4 - Verification that checks the right thing (Priority: P4)

Before work starts, each piece of work has concrete acceptance criteria in the manifest, agreed
between the lead and whoever builds it. Fallow runs the gates itself on each part and on the
integrated result. A reviewer in a fresh session, which may run commands but not edit, checks the
integrated result against the criteria as a black box, including cases that must fail. Fallow cannot
see whether the reviewer really ran its checks; it checks that the verdict answers every criterion and
that the reviewer changed nothing, and it trusts only its own gates and the hidden tests for what
must hold.

**Why this priority**: Independent verification is what Anthropic kept even after simplifying its
harness. 004 has most of the machinery; this story makes it check the agreed criteria.

**Independent Test**: Run a piece of work whose criteria include a case that must fail; have the
reviewer approve without answering each criterion; verify the approval does not count; have it
answer each criterion with what it ran and the result; verify it counts; have it change a file;
verify its verdict stops counting.

**Acceptance Scenarios**:

1. **Given** a piece of work has no acceptance criteria in the manifest, **When** it is handed to a
   builder or the lead starts building it, **Then** Fallow refuses until criteria are committed.
2. **Given** a builder starts a part, **When** it receives the task, **Then** the task points to the
   part's criteria, and the builder may propose changes to them as a signal before building.
3. **Given** work is integrated, **When** the gates run, **Then** they run on the integrated result as
   well as per part, against the baseline on the starting commit.
4. **Given** a reviewer gives its verdict, **When** it does not answer each acceptance criterion with
   what it ran and the result, including at least one case that must fail, **Then** it does not count
   toward the merge and the reviewer is told why.
5. **Given** a reviewer runs commands, **When** its worktree gains changes or commits, **Then** its
   verdict stops counting and the engineer is warned (as for read-only roles in 004).
6. **Given** a squad asks for several reviewers, **When** they are started, **Then** each gets a
   distinct focus named in the manifest, and the merge needs each of their approvals.
7. **Given** a gate fails or a reviewer asks for changes, **When** the failure belongs to a part,
   **Then** the repair goes to whoever builds that part, within the repair cap.

---

### User Story 5 - A lead that can be replaced, told only what needs a decision (Priority: P5)

The lead's session dies halfway through a burst. The engineer starts a new session and names it the
lead. It reads the manifest, plan and contracts and the squad status, and carries on without the old
lead's context. During the burst the lead is told only what needs a decision from it, never periodic
status.

**Why this priority**: A burst should not depend on one context window, and every message to the lead
costs tokens and attention.

**Independent Test**: Run a burst with two parts; stop the lead mid-burst; name a new session the
lead; verify it can list every part's state, open signals and the latest manifest commit, and that it
finishes the burst; verify that while nothing needs a decision, the lead receives no messages.

**Acceptance Scenarios**:

1. **Given** a burst runs, **When** a new lead takes over, **Then** its briefing and the squad status
   give it the latest manifest commit, every piece of work with its builder, state and evidence, open
   signals and what the merge still needs.
2. **Given** a burst runs, **When** nothing needs a decision from the lead, **Then** Fallow sends it
   nothing.
3. **Given** a burst runs, **When** a signal, a failed gate, a verdict, an exited session, a session
   stuck at its agent's permission prompt or a ready merge occurs, **Then** the lead is told once,
   with what it needs to act.
4. **Given** Fallow restarts during a burst, **When** the engineer reopens the project, **Then** Fallow
   rebuilds what it can from the repository (manifest, parts, their branches and commits) and tells the
   engineer what was lost (permissions, tasks, signals) and must be granted or started again.

---

### User Story 6 - Fits a small machine (Priority: P6)

The engineer runs Fallow on a 4 GB virtual server. Fallow knows how much memory each agent session
uses and does not start more sessions than fit. When a start has to wait for memory, the lead and the
engineer are told, and work proceeds with fewer parallel sessions.

**Why this priority**: Parallelism that exhausts the machine is worse than none, and the engineer's
real environment is small.

**Independent Test**: Set a memory budget that fits two sessions; ask for three; verify the third
waits with a stated reason and starts when a session ends.

**Acceptance Scenarios**:

1. **Given** sessions are running, **When** Fallow decides whether another fits, **Then** it uses the
   memory those sessions actually use, plus a margin for the machine.
2. **Given** a start does not fit, **When** it is asked for, **Then** it waits, and the lead and the
   engineer are told the reason and how much memory is missing.
3. **Given** a start waits for memory, **When** a session ends and memory is free, **Then** the start
   proceeds without being asked again.

---

### Edge Cases

- Parallel parts with overlapping owned files: refused, naming the files (FR-004).
- The lead tries to split one feature into a building and a testing session: the testing part owns
  only test files, so it is refused (FR-006).
- A contract changes after both builders built against it: each part's done report needs the change
  (FR-020), and the gates on the integrated result decide.
- A builder's question stays unanswered: after a set time the engineer is told, since the squad waits
  on the lead.
- A builder reports done while its question is still open: accepted only if the builder states the
  question no longer blocks the part; the question stays visible to the lead.
- The hidden tests cannot run on what the run left (it does not build): every hidden test counts as
  failed, not as unmeasured.
- A plain run has no lead, gates or reviewer: its record holds what Fallow can observe (time,
  interventions, the hidden tests' outcome, cost where reported).
- An agent reads the task set's hidden tests from disk anyway: Fallow cannot prevent a process running
  as the engineer from reading their files; the hidden tests are only kept out of the project,
  worktrees, prompts and briefings, and the engineer is told this when registering a task set.
- The engineer switches off the reviewer for a run: the record says so, and the merge needs the
  engineer's explicit approval with no review verdict.
- The memory budget fits only the lead: the burst runs as the lead's single loop, and parallel
  decisions wait, with the reason told.
- The task set changes between runs of one task: the fingerprints differ, and the comparison keeps
  those runs apart.

## Requirements *(mandatory)*

### Functional Requirements

**Delegation (US1)**

- **FR-001**: A burst MUST build each piece of work in the lead's own session unless the work
  manifest records another decision for it.
- **FR-002**: The manifest MUST record, for each piece of work, one of: built by the lead, built by
  one fresh builder from the committed plan, independent features, or parallel parts; with the lead's
  reason for anything other than the lead building it.
- **FR-003**: Fallow MUST refuse to start a builder for work recorded as built by the lead, and a
  second builder for work recorded as one builder, with the reason and what a parallel split needs.
- **FR-004**: Fallow MUST refuse to start builders for parallel parts until each part's owned files
  and a contract for each boundary between parts are committed and the owned files do not overlap.
- **FR-005**: Independent features MUST each get their own builder and worktree without a contract
  between them.
- **FR-006**: Each part MUST own code it builds and the tests for that code, distinguished by test
  file patterns the squad declares (with defaults per language); Fallow MUST refuse a part that owns
  only tests, only plan documents or nothing.
- **FR-007**: The Squad tab MUST show each piece of work, how it is built and the lead's reason.
- **FR-008**: Briefings MUST tell every session to use its own subagents for investigation, and the
  lead to build itself unless the work has clean boundaries or needs a fresh context.
- **FR-009**: The lead MUST be able to start several reviewers with distinct focuses, or several
  investigators for competing hypotheses, within the burst's limits. Investigators MUST use the
  reviewer's level: run commands, no edits.
- **FR-010**: A squad file MAY cap how many builders run in parallel; the manifest's decisions apply
  within that cap.

**Measurement and comparison (US2)**

- **FR-011**: The engineer MUST be able to register a task set kept outside the project: per task, a
  name, a starting commit, a fixed description, the run's time limit, optional setup files (such as
  local agent settings) and hidden acceptance tests. Fallow reads it and never places the hidden
  tests anywhere an agent of a running run can reach through Fallow (project, worktree, prompt,
  briefing).
- **FR-011a**: Each run MUST take place in its own copy of the project that holds only the history up
  to the task's starting commit, plus the task's setup files, so no run can see another run's work
  and the engineer's repository is not written. Fallow removes the copy after the run is recorded or
  discarded.
- **FR-012**: The engineer MUST be able to run a task plain (one session, no Fallow orchestration
  tools), solo (the lead builds alone with Fallow's gates and a reviewer) or as a squad, from the
  task's starting commit and fixed description. In every mode the editing sessions (the plain session, the
  lead, builders) run at the same agent permission level (edit and run commands, never asking the
  engineer), so no mode can interview the engineer or stall at a prompt the others skip.
- **FR-013**: A run MUST end when no session of the run has been working for a set time (default 3
  minutes) while nothing of the run is in progress (no gate running, no start waiting), when its time
  limit passes, or when the engineer ends it. Fallow then asks the engineer once to close the run's
  sessions (naming them); after they are closed it copies the hidden tests into the lead's worktree (the plain session's in plain mode), runs them there, and records the counts passed and failed. The tested state is that
  worktree as the run left it, committed or not, the same for every mode.
- **FR-014**: Every recorded run MUST record per part and per role: time working, time blocked,
  signals raised, gate outcomes, review outcomes, repair rounds and the engineer's interventions. Time
  blocked is time a session waits on an open signal it raised, a start waiting for memory, or a task
  whose preconditions are unmet. An intervention is any line the engineer sends into a session of the
  run and any approval or decision they give during the run; recorded runs are set up to need none,
  and closing the sessions at the end is not one.
- **FR-015**: Fallow MUST record cost per session where the agent reports it about its own use, and
  mark it unknown otherwise; it MUST NOT estimate cost or tokens. Tokens are recorded only if an agent
  reports a cumulative count.
- **FR-016**: For runs the engineer chose to record, Fallow MUST keep the run record on disk so
  comparisons span runs and restarts. A record holds only what the constitution (v5.0.0, Principle
  VII) allows: mode, components switched off, fingerprints of the task description (line endings
  normalised) and of the hidden tests, the agent, model and version identifiers
  agents report, commit identifiers, start time, durations, counts, fixed-value outcomes, and reported
  token and cost figures. The engineer can list and delete records.
- **FR-017**: Every orchestration component MUST be switchable off for a recorded run, with this effect:

  | Component off | Effect |
  |---|---|
  | reviewer | no reviewer starts; the review condition is dropped from the merge |
  | gates | no gate runs during the run; gate conditions are dropped (hidden tests still run) |
  | contracts | parallel parts may start without a contract per boundary |
  | plan-commit check | done reports are not checked for criteria and contract changes |
  | criteria precondition | work may be built without committed criteria; verdicts then answer the description |
  | ownership check | edits outside owned files are not reported |
  | verdict check | any approval counts |
  | lead updates | after the run's description, the lead receives nothing typed by Fallow; it learns only through its tools |
  | parallel builders | every piece of work is built by the lead |

  The record names the components switched off.
- **FR-018**: Fallow MUST run a series of recorded runs one at a time, in a shuffled order of task and
  mode, with the number of repeats per task and mode the engineer sets (default 3). The series waits
  at each run's end for the engineer to close its sessions (FR-013).
- **FR-019**: Fallow MUST show the runs of a task set grouped by task and mode, with pass rate, median
  time, interventions, cost per passing run where known, and the spread across repeats; runs whose
  fingerprints differ are not grouped together.
- **FR-019a**: A run in progress when Fallow exits or restarts MUST be discarded, not recorded, and
  the engineer told.

**Decisions in the repo (US3)**

- **FR-020**: The work manifest MUST be one structured file at a fixed place in the project listing
  the pieces of work and how each is built, the parts with their owned files, each piece's acceptance
  criteria, the contracts between parts and the reviewers' focuses. Prose (in Spec Kit projects the
  spec, plan and tasks) stays in documents the manifest refers to; Fallow reads only the manifest.
- **FR-021**: Builders MUST raise decisions only as structured signals to the lead: a question, or a
  proposed change to the plan, manifest or a contract with its diff. Builders MUST NOT be able to
  message each other.
- **FR-022**: The lead's answer to a signal that changes a decision MUST refer to a commit that
  changed the plan, manifest or a contract; an answer that changes nothing needs none.
- **FR-023**: Builders MUST start from the lead's branch at the latest manifest commit. A builder's
  done report MUST be refused unless its commit contains every commit since it started that changed
  its part's criteria or a contract it builds against; it is told which commit to take in.
- **FR-024**: A change to the manifest, plan or a contract by a session other than the lead MUST be
  reported to the lead and the engineer as a signal from Fallow, and that work MUST NOT count until
  the lead accepts it by answering that signal.
- **FR-025**: A builder's change to a file its part does not own MUST be reported to the lead with the
  file and its owner.
- **FR-026**: A builder with an open question MUST be told, in its briefing and in the signal's reply,
  to carry on with work the question does not block.

**Verification (US4)**

- **FR-027**: A piece of work MUST NOT be handed to a builder until its acceptance criteria are
  committed in the manifest; a builder MAY propose changes to them as a signal before building. Work
  the lead builds itself may have criteria; without them, the task description is its one criterion.
- **FR-028**: Gates MUST run on each part and on the integrated result, classified against the
  baseline on the starting commit (as in 004). The lead integrates builders' branches into its own
  branch; the integrated result is the lead's branch, and the final gates run on its commit.
- **FR-029**: A review verdict MUST count toward the merge only if it answers each acceptance
  criterion with what was run and the result, including at least one case that must fail; otherwise
  the reviewer is told why it does not count. This is a check of the verdict's completeness, not of
  whether the reviewer's checks ran.
- **FR-030**: Reviewers MUST check the integrated result in a fresh session at a level that may run
  commands but not edit files, given by the agent's flags; a reviewer whose worktree changes stops
  counting, as read-only roles in 004. Commands can still write outside the worktree, which Fallow
  does not see; the engineer is told this when a squad uses the level.
- **FR-031**: Several reviewers MUST each have a distinct focus named in the manifest, and the merge
  MUST need an approval for each focus. 004's warning about reviewers on the same agent and model
  stays.
- **FR-032**: Repairs MUST go to whoever builds the part, within the repair cap (as in 004).

**The lead (US5)**

- **FR-033**: The lead's briefing and the squad status MUST give a new lead everything it needs to
  continue: the latest manifest commit, each piece of work with its builder, state and evidence, open
  signals, and what the merge still needs.
- **FR-034**: The engineer MUST be able to name another session the lead during a burst.
- **FR-035**: Fallow MUST tell the lead only what needs a decision from it: a signal, a failed gate, a
  verdict, an exited session, a session stuck at its agent's permission prompt, a ready merge, and a
  question it left unanswered too long. It MUST NOT send periodic status.
- **FR-036**: After Fallow restarts during a burst, it MUST rebuild what it can from the repository
  (the manifest, and builder branches named after their work and part) and tell the engineer what was
  lost (permissions, tasks, signals) and must be granted or started again.

**Resources (US6)**

- **FR-037**: Fallow MUST decide whether another session fits from the memory its running sessions
  actually use plus a margin, against a budget the engineer sets.
- **FR-038**: A start that does not fit MUST wait, telling the lead and the engineer the reason and the
  memory missing, and proceed by itself when memory is free.

**Across the feature**

- **FR-039**: 004's enforcement MUST stay: roles fixed by Fallow, tools a role may not use refused,
  worktrees of non-editing roles checked, gates run and signalled by Fallow, evidence checked, repairs
  capped, the merge left to the engineer.
- **FR-040**: Fallow MUST NOT run a command, start a session or change a repository except as 004
  allows and as this feature adds: creating and removing a run's own copy of the project (FR-011a),
  starting the runs the engineer asked for, and running the hidden tests in that copy after its
  sessions are closed.
- **FR-041**: Every new command MUST be in the palette, and every new key checked against the known
  conflicts (as in 004).

### Changes to earlier features

- **004 built-in squad and task ranges (FR-006, FR-019)**: replaced by the lead's single loop with
  optional, prepared delegation (FR-001 to FR-006). Squad files keep their format; their roles, gates
  and merge conditions still apply, and may cap parallel builders (FR-010).
- **004 reviewer level**: reviewers and investigators run at a new level that may run commands but not
  edit (FR-009, FR-030); the worktree check of read-only roles applies to it.
- **004 two reviewer roles (FR-027)**: generalised to reviewers with distinct focuses (FR-031); the
  same-model warning and disagreement shown to the engineer stay.
- **004 messages to the lead (FR-032, FR-037, FR-026)**: quiet-time status and repeated waiting notices
  to the lead are removed; the lead is told only what FR-035 lists. Everyone-waiting notices to the
  engineer stay.
- **004 consulting the lead (FR-034) and deliveries between builders**: replaced by structured signals
  (FR-021); builders can no longer deliver to each other.
- **004 memory-only data (FR-029)**: unchanged except run records of recorded runs (FR-016), after the
  constitution amendment.
- **004 repository access (FR-030)**: extended by FR-040; recorded runs never touch the engineer's
  repository (FR-011a).
- **004 start allowance and kept starts (FR-039)**: stay; the memory budget (FR-037) applies as well.
- **Squads written for 004 (implementation decision, 2026-10-29)**: 005's workflow applies to squads
  whose merge needs `work:done`, which the built-in squad now does. A squad file whose merge needs
  004's `done:<role>` keeps 004's behaviour: builders start without a manifest, the lead gets no
  task of its own, and its lead still receives 004's quiet-time status. Everything else of 005
  (signals, the check level, verdict checks, runs) applies to every squad.

### Key Entities

- **Work manifest**: one structured file in the project; its latest commit is the manifest commit.
  Holds pieces of work, how each is built and why, parts, criteria, contracts and reviewer focuses.
- **Piece of work**: something to build, recorded as built by the lead, by one builder, as
  independent features or as parallel parts, with acceptance criteria.
- **Part**: a piece of parallel work with a builder, owned files (code and its tests), acceptance
  criteria and evidence.
- **Contract**: a committed description of a boundary between parts that both sides build against.
- **Signal**: a builder's structured question or proposed change to the lead, with its answer commit
  when the answer changed a decision.
- **Task set**: the engineer's tasks for comparison, kept outside the project: name, starting commit,
  fixed description and hidden acceptance tests per task.
- **Run**: one attempt at a task in a mode (plain, solo, squad), with the components switched off, its
  fingerprints, measurements and the outcome of the hidden tests.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Over a task set of at least 10 of the engineer's tasks, each run at least 3 times per
  mode, squad runs pass the hidden tests at least as often as plain runs.
- **SC-002**: On the tasks the engineer marked in advance as having several independent components,
  squad runs pass the hidden tests more often than plain runs.
- **SC-003**: On tasks the lead built in a single loop, squad runs take no more than 25% more time than
  plain runs of the same task, with no more interventions.
- **SC-004**: Solo runs pass the hidden tests at least as often as plain runs; if not, the gates and the
  reviewer are not paying for themselves and are reported as such.
- **SC-005**: Where cost is reported for every session of a run, a squad's cost per passing run is no
  more than 3 times that of plain runs.
- **SC-006**: No builder's work counts as done while it lacks a change to its part's criteria or
  contracts, in every test run.
- **SC-007**: A new lead finishes a burst whose lead was stopped halfway, without the engineer
  restating anything already in the manifest or plan, in the manual test.
- **SC-008**: On a 4 GB machine, Fallow never starts a session when the measured memory of the running
  sessions (gate sessions included) plus the new session's expected size exceeds the engineer's
  budget. Short spikes from builds inside sessions are not covered.
- **SC-009**: Every recorded run can be repeated from its record and the task set: the record's
  fingerprints identify the task description and hidden tests, its durations give the run settings, and it names the mode,
  components switched off, starting commit and the agents' reported versions. The squad file comes
  from the starting commit.

## Assumptions

- Like 002 to 004, this feature is developed on the existing branch `001-llm-aware-tui-editor`.
- The engineer writes the task set and its hidden tests, and freezes them before the first run of a
  series; Fallow provides the runs, measurement and comparison, not tasks.
- "Hidden" means outside the run's copy, worktrees, prompts and briefings until the run's sessions
  are closed. Agents run as the engineer's user, so Fallow cannot stop one from reading the
  engineer's files on purpose.
- A run's copy lacks the engineer's untracked local files; the task set's setup files supply what all
  modes need (for example local agent settings), the same for every mode.
- Token and cost figures come only from what an agent reports about its own use (for example a usage
  summary its CLI offers). Fallow does not read agents' network traffic or transcripts (Principle VII),
  so cost may be unknown for some agents; on a subscription it may be reported as tokens only.
- Plain runs use the same agent and model as the other modes, with the engineer's usual skills, the
  same permission level as builders, and no Fallow orchestration tools. Comparisons are meant for one
  agent at a time; cost is known only for agents that report it (Claude Code today).
- A task's success is decided by its hidden tests; code style and review quality are out of scope for
  the comparison.
- The manifest's format, its place in the project and the default test file patterns are settled in
  planning.
- Contract formats are the project's choice; Fallow checks that a contract file for each boundary is
  committed, and contract tests run as a gate where the squad declares them.
- The memory budget defaults to what fits the machine with a margin; on a 4 GB machine this is
  expected to allow the lead and one or two further sessions.
- Messages, tasks and signals stay in memory only (Principle VII); what survives a restart is what
  agents committed to the repository, and the run records of recorded runs (FR-016).
