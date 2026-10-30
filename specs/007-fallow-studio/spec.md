# Feature Specification: Fallow Studio

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/007-fallow-studio`)

**Created**: 2026-10-30

**Status**: Draft

**Input**: User description: "Fallow Studio: Fallow becomes a local dashboard for spec-driven development with multi-agent orchestration. It builds on 006 (the headless orchestrator) and replaces 006's single-task `fallow run` flow, in which the agent planned and built straight away, possibly from unclear specs. The UI must match the approved mockup exactly (the claude.ai canvas "Fallow Studio"). P1 projects; P2 the spec session; P3 the orchestration loop over a locked tasks.md; P4 visualizing the loop; P5 questions through the lead; P6 agents and pipelines; P7 model-agnostic runtimes. Out of scope: remote or multi-user operation, pull requests and CI, showing agents' thinking." (The full description is kept in the conversation of 2026-10-30.)

## Context

006 built the core: a loop that runs agent steps without a terminal, commits their work itself, runs the project's gates against a baseline, has an independent reviewer check every criterion by running something, sends failures back to the builder in the same session, and survives a restart. Its real runs worked. But a run started from a one-line task, and the planner wrote a plan and the builder built straight away, even when the task was unclear.

Fallow Studio puts proper spec-driven development in front of that loop and makes the whole process visible:

1. The engineer adds a project.
2. They work out a feature with a lead agent, following Spec Kit: constitution, specify, clarify, plan, tasks, analyze.
3. They lock the result.
4. Fallow then runs the locked `tasks.md` as an orchestration loop the engineer can watch: which agent does what, what it says, what it ran, and what Fallow checked.

**The reference for the look and layout** is the approved mockup, the canvas "Fallow Studio" (https://claude.ai/artifact/CyJcgKNpnC8tx2W1nLj13B). A copy of its screens is kept with this spec in `mockup/`. Its five screens are:
- Dashboard;
- Spec session;
- Orchestration loop;
- Pipeline designer;
- Project conversation.

## Clarifications

### Session 2026-10-30

- Q: Can the engineer change the spec after the lock, and how? → A: Unlock. The loop pauses, the spec session opens again, and after the new lock the loop continues and verifies the affected user stories again (FR-009).
- Q: Is the lead chat only per feature, or is there also a project-level conversation? → A: Both. Each project has its own conversation with the lead, and features are started from it (FR-034).
- Q: Should the builder keep one session for the whole feature, or start fresh per phase? → A: A fresh session per phase of `tasks.md`; the tasks of a phase share it. Every builder session starts from the same markdown files, the locked documents plus the handovers that earlier builders wrote (FR-010a).
- Q: How much may the lead answer without the engineer? → A: A setting per project and per feature, the autonomy level. Strict: every question goes to the engineer. Normal (the default): the lead answers what the documents or the conversations settle. Free: the lead also decides small open questions itself, and the engineer sees them afterwards (FR-020a).
- Q: How does Fallow get the engineer's attention? → A: The "Needs you" inbox and its count, plus a desktop notification that opens the item when clicked, with a switch to turn notifications off (FR-003a).
- Q (analysis, 2026-10-30): When a question waits for the engineer, what happens to the rest of the loop? → A: The loop waits; other loops and conversations go on (FR-021).
- Q (analysis, 2026-10-30): What is a "small" question at the Free level? → A: One that touches no reserved item, no acceptance scenario, no public interface (commands, API, file or data formats) and no dependency (FR-020a).
- Q (analysis, 2026-10-30): May Fallow tick tasks in the locked `tasks.md`? → A: Yes. After the lock, Fallow may tick tasks, add handovers and commit answers; nothing else changes the documents (FR-009).
- Q: What spending and time limit does a loop have by default? → A: A cost and time budget that Fallow suggests at the lock from the number of tasks, which the engineer can edit there. The loop pauses and asks when it is reached (FR-016a).
- Q: What may the lead run in the project conversation and the spec session? → A: The engineer switches modes as in Claude Code: Shift+Tab or a chip by the message box cycles Read only, Approved commands, Ask (the default) and Run anything (FR-007a).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Add a project and work out a feature with the lead (Priority: P1)

The engineer adds a project folder on the dashboard. Fallow installs Spec Kit in it. The engineer starts a new feature and talks with the lead agent in a chat. The lead follows Spec Kit's steps:
- specify;
- clarify, with questions shown as choices and a recommendation;
- plan;
- tasks;
- analyze.

The documents appear next to the chat as the lead writes them. The engineer can edit them directly, and the lead takes those edits into account on its next turn. When tasks and analyze are done, the engineer locks the feature.

**Why this priority**: this is what 006 lacked. Nothing should be built from a spec that was never made clear, and the locked spec is what every later step is checked against.

**Independent Test**: add a fresh folder, work out a small feature with the lead until it can be locked, lock it, and check that the documents are committed and can no longer be changed through the session. This works without any loop running.

**Acceptance Scenarios**:

1. **Given** a folder that has never been opened in Fallow, **When** the engineer adds it, **Then** it appears on the dashboard with its kind (git or plain folder), and the Spec Kit skills and templates are installed in it, without overwriting files that are already there.
2. **Given** a project, **When** the engineer starts a new feature and describes it, **Then** the lead runs Spec Kit's specify step and the new `spec.md` appears beside the chat. The stepper shows Specify done and Clarify current.
3. **Given** the lead's clarify step, **When** it has a question, **Then** the question appears in the chat as choices with one marked recommended. Choosing one writes the answer into the spec, and the spec's open `[NEEDS CLARIFICATION]` markers stay highlighted until each is answered.
4. **Given** a spec document open beside the chat, **When** the engineer edits it and sends the next message, **Then** the lead's reply takes the edit into account, and the document shows the edit as the engineer's.
5. **Given** a project, **When** the engineer opens its conversation with the lead and asks to start a feature from what they discussed, **Then** the feature's spec session opens, and the lead's first draft reflects that discussion.
6. **Given** a feature whose tasks and analyze steps are not both done, **Then** "Lock & start the loop" is unavailable and says what is missing. **Given** both are done, **When** the engineer locks, **Then** spec, plan, tasks and the constitution are committed on the feature's branch, and the session can no longer change them.

---

### User Story 2 - Run the locked feature as one loop to a single merge (Priority: P1)

After the lock, Fallow runs the feature's whole `tasks.md` with the project's pipeline (by default, "Standard"):
- A builder works through it phase by phase and task by task. Each phase gets a fresh builder session, and the tasks of a phase share it.
- Each new session starts from the same markdown files: the locked documents and the handovers the earlier phases' builders wrote.
- Fallow commits each task and runs the gates against the baseline after each task.
- At each user story's checkpoint, a reviewer checks every acceptance scenario of that story by running something.
- Failures go back to the builder as a repair in its session, up to a cap, then to the engineer.

When every phase is done and verified, the feature is ready to merge, and the engineer merges once.

**Why this priority**: it is the point of the product. The engineer locks a clear spec and gets a verified result with as few interventions as possible.

**Independent Test**: lock a feature of two user stories in the sandbox repository, start the loop, and do nothing until it is ready to merge. Then check:
- each task has its own commit and gate result;
- each user story was reviewed with evidence;
- one merge brings the whole feature to the target branch.

**Acceptance Scenarios**:

1. **Given** a locked feature, **When** the loop starts, **Then** Fallow runs the gates once on the locked commit as the baseline, and the builder starts on the first task of the first phase.
2. **Given** a task the builder reports done, **When** Fallow checks it, **Then** it commits the task's changes as their own commit and runs the gates against the baseline. A task counts as done only when the gates show no test failing that passed in the baseline.
3. **Given** the last task before a user story's checkpoint, **When** it is accepted, **Then** a fresh reviewer checks every acceptance scenario of that story by running something, including the cases that must fail. The story counts as reviewed only when every scenario is answered with what was run and the result.
4. **Given** a failing gate or a failing review, **When** Fallow handles it, **Then** the builder gets a repair with the reasons in the same session. After the cap (3 by default), the loop asks the engineer instead.
5. **Given** every phase done, every story reviewed and the gates green, **Then** the feature shows "Ready to merge" with the gate results, the criteria with evidence, the time and the cost. **When** the engineer merges, **Then** exactly the gated commit reaches the target branch, once.
6. **Given** a pipeline from which the gates or the reviews were removed, **When** the loop finishes, **Then** the result reads "Done, not verified by Fallow" and names what was not checked. It is never shown as a green "Ready to merge".
7. **Given** a phase whose tasks are all accepted, **When** the builder finishes it, **Then** its handover is committed in the feature's folder, and the next phase starts in a fresh session given the locked documents and every handover so far.
8. **Given** Fallow stops in the middle of a task, **When** it starts again, **Then** the loop resumes that task in the same session, and no completed task, gate run or review runs again.

---

### User Story 3 - Watch what every agent does (Priority: P1)

While a loop runs, its page shows the process as in the mockup's "Orchestration loop" screen:
- **The pipeline**, with the stage it is at.
- **`tasks.md` as a tree**, with the current task marked and done tasks ticked.
- **A card per agent**, with its role, runtime and model, branch, status, current task and cost.
- **One activity feed**, with what each agent says (its messages, not its thinking), the files it read, the edits it made with their diffs, and the commands it ran with their output. The feed can be filtered by agent and follows live.
- **Fallow's own checks**: gates against the baseline, each story's criteria with evidence, and the commit per task.
- **The engineer's controls and the run's totals**: pause, stop, a note for the next step, limits; time, cost against the budget, and repairs.

**Why this priority**: the engineer must be able to understand from the page alone what each agent did and why the result can be trusted. Without this, the loop is a black box.

**Independent Test**: during a loop, pick any finished task. From the page alone, name the agent that did it, the files it changed with the diff, the commands it ran with their results, the commit Fallow made, and the gate result.

**Acceptance Scenarios**:

1. **Given** a running loop, **When** an agent reads a file, edits one or runs a command, **Then** it appears in the feed within 2 seconds, attributed to that agent and the current task.
2. **Given** an edit in the feed, **When** the engineer opens it, **Then** its diff is shown. **Given** a command, **Then** its output is shown, and its result (passed, failed, refused) is marked.
3. **Given** the feed, **When** the engineer filters by one agent or by Fallow's checks, **Then** only those entries show. Turning off "follow live" stops the feed from scrolling.
4. **Given** the task tree, **When** a task is accepted, **Then** it is ticked and linked to its commit and gate result. The current task is highlighted, and a task waiting for an answer is marked as waiting.
5. **Given** the page is closed and reopened, **Then** the whole history of the loop is shown again. The loop never depends on the page being open.

---

### User Story 4 - Questions go through the lead (Priority: P2)

During a step, an agent that meets something the spec does not settle asks through Fallow; agents never talk to each other directly.
- **If the spec, or the conversation from the spec session, answers it,** the lead answers. Fallow commits the answer to the spec, and the asking agent continues within the same step. The feed shows "lead answered", and the engineer can overrule it.
- **If the question is genuinely open,** the lead passes it to the engineer with options and a recommendation. The loop waits for the answer, while other loops and conversations go on. The asking agent's step ends, and it resumes in its session with the committed answer.

**Why this priority**: it keeps the loop moving without guessing. It needs User Story 2 to exist, but is not needed for a first verified result, because a builder can already stop and ask in 006's way.

**Independent Test**: with a scripted agent that asks two questions, one answered by the spec and one not, check that:
- the first is answered by the lead and committed, and the agent continues;
- the second reaches the engineer with options, and the loop waits for it;
- both appear in the feed with who answered and the commit.

**Acceptance Scenarios**:

1. **Given** a builder that asks a question the spec answers, **When** the lead answers it, **Then** the answer is committed to the spec's clarifications, the builder receives it without its step ending, and the feed shows the question, "lead answered", and the commit.
2. **Given** a question the lead judges open, **When** it is relayed, **Then** the engineer sees it in the "Needs you" inbox and on the loop page, with options and the lead's recommendation. The loop waits for the answer; other loops and conversations go on.
3. **Given** the engineer answers, **Then** the answer is committed, and the asking agent resumes in its session with it.
4. **Given** an answer the lead gave, **When** the engineer overrules it, **Then** the reversal is committed, and the tasks built on the old answer are reopened and verified again.

---

### User Story 5 - The dashboard: every project and loop at a glance (Priority: P2)

The dashboard shows, as in the mockup's "Dashboard" screen:
- **the projects**, with their path, git or plain folder, Spec Kit installed, and their number of features;
- **a "Needs you" inbox**, with questions waiting and features ready to merge;
- **the running loops**, with phase progress, the current phase and task, what each agent is doing, time and cost;
- **every feature with its stage**: spec session, locked, building, ready to merge, merged.

Several loops can run at once, in one project or several, within a concurrency and memory cap.

**Why this priority**: it turns Fallow from one loop into a place to work from. A single project and loop is already usable through User Stories 1–3.

**Independent Test**: add two projects, start a loop in each, and check the dashboard. It must show both loops with correct progress, put a question from either in the inbox, and keep the number of agents running at once within the cap.

**Acceptance Scenarios**:

1. **Given** two running loops, **Then** the dashboard shows both with their phase progress, current task, agents' activity in a few words, time and cost, updated live.
2. **Given** a loop that needs the engineer, **Then** it appears in "Needs you" with the question or the merge, and the header shows how many items need them.
3. **Given** more loops ready to start an agent step than the cap allows, **Then** the extra steps wait and say they wait for a slot.

---

### User Story 6 - Agents and pipelines the engineer designs (Priority: P3)

The engineer keeps a library of agents and composes pipelines from fixed building blocks, as in the mockup's "Pipeline designer" screen.
- **Agent definitions** are markdown files with frontmatter: name, description, runtime, model, kind, the commands it may run, and its result shape. There are two kinds:
  - a **maker** changes files: Fallow commits its work and runs the gates;
  - a **checker** cannot edit, but must run and verify, answering every check with what it ran and the result. A failing verdict leads to a repair.
- **The defaults** are lead, builder, reviewer, test-writer, security-reviewer and docs-writer.
- **Where they live**: the defaults and the engineer's own agents are in the user's Fallow folder, and a project can override them in its own `.fallow/agents/`.
- **A pipeline step** names:
  - an agent;
  - when it runs: per task, per phase, per user story checkpoint, once, or only when given paths changed;
  - what happens when it fails: a repair by the builder, or the engineer.
- **The default pipelines** are Standard, Test first, and Solo (no review). The lead can design a pipeline with the engineer.

**Why this priority**: it makes Fallow fit different ways of working. The Standard pipeline already covers User Stories 1–5.

**Independent Test**: add a custom checker agent, add it to a copy of Standard as a step that runs only when files under `api/` change, and run a loop. Check that:
- it runs only after such changes;
- its verdict lists what it ran;
- its failure leads to a repair.

**Acceptance Scenarios**:

1. **Given** the agent library, **Then** it lists the default and the engineer's agents with their kind and source (default, yours, or a project override). Opening one shows its definition, which can be edited.
2. **Given** a checker agent, **When** it runs, **Then** it cannot change files, and a verdict that does not say what was run for each check does not count.
3. **Given** a pipeline, **When** the engineer adds, removes or reorders steps, **Then** the designer shows the result as in the mockup. Removing the gates or every review is allowed, with a warning that results will be "not verified by Fallow".
4. **Given** the engineer asks the lead to design a pipeline, **When** the lead proposes one, **Then** it appears in the designer for the engineer to edit and save. It is never used before the engineer saves it.

---

### User Story 7 - Agents from different providers (Priority: P3)

Every agent runs through one runtime interface. The interface can:
- start a session with an instruction, a working copy, the allowed commands and a result shape;
- stream back the agent's messages, tool calls, result and cost;
- resume a session.

Claude Code is the first runtime. Others (Codex, Gemini) can be added later without changing the loop, the pipelines or the page.

**Why this priority**: the product must not be tied to one provider. Only one runtime is needed for the first version.

**Independent Test**: run the full loop once with the Claude runtime and once with a scripted second runtime that implements the same interface. Check that the loop, the feed and the checks behave the same.

**Acceptance Scenarios**:

1. **Given** an agent definition naming a runtime, **When** its step runs, **Then** that runtime is used, and its card shows the runtime and model.
2. **Given** a runtime that is not installed or fails to start, **Then** the step fails with a message naming the runtime, and the loop asks the engineer.

---

### Edge Cases

- **A plain folder (no git).** Spec sessions work. Starting a loop needs git: Fallow offers to set it up (an initial commit of what is there) and only does so if the engineer agrees.
- **Spec Kit already installed**, possibly another version. Fallow keeps the existing files, installs nothing over them, and says which version it found.
- **A project folder that was moved or deleted.** The project shows as missing, with its history kept, until the engineer removes it or points it to the new place.
- **The engineer edits a document while the lead's turn runs.** Neither edit is lost: while a turn runs, the documents in the page are read-only, and what the engineer types is kept as a draft. After the turn, a draft for a document the lead did not change is applied; for one it did change, both versions are shown for the engineer to merge.
- **Locking with open `[NEEDS CLARIFICATION]` markers or analyze findings.** The lock is refused, and the open items are named.
- **A `tasks.md` Fallow cannot read** (no phases, no task ids, or no user story checkpoints). The lock is refused with what is wrong, before any agent step.
- **A task the builder cannot finish in one step.** It continues the task in its session. A task that fails beyond the repair cap waits for the engineer.
- **The lead's answer is later overruled after the story was reviewed.** The story's tasks built on it are reopened and its review runs again.
- **A loop running while the engineer wants to change the spec.** They unlock it (FR-009); the loop pauses, and after the new lock the stories the change affects are verified again.
- **Unlocking while a question waits for the engineer.** The question stays open in the spec session, and the engineer can answer it there or make it moot with the change.
- **Cost or time budget reached in the middle of a task.** The loop pauses at the end of the current step and asks; paused time does not count.
- **Two loops in one project touching the same files.** Each works on its own branch and working copy. A conflict only shows at merge, where Fallow refuses to merge a branch that does not merge cleanly and says so.

## Requirements *(mandatory)*

### Functional Requirements

**Projects and the dashboard**

- **FR-001**: The engineer MUST be able to add any local folder as a project, git repository or not, and remove it from Fallow without deleting anything in it.
- **FR-002**: Adding a project MUST install the Spec Kit skills and templates in it, and MUST NOT overwrite existing files. Fallow MUST ship the Spec Kit version it installs, and MUST NOT download it.
- **FR-003**: The dashboard MUST show the projects, the "Needs you" inbox, the running loops with phase progress, and every feature with its stage (spec session, locked, building, ready to merge, merged, done not verified), as in the mockup's Dashboard screen.
- **FR-003a**: When something needs the engineer, Fallow MUST show it in the "Needs you" inbox, with the count in the header, and MUST raise a desktop notification that opens the item on the page when clicked. This covers a question for them, a feature ready to merge, and a loop that stopped at a limit, a repair cap or an error.
  - The notification is raised by Fallow on the engineer's machine; nothing leaves it.
  - It names the project, the feature and what is needed, and never includes code or command output.
  - The engineer MUST be able to turn notifications off. One item raises one notification, not one per retry.
- **FR-003b**: Fallow MUST show what it keeps on disk and how large it is: per project, its loops, conversations and worktrees. The engineer MUST be able to delete a loop's or conversation's data, after a confirmation naming what is removed (constitution V). This view follows the mockup's style; it is reached from the dashboard.
- **FR-004**: Several loops MUST be able to run at once, across or within projects, limited by a cap on concurrent agent steps and Fallow's memory budget. Steps over the cap wait and say so.

**The spec session**

- **FR-005**: The engineer MUST be able to start a feature in a project and hold a chat with the lead agent about it, in the page. The conversation MUST be kept with the feature and survive restarts.
- **FR-006**: The lead MUST follow Spec Kit's steps (constitution while the project's is still Spec Kit's template, specify, clarify, plan, tasks, analyze). A stepper MUST show the step it is at, and the engineer MUST be able to move it to the next step or back to an earlier one.
- **FR-007**: The feature's documents MUST be shown beside the chat and update as the lead writes them. The engineer MUST be able to edit them directly; the lead MUST receive the engineer's edits on its next turn. Open `[NEEDS CLARIFICATION]` markers MUST be highlighted. Clarify questions MUST be shown as choices with the lead's recommendation.
- **FR-007a**: In the project conversation and the spec session, what the lead may run MUST follow a mode the engineer switches like Claude Code's permission modes. The current mode is shown as a chip by the message box, and Shift+Tab or a click on the chip moves to the next mode:
  - **Read only**: the lead reads files and writes the spec documents, and runs nothing;
  - **Approved commands**: it also runs the project's approved commands (the gates and the plan's commands) without asking;
  - **Ask** (the default): it also asks the engineer before any other command, showing the exact command. The engineer can allow it once or "always in this project", which adds it to the project's approved commands. The engineer can see and edit that list;
  - **Run anything**: any command without asking, until the engineer switches back.

  In every mode, the lead never edits code: in these chats it writes only `specs/`, the constitution and drafts in `.fallow/drafts/`, never the agent definitions, pipelines or gates that decide what agents may run. The mode belongs to that one chat, and a new conversation starts in Ask. When the lead answers questions during a loop, it has the rights of Approved commands, whatever mode the chat was in.
- **FR-008**: Locking MUST be possible only when tasks and analyze are done, with no open clarification markers, no CRITICAL analyze finding left undismissed, `spec.md`, `plan.md` and `tasks.md` readable by Fallow, and every user story having at least one acceptance scenario marked `(must fail)`. A refused lock names each problem. Locking MUST commit the documents on the feature's branch (creating it if needed) and start the loop.
- **FR-009**: After the lock, Fallow MUST change the documents only by ticking accepted tasks in `tasks.md`, adding handovers, and committing answers to questions (User Story 4). The engineer MUST be able to unlock a feature:
  - the loop pauses at the end of its current step, and the spec session opens again with its history;
  - when the engineer locks again, the change is committed, and the loop continues;
  - the user stories the change affects are reopened and verified again, including stories already reviewed;
  - completed work the change does not affect is kept.

**The loop**

- **FR-010**: A loop MUST run the whole locked `tasks.md`, phase by phase and task by task, in the order and grouping it gives, with the steps of the feature's pipeline.
- **FR-010a**: Each phase MUST get a fresh builder session; the tasks of a phase, and the repairs from its review, share it.
  - At the end of its phase, the builder MUST write a short handover in markdown: what it built, where, the decisions it made, and what the next phase should know. Fallow commits it to the feature's folder (`specs/NNN-name/handover/`).
  - A new builder session MUST start from the locked documents and every earlier handover, and from nothing else carried over.
  - A phase whose handover is missing or empty MUST NOT count as done.
- **FR-011**: Fallow MUST commit each task's changes as their own commit, and run the gates against the baseline from the locked commit after each task. A task counts as done only when Fallow's checks pass, never on an agent's word.
- **FR-012**: At each user story checkpoint, a reviewer (a checker agent) MUST check every acceptance scenario of that story by running something, including the must-fail cases. An incomplete verdict MUST be asked for again, not passed to the engineer.
- **FR-013**: A failing gate or verdict MUST become a repair for the builder in its session, with the reasons, up to the pipeline's cap (3 by default). Beyond the cap, the loop MUST ask the engineer.
- **FR-014**: The engineer MUST merge once per feature, and exactly the gated commit MUST reach the target branch. A branch that does not merge cleanly MUST NOT be merged.
- **FR-015**: A result from a pipeline without gates or without reviews MUST be shown as "Done, not verified by Fallow", naming what was not checked.
- **FR-016**: Everything 006's loop does MUST keep working, now per task:
  - resume after a stop or crash without repeating completed work;
  - limits on time, cost and repairs (FR-016a, FR-013);
  - pause, stop, and a note for the next step;
  - retrying a step lost to a transient error.

**Seeing the loop**

- **FR-016a**: Every loop MUST have a cost budget and a time budget:
  - **At the lock**, Fallow suggests both from the number of tasks, using per-task amounts set per project (by default $0.25 and 10 minutes per task). The engineer can change them before locking, and raise them later.
  - **When either is reached**, the loop pauses at the end of the current step and asks the engineer, offering "continue for one step" and "raise the limit". Paused time and time spent waiting for the engineer do not count.
  - **The cost counts every agent of the loop**, including the lead's answers during the loop. The spec session and the project conversation are not counted.
- **FR-017**: The loop page MUST show:
  - the pipeline with its current stage;
  - the task tree;
  - the agent cards;
  - the activity feed;
  - Fallow's checks: gates against the baseline, criteria per story with evidence, and the commit per task;
  - the engineer's steering;
  - time, cost against the budget, and repairs.

  The layout follows the mockup's "Orchestration loop" screen.
- **FR-018**: The activity feed MUST show each agent's messages, its file reads, its edits with diffs, and its commands with their output and result, attributed to the agent and the task, within 2 seconds. It MUST NOT show agents' thinking. It MUST be filterable by agent and by Fallow's checks, and follow live unless the engineer turns that off.

**Questions through the lead**

- **FR-019**: Every agent MUST be able to ask a question through Fallow during its step. Agents MUST NOT be able to message each other or the engineer directly.
- **FR-020**: The lead MUST answer a question when the locked documents or the spec session's conversation settle it. The answer MUST be committed to the spec's clarifications, and the asking agent MUST receive it within the same step.
- **FR-020a**: The lead's freedom to answer MUST follow the autonomy level, set per project and overridable per feature until the lock (or at an unlock):
  - **Strict**: every question goes to the engineer, with the lead's recommendation;
  - **Normal** (the default): the lead answers what the locked documents or the engineer's conversations settle;
  - **Free**: the lead also decides small open questions itself. A small question is a question that touches no reserved item, no acceptance scenario, no public interface (commands, API, file or data formats) and no dependency. Each such decision is committed, marked "decided by the lead", and listed for the engineer to confirm or overrule; it never blocks the loop.

  Whatever the level, a question about something the engineer marked as theirs in the spec, or that would change an acceptance criterion, always goes to the engineer.
- **FR-021**: A question the lead judges open MUST go to the engineer with options and the lead's recommendation. The loop waits for the answer: with one builder working in order, the rest of the loop depends on it. Other loops and conversations go on. The asking step ends, and resumes in its session with the committed answer.
- **FR-022**: The engineer MUST be able to overrule any answer the lead gave. The reversal is committed, and the tasks built on the old answer are reopened and verified again.
- **FR-023**: Every question and answer MUST appear in the feed, with who answered (lead or engineer) and the commit.

**Agents and pipelines**

- **FR-024**: Agent definitions MUST be markdown files with frontmatter giving name, description, runtime, model, kind (maker or checker), the commands it may run, and its result shape. The defaults and the engineer's own agents MUST live in the user's Fallow folder; a project's `.fallow/agents/` MUST override them by name.
- **FR-025**: Fallow MUST ship the default agents (lead, builder, reviewer, test-writer, security-reviewer, docs-writer) and the default pipelines (Standard, Test first, Solo), and MUST NOT overwrite the engineer's changes to them on update.
- **FR-026**: Fallow MUST enforce the agent's kind:
  - a maker's changes are committed and gated by Fallow;
  - a checker cannot edit, and its verdict counts only when every check names what was run and the result.

  Whatever the definition says, every agent MUST get its own working copy, only the commands listed (the gates, the definition's, and those the locked plan names), a fixed result shape, and limits on time, turns and cost.
- **FR-027**: A pipeline step MUST name an agent, when it runs (per task, per phase, per user story checkpoint, once, or when given paths changed), and what happens on failure (repair by the builder, or the engineer). The engineer MUST be able to create, edit, copy and delete pipelines in the designer, and choose one per project and per feature before the lock.
- **FR-028**: The lead MUST be able to propose a pipeline in conversation with the engineer. It MUST NOT be used until the engineer saves it.

**Runtimes**

- **FR-029**: Every agent step MUST go through one runtime interface: start, stream events (messages, tool calls, result, cost), resume. Claude Code MUST be the first runtime. Adding another MUST NOT change the loop, the pipelines or the page.

**The look**

- **FR-030**: The page MUST match the approved mockup:
  - its five screens and their layout and components;
  - its fonts: Inter for the interface, JetBrains Mono for code, paths and commands;
  - its colors:
    - ground #F6F6F3, panels #FFFFFF, borders #E3E2DC;
    - ink #1C1C1A, muted text #5E5D58;
    - accent #2F52C9;
    - "needs you" #B4470F on #FBEDE3, success #226640 on #E6F3EA, spec sessions #5B3BB0 on #F3EEFB;
  - the Fallow logo (three curved strokes in #2F52C9).

  The fonts MUST be shipped with Fallow, since the page loads nothing from outside Fallow.
- **FR-031**: The page MUST work from 390 px wide (phone) to wide screens without horizontal page scrolling. Side columns stack on narrow screens, and wide tables scroll inside their box.
- **FR-032**: Every control MUST be reachable and usable by keyboard. Text MUST meet a 4.5:1 contrast ratio.

**Project scope**

- **FR-033**: 006's single-task `fallow run` flow, in which planning and building happened without a spec session, MUST be replaced. Starting Fallow opens the dashboard.
- **FR-034**: Each project MUST have its own conversation with the lead, about the project as a whole:
  - questions about the code;
  - designing pipelines;
  - deciding which features to make.

  The engineer MUST be able to start a feature from it, which opens the feature's spec session with the relevant context carried over. The project conversation MUST be kept with the project and survive restarts. Its screen is the mockup's fifth screen, "Project conversation".

### Key Entities

- **Project**: a local folder the engineer added. It has a path, its kind (git or plain), its Spec Kit version, its features, its default pipeline, and its agent overrides.
- **Feature**: one `specs/NNN-name` in a project. It has its stage, its documents, its spec session, its pipeline, its branch, and its loop once locked.
- **Project conversation**: the engineer's chat with the lead about a project as a whole, and the features started from it.
- **Spec session**: the conversation between the engineer and the lead for a feature. It has the messages, the Spec Kit step reached, and the engineer's direct edits.
- **Lock**: the commit that fixes a feature's documents and starts or continues its loop. It holds the commit, the time, and the pipeline used. A feature can be unlocked and locked again; each lock is kept.
- **Loop**: the run of a locked feature. It has its state, the current phase and task, its agent steps, gate runs, verdicts, questions, repairs, steering actions, time and cost. It is kept as 006's append-only event log.
- **Handover**: a markdown file the builder writes at the end of a phase, committed in the feature's folder. It is what the next phase's builder starts from, besides the locked documents.
- **Task**: an entry of `tasks.md`. It has its id, phase, user story, the parallel mark, its state, its commit, and its gate result.
- **Agent definition**: a markdown file. It has its name, description, runtime, model, kind, allowed commands, result shape, and source (default, the engineer's own, or a project override).
- **Pipeline**: an ordered list of steps (agent, when, on failure). It has a name and a source.
- **Chat mode**: Read only, Approved commands, Ask or Run anything, per conversation with the lead.
- **Approved commands**: the project's list of commands agents may run without asking (the gates, the plan's commands, and those the engineer allowed always), visible and editable.
- **Autonomy level**: Strict, Normal or Free. It is set per project, can be overridden per feature, and is shown on the loop page.
- **Question**: asked by an agent at a task. It holds the text, the options, who answered (lead or engineer), the answer, the commit, and whether it was overruled.
- **Activity event**: one thing an agent said or did (a message, a read, an edit with diff, a command with output). It has the time, the agent and the task.
- **Runtime**: a provider adapter. It has its name, whether it is installed, and the models it offers.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From adding a folder to the lead's first reply in a new feature takes under 1 minute, excluding the agent's own response time.
- **SC-002**: On the engineer's task set, in at least 4 of 5 loops, the only interventions after the lock are the merge and questions the lead relayed as genuinely open. 007 records every loop for this; the measurement on a task set with hidden tests is a later feature, and until then 007 does not claim it (constitution VIII).
- **SC-003**: No feature is ever shown as "Ready to merge" without every task's gates and every story's review having passed. This is shown by tests with injected false claims, missing reviews and removed gates.
- **SC-004**: For any task of a finished loop, the engineer can name from the page alone, in under 1 minute:
  - the agent that did it;
  - its changes with the diff;
  - the commands it ran with their results;
  - the commit;
  - the gate result.
- **SC-005**: An agent's action appears on the page within 2 seconds, p95 over the reference run.
- **SC-006**: A loop stopped at any stage resumes after a restart without repeating a completed task, gate run or review.
- **SC-007**: A side-by-side check of the five screens against the mockup, at 1440 px wide, shows the same fonts, colors, logo and layout. The engineer approves it.
- **SC-008**: The five screens work at 390 px wide with no horizontal page scrolling.
- **SC-009**: Fallow's own process stays within the constitution's memory budgets (under 60 MB idle, under 200 MB with 4 concurrent agent steps), and the dashboard answers within 500 ms of Fallow starting.

## Assumptions

- **006's loop engine is the core of each loop.** Its event log, gates against a baseline, verdict checks, repairs in the same session, resume, limits and the Claude Code adapter are reused. The loop runs per task instead of per plan part.
- **The questions channel is a local MCP server that Fallow serves to each agent**, with an `ask` tool. When the answer must come from the engineer, the asking step ends and is resumed later rather than holding the call open.
- **The lead continues from its spec-session context** when answering questions, so it knows what the engineer said and not only what was written.
- **One builder per loop in this version.** `[P]` tasks run one after another. Parallel builders for `[P]` tasks or parts are later work. Checkers that run at the same point (for example, a review and a security review) may run at the same time.
- **The constitution is per project.** While it is still Spec Kit's template (its `[PROJECT_NAME]` placeholders unfilled), the lead's first spec session starts with it.
- **Spec Kit is shipped with Fallow** at a pinned version and copied into the project. Fallow does not run Spec Kit's own installer, which downloads its templates.
- **Git is required for a loop.** A plain folder gets git only when the engineer agrees, at the first lock.
- **The target branch for the merge** is the branch the feature was locked from. Pushing, pull requests and CI (006's P5) are out of scope here and kept for later.
- **The engineer's agent definitions live in the user's Fallow folder** (`~/.fallow/agents/`), not in Claude Code's own folder, so they do not appear as subagents in the engineer's other Claude Code work.
- **The constitution needs amendments.** Installing Spec Kit and agents into a project writes outside a run's working copies (VI). The lead answering questions and the user removing gates must be squared with II and I. These amendments are proposed with this feature's plan, as 006 did.
- **The mockup's content is illustrative.** Its screens fix the look and layout, not the sample data.
