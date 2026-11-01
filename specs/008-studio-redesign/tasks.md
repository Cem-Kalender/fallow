# Tasks: Fallow Studio redesign

**Input**: Design documents from `specs/008-studio-redesign/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: requested. Every behaviour is tested with the fake runtime, as in 007. The screens are checked against the mockup with headless Chrome screenshots.

**Organization**: one phase per user story. US5 (parallel checkers) is engine work and independent of the page stories. After every phase, a fresh-context reviewer verifies it, and the implementer fixes the confirmed findings before the next phase. That is done outside this list.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1–US6)

---

## Phase 1: Setup

**Purpose**: the reference the screens are built against, and test support for two agents at once. T001 comes before every page task; the engineer approves the mockup afterwards (decision of 2026-11-01), and any change they ask for is a follow-up.

- [X] T001 Make the reference mockup in `specs/008-studio-redesign/mockup/`, from the mocks of 2026-10-31 and 2026-11-01, in 007's tokens, fonts and logo:
  - `loop.html`: the header with the steering, the "Verified by Fallow" row, an open evidence panel with one failure, and the side column without story and steering cards;
  - `pipelines.html`: the list, the graph of Standard with a parallel security reviewer, and the selected step in brief;
  - `agents.html`: the grouped list and the editor with fields.
  
  Add screenshots of each at 1440 px and 390 px, and `mockup/README.md` naming what each shows (FR-017; research R-8).
- [ ] T002 [P] Make the fake agent safe for two agents at once: take a lock file around its read-modify-write of `scenario.used` in `src/bin/fallow-fake-agent.rs`, so two steps never pick the same scripted step (research R-7).

---

## Phase 2: User Story 1 - See what Fallow verified, not the spec again (Priority: P1) 🎯 MVP

**Goal**: the loop page's "Verified by Fallow" row and its evidence panels.

**Independent Test**: a fake-runtime loop with one passing story and one failing criterion. Check the loop view's summary, then the page's row, the panel that opens by itself, and "What was verified".

- [X] T003 [P] [US1] Write `tests/us8_verification.rs`, testing `GET /api/loops/<l>` (`Request::Loop`) per contracts/loop-view.md:
  - `state` is `checked` with `checked = total` for a passed story;
  - `failed` with `failed = 1`, and the criterion's `state = "failed"` and `expected` set, for a failing one;
  - `progress` while the review is under way;
  - a story whose criteria all passed but whose second checker (for example a security reviewer) failed is `failed` with `findings = true`, never `checked` (a must-fail case, constitution I);
  - `not_checked` with `checked_by_pipeline = false` on a pipeline without a checker at the checkpoints (a must-fail case);
  - after a repair, criteria are `not_yet` until answered again.
- [X] T004 [US1] In `src/run/activity.rs`, add the story summary to each entry of `checks.stories[]`:
  - `checked_by_pipeline` (the pipeline has a scenario checker, feature.rs `scenario_checker`), `total`, `checked`, `failed`, `findings` (any checker of the story failed in the current round);
  - `state`, following data-model.md's rule: "`not_checked` if not checked_by_pipeline; else `failed` if failed > 0 or findings; else `checked` if checked = total; else `progress`";
  - per criterion, `expected` (the scenario's **Then** part, emphasis removed) when it failed.
  
  The current round is the review steps with `queued_seq` after the story's last repair or reopen, as `checkpoint()` decides; criteria keep 007's state values (`passed`, `failed`, `not yet`).
- [X] T005 [US1] In `src/service/web/screens/loop.js`, replace the story cards of the side column with the "Verified by Fallow" row under the pipeline strip:
  - one pill per story: `✓ n/n`, `✗ k failed`, `k/n`, or `not checked`;
  - at most one evidence panel under it, with each criterion's text, status, `ran`, `result` and `expected`, wrapping long text inside the panel;
  - the panel opens by itself when a story turns `failed`;
  - a failure line in the feed;
  - pills are buttons, reachable and usable by keyboard, with labels ("US2: 1 criterion failed, open the evidence"); text contrast at least 4.5:1 (FR-018).
  
  Keep the page's place across redraws (the open story). No pill for a story without criteria.
- [X] T006 [US1] In `src/service/web/screens/loop.js`, add "What was verified" next to Merge / Merge anyway; it opens a dialog (007's `confirm`-style modal) with every story's evidence, apart from the row's one-panel rule.
- [X] T007 [P] [US1] Add the styles of the row, the pills (green, orange, grey) and the panel to `src/service/web/app.css`, keeping 007's tokens; at 390 px they wrap without sideways scrolling.

**Checkpoint**: US1 works on its own; 007's tests that read `checks.stories` still pass (adjust any that asserted the old shape only where it changed).

---

## Phase 3: User Story 2 - Steer the loop from its header (Priority: P1)

**Goal**: the steering lives in the header.

**Independent Test**: on a fake-runtime loop, leave a note and change a limit from the header, and see the latest steering line. At ready, no Pause.

- [X] T008 [US2] In `src/service/web/screens/loop.js`'s header, place Note for the next step, Limits, Pause or Go on, and Stop (buttons reachable by keyboard, FR-018), using the existing `note`, `limits`, `pause`, `resume` and `stop` actions and dialogs. Show the latest entry of `steering[]` on a line under the header. Hide Pause at `ready`, `done_unverified` and final stages (FR-005).
- [X] T009 [US2] Remove the "Your steering" card from the side column in `src/service/web/screens/loop.js`; the side column keeps gates, questions and commits (contracts/page.md).
- [X] T010 [P] [US2] Style the header controls and the steering line in `src/service/web/app.css`, wrapping at 390 px.

**Checkpoint**: US1 and US2 together give the redesigned loop page.

---

## Phase 4: User Story 3 - Pipelines and agents, each in its own tab (Priority: P1)

**Goal**: two tabs, the agent form, "Save for".

**Independent Test**: switch agents, save a model change for this project only, check the file. Select a step in Pipelines and follow its link to Agents.

- [ ] T011 [P] [US3] Write `tests/us8_agent_fields.rs` (contracts/agents-api.md):
  - saving `{fields: {model: "opus"}}` for `project:<p>` on a default agent writes the project copy, differing from the default only in the `model:` line;
  - a comment, the order of the keys and a hand-written value (an unknown model) are kept;
  - an item with a space (`"npm audit"`) round-trips through 007's reader unchanged;
  - an absent key is added at the end of the frontmatter;
  - a list (`may_run`) is written as `[a, b]`;
  - an unknown field key, or `name`, is refused naming it;
  - a change that stops a pipeline in use from verifying is refused as in 007 (a must-fail case).
- [ ] T012 [US3] In `src/studio/defs.rs`, accept `fields` in `put_agent`:
  - take the text at that scope, else the resolved definition, else refuse;
  - rewrite only those keys' frontmatter lines in place (keeping a trailing comment), adding absent ones at the end of the frontmatter;
  - run 007's checks on the result.
  
  Allowed keys are exactly `description`, `runtime`, `model`, `kind`, `may_run` and `returns` (contracts/agents-api.md). Add `fields` to `Req::PutAgent` in `src/studio/mod.rs` and to the route's body in `src/service/http.rs`.
- [ ] T013 [US3] Create `src/service/web/screens/agents.js`, the Agents tab, with the selected project kept in `#/agents/<project>` and an agent opened by `#/agents/<project>/<agent>` (`_` for no project). Serve it like the other screens (`include_str!` and its route in `src/service/http.rs`) and load it from `src/service/web/index.html`:
  - agents grouped Lead / Makers / Checkers, each with its source and "in N pipelines";
  - the editor:
    - name, kind and source, with "Used in" links to `#/pipelines/<p>/<pipeline>`;
    - fields for runtime (from `/api/runtimes`), model (the runtime's models, keeping an unknown value), kind, `may_run` and `returns`, sent as `{fields}`;
    - the instructions as text;
    - "Edit as text" for the whole file;
    - New agent, and Save, Reset and Delete as in 007.
  - Clicking an agent always shows that agent (the editor's draft is kept only for the agent shown).
  - Every field and button is reachable by keyboard and labelled (FR-018).
- [ ] T014 [US3] Rework `src/service/web/screens/pipelines.js` into the Pipelines tab:
  - the pipeline list on the left, each with verifying or not verifying, what is missing, and the project's own pipeline marked;
  - the editor in the middle;
  - the graph over the rest of the width, and under it a row with the selected step's settings, its agent in brief (name, kind, runtime and model) with "Edit this agent in Agents →" (`#/agents/<p>/<agent>`), and what the pipeline guarantees;
  - `#/pipelines/<p>/<pipeline>` opens that pipeline;
  - the agent editor moves out of this screen.
- [ ] T015 [US3] In `src/service/web/index.html` and `src/service/web/app.js`:
  - the navigation (in index.html) becomes Dashboard · Spec sessions · Loops · Pipelines · Agents;
  - the router (app.js) takes `#/agents`, `#/agents/<p>`, `#/agents/<p>/<agent>` and `#/pipelines/<p>/<pipeline>` (contracts/page.md);
  - a shared "Save for: All my projects / This project only" select, defaulting to this project when one is selected, with the folder paths in its tooltip, used by both tabs;
  - the long override note becomes that tooltip.
- [ ] T016 [P] [US3] Add the styles of the Agents tab, the grouped list, the field grid and the Pipelines list to `src/service/web/app.css`, wrapping at 390 px.

**Checkpoint**: both tabs work, and US3's acceptance scenarios hold.

---

## Phase 5: User Story 4 - The pipeline as a graph (Priority: P2)

**Goal**: the graph replaces the list-style flow.

**Independent Test**: open Standard, add a parallel security reviewer through a +, remove checkers, and use the keyboard only.

- [ ] T017 [US4] In `src/service/web/screens/pipelines.js`, write the layout as a pure function `layout(pipeline)`, returning nodes, edges, back-edges and groups in a fixed 1360 × 430 space, by contracts/page.md's columns:
  1. Spec session and Lock, both fixed;
  2. "For each phase in tasks.md": `at = start` phase steps, the per-task steps, then `at = end` phase steps;
  3. "At each story's checkpoint": `at = start` steps, then the checkers, with each `parallel_with` set stacked in one column;
  4. `once` steps;
  5. Merge, fixed.
  
  Forward edges branch into a stacked column and join after it. Add the dotted "next task" back-edge, red repair back-edges labelled with the caps, and "→ you" for `on_fail = engineer`.
- [ ] T018 [US4] Draw the layout as SVG in the editor's box (a `viewBox` scaled to fit; scrolling inside the box below 700 px). Nodes carry a role colour (maker blue, checker green, `fallow:gates` grey, the engineer's fixed steps orange) and the "when" label. This replaces the list-style flow; "Edit as TOML" stays (FR-011).
- [ ] T019 [US4] Add the + on each forward edge: a small form (agent; `when` and `at` preset from the edge's place) appends the step to the draft pipeline, and the graph redraws. There are no free-drawn edges (FR-012).
- [ ] T020 [US4] Selecting a node shows its settings under the graph (agent, when, at, paths, alongside (`parallel_with`), on failure), reusing 007's step fields. A change updates the draft, the graph and the TOML. Removing a node removes the step; Spec session, Lock and Merge cannot be removed. After a removal, the pipeline check (`/api/pipelines/check`) says what is no longer verified (FR-013).
- [ ] T021 [US4] Keyboard and labels (FR-018):
  - nodes and + are focusable `role="button"` elements with an `aria-label` naming the step, when it runs and its agent;
  - arrow keys move between nodes in layout order;
  - Enter selects, Delete removes (with confirmation).
- [ ] T022 [P] [US4] Style the graph (groups, nodes, edges, back-edges, +, the selected node, the settings panel) in `src/service/web/app.css`, in 007's tokens.

**Checkpoint**: the graph shows every default pipeline correctly, and adding, editing and removing steps keeps the TOML in step.

---

## Phase 6: User Story 5 - Checkers that run alongside each other really do (Priority: P2)

**Goal**: a checkpoint's `parallel_with` checkers run at the same time, are judged together, and lead to one repair.

**Independent Test**: `tests/us8_parallel.rs` with the fake runtime.

- [ ] T023 [P] [US5] Write `tests/us8_parallel.rs`:
  - two checkers with `parallel_with`, each scripted to take 3 s, have overlapping `started`/`finished` times, and the checkpoint is judged only after both;
  - both failing lead to one repair whose instruction holds both checkers' reasons, then both run again;
  - with `max_parallel = 1` they run one after the other with the same outcome (a must-fail case for overlap);
  - Fallow stopped while both run, then started, resumes each in its own session, and no completed step runs again;
  - a question from one of them is answered while the other still runs;
  - one member that cannot start (its agent missing) does not stop the other, and is asked again;
  - SC-004: with `max_parallel = 2`, the checkpoint takes no longer than the slower checker plus 10%.
- [ ] T024 [US5] In `src/run/engine.rs`:
  - `Run` keeps `busy` for Gates and Merge, and gains `running: BTreeMap<u32, Running { stop, group, tool, asking }>`;
  - a helper `is_busy()` is true while either is set;
  - `Msg::Done { Step { n } }` removes only entry `n`;
  - shutdown and stop signal every entry;
  - `refresh_slots` and `agents_running` count running steps and sum their groups;
  - `run_of_token` searches every entry;
  - `delete_data` refuses while `is_busy()`;
  - `view()["busy"]` lists the running steps.
- [ ] T025 [US5] In `src/run/machine.rs`:
  - `drive` stops only while Gates or a Merge run (`busy`), not while steps run: it goes on to `feature::drive` with steps running, so that a finished member is processed while another still runs; `feature::drive`'s loop stops when `step()` starts nothing;
  - `stop`, `merge`, `retry` and `answer` use `is_busy()` and the running set instead of `busy == Step(n)`;
  - starting gates or a merge still requires that nothing runs.
- [ ] T026 [US5] In `src/run/questions.rs`:
  - `open`, `call` and `ask` find the step by its token in the running set;
  - `ask` refuses only a second open question of the same step;
  - `lead_done` takes `asking` from the entry that asked;
  - `deliver`'s "nothing later" test also compares the step's agent (`def`).
- [ ] T027 [US5] In `src/run/feature.rs`'s `checkpoint()`:
  - compute the story's parallel groups (the connected sets of `parallel_with` among the chosen checkers);
  - queue every member without a review since the last repair, and treat running members as pending;
  - judge only when every member has a verdict:
    - an incomplete or cut-short member is asked again alone (up to `CHECK_TRIES`);
    - a blocked member goes to the engineer;
    - otherwise all failing members' reasons go to one `repair_or_ask` (the review-round cap unchanged).
  
  Checkers not in a group stay sequential.
- [ ] T028 [US5] In `src/run/feature.rs`'s `step()` and `start()`:
  - while steps run, `step()` starts nothing but queued members of the open parallel group, and decides nothing else until the group is judged;
  - start every queued member of a group while `machine::slot` gives a slot;
  - a member that fails to start ends as failed (007's start retries first) and is handled like an incomplete member; the members already running go on;
  - run `take_answers` once before a group starts, not between its members;
  - give each member the remaining budget divided by the size of its group (not by how many start together);
  - `start()` inserts into the running set and no longer replaces a shared stop flag.
- [ ] T029 [US5] In `src/run/feature.rs`, make `requeue_cut_short` take up every cut-short step that has no later step of the same kind, part and agent, not only the last one (research R-7).
- [ ] T030 [P] [US5] In `src/run/activity.rs` and `src/run/dashboard.rs`, show every running step's agent as working, with its task and tool count.
- [ ] T031 [US5] Update `specs/007-fallow-studio/contracts/pipeline.md` (remove the "runs one after the other" caveat), `specs/007-fallow-studio/plan.md`'s Complexity Tracking (the `parallel_with` gap is closed by 008), and the pipeline designer's wording in `src/service/web/screens/pipelines.js`.

**Checkpoint**: US5's tests pass, and 007's loop tests (us2_*, us3_*, us4_ask, us5_dashboard) still pass.

---

## Phase 7: User Story 6 - One reference for the look (Priority: P3)

**Goal**: the changed screens match the mockup.

- [ ] T032 [US6] Take screenshots of the loop page, Pipelines and Agents at 1440 px and 390 px with headless Chrome, against loops of the fake runtime, into `specs/008-studio-redesign/visual-check/`. In its README, compare them with `mockup/`, noting every difference, and confirm that nothing scrolls sideways at 390 px (no page wider than the window). Check keyboard reach of the new controls (Tab order through pills, header, agent form, graph nodes) and text contrast (FR-018), and that every steering control is visible without scrolling at 1440 × 900 (SC-002).
- [X] T033 [US6] In `specs/007-fallow-studio/tasks.md`, mark T085 as superseded by 008's visual check, with the date and the engineer's decision of 2026-11-01.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T034 [P] Update `specs/007-fallow-studio/contracts/web-api.md`:
  - the loop view's new fields;
  - `PUT /api/agents` with `{fields}`;
  - the routes `#/pipelines` and `#/agents`.
  
  Update `README.md`'s description of the loop page and of Pipelines and Agents.
- [ ] T035 Measure:
  - `cargo xtask bench` (page ready ≤ 500 ms, idle ≤ 60 MB, running ≤ 200 MB);
  - SC-004 through the timing case of `tests/us8_parallel.rs` (T023), which runs in CI with `cargo test`.
  
  Record the results here.
- [ ] T036 A fresh-context review of the whole change against spec.md, the contracts and the constitution, then fix its findings.
- [ ] T037 Run quickstart Q1–Q5 with real agents (`claude`, Sonnet) in the sandbox, within $60. Record the runs, their costs and the problems found in this file, and fix what they find.

---

## Dependencies & Execution Order

- **Setup (Phase 1)**: T001 before T032; T002 before T023.
- **US1 and US2** both change `loop.js`; do US1, then US2.
- **US3** is independent of US1 and US2. **US4** needs US3's Pipelines tab (T014).
- **US5** is independent of the page stories; it needs T002.
- **US6** needs US1–US4 and T001.
- **Polish** comes last; T037 needs everything.

## Parallel Opportunities

- T002 alongside T001.
- Within a story, the test task [P] alongside the styles [P].
- US5 (engine) alongside US3 and US4 (page), by different people.

## Implementation Strategy

1. **MVP:** US1, the verification row, which is most of the value for the engineer at merge time.
2. Then US2 (header), US3 (tabs), US4 (graph) and US5 (parallel), each reviewed before the next.
3. US6 checks the look against the mockup.
4. The real-agent run (T037) confirms it end to end.
