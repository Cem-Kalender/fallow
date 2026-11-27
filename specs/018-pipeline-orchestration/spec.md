# Feature Specification: Pipelines as an orchestration system

**Feature Branch**: `018-pipeline-orchestration` (the spec lives in `specs/018-pipeline-orchestration`)

**Created**: 2026-11-27

**Status**: Draft

**Input**: User description:

> The engineer, 2026-11-27, looking at the pipeline designer:
> - "please remove the labels "verifying" and "not verifying"."
> - "I also want to make the DAG of the pipeline more functional. The user should be able to fan-out, using multiple agents (i.e. builders, reviewers, testers) in parallel. Now the DAG is very basic, and it needs to be an orchestration system where you can place agents at every place if you wanted to."
> - Asked why a reviewer "at every phase" is "not verifying" while "at each story's checkpoint" is; the answer stands: a phase is a block of tasks, a checkpoint is a finished user story whose acceptance scenarios a checker must run.
> - On parallel builders: "inclined to choose option 1 ([P] tasks in parallel). But what if it doesnt make sense to use multiple agents, even though the user has selected that pipeline when locking the spec?"
>
> Decisions by the engineer (2026-11-27), listed under Clarifications "from the brief": the labels go from the designer and the pipeline list only; `[P]` tasks run at the same time, each with its own builder in its own working copy, up to a maximum the pipeline sets, and Fallow says at the lock what will actually happen; checkers at the same point run in parallel and the point passes only when all pass; agents can be placed at fixed points that follow `tasks.md`, never in a free-form graph.

## Context

A pipeline is a TOML file of steps (007 contracts/pipeline.md). Each step names an agent, when it runs (`task`, `phase`, `checkpoint`, `once`), where (`at` start or end, not for `task`), an optional `paths` limit, what happens on a failure, and which checkers run alongside it (`parallel_with`). The loop (007 US2, FR-010 to FR-016a) runs the locked `tasks.md` phase by phase and task by task with one builder session per phase. Today:

- **The designer labels every pipeline.** The pipeline list and the designer mark each pipeline with a green "verifying" or an orange "not verifying" pill, and "Based on" adds "(not verified)" (008 FR-009, US4 AS5). The engineer finds the labels confusing: a reviewer at every phase is "not verifying" while one at each story's checkpoint is, which the label does not explain.
- **Only a few places take agents.** Per task, only the builder and the gates may run: a checker per task is refused, and nothing runs before or after a task. A checker at a start is refused. Exactly one maker runs per task.
- **"Alongside" only half works.** Checkers marked `parallel_with` run at the same time only at a story's checkpoint end (008 FR-014). Elsewhere the field is accepted but they run one after the other.
- **One builder, always.** Tasks marked `[P]` in `tasks.md` (Spec Kit's "different files, no dependencies") run one after the other like any other task, in the phase's one builder session.

**What the engineer wants**: a pipeline places any number of agents of any kind at any of the fixed points that follow `tasks.md` (before and after each task, at the start and end of each phase, at each story's checkpoint, once at the start and end), one after the other or side by side; the builders fan out over `[P]` tasks, up to a maximum the pipeline sets; checkers at the same point fan in (the point passes only when all pass). Fallow, not an agent, still decides what runs next, from `tasks.md` and what it has verified. The designer drops the verifying labels and says plainly what a pipeline checks.

**What this replaces in earlier features** (FR-030 lists them one by one):
- 007 contracts/pipeline.md: the checks "a checker per task is refused", "a checker at the start is refused", "exactly one maker with `when = task`", the `at` row (now also for tasks), the `parallel_with` row ("elsewhere … run one after the other"), the sentence "The designer shows the same warning"; a new cap for builders.
- 007 FR-010 and FR-010a: `[P]` tasks may run at the same time, each in its own working copy and session.
- 007 FR-011: a merged group of `[P]` tasks is gated as a whole too.
- 007 FR-027: a step may run before or after each task, and checkers may run per task.
- 007 US6 AS3: the warning on removing the gates or every review becomes a plain statement without a label.
- 008 FR-009, US3's independent test ("verifying" marks), US4 AS5: no verifying label; 008 FR-011 and FR-013: the graph's points and settings; 008 FR-014 and FR-015: running alongside at every point, not only at a checkpoint's end.

**What does not change**: where pipelines live and which one wins (built in, `~/.fallow/pipelines/`, a project's `.fallow/pipelines/`); how shipped defaults are copied and updated (007 FR-025); when a result is verified (constitution I, 007 FR-015: gates after every task and a checker without `paths` at every story's checkpoint end) and the words "Done, not verified by Fallow: <what was not checked>"; the checkpoint's scenario check (007 FR-012); the caps on repairs and review rounds; the loop's budgets (007 FR-016a); the slot limit on agents at once across loops (007 research S-19); the rights of each kind (constitution VI); 011's updates and overlap warnings between loops; agents never message each other (constitution III).

**Out of scope**:
- A free-form graph (edges the engineer draws, conditions, loops other than repairs). The points are fixed and follow `tasks.md`.
- Makers side by side at one point (for example two writers after a phase): only `[P]` tasks fan builders out (constitution IV, Clarifications).
- Fallow deciding by itself to split a task, or running tasks in parallel that `tasks.md` does not mark `[P]`.
- An agent choosing which step runs next or how many builders run.
- New shipped agents (a "tester" is any checker the engineer defines).

## Clarifications

### Session 2026-11-27

- Q: Which labels go? → A (from the brief): the "verifying" and "not verifying" labels and their colours, from the pipeline designer and the pipeline list only. A finished run still says "Done, not verified by Fallow: <what was not checked>" where that applies (constitution I). The designer may still say plainly what a pipeline checks.
- Q: How do builders fan out? → A (from the brief): tasks marked `[P]` in the locked `tasks.md` run at the same time, each with its own builder in its own working copy. Fallow checks the files each one actually touched, and merges them. The pipeline sets a maximum ("up to N builders"), not a fixed number: Fallow runs as many as there are `[P]` tasks ready, at most N; with no `[P]` tasks, one builder, as today.
- Q: What if parallel builders make no sense for the feature locked with that pipeline? → A (from the brief): at the lock, Fallow states what will actually happen ("no [P] tasks: the pipeline's 3 builders will work as 1"; "phase 3 has 4 [P] tasks: 3 builders at a time"), so the engineer can choose another pipeline.
- Q: What if parallel tasks touched the same files? → A (from the brief): Fallow does not merge blindly. The later one is repaired on top of the earlier (011's conflict handling) and it is reported. Fallow decides, not an agent.
- Q: How do several checkers at one point combine? → A (from the brief): they run in parallel; the point passes only when all pass; any finding goes back for repair, within the caps.
- Q: Is the pipeline a free graph? → A (from the brief): no. Fixed points that follow `tasks.md` (before and after each task, start and end of each phase, each story's checkpoint, once at the start and the end), and at every point any number of agents of any kind, in parallel or in sequence. Fallow decides the order from `tasks.md`.
- Q: May makers run side by side at a point? → A (decided in /speckit-specify, not in the brief): no. Makers at one point run one after the other; checkers may run side by side. Two makers editing at once is a split the plan did not declare (constitution IV); the only fan-out of makers is the builders over `[P]` tasks, which the locked plan declares.
- Q: Which tasks are "ready" at the same time? → A (decided in /speckit-specify, not in the brief): a group is a run of consecutive `[P]` tasks within one phase. Its tasks may run at the same time; the task after the group (or the phase's end) waits until every task of the group is accepted and merged. Tasks are merged in `tasks.md` order, so "the later one" is the later in `tasks.md`.
- Q: When is a group run in parallel? → A (decided in /speckit-specify, not in the brief): when the pipeline allows more than one builder and the group has at least two tasks. Then every task of the group gets its own working copy and builder session, whatever the free slots, so what happens follows from the pipeline and `tasks.md` alone. Otherwise the tasks run one after the other in the phase's session, as today.
- Q: How is a merged group gated? → A (decided in /speckit-specify, not in the brief): each task is gated on its own commit, as today; when all of the group is merged, the gates run again on the merged result before anything else runs. A failure there is a repair of the group, within the cap on repairs per task.
- Q: What is the most a pipeline may allow? → A (decided in /speckit-specify, not in the brief): between 1 and 8 builders; 1 when not written. The setting for agents at once across Fallow (default 4) still applies on top.
- Q: May a checker run at a start point? → A (decided in /speckit-specify, not in the brief): yes, with `on_fail = "engineer"` only: at a start nothing of that point's work is built yet, so there is nothing for the builder to repair.
- Q: Does Fallow ship a pipeline that uses the new points? → A (decided in /speckit-specify, not in the brief): yes, one more default, `parallel`: up to 3 builders on `[P]` tasks, the gates after every task, and the reviewer and the security reviewer side by side at each story's checkpoint. Standard, Test first and Solo do not change, so the engineer's copies of them are not touched by the update.
- Q (/speckit-clarify): How is a task "repaired on top of the earlier" one? → A (decided in /speckit-clarify, from 011's update): Fallow merges the build branch as it is then into the task's own working copy, as 011 merges the target into a loop's branch. The task's builder, in the task's session, gets a repair naming each shared file and what each side changed, with the instruction that both tasks' work must keep working; where the merge conflicted, the markers are left for it to resolve. Fallow then checks as after an update (no markers left, the changes committed, the gates on the task's new commit) and compares again, against what the build branch changed since that merge.
- Q (/speckit-clarify): Who writes the handover of a phase whose tasks ran partly in parallel? → A (decided in /speckit-clarify): the phase's own builder session, as today (007 FR-010a). Its instruction names the tasks built alongside it and their commits on the build branch; when the phase had no session of its own yet (it began with a group), a fresh session writes it from the locked documents and those commits.
- Q (/speckit-clarify): Which session repairs a merged group whose gates failed? → A (decided in /speckit-clarify): the phase's session, or a fresh one with the phase's instruction when there is none, as 007's repairs do today; the repair is the group's, named by its tasks.
- Q (/speckit-clarify): Must the designer's new controls be usable by keyboard and screen reader? → A (decided in /speckit-clarify, from 008 FR-018): yes, every + at a point, step and the builders field, with 4.5:1 text contrast.
- Q (after the US3 review, round 1): What happens to a group in progress when the engineer locks again? → A (decided by the engineer's stand-in after review): the new lock is taken in only when no step of the group is running: the running builders finish their current step first, and nothing new of the group starts meanwhile. When it is taken in, the tasks of the group already merged into the build branch stay merged; every task of the group not yet merged is dropped: its branch and working copy are removed, it is reset to not built, and the history says so ("T007 built alongside was not merged before the new lock: built again"). The phase goes on under the new lock, with groups computed from the new `tasks.md`. A task is never left accepted with its work unmerged; no two builders ever share a working copy; the builders cap is never exceeded.
- Q (after the US3 review, round 1): What if two tasks of a group ask the engineer at once? → A (decided after review, from FR-018): each question holds only its own task; neither replaces the other; the group's other tasks go on, and each answer goes to the step of the task that asked.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The designer says what a pipeline checks, without labels (Priority: P1)

The engineer opens Pipelines. No pipeline carries a "verifying" or "not verifying" pill, in the list, in the designer, or in "Based on". With a pipeline selected, the designer says in plain words what it checks ("Fallow runs the gates after every task; the reviewer checks every user story's acceptance scenarios at its checkpoint") and, when something is not checked, what a result will say ("A result of this pipeline ends as 'Done, not verified by Fallow': no review at checkpoints"). A finished loop still says "Done, not verified by Fallow" with what was not checked.

**Why this priority**: the engineer asked for it directly, and it is independent of everything else.

**Independent Test**: open the Pipelines tab with Standard, Solo and a pipeline whose only checker runs per phase; check the list, the designer and "Based on" for the labels and their colours; read each pipeline's statement; run a loop on Solo and read its outcome.

**Acceptance Scenarios**:

1. **Given** the Pipelines tab, **When** it shows Standard and Solo, **Then** neither the list nor the designer nor "Based on" shows the words "verifying" or "not verifying", or a green or orange mark for them.
2. **Given** Standard selected, **When** the designer shows it, **Then** it states that the gates run after every task and that the reviewer checks every user story at its checkpoint.
3. **Given** a pipeline whose only checker runs at the end of every phase, **When** the designer shows it, **Then** it states that a result ends as "Done, not verified by Fallow" and names "no review at checkpoints".
4. **Given** a loop on Solo, **When** it finishes, **Then** its page reads "Done, not verified by Fallow: no review at checkpoints", never in the colours or words of a verified result. (must fail: it is not shown as verified)
5. **Given** the engineer removes the last checkpoint checker in the designer, **When** the graph is redrawn, **Then** the statement names what is no longer checked, without a label.

---

### User Story 2 - Agents at every point, one after another or side by side (Priority: P1)

The engineer places agents at any fixed point: before or after each task, at the start or end of each phase, at the start or end of each story's checkpoint, once at the start or end of the loop. At one point there may be several agents. Checkers marked to run alongside each other run at the same time and the point passes only when all pass; their findings go to one repair. Makers at one point run one after the other. Fallow runs the points in the order `tasks.md` gives.

**Why this priority**: it is the orchestration the engineer asked for; parallel builders (US3) build on the same points.

**Independent Test**: on the fake runtime, lock a feature with a pipeline that has a checker after each task, a maker before each task, the reviewer and a second checker alongside each other at the end of every phase, and a checker once at the end; check the order and overlap of the steps, the repairs, and the outcome.

**Acceptance Scenarios**:

1. **Given** a checker placed after each task, **When** a task's builder finishes and the gates pass, **Then** the checker runs on that task's commit before the next task starts.
2. **Given** a checker after each task that reports a finding, **When** it is judged, **Then** the task goes back to its builder as a repair with the finding, counted against the repairs per task; beyond the cap, the loop asks the engineer. (must fail: the task is not accepted)
3. **Given** a maker placed before each task, **When** a task is due, **Then** the maker runs first, its work is committed by Fallow, and the builder starts on top of it.
4. **Given** two checkers alongside each other at the end of every phase, **When** a phase ends, **Then** both start without waiting for each other, the point is judged when both have finished, and the next point starts only when both passed.
5. **Given** those two checkers, **When** both report findings, **Then** one repair carries both findings, and afterwards both run again, within the cap. (must fail: the phase does not end)
6. **Given** one of them fails and the other passes, **When** the point is judged, **Then** the point does not pass. (must fail)
7. **Given** two makers placed at the same point, **When** the pipeline is saved with them marked alongside each other, **Then** the save is refused, naming the steps and saying makers at one point run one after the other. (must fail)
8. **Given** a checker at a start point with `on_fail = "repair"`, **When** the pipeline is saved, **Then** it is refused with the reason (nothing built yet to repair); with `on_fail = "engineer"` it is saved. (must fail for the first)
9. **Given** the designer, **When** the engineer opens it, **Then** the graph shows each point as a place with a + to add a step, the steps at each point in order, side-by-side checkers as branches that join again, and the repair back-edges; the TOML view shows the same pipeline.
10. **Given** the gates step or the builder, **When** the engineer tries to place it before or after a task, at a checkpoint or once, **Then** it is refused: the builder is the task, and the gates run right after it (or per phase).

---

### User Story 3 - Builders fan out over [P] tasks (Priority: P1)

The engineer locks a feature with a pipeline that allows up to 3 builders. In a phase with four consecutive `[P]` tasks, Fallow starts three builders, each on one task in its own working copy and session, and the fourth when one of them is done. Each task is gated on its own commit. Fallow checks the files each task actually changed and merges the tasks into the build branch in `tasks.md` order. When a later task changed a file an earlier one of the group changed, Fallow does not merge it: its builder repairs it on top of the earlier one, and the loop page says so. When the group is merged, the gates run on the result; then the next task starts.

**Why this priority**: it is the fan-out the engineer asked for, and the riskiest part (merges, cost).

**Independent Test**: on the fake runtime, lock a feature whose phase 2 has four `[P]` tasks (two of which change the same file) and then a plain task, with a pipeline allowing 3 builders; check the overlap in time, the working copies, the merges, the repair, the gates on the merged result, the next task's start, the costs and the outcome.

**Acceptance Scenarios**:

1. **Given** four consecutive `[P]` tasks and up to 3 builders, **When** the phase reaches them, **Then** three builder steps run at the same time, each in its own working copy, and the fourth starts when one of the three is accepted.
2. **Given** the "most agents at once" setting is 2, **When** the same phase runs, **Then** at most 2 builders run at once, and the rest wait for a free place.
3. **Given** `[P]` tasks that changed different files, **When** all are accepted, **Then** Fallow merges them into the build branch in `tasks.md` order and the build branch holds each task's commit and its tick in `tasks.md`.
4. **Given** two `[P]` tasks of a group that both changed the same file, **When** the later one (in `tasks.md`) is to be merged, **Then** it is not merged; the build branch with the earlier task merged is merged into its working copy and its builder gets a repair there, naming each such file and what each side changed; the repair counts against the repairs per task; the loop page and history say which tasks changed which files. (must fail: the later task's first commit is never merged as it was)
5. **Given** that repair, **When** it is accepted and gated, **Then** it is merged and the group goes on.
6. **Given** a group whose tasks each pass the gates on their own, **When** their merged result fails a gate, **Then** the next task does not start; the group goes back for a repair with the failure, within the cap. (must fail)
7. **Given** a task after the group, **When** a task of the group is still running or not merged, **Then** that task does not start.
8. **Given** a phase without `[P]` tasks and up to 3 builders, **When** it runs, **Then** one builder works through its tasks in one session, as today.
9. **Given** a pipeline that allows 1 builder, **When** a phase has `[P]` tasks, **Then** they run one after the other in the phase's session, exactly as today.
10. **Given** a builder of a `[P]` task that asks a question the engineer must answer, **When** it waits, **Then** the other builders of the group go on; the task after the group waits.
11. **Given** three builders running and the loop's cost budget reached, **When** the next step is due, **Then** no new step starts, the running ones finish within their share, and the loop pauses and asks the engineer.
12. **Given** Fallow stops while three builders run, **When** it starts again, **Then** each task resumes in its own working copy and session, no merged task is merged again, and no accepted task is built again.

---

### User Story 4 - At the lock, Fallow says what will actually happen (Priority: P2)

In the lock form, under the chosen pipeline, Fallow says how the builders will work for this feature: "No [P] tasks: the pipeline's 3 builders will work as 1", or "Phase 3 has 4 [P] tasks: up to 3 builders at a time", and when the setting for agents at once is lower, that it limits them. Choosing another pipeline updates it. The engineer can lock with another pipeline.

**Why this priority**: it answers the engineer's question about a pipeline that does not fit the feature, but the loop works without it.

**Independent Test**: open the lock form of a feature without `[P]` tasks and of one with a group of 4, switch between Standard and a pipeline allowing 3 builders, and read the statement each time.

**Acceptance Scenarios**:

1. **Given** a feature without `[P]` tasks and a pipeline allowing 3 builders, **When** the lock form shows it, **Then** it reads that the pipeline's 3 builders will work as 1.
2. **Given** a feature whose phase 3 has a group of 4 `[P]` tasks, **When** the lock form shows the same pipeline, **Then** it reads "Phase 3 has 4 [P] tasks: up to 3 builders at a time", one line per group.
3. **Given** a pipeline allowing 1 builder, **When** the lock form shows it, **Then** it says nothing about builders (no line).
4. **Given** the setting for agents at once is 2 and the pipeline allows 3, **When** the lock form shows it, **Then** it says at most 2 builders will run at once because of that setting.
5. **Given** the statement, **When** the engineer chooses another pipeline, **Then** the statement follows the newly chosen pipeline before the lock.

---

### User Story 5 - Existing pipelines and loops go on working (Priority: P1)

The engineer updates Fallow. Their copies of the shipped pipelines, their own pipelines and their projects' pipelines all still load and mean what they meant. Loops running across the update go on where they were, with the pipeline they recorded at the lock; finished loops show as before. No pipeline file is rewritten.

**Why this priority**: every engineer who used Fallow has such files and loops; losing them silently would break trust (as 017 handled agent files).

**Independent Test**: with a Fallow data folder from 0.4.0 holding an unedited copy of Standard, an edited copy of Test first, the engineer's own pipeline with a path-limited security reviewer marked alongside the reviewer, a project pipeline with a docs-writer once at the end, a loop stopped mid-phase on that project pipeline, and a finished loop on Solo, start the updated Fallow and check each case below.

**Acceptance Scenarios**:

1. **Given** unedited copies of the shipped pipelines, **When** Fallow starts after the update, **Then** they are unchanged (the shipped text did not change) and the new shipped `parallel` pipeline is added beside them.
2. **Given** an edited copy of a shipped pipeline, the engineer's own pipelines and a project's pipelines that were valid in 0.4.0, **When** Fallow loads them, **Then** each loads without a problem, with the same steps at the same points, one builder, and the same result rule; none is rewritten. (must fail: a pipeline valid in 0.4.0 that is now refused)
3. **Given** a pipeline file that was invalid in 0.4.0 for a reason 018 keeps (an unknown agent, a misspelt key), **When** Fallow loads it, **Then** it is reported with its problem as before.
4. **Given** a loop locked with 0.4.0 and stopped in the middle of a phase, **When** Fallow starts again, **Then** it resumes where it was with its recorded pipeline, as one builder, without repeating a completed step, and ends with the same outcome rule.
5. **Given** a finished loop, **When** the engineer opens it, **Then** its outcome and evidence show as before, and its recorded pipeline is shown without a verifying label.
6. **Given** a pipeline saved from the 018 designer that uses none of 018's new points and one builder, **When** its file is read, **Then** it holds no field 0.4.0 does not know, so an engineer going back to 0.4.0 can still use it; a pipeline that uses them names them in the file, and the README and the v0.5.0 release text say 0.4.0 cannot use such a file.

---

### Edge Cases

- **A cap reached by one task of a group** (its repairs used up): the loop asks the engineer as at any cap; the group's steps already running finish, and no new step starts until the engineer answers (as 007 FR-013). Only a question to the engineer (FR-018) holds just its own task.
- **Two groups in one phase** (a plain task between two runs of `[P]` tasks): two groups, one after the other; the lock form gives one line for each.
- **The lock form's pipeline choice**: keeps "(not verified)" beside a pipeline whose results would not be verified; it is not the designer or the pipeline list (Clarifications), and it states what the result will be.
- **A group of `[P]` tasks that spans two phases**: a group never crosses a phase: phases are boundaries in `tasks.md`, and the phase's end waits for its whole group.
- **A `[P]` task followed directly by a plain task, with up to 3 builders**: a group of one; it runs in the phase's session as today.
- **A `[P]` task that changes no file besides its tick**: accepted as today when its claims check out; nothing to compare, merged as a tick.
- **Fallow's own tick of `tasks.md`**: not counted as a file a task touched; Fallow applies the ticks when it merges, so tasks of a group never overlap through `tasks.md`.
- **Two `[P]` tasks that touched the same file without a git conflict**: still not merged blindly; the later one is repaired on top (Clarifications).
- **An overlap repair that touches the same file again in a way that cannot merge**: it builds on the branch with the earlier task merged, so its commit merges forward; if it still cannot be merged, it is repaired again within the cap, then the engineer is asked.
- **A `[P]` task whose builder edits a locked document**: undone and refused as for any task (007 FR-009).
- **A builder that commits itself, or moves its branch**: handled as today (its commits taken apart, the branch put back) in its own working copy.
- **The engineer pauses or stops the loop while builders run**: pause holds new steps; running ones finish; stop ends them all and asks per action (constitution II). Taking over a part covers one task's working copy.
- **The engineer re-locks while a group runs** (decided after review, FR-020a): the new lock is taken in only when no step of the group runs; the running builders finish their current step and nothing new of the group starts meanwhile. Tasks of the group already merged stay merged; every task not yet merged is dropped (branch and working copy removed, reset to not built, a history line says so) and built again under the new lock, in the groups the new `tasks.md` gives.
- **Two tasks of a group ask the engineer at once**: each question holds only its own task, neither replaces the other, and each answer goes to the step of the task that asked (FR-018).
- **A checker after each task with `paths`**: runs only when that task's changes touch its paths.
- **Checkers alongside each other where one has `paths` that were not touched**: it is skipped for that point; the others are judged together.
- **A checker alongside each other at a task point when several tasks run at once**: each task has its own checkers on its own commit.
- **A step at a point marked `parallel_with` in a 0.4.0 pipeline outside a checkpoint's end**: in 0.4.0 they ran one after the other; now they run at the same time, as the file says. The result rule is the same (all must pass); their findings now go to one repair.
- **A loop's budget lower than one step's share with several builders**: each starting step gets an equal share of what is left (008 R-7); a step whose share is too small fails as a step over budget does today.
- **A pipeline allowing 8 builders on a machine with less memory**: the memory room check holds steps back as today; the lock form's statement names the setting, not the memory.

## Constitution Check

| Principle | How 018 holds it |
|---|---|
| I. Verified, Not Trusted | Unchanged rule: a result is verified only with the gates after every task and a checker without `paths` at every story's checkpoint end (FR-021). Each `[P]` task is gated on its own commit, and the merged group again (FR-016), so what is merged has been gated. The labels go from the designer and the list only; a result still says "Done, not verified by Fallow: <what was not checked>" (FR-001, FR-003). |
| II. The Engineer Decides | The lock form says what will actually happen before the engineer locks (FR-022). Caps still end in a question to the engineer; pause, stop and take over cover every builder. |
| III. Coordination in Code | Fallow decides every next step from the locked `tasks.md` and what it verified: which tasks form a group, how many builders, the merge order, an overlap's repair (FR-012 to FR-018). Builders of a group never message each other; each sees the locked documents and the handovers. |
| IV. Simplest Setup First | One builder unless the pipeline allows more (default 1, FR-011). Builders fan out only over tasks the locked plan marks `[P]` ("different files, no dependencies"): the plan declares the split. Fallow checks the files each task actually touched and never merges an overlap blindly (FR-015). Makers at one point never run side by side (FR-007). Every component can be switched off for a recorded run by the pipeline chosen at the lock (builders 1, any step removed), and the run record says how many builders and overlaps a run had (FR-026). See plan, Complexity Tracking, on "a committed contract for each boundary". |
| V. Local and Private | No new network use. The run record holds only numbers and fixed values for the new fields (FR-026). |
| VI. Least Privilege | Each builder edits only its own working copy; checkers stay read-only, in their own copies; rights still come from the kind (FR-019). |
| VII. Runs Survive | Groups, working copies, merges and overlap repairs are decided again from the event log; a restart resumes each task and merges nothing twice (FR-020). |
| VIII. Small and Measured | Parallel builders count against the existing limit on agents at once and the memory room, and the 400 MB budget with 4 concurrent agent steps (SC-006). Any claim that parallel builders pay comes from recorded runs, not from this feature. |
| IX. Runtime-Agnostic | Groups, merges and overlaps are decided from git and Fallow's records; tested on the fake runtime (SC-008). |

## Requirements *(mandatory)*

### Functional Requirements

**The labels**

- **FR-001**: The pipeline list, the designer and the "Based on" choice MUST NOT show the words "verifying" or "not verifying", nor a colour or mark that stands for them.
- **FR-002**: The designer MUST state in plain words what the selected pipeline checks (the gates and when they run, and which checkers check what, at which points) and, when a result of the pipeline would not be verified, that it ends as "Done, not verified by Fallow" and what is not checked (the `missing` items of 007 FR-015).
- **FR-003**: A loop's outcome, its page and its notification MUST keep saying "Done, not verified by Fallow: <what was not checked>" where that applies (007 FR-015, constitution I); this feature changes nothing there.

**Points and steps**

- **FR-004**: A pipeline MUST be able to place steps at these points, and only these: before and after each task; at the start and end of each phase; at the start and end of each user story's checkpoint; once at the start and once at the end of the loop. The order of points is fixed by `tasks.md`: per task, the steps before it, the builder, the gates (when they run per task), the steps after it; per phase, the steps at its start, its tasks, the gates (when they run per phase), the handover, the steps at its end, then the checkpoints of the stories that end there.
- **FR-005**: At each point any number of steps of any kind (maker or checker) MAY be placed. Steps at one point run in the pipeline's order; checkers marked to run alongside each other form one unit that starts together.
- **FR-006**: A unit of checkers alongside each other MUST run at the same time at every point (within the limit on agents at once), MUST be judged only when all of them have finished, and MUST pass only when all passed. Their findings MUST go to one repair, after which every checker of the unit runs again, within the cap of that point (repairs per task for task and phase points and once, review rounds for a checkpoint).
- **FR-007**: Makers MUST NOT be marked to run alongside anything; such a pipeline MUST be refused, naming the step.
- **FR-008**: A checker after a task MUST check that task's commit; its failure MUST be a repair of that task by the task's builder, counted against the repairs per task. A maker before a task MUST run before that task's builder, its work committed by Fallow and handled as a maker at a start is today (gated as a new baseline, not as a task).
- **FR-009**: A checker at a start point MUST have `on_fail = "engineer"`; with `"repair"` the pipeline MUST be refused, saying nothing of that point's work is built yet.
- **FR-010**: The builder MUST be the one maker per task without a start or end; the gates MUST run per task or per phase only. Placing either at another point MUST be refused with the step's number.

**Builders over [P] tasks**

- **FR-011**: A pipeline MUST be able to allow up to N builders, N from 1 to 8, 1 when not written. Values outside that range MUST be refused.
- **FR-012**: A group MUST be a run of consecutive tasks marked `[P]` in one phase of the locked `tasks.md`. When the pipeline allows more than one builder and a group has at least two tasks, each task of the group MUST get its own working copy, based on the build branch as it was when the group started, and its own builder session; otherwise tasks MUST run one after the other in the phase's session, as today.
- **FR-013**: Fallow MUST run at most N of a group's tasks at once, starting each as soon as a place is free, and never more than the setting for agents at once and the memory room allow (007 research S-19).
- **FR-014**: Each task of a group MUST be checked and gated on its own commit as any task (007 FR-011), with its steps before and after it, in its own working copy.
- **FR-015**: Fallow MUST merge the accepted tasks of a group into the build branch in `tasks.md` order. Before merging a task, it MUST compare the files that task changed since its working copy's base with the files the build branch changed since that base (Fallow's ticks of `tasks.md` excepted). When they share a file, Fallow MUST NOT merge that task: it MUST merge the build branch as it is then into the task's working copy and queue a repair of the task, by its builder in the task's session, naming each shared file and the tasks that changed it and what each side changed (as 011 FR-005 names both sides), counted against the repairs per task; the repair MUST be checked as after 011's update (no conflict markers left, the changes committed, the gates on its commit) and compared again against the build branch's changes since the new base. Each overlap MUST be recorded and shown in the loop's history and page.
- **FR-016**: When every task of a group is merged, Fallow MUST run the gates on the merged result before the next task or the phase's handover. A failure MUST be a repair of the group, named by its tasks, in the phase's session (or a fresh one with the phase's instruction when there is none), within the repairs per task, then the engineer.
- **FR-016a**: The phase's handover MUST be written by the phase's builder session as today; its instruction MUST name the tasks built alongside it and their commits, and when the phase has no session of its own yet, a fresh session MUST write it.
- **FR-017**: A task after a group MUST NOT start until every task of the group is accepted, merged and the merged result gated.
- **FR-018**: A question that waits for the engineer from one task of a group MUST hold only that task and what comes after the group; the group's other tasks MUST go on (constitution II). Several such questions MUST each hold only their own task: none MUST replace another, and each answer MUST go to the step of the task that asked.
- **FR-019**: Each builder MUST edit only its own working copy; checkers MUST check in copies of their own; rights MUST still come from the kind (constitution VI).
- **FR-020**: Groups, working copies, merges, overlap repairs and gate runs MUST be derived again from the loop's events after a restart; a merged task MUST NOT be merged twice and an accepted task MUST NOT be built again (constitution VII).
- **FR-020a** (decided after review): A new lock while a group is in progress MUST be taken in only when no step of the group is running; meanwhile no new step of the group MUST start. When it is taken in, the group's tasks already merged into the build branch MUST stay merged, and every task of the group not yet merged MUST be dropped: its branch and working copy removed, its state reset to not built, and a line in the loop's history saying so; the phase then MUST go on under the new lock with the groups of the new `tasks.md`. The working copies and branches of the group's merged tasks MUST be removed too (review 2), and no step of the dropped group (a task's, a task point's, or the group's own repair) MUST be resumed or count for a later group. `tasks.md` MUST be ticked only for tasks merged into the build branch. No task MUST be left accepted with its work unmerged, no two builders MUST share a working copy, and the builders cap MUST NOT be exceeded.

**Verification, the lock, cost**

- **FR-021**: Whether a result is verified MUST stay as 007 FR-015 says: gates after every task, and a checker without `paths` at the end of every story's checkpoint. Checkers at other points MUST NOT make a pipeline count as checking every story, and their absence MUST NOT make it count as not checking.
- **FR-022**: The lock form MUST state, for the chosen pipeline and the feature's `tasks.md`, how the builders will work: that the pipeline's N builders will work as 1 when there are no groups; for each group, its phase, how many `[P]` tasks it has and how many builders at a time; and when the setting for agents at once is lower than N, that it limits them. It MUST follow the chosen pipeline before the lock.
- **FR-023**: The loop's cost and time budgets MUST hold with builders and checkers running at once: each step starting MUST get an equal share of what is left of the cost budget among the steps running or waiting to start (008 R-7); when a budget is reached, no new step MUST start, and the loop MUST pause and ask as today (007 FR-016a).
- **FR-024**: The loop page MUST show the builders of a group as they run (one per task), each with its task and its working copy, and a task held back by an overlap with which files and which task.

**Existing pipelines and loops**

- **FR-025**: Every pipeline file valid in Fallow 0.4.0 MUST load in 018 with the same steps at the same points, one builder, and the same result rule; none MUST be rewritten. Shipped Standard, Test first and Solo MUST keep their text. A new shipped default `parallel` MUST be added (up to 3 builders, the gates after every task, the reviewer and the security reviewer alongside each other at each story's checkpoint), installed as 007 FR-025 installs a default.
- **FR-026**: A loop MUST keep using the pipeline recorded at its lock; a loop locked before 018 MUST resume and finish with it, as one builder. The run record (constitution V) MUST add only numbers: the builders allowed, the most that ran at once, the groups run in parallel, and the overlap repairs; records written before 018 MUST still be read.
- **FR-027**: The pipeline's file format MUST add only optional fields, so that a file not using them means what it meant in 0.4.0; checkers alongside each other outside a checkpoint's end MUST now run at the same time (FR-006).

**The designer**

- **FR-028**: The designer MUST draw every point of FR-004 in the order the loop runs them, with a + at each point to add a step there, the steps at a point in their order, units of checkers alongside each other as branches that join again, the builder with its "up to N builders on [P] tasks", and the repair back-edges with their caps. Selecting a step MUST show its settings (agent, point, paths, alongside which checkers, on failure); "Edit as TOML" MUST remain and show the same pipeline.
- **FR-029**: The designer MUST offer "alongside" only between checkers at the same point, "on failure: a repair" only where FR-009 allows it, and the number of builders from 1 to 8; refusals MUST name the step and the reason, as 007.
- **FR-029a**: Every control the designer adds (a + at each point, the builders field, the settings of a step at a new point) MUST be reachable and usable by keyboard and labelled for screen readers, with 4.5:1 text contrast, as 008 FR-018.

**What this replaces**

- **FR-030**: The earlier documents MUST be marked as replaced or amended by 018 where they say otherwise: 007 contracts/pipeline.md (the checks on checkers per task, checkers at a start, one maker per task; the `at` and `parallel_with` rows; "The designer shows the same warning"; the new cap and points); 007 FR-010, FR-010a, FR-011 and FR-027 as amended here; 007 US6 AS3's wording; 008 FR-009, US3's independent test, US4 AS5, FR-011, FR-013, FR-014 and FR-015. 011 is not replaced: its repair of conflicting work is reused for overlapping tasks.

### Key Entities

- **Point**: one of the fixed places of FR-004 (task start or end, phase start or end, checkpoint start or end, once start or end), each holding steps in order.
- **Step**: an agent at a point, with its paths, what happens on failure, and the checkers it runs alongside.
- **Unit**: the checkers at one point that run alongside each other; judged together.
- **Builders cap**: the most builders a pipeline allows at once (1 to 8).
- **Group**: a run of consecutive `[P]` tasks in one phase; its tasks, their working copies and sessions, their merge order, the overlaps found, and the merged result's gate runs.
- **Overlap**: a task of a group held back because it changed a file the build branch changed since that task's working copy was based (the group's start, or the merge of its last overlap repair); the files, the tasks, and the repair that answered it.
- **Lock statement**: what the lock form says the builders will do for this feature and pipeline.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The pipeline list, designer and "Based on" contain no "verifying"/"not verifying" text or mark for 100 % of the shipped pipelines and the test pipelines; 100 % of loops on a pipeline without a checkpoint checker or per-task gates still end as "Done, not verified by Fallow" with what was not checked.
- **SC-002**: On the fake runtime, a phase with 4 `[P]` tasks of equal length takes at most 60 % of its time with 1 builder when 3 builders are allowed.
- **SC-003**: 0 tasks of a group are merged while sharing a file with the build branch's changes since the group started; every such case is a repair and is shown on the loop page.
- **SC-004**: 100 % of merged groups are gated on the merged result before the next task starts; a loop's merged commit always contains only gated work (007 FR-014's check passes).
- **SC-005**: 100 % of pipeline files valid in 0.4.0 (the shipped defaults, the contract's example, and every pipeline in Fallow's tests) load in 018 with identical steps and result rule; 0 pipeline files are written by the update besides the new default.
- **SC-006**: With 3 builders and 1 checker running (4 concurrent agent steps), Fallow stays within its 400 MB running budget (constitution VIII); the cost of the steps started never exceeds the loop's remaining cost budget at their start.
- **SC-007**: For each of 0, 1 and 4 `[P]` tasks per phase and builders 1 and 3, the lock form's statement matches what the loop then runs (the most builders at once observed).
- **SC-008**: Every acceptance scenario of US2, US3 and US5 runs on the fake runtime in CI.

## Assumptions

- `[P]` in the locked `tasks.md` is Spec Kit's marker for "different files, no dependencies"; Fallow's tasks parser already reads it. A task is in a group only when marked `[P]` exactly as Spec Kit writes it.
- Builders of a group start from the same build commit; they cannot see each other's work until it is merged, which is what `[P]` promises they do not need.
- Running builders at the same time does not lower the cost of a feature and may add overlap repairs; the lock form's budget suggestion stays based on the number of tasks.
- Fallow 0.4.0 refuses a pipeline file with fields it does not know; an engineer going back from 018 sees such files listed as unusable (FR-025's counterpart, US5 AS6). The README and the v0.5.0 release text say so.
- Fallow's setting for agents at once (default 4) and its memory room check stay as they are.
