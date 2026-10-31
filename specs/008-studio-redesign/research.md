# Research: Fallow Studio redesign (008)

Decisions for [spec.md](./spec.md), on top of 007's code (see `specs/007-fallow-studio/research.md`). Each has a decision, why, and what else was considered.

## R-1 Where the verification comes from

**Decision**: the loop view (`GET /api/loops/<l>`) already carries `checks.stories[]`: per story its criteria, each with `state`, `ran`, `result`, `must_fail` and `text`, plus the story's checkers and their reasons (007's `activity::view`). 008 adds, per story, a summary (`checked`, `failed`, `total`, `state`: `checked | failed | progress | not_checked`), whether the pipeline checks it at all, and per failed criterion what was expected (the scenario's Then part). The page draws the row and the panels from that; no new endpoint.

**Why**: the evidence is recorded already (the checkers' verdicts, 007 S-9); only the presentation and a summary are missing. One place computes the state, so the row, the panel and "What was verified" cannot disagree.

**Alternatives**: computing the state in the page (rejected: the page would need the pipeline's rules to tell "not yet" from "not checked"); a separate evidence endpoint (rejected: two requests for one screen, and the loop view is already the page's source).

## R-2 Steering in the header

**Decision**: the header's controls (Note, Limits, Pause or Go on, Stop) open the dialogs 007 already has (`data-act="note"`, `limits`, `pause`, `resume`, `stop`); the latest steering entry (`steering[]`'s last) is shown under the header. Pause is hidden at `ready`, `done_unverified` and final stages. The side column loses the steering card and the story cards; it keeps gates, questions and commits.

**Why**: no new behaviour, only placement; the API is unchanged.

## R-3 Two tabs: Pipelines and Agents

**Decision**: two routes, `#/pipelines[/<project>]` and `#/agents[/<project>]`, two screens (`screens/pipelines.js` keeps the pipeline editor, a new `screens/agents.js` takes the agent editor out of it). Both read the same endpoints as today (`/api/agents`, `/api/pipelines`, `PUT`/`DELETE` for each). The navigation gets the two entries; `#/pipelines/<p>` from 007 keeps working.

**Why**: the two jobs were already separate in the API; splitting the page is a front-end change only.

## R-4 The agent form

**Decision**: the page shows the definition's frontmatter as fields: runtime (a select from `/api/runtimes`), model (the runtime's models, keeping an unknown value as written), kind (maker, checker, lead), `may_run` (a list), `returns` (the result shapes). On save, the page sends only the fields that changed: `PUT /api/agents/<name>?scope=…` with `{fields: {model: "opus"}}` instead of `{text}`. The server takes the definition's current text, the one at that scope, or the resolved one when it is saved to a new scope, and rewrites only those keys' lines. It keeps every other line, comment and the body as they are, and adds a key that was absent at the end of the frontmatter. Then the same checks as 007's `put_agent` run on the result. "Edit as text" still sends `{text}`.

**Why**: SC-003 (only the changed line differs) and the edge case of hand-written values. The rewrite is a Rust function with unit tests, not page code without tests. The text stays the only source of truth, and the server stays the only judge (US3 scenario 5).

**Alternatives**: rewriting in the page (rejected: untestable in this project's test setup); regenerating the frontmatter from the fields (rejected: loses comments and order, so diffs grow).

## R-5 "Save for"

**Decision**: one select, "All my projects" (`scope=user`) or "This project only" (`scope=project:<p>`), defaulting to the project when one is selected; the paths (`~/.fallow/…`, `.fallow/…`) in its `title`. The same control in both tabs. The override note becomes that tooltip.

## R-6 The pipeline graph

**Decision**: the page lays the pipeline out itself, as SVG inside the editor's box, from the pipeline's steps (no layout library; constitution VIII):
- columns, left to right: Spec session, Lock (fixed); the per-phase group (`when = task`, then `when = phase` with `at = start` before and `at = end` after); the checkpoint group (`at = start` steps, then the checkers, where each `parallel_with` set is one column of stacked nodes); `once` steps; Merge (fixed);
- edges: one forward edge between consecutive columns, branching to and joining from a stacked column; a dotted back-edge "next task" over the per-task steps; red dashed back-edges for `on_fail = repair` (to the builder, labelled with the caps) and a note for `on_fail = engineer`;
- a node is a `<g>` with a `<rect>` and text; its role colour from the agent's kind (maker blue, checker green, `fallow:gates` grey, the engineer's fixed steps orange);
- a `+` button on each forward edge opens a small form: the agent, and when it runs (preset from where the + is); Fallow appends the step to the TOML with that `when`/`at`, and the layout puts it in place;
- selecting a node shows its settings form under the graph (agent, when, at, paths, parallel_with, on_fail), the same fields 007's step card had; removing it deletes the step;
- the layout is computed in a fixed 1360 × 430 coordinate space and scaled to the box (`viewBox`, `preserveAspectRatio="xMidYMid meet"`), so edges and nodes stay aligned (a lesson of the mock); below 700 px wide, the graph scrolls inside its box.

Keyboard: every node and `+` is a focusable `<g tabindex="0" role="button">` with an `aria-label` naming the step, when it runs and its agent (FR-018); arrow keys move between nodes in layout order; Enter selects; Delete removes (with the usual confirmation).

**Why**: the model is small and fixed (007 contracts/pipeline.md), so a hand-written layered layout is short, predictable and testable; the TOML stays the only source of truth (FR-011, FR-013).

**Alternatives**: a graph library (rejected: constitution VIII, no new dependencies, and its free-form editing contradicts FR-012); drawing with HTML boxes and CSS (rejected: back-edges and branches need paths).

## R-7 Running steps alongside each other

**Context** (from reading `src/run/`): a loop holds one `busy` (`Busy::Step(n) | Gates | Merge`), with one `stop` flag, process group, tool token and pending `asking`; `Msg::Done` clears all of them; `checkpoint()` runs the story's checkers in pipeline order and returns on the first that has not passed; `requeue_cut_short` takes up only the last cut-short step; slots count one per busy loop. `parallel_with` is validated (`defs/pipeline.rs`) but never read in `src/run`.

**Decision**: a set of running steps, one agent call per checker (not a "group step"):
- `Run` keeps `busy` for Gates and Merge only, and gains `running: BTreeMap<u32, Running { stop, group, tool, asking }>`; "the loop is busy" means either.
- `start()` inserts into `running` and no longer replaces a shared stop flag; `Msg::Done { Step { n } }` removes only `n`; stop, shutdown and unlock signal every entry.
- The tool bridge (`open`, `call`, `run_of_token`) finds the step by its token in `running`; `ask` refuses only a second open question of the same step; `lead_done` takes `asking` from the entry that asked.
- Slots and `agents_running` count running steps, not loops, and sum their groups' memory.
- `checkpoint()` computes the parallel group of the story's checkers (the connected sets of `parallel_with`); it queues every member without a review since the last repair, treats running members as pending, and judges only when every member has a verdict: an incomplete or cut-short member is asked again alone (up to `CHECK_TRIES`); any blocked member goes to the engineer; otherwise all failing members' reasons go to one `repair_or_ask` (the review-round cap unchanged), after which every checker runs again (already 007's behaviour after a repair).
- `step()` starts every queued member of the group while a slot is free, not only the oldest queued step; with `max_parallel = 1` they run one after the other (spec US5 scenario 4).
- `take_answers` (which merges answers into the build branch) runs once before a group starts, not between its members, so they check the same commit.
- Each member's budget is the remaining budget divided by the members starting together.
- `requeue_cut_short` takes up every cut-short step that has no later step of the same kind, part and agent; `deliver`'s "nothing later" test also compares the agent.
- Agent cards and the dashboard show every running step as working.

**Why**: per-step worktrees (`check-<story>-<agent>`), sessions, activity files, costs and verdicts are already per step, so one call per checker keeps all of that unchanged; only "one at a time" assumptions change, and each has a test today (us2_slots, us2_resume, us2_repairs, us4_ask, us5_dashboard).

**Alternatives**: a group step that runs several agent calls and reports once (rejected: duplicates the per-step machinery, and a restart would lose the members' own sessions); running any two steps concurrently (rejected: builders share a worktree and a phase session; only checkers, which never edit, can safely overlap).

**Risks and their answers**: two threads report through one channel, so the event log stays ordered; the fake runtime's record of used scenario steps is not safe for two agents at once, so the fake agent takes a file lock around it (tests only); decisions stay replayable from the log (machine.rs's rule), because group membership comes from the pipeline and the recorded steps.

## R-8 The reference mockup

**Decision**: `specs/008-studio-redesign/mockup/` holds three static HTML screens built from the mocks of 2026-10-31 and 2026-11-01 (loop page, Pipelines with the graph, Agents), with 007's tokens, fonts and logo, plus their screenshots at 1440 px and 390 px. They are the reference for FR-017 and the visual check; the engineer approves them.

**Why**: the engineer chose the mocks as the reference (2026-11-01); static files keep them reviewable in the repository.
