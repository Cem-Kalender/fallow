# Feature Specification: Fallow Studio redesign

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/008-studio-redesign`)

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "Fallow Studio redesign (008): a sharper, more intuitive page for Fallow Studio (007), built on 007 as it is. The loop page shows verification, not specification; the engineer's steering moves into the loop page's header; 'Pipelines & agents' becomes two tabs, Pipelines and Agents; saving reads 'Save for: all my projects / this project only'; the pipeline editor becomes a graph of the fixed step model; steps marked to run alongside each other really run at the same time; 008 has its own reference mockup. Out of scope: arbitrary graph edges or new step kinds, new runtimes, changes to the spec session or the dashboard beyond the shared navigation, pushing or pull requests." (The full description, with the decisions behind it, is kept in the conversation of 2026-10-31 and 2026-11-01.)

## Context

007 built Fallow Studio and was then run with real agents (007's T086). The engineer reviewed the page while loops ran and found it harder to read than it should be:

- **The loop page repeated the spec.** Every user story's acceptance criteria were printed in full in a narrow side column. That text is `spec.md`'s, which the engineer wrote and can open at any time. What only Fallow knows is the verification: which criteria were checked, what the reviewer ran, and what came out. That was buried in the same narrow cards.
- **Steering was tucked away.** "Your steering" (a note for the next step, limits) sat at the bottom of the side column, below the stories and the commits.
- **One tab held two jobs.** Designing pipelines and writing agents shared "Pipelines & agents", in three crowded columns. Agents were edited as raw frontmatter, and "Yours (~/.fallow)" did not say what it chose.
- **The pipeline was a list.** The designer showed the steps as a list of cards. Where steps branch, run alongside each other or loop back on a failure was hard to see.
- **"Alongside" was not real.** A pipeline could mark checkers to run alongside each other (`parallel_with`), but a loop ran one step at a time, so they ran one after the other (007's plan, "Complexity Tracking").

The engineer agreed the changes in this spec from mocks made on the real page (2026-10-31 and 2026-11-01).

**What does not change**: the spec session, the dashboard (except the shared navigation), the project conversation, and everything 007 guarantees about verification, commits, questions and limits.

## Clarifications

### Session 2026-11-01

The engineer was not available for clarify; Claude answered from the agreed mocks and the engineer's earlier decisions. Every answer below is **decided for the engineer, to confirm**.

- Q: Where does the full text of a criterion appear, now that the story cards go? → A: In the story's evidence panel, beside what the reviewer ran and what came out, as the reference for that check. It is not shown anywhere else on the loop page (FR-001, FR-002).
- Q: Should a story's evidence panel open by itself? → A: Only when one of its criteria fails, or when the engineer opens it. At most one panel is open at a time (FR-003).
- Q: How many steps may run alongside each other in one loop? → A: Steps marked to run together at the same point, within the loop's concurrency cap (Fallow's existing cap on agents running at once, 007's FR-004). When the cap is reached, the rest wait for a free place (FR-014).
- Q: If two checkers running alongside each other both find problems, how many repairs follow? → A: One. The builder repairs once with all findings, and then every checker of that checkpoint runs again (FR-015).
- Q: Which "Save for" choice is the default? → A: "This project only" when a project is selected, "All my projects" otherwise (FR-010).
- Q: Can the engineer still edit an agent's definition as text? → A: Yes. The form fields are shown above the instructions, and "Edit as text" shows the whole file, as today (FR-008).
- Q: Does the graph replace the list-style flow, or sit beside it? → A: It replaces it. "Edit as TOML" stays as the text view (FR-011).
- Q: Must the graph and the verification row work without a mouse? → A: Yes, as every control in 007 (007's FR-032): nodes, the + on edges and the story pills are reachable and usable by keyboard, with labels a screen reader announces (FR-018).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See what Fallow verified, not the spec again (Priority: P1)

While a loop runs, the engineer sees one "Verified by Fallow" row under the pipeline, with one pill per user story. A pill shows the story's state: all criteria checked, how many failed, not checked yet, or not checked at all because the pipeline has no checker. Opening a story shows its criteria with what the reviewer ran and what came out. When a criterion fails, its story opens by itself. Before merging, "What was verified" next to the Merge button opens the same evidence.

**Why this priority**: verification is what Fallow adds. The engineer decides to merge on that evidence, and should not have to dig for it.

**Independent Test**: run a loop with the fake runtime to the end, with one story passing and another with a failing criterion, and check the row, the panel that opens by itself, and the evidence behind "What was verified". This works without the other stories of this spec.

**Acceptance Scenarios**:

1. **Given** a loop whose reviewer checked all 5 criteria of US1, **When** the engineer opens the loop page, **Then** the row shows `US1 ✓ 5/5` and no criterion text is shown until US1 is opened.
2. **Given** that loop, **When** the engineer opens US1, **Then** a panel lists each criterion with its status, what the reviewer ran and what came out.
3. **Given** a loop whose reviewer failed one criterion of US2, **When** the verdict arrives, **Then** the row shows `US2 ✗ 1 failed`, US2's panel opens by itself with the failed criterion marked, and the feed shows the failure.
4. **Given** a loop on a pipeline without a checker at the checkpoints, **When** the engineer opens the loop page, **Then** every story shows "not checked", and its panel says that no checker in this pipeline verifies these criteria. (must fail)
5. **Given** a loop that is ready to merge, **When** the engineer chooses "What was verified" next to Merge, **Then** the evidence of every story is shown before the merge.
6. **Given** a loop page, **When** the engineer looks at the side column, **Then** it holds no story card with criterion text. (must fail)

---

### User Story 2 - Steer the loop from its header (Priority: P1)

The engineer's controls sit together in the loop page's header: a note for the next step, limits, pause or go on, and stop, with the latest steering line under the header.

**Why this priority**: steering is how the engineer acts on what they see; it belongs where they look first.

**Independent Test**: on a running loop with the fake runtime, leave a note and change a limit from the header, and check that both are recorded and that the latest steering line shows them.

**Acceptance Scenarios**:

1. **Given** a running loop, **When** the engineer opens the loop page, **Then** the header holds Note, Limits, Pause and Stop, and no "Your steering" card remains in the side column.
2. **Given** the engineer leaves a note from the header, **When** the next step starts, **Then** the step receives the note and the latest steering line shows it.
3. **Given** a loop that is ready to merge or done, **When** the engineer looks at the header, **Then** Pause is not offered. (must fail)

---

### User Story 3 - Pipelines and agents, each in its own tab (Priority: P1)

"Pipelines & agents" becomes two tabs. In Agents, the engineer sees the agents grouped by what they may do (Lead; Makers: change files; Checkers: only run and verify) and edits the selected one in a wide editor: its runtime, model, kind, the commands it may run and its result shape as fields, and its instructions as text, with links to the pipelines that use it. In Pipelines, the engineer picks a pipeline from a list that says which ones verify and which one the project uses, edits it, and sees the selected step's agent in brief with a link to it in Agents. Saving says "Save for: all my projects / this project only".

**Why this priority**: designing pipelines and writing agents are different tasks; one crowded tab served neither well, and a click on an agent did not even show that agent (007's T086).

**Independent Test**: open both tabs on a project, switch between agents, change an agent's model in the form and save it for this project only, and check the file Fallow wrote and the pipeline list's "verifying" marks.

**Acceptance Scenarios**:

1. **Given** the Agents tab, **When** the engineer clicks one agent and then another, **Then** the editor shows the second agent's definition.
2. **Given** an agent selected in Agents, **When** the engineer changes its model in the form and saves for "this project only", **Then** the project's own definition holds the new model and the rest of the file is unchanged.
3. **Given** the Pipelines tab, **When** the engineer selects a step, **Then** the step's agent is shown in brief with a link that opens it in Agents.
4. **Given** a project is selected, **When** the engineer saves an agent or a pipeline, **Then** "Save for" defaults to "this project only".
5. **Given** an agent change that would stop a pipeline in use from verifying, **When** the engineer saves it, **Then** the save is refused with the reason, as in 007. (must fail)

---

### User Story 4 - The pipeline as a graph (Priority: P2)

The pipeline editor draws the pipeline as a graph of Fallow's fixed steps:
- the Spec session and the Lock, which are always there;
- a group "For each phase in tasks.md" with the per-task steps;
- a group "At each story's checkpoint" with the checkers, where steps that run alongside each other branch and join again;
- the steps that run once, and the merge.

Back-edges show what happens on a failure. The engineer adds a step with a + on an edge, selects a node to change its settings, and removes a node to drop the step; the pipeline then says plainly what is no longer verified.

**Why this priority**: the graph shows at a glance what a pipeline does and guarantees; the list did not. It builds on User Story 3's Pipelines tab.

**Independent Test**: open Standard, add a security reviewer alongside the reviewer through a + on an edge, and check the graph, the step's settings, the saved pipeline and its "verifying" mark; then remove the reviewer and check what the pipeline says is no longer verified.

**Acceptance Scenarios**:

1. **Given** the Standard pipeline, **When** the engineer opens it, **Then** the graph shows Spec session and Lock, the builder and the gates in the per-phase group, the reviewer in the checkpoint group, Merge at the end, and the repair back-edges.
2. **Given** that graph, **When** the engineer adds a checker alongside the reviewer through a + on an edge, **Then** the two are drawn as parallel branches that join again, and the saved pipeline marks them to run alongside each other.
3. **Given** a selected node, **When** the engineer changes when it runs, **Then** Fallow moves it to the place that matches and the TOML view shows the same pipeline.
4. **Given** the graph, **When** the engineer tries to connect two steps with an edge of their own, **Then** nothing is drawn: edges follow from when the steps run. (must fail)
5. **Given** a pipeline whose last checker is removed, **When** the graph is shown, **Then** the pipeline is marked "not verifying" and says which checks are missing.

---

### User Story 5 - Checkers that run alongside each other really do (Priority: P2)

When a pipeline marks steps to run alongside each other at a story's checkpoint, Fallow runs them at the same time, within the loop's concurrency cap. The checkpoint is judged when all of them have finished, and one repair answers all their findings.

**Why this priority**: the pipeline promised it and the graph will show it; the loop has to do what both say. It also shortens checkpoints with several checkers.

**Independent Test**: with the fake runtime, run a loop whose checkpoint has two checkers marked to run alongside each other, each taking several seconds, and check that their steps overlap in time, that the checkpoint waits for both, and that one repair follows when both fail.

**Acceptance Scenarios**:

1. **Given** two checkers marked to run alongside each other, **When** a story's checkpoint starts, **Then** both start without waiting for each other, and the loop page shows both working.
2. **Given** one of them finishes first, **When** the other is still working, **Then** the checkpoint is not judged yet.
3. **Given** both report findings, **When** the checkpoint is judged, **Then** one repair receives all the findings, and then both checkers run again.
4. **Given** a concurrency cap of one agent at a time, **When** the checkpoint starts, **Then** the checkers run one after the other, and the result is the same. (must fail)
5. **Given** Fallow stops while both run, **When** it starts again, **Then** each resumes its own check, and nothing completed runs again.

---

### User Story 6 - One reference for the look (Priority: P3)

008 has its own reference mockup for the screens it changes: the loop page, Pipelines and Agents. It keeps 007's colours, fonts and logo, and the visual check compares each changed screen with it at 1440 px and 390 px. 007's visual check is superseded by 008's.

**Why this priority**: a shared reference lets the engineer approve the look once and lets the build be checked against it.

**Independent Test**: open each changed screen next to its mockup at both widths and compare; check that nothing scrolls sideways at 390 px.

**Acceptance Scenarios**:

1. **Given** the mockup of the loop page, **When** the built page is shown next to it at 1440 px, **Then** the regions, their order and their contents match.
2. **Given** any changed screen at 390 px, **When** it is shown, **Then** nothing scrolls sideways and no text runs out of its card. (must fail)

---

### Edge Cases

- **A story with no criteria in the spec** (a setup phase only): it has no pill.
- **A long criterion or a long command in the evidence**: they wrap inside the panel and never widen the page.
- **The reviewer's verdict covers some criteria but not all yet**: the pill shows progress, such as `US1 3/5`, and the unanswered ones are "not yet".
- **A checker marked to run alongside a step that is not in the pipeline**: the pipeline is refused with the reason, as in 007.
- **One of two parallel checkers fails to start**: the other still runs; the failed start is retried as in 007, then the engineer is asked.
- **A graph too wide for the window**: it scales down to fit, or scrolls inside its own box; the page never scrolls sideways.
- **An agent's form field cannot show a value written in the file by hand** (an unknown model, for example): the field shows it as written and the save keeps it; the definition's checks decide whether it is valid.
- **Two pages save the same agent or pipeline at once**: the later save wins, as in 007 (agent and pipeline saves carry no version; only documents do).

## Requirements *(mandatory)*

### Functional Requirements

**The loop page**

- **FR-001**: The loop page MUST show a "Verified by Fallow" row under the pipeline, with one entry per user story that has criteria. Each entry shows one of four states: all checked (`✓ n/n`), failed (`✗ k failed`, or "✗ findings" when a checker such as a security reviewer failed the story without failing a criterion), in progress (`k/n`), or not checked (no checker answers the criteria in this pipeline). A story is never shown as checked while any of its checkers failed in the current round (constitution I).
- **FR-002**: Opening a story MUST show, for each criterion, its text, its status, what the checker ran and what came out, and for a failure what was expected. Criterion text MUST NOT appear elsewhere on the loop page.
- **FR-003**: A story with a failed criterion MUST open its panel by itself when the verdict arrives, and the failure MUST appear in the feed. At most one panel is open at a time.
- **FR-004**: Next to Merge and Merge anyway, a "What was verified" link MUST open a dialog with the evidence of every story (apart from the row's one-panel rule).
- **FR-005**: The loop page's header MUST hold the engineer's steering: Note for the next step, Limits, Pause or Go on, and Stop, with the latest steering line under it. Pause is not offered once the loop is ready or done. The side column MUST NOT hold a steering card.

**Pipelines and agents**

- **FR-006**: The navigation MUST offer Pipelines and Agents as separate tabs; each keeps the selected project.
- **FR-007**: The Agents tab MUST list the agents grouped by kind (lead, makers, checkers), each with its source and the number of pipelines that use it, and MUST show the selected agent in an editor with links to those pipelines. Selecting another agent MUST always show that agent's definition.
- **FR-008**: The agent editor MUST offer the runtime, model, kind, the commands the agent may run and its result shape as form fields, and the instructions as text. "Edit as text" MUST show the whole definition. Saving MUST change only what the engineer changed, and keep any value written by hand.
- **FR-009**: The Pipelines tab MUST list the pipelines, each marked verifying or not with what is missing, and mark the one the selected project uses. Selecting a step MUST show its agent in brief (name, kind, runtime and model) with a link to it in Agents.
- **FR-010**: Saving an agent or a pipeline MUST offer "Save for: all my projects / this project only", defaulting to "this project only" when a project is selected. Folder paths appear only in a tooltip.

**The pipeline graph**

- **FR-011**: The pipeline editor MUST show the pipeline as a graph of its steps, replacing the list. Spec session and Lock are always shown and cannot be removed. Steps are grouped by when they run (each task, each phase, a story's checkpoint, once). Steps that run alongside each other are drawn as parallel branches that join again. What happens on a failure is drawn as a back-edge. "Edit as TOML" MUST remain.
- **FR-012**: Edges MUST follow from when each step runs; the engineer cannot draw edges of their own. Adding a step MUST be possible from a + on an edge; Fallow places the new step by when it runs.
- **FR-013**: Selecting a node MUST show its settings (agent, when it runs, only when these paths changed, alongside which step, on failure), and changing them MUST update the graph and the TOML alike. Removing a node MUST remove the step, and the pipeline MUST then say plainly what is no longer verified.

**Running alongside**

- **FR-014**: Steps a pipeline marks to run alongside each other MUST run at the same time, within the loop's concurrency cap; when the cap is reached, the rest wait for a free place.
- **FR-015**: A checkpoint with steps running alongside each other MUST be judged only when all of them have finished. Their findings MUST go to one repair, after which every checker of that checkpoint runs again, within the existing review-round cap.
- **FR-016**: Each step running alongside another MUST keep its own worktree, session, activity and cost, and MUST resume on its own after a restart; nothing completed runs again.

**The look**

- **FR-017**: The changed screens (the loop page, Pipelines, Agents) MUST match 008's reference mockup in `mockup/`, keeping 007's colours, fonts and logo. At 390 px no changed screen scrolls sideways and no text runs out of its card. 007's visual check is superseded by 008's.

**Access**

- **FR-018**: Every new control (story pills, evidence panels, header steering, the graph's nodes and its + on edges, the agent form) MUST be reachable and usable by keyboard and labelled for screen readers, and text MUST meet a 4.5:1 contrast ratio, as 007's FR-032 requires. A graph node's label names its step, when it runs and its agent.

### Key Entities

- **Story verification**: per user story of a loop: its criteria, each with a status, what was run, what came out and what was expected; derived from the checkers' verdicts that 007 already records.
- **Pipeline graph**: a drawing of a pipeline's steps, grouped by when they run, with edges that follow from that and back-edges for failures; it holds no information the pipeline's TOML does not.
- **Parallel group**: the steps a pipeline marks to run alongside each other at one point of the loop; judged together.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From the loop page, the engineer can say which criteria of a story failed and what the reviewer ran for them within 10 seconds, without opening `spec.md`.
- **SC-002**: The engineer reaches every steering control of a loop without scrolling, on a 1440 × 900 window.
- **SC-003**: Changing an agent's model takes at most 3 actions in the Agents tab (select, change, save), and the saved file differs from the old one in that line only.
- **SC-004**: A checkpoint with two checkers running alongside each other takes no longer than the slower of the two plus 10% (with a concurrency cap of at least 2).
- **SC-005**: Every changed screen passes the visual check against 008's mockup at 1440 px and 390 px, approved by the engineer.
- **SC-006**: Everything 007 measured still holds: the page ready within 500 ms of start, under 60 MB idle and under 200 MB with 4 agents running.

## Assumptions

- 007 is complete and verified (T001–T088); 008 changes its page and one part of its loop, and keeps its data, API and files compatible. Existing pipelines and agent definitions keep working unchanged.
- The concurrency cap and the review-round cap are 007's; 008 adds no new limit.
- The reference mockup is made from the mocks of 2026-10-31 and 2026-11-01, refined to cover every changed screen, and is approved by the engineer before the visual check.
- The graph is drawn by the page itself, without a new library, like the rest of 007's page.
