---

description: "Tasks for 007, Fallow Studio"
---

# Tasks: Fallow Studio

**Input**: Design documents from `specs/007-fallow-studio/`: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md, mockup/.

**Tests**: included. Constitution v6.1.0 makes several of them CI gates:
- false claims and unverified results (I);
- reserved questions always relayed (II);
- the loopback, secret, Host and Origin checks, and egress (V);
- writes only where allowed (VI);
- resume at every stage (VII).

Unless a task says otherwise, tests use the `fake` runtime (research S-13).

**Organization**: by user story (spec.md US1 to US7). Work stays on branch `001-llm-aware-tui-editor`. Builds are debug builds (C: is nearly full).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story the task serves (US1 to US7)

---

## Phase 1: Setup (verifications, shipped files, removing what 007 replaces)

**Purpose**: verify the two Claude Code behaviours the design relies on, add the files Fallow ships, and remove 006's single-task flow.

- [X] T001 Verify research S-7 on Windows:
  - in a scratch repository, run `claude -p` with `--permission-mode dontAsk`, an allowlist without `Bash(git log:*)`, `--mcp-config` naming a stub MCP server that has an `approve` tool, and `--permission-prompt-tool mcp__fallow__approve`;
  - ask it to run `git log -1`;
  - record in `specs/007-fallow-studio/research.md` S-7 whether the tool is called, the input it gets, and how `allow` and `deny` answers are taken. If it is not called, record that Ask uses the fallback.
- [X] T002 Verify research S-5:
  - run `claude -p --resume <id> --fork-session --include-partial-messages --output-format stream-json`;
  - record in research.md S-5 the new session id's place in the stream, the partial-message event shape, and how thinking blocks appear, so the Claude runtime can drop them.
- [X] T003 [P] Add Inter (400, 500, 600, 700) and JetBrains Mono (400, 500) as Latin-subset woff2 files, with `OFL.txt`, in `assets/fonts/`. List their sizes in research.md S-3.
- [X] T004 [P] Copy Spec Kit 1.0.12 into `assets/spec-kit/`, with its MIT `LICENSE`:
  - `.specify/scripts/bash/*`, `.specify/templates/*` and `.specify/init-options.json` from this repository;
  - the constitution template as `.specify/memory/constitution.md`;
  - `.claude/skills/speckit-*/SKILL.md`.

  Add `xtask/src/spec_kit.rs` (`cargo xtask spec-kit <checkout>`) to refresh the copy.
- [X] T005 [P] Write the default agent definitions in `assets/agents/`: `lead.md`, `builder.md`, `reviewer.md`, `test-writer.md`, `security-reviewer.md` and `docs-writer.md`, as contracts/agent-definition.md describes. The lead's body covers the Spec Kit steps, `(must fail)` markers, the reserved list, and answering questions only from the documents or the conversations.
- [X] T006 [P] Write the default pipelines `assets/pipelines/{standard,test-first,solo}.toml`, as contracts/pipeline.md describes.
- [X] T007 Remove what 007 replaces (research S-18), keeping the build and the reused tests green:
  - the `run` command in `src/main.rs`;
  - the planner's turns (`plan_turn`, `say`, `queue_plan_turn`) in `src/run/machine.rs`;
  - parts and owned files;
  - `src/plan/`: move `command_problem` and its test to `src/project/commands.rs`;
  - `src/service/web/run.html`;
  - **not yet** 006's tests of verification, repairs, resume and limits (`tests/us1_*.rs`), or the planner path they drive: constitution VII's resume gate must not lapse. T044 removes them once T038 to T043 replace them.
  - **Done (2026-10-30)**: the `run` command is removed, and `command_problem` moved to `src/project/commands.rs`. The planner's turns, `src/plan/` and parts are still what the kept tests drive, so they go with T044. `run.html` goes with T020, which rewrites the page.
- [X] T008 [P] Create the module skeletons with doc comments naming their contracts, as in plan.md:
  - `src/project/`, `src/conversation/`, `src/speckit/` and `src/defs/`;
  - `src/agent/{rights,fake}.rs`;
  - `src/run/{questions,relock,activity}.rs`;
  - `src/service/notify.rs` and `src/platform/notify.rs`.

**Checkpoint**: `cargo build`, `cargo clippy --all-targets` and `cargo test` pass. T001 and T002 are recorded.

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: the runtime interface, the stores, the formats and the page shell that every story builds on.

- [X] T009 Define the runtime interface in `src/agent/mod.rs`, as contracts/runtime.md describes:
  - `trait Runtime`, `Call` (with `Rights`), `Session::{New, Resume, Fork}`, `Event` and `End`;
  - convert 006's `Call` and `Outcome` callers to it.
- [X] T010 Port `src/agent/claude.rs` to the trait:
  - `Rights` become `--allowedTools` rules under `dontAsk`, with the PowerShell mirrors (006 R-4); protected files and Read only mode become `--disallowedTools`; Ask mode runs under `--permission-mode default` with `--permission-prompt-tool` (T001);
  - `Fork` becomes `--resume <id> --fork-session`, and the partial messages become `message_delta`;
  - tool uses become `tool_call` events, with diffs for `Edit` and `Write` and the command for `Bash` and `PowerShell`;
  - tool results become `tool_result` events, with output trimmed to 64 KB;
  - thinking blocks are dropped.
- [X] T011 Write the `fake` runtime in `src/agent/fake.rs`, and extend `src/bin/fallow-fake-agent.rs`:
  - fork (a new session id with the old steps kept);
  - partial messages;
  - scenario steps matched by task id (`when = "T014"`);
  - a scripted `ask` call or `approve` request through the MCP bridge;
  - edits of any path, including locked documents, for the false-claim tests.
- [X] T012 [P] Write `src/agent/rights.rs`, replacing `allow.rs`, with unit tests:
  - rights by kind: maker, checker, lead;
  - rights by chat mode: read only, approved, ask, run anything;
  - the spec paths;
  - `may_run`, expanded with `gates` and `plan.commands`.
- [X] T013 [P] Settings and paths in `src/config/`:
  - `%USERPROFILE%\.fallow\` holds `settings.toml`, `agents\` and `pipelines\`;
  - the 006 settings file is moved there once;
  - user-only ACLs on new directories (006's platform code).
- [X] T014 [P] Add the conversation and loop events of contracts/storage.md in `src/run/events.rs` and `src/conversation/events.rs`, with their folds and round-trip tests. 006's runs stay readable.
- [X] T015 [P] Write the `tasks.md` and `spec.md` readers in `src/speckit/{tasks,spec}.rs`, as contracts/spec-kit-files.md describes:
  - phases, tasks, story phases, scenarios `USn/k`, must-fail marks, the reserved list, and markers;
  - `plan.md`'s `## Commands` section, checked like approved commands;
  - errors with file and line;
  - the task tick writer;
  - unit tests on Spec Kit's own template output and on broken files.
  - The tests read this repository's own `specs/*/tasks.md` and `spec.md`, made with Spec Kit, rather than its templates, which hold placeholders (`Phase N`, `TXXX`).
- [X] T016 [P] Write the agent definition parser in `src/defs/agent.rs`: the strict YAML subset of research S-11, with errors at the line, and unit tests.
- [X] T017 [P] Write the pipeline parser and checks in `src/defs/pipeline.rs`: `when` and `at`, `paths`, `on_fail`, `parallel_with` and caps, and the derived `verifying` with its missing list. Unit tests cover each check.
- [X] T018 Resolve definitions in `src/defs/mod.rs`:
  - built in, then `~/.fallow`, then the project;
  - on first start, the defaults are copied, and they are never overwritten after that;
  - a changed default is written as `<name>.md.new`.
- [X] T019 Extend long polling to scopes (`global`, `loop:<id>`, `conv:<id>`) in `src/service/http.rs`, as contracts/web-api.md describes.
  - `conv:<id>` is answered once conversations exist (T029); until then it is refused as an unknown scope.
- [X] T020 Build the page shell from the mockup:
  - `src/service/web/index.html`, the logo SVG, the header with the nav, and the needs-you count;
  - `app.css`: the colour tokens of research S-2, the mockup's classes (`card`, `pill`, `btn`, `btn2`, `nav`, `stage`, `task`, `ev`, `row`) and `@font-face` rules for `/fonts/*`;
  - `app.js`: the hash router, the long-poll client, and the render helpers;
  - a confirmation dialog that names what is affected, used by every destructive action on every screen (stop, unlock, removing a project, deleting data, definitions or pipelines; constitution II);
  - serve the fonts; keep the CSP `default-src 'self'`;
  - remove 006's `run.html` and its route (T007).
- [X] T021 [P] Extend the page's security tests (they are unit tests in `src/service/http.rs`; there is no `tests/web_api_auth.rs`): the new routes need the secret and pass the Host and Origin checks, and the page and fonts load nothing external (CI gate V).

**Checkpoint**: the shell opens at `fallow` with the empty dashboard. The parsers and the runtimes pass their tests.

---

## Phase 3: User Story 1 - Add a project and work out a feature with the lead (Priority: P1) 🎯 MVP

**Goal**: projects, the project conversation, spec sessions following Spec Kit, chat modes, and the lock.

**Independent Test**: add a fresh folder, work out a small feature with the lead until it can be locked, and lock it. The documents are committed and can no longer be changed through the session (spec, US1).

### Tests for User Story 1

- [X] T022 [P] [US1] `tests/us1_projects.rs`:
  - adding a project installs Spec Kit only where files are missing, overwriting nothing (CI gate VI);
  - an existing Spec Kit version is reported;
  - the git and folder kinds are recognised;
  - a missing path is shown as missing;
  - removing a project deletes nothing in its folder.
- [X] T023 [P] [US1] `tests/us1_conversation.rs`:
  - a turn resumes its session, and a spec session forks from the project conversation;
  - the reply streams as events;
  - after a turn, changes outside the spec paths are discarded and named (CI gate VI), and the spec documents are committed;
  - the engineer's edits reach the next turn;
  - a document edit with an old version returns 409.
- [X] T024 [P] [US1] `tests/us1_modes.rs`:
  - each mode gives the expected rights;
  - Shift+Tab's order is right;
  - `approve` handles once, always (the prefix is added after confirmation), deny, and the 10-minute time-out;
  - a new conversation starts in Ask.
- [X] T025 [P] [US1] `tests/us1_lock.rs`:
  - the lock is refused for open markers, analyze not done, a critical finding, a story without a must-fail scenario, and a broken `tasks.md`, naming each;
  - a lock commits the documents and creates the loop with its phases and the suggested budget.

### Implementation for User Story 1

- [X] T026 [US1] Projects in `src/project/mod.rs`: `projects.json` written atomically; add, remove and patch; the kind; the missing state; features scanned from `specs/NNN-*`.
- [X] T027 [US1] Spec Kit installation in `src/project/install.rs`: the embedded files are copied only where missing; the version is detected; the result is reported.
- [X] T028 [US1] Approved commands in `src/project/commands.rs`: the check, the list per project, and the API to read and edit it.
- [X] T029 [US1] Conversations in `src/conversation/mod.rs`:
  - turns through the runtime, with the session and its fork;
  - worktrees for git projects (research S-6);
  - commit or discard after each turn;
  - the notice for plain folders;
  - the conversation's event log, and resume after a restart.
- [X] T030 [US1] Chat modes and approvals in `src/conversation/modes.rs`, and the `approve` tool in `src/mcp/`, as contracts/mcp-tools.md describes. Use the fallback when T001 says so.
- [X] T031 [US1] The lead's instructions in `src/agent/prompts.rs`: per Spec Kit step, the `reply` schema in `src/agent/schemas.rs`, and the proposed feature.
- [X] T032 [US1] Features in `src/conversation/features.rs`:
  - a new feature gets its number, its branch, its worktree and a fork of the project conversation;
  - the stepper and its moves;
  - documents read and written with versions; while a lead turn runs, writes are refused (research S-21);
  - the branches of research S-9: `NNN-name` from the base branch, which is recorded as the merge target.
- [X] T033 [US1] The lock in `src/speckit/lock.rs`: the checks, the budget suggestion, the lock commit, the `git init` offer for plain folders, and creating the loop. The loop's start arrives in US2.
- [X] T034 [US1] The API routes for projects, conversations and features, as contracts/web-api.md describes, in `src/service/http.rs`.
- [X] T035 [US1] The project conversation screen in `src/service/web/screens/project.js`, matching `mockup/Project.dc.html`:
  - the features and project files column;
  - the chat with the streaming reply and the files read;
  - the proposed feature card with "Start spec session";
  - the mode chip with Shift+Tab;
  - the approval prompt;
  - the "lead knows" and ideas panels.
- [X] T036 [US1] The spec session screen in `src/service/web/screens/spec.js`, matching `mockup/Spec.dc.html`:
  - the stepper;
  - the chat, with clarify questions as choices;
  - the document tabs, editable with versions, with markers highlighted; read-only while a lead turn runs, keeping the engineer's draft, then applying it or showing both versions to merge (research S-21);
  - "Lock & start the loop" with its checks;
  - the lock dialog: pipeline, autonomy and budget.
- [X] T037 [US1] The minimal dashboard in `src/service/web/screens/dashboard.js`: the projects column with "Add folder" (a path input), and the features table. The rest comes in US5.

**Checkpoint**: Q1 and Q2 of quickstart.md work with real agents. US1's tests pass.

---

## Phase 4: User Story 2 - Run the locked feature as one loop to a single merge (Priority: P1) 🎯 MVP

**Goal**: the locked `tasks.md` runs task by task, with a session per phase, handovers, gates per task, checkers at checkpoints, repairs, budgets, unlock, and one merge.

**Independent Test**: lock a two-story feature in the sandbox and do nothing until it is ready. Each task has its commit and gate result, each story is reviewed with evidence, and one merge brings it all (spec, US2).

### Tests for User Story 2

- [X] T038 [P] [US2] `tests/us2_loop.rs`:
  - a two-story feature with the Standard pipeline;
  - a commit, a tick and a gate run per task;
  - a new session per phase, resumed within it;
  - a handover per phase;
  - a review per checkpoint answering every `USn/k`;
  - `ready`, then a merge of exactly the gated commit;
  - `tasks.md` on the target shows every task ticked.
- [X] T039 [P] [US2] `tests/us2_false_claims.rs` (CI gate I). Each of these is rejected or never reaches `ready`:
  - an edit to the locked documents;
  - a wrong task id;
  - files that were claimed but not changed;
  - a missing or empty handover;
  - an incomplete verdict, or a missing failing case;
  - a checker whose worktree changed;
  - a pipeline without gates or without a review, which ends `done_unverified` with its missing list.
- [X] T040 [P] [US2] `tests/us2_resume.rs` (CI gate VII): stops mid-task, mid-gates, mid-handover, mid-checkpoint and at ready. Each resumes in the right session, without repeating a completed task, gate run or review.
- [X] T041 [P] [US2] `tests/us2_repairs.rs`:
  - a task's failing gate becomes a repair in the phase session, and after the cap the loop asks the engineer;
  - a failing review becomes a repair, then the checkpoint runs again, capped by `review_rounds`;
  - steps cut short do not count.
- [X] T042 [P] [US2] `tests/us2_unlock.rs`:
  - unlocking pauses the loop after its current step;
  - the relock preview names the affected stories (research S-17);
  - after the relock, only those phases are rebuilt and reviewed;
  - a new baseline runs on the new lock commit.
- [X] T043 [P] [US2] `tests/us2_budget.rs`: the suggestion is tasks × the per-task amounts; the loop pauses at the limit with "continue for one step"; paused time and time waiting for the engineer do not count.

### Implementation for User Story 2

- [X] T044 [US2] Loop state in `src/run/state.rs`: the loop, its phases and tasks (data-model.md) from the lock's `tasks.md`; the folds of the new events; phase sessions; the build branch `fallow/NNN/build` from the lock commit, with `NNN-name` merged in at task boundaries (research S-9). Then remove 006's planner path, the service's `run` request (`src/service/mod.rs`), `src/plan/`, parts, and their tests, now replaced (T007).
- [X] T045 [US2] Scheduling from the pipeline in `src/run/machine.rs` (research S-9):
  - the builder per task: a new session at the start of a phase, then resumed;
  - `fallow:gates` per task or per phase;
  - checkpoint, phase and once steps, in fresh sessions;
  - the `paths` filter;
  - `parallel_with` within the cap. *Built so far: such checkers run one after the other at the same point, since a loop runs one step at a time. Running them at once is left for US5 (several loops and steps at once).*
- [X] T046 [US2] Checks per task in `src/verify/claims.rs` and `src/run/machine.rs`: the claim against the worktree, the locked documents untouched, the task tick, Fallow's commit, and the gates against the baseline.
- [X] T047 [US2] The handover step: its instruction in `src/agent/prompts.rs`, its schema in `src/agent/schemas.rs`, its check of the three sections and that nothing else changed, and Fallow's commit.
- [X] T048 [US2] Checker verdicts in `src/verify/verdict.rs`: criteria `USn/k` from `spec.md`, must-fail cases, and findings with severity, where `high` or above fails.
- [X] T049 [US2] Repairs and caps from the pipeline in `src/run/machine.rs`, keeping 006's rule that cut-short steps do not count.
- [X] T050 [US2] The outcome in `src/run/machine.rs` and `src/publish/local.rs`:
  - `ready`, or `done_unverified` with its missing list;
  - the merge of exactly the gated commit, refused when it does not merge cleanly.
- [X] T051 [US2] Budgets in `src/run/time.rs` and `src/run/state.rs` (FR-016a): the per-task suggestion, pausing at the limit, and excluding paused time and time waiting for the engineer. This also fixes 006's known gap.
- [X] T052 [US2] Unlock and relock in `src/run/relock.rs` (research S-17): the affected stories from the two lock commits, the preview API, reopening phases, and a new baseline.
- [X] T053 [US2] The concurrency cap and the memory budget across loops and conversations in `src/run/engine.rs` (research S-19): restore 005/006's memory sampler into `src/platform/`, check the budget before a step starts, put chat turns first, and show "waiting for a slot".

**Checkpoint**: Q3 and Q5 of quickstart.md work with real agents. US2's tests pass.

---

## Phase 5: User Story 3 - Watch what every agent does (Priority: P1) 🎯 MVP

**Goal**: the loop screen of the mockup: the pipeline, the task tree, agent cards, the feed, Fallow's checks, steering and totals.

**Independent Test**: for any finished task, name from the page alone the agent, its diff, its commands and results, the commit and the gate result (spec, US3; SC-004).

### Tests for User Story 3

- [X] T054 [P] [US3] `tests/us3_activity.rs`:
  - runtime events become activity attributed to the agent and the task: diffs, outputs and results (ok, failed, refused);
  - no thinking is shown;
  - Fallow's own check entries appear;
  - the activity reaches long polling within 2 s, p95 over a scripted loop (SC-005).
- [X] T055 [P] [US3] `tests/us3_task_detail.rs`: the task endpoint returns its steps, diff, commands, commit and gate result. A reopened page gets the whole history.

### Implementation for User Story 3

- [X] T056 [US3] Activity in `src/run/activity.rs`: from runtime events to activity lines with agent and task; Fallow's check entries (commits, gates, verdicts, answers); kept per step.
- [X] T057 [US3] The loop API in `src/service/http.rs`: state, pipeline stage, phases and tasks, agents with runtime, model, branch, status and cost, checks, activity with filters, and the task detail.
- [X] T058 [US3] The loop screen in `src/service/web/screens/loop.js`, matching `mockup/Run.dc.html`:
  - the header with totals and pause and stop;
  - the pipeline strip with the current stage;
  - the question banner;
  - the task tree with ticks, the current task and waiting tasks;
  - the agent cards;
  - the feed with filters, "follow live", diffs and command output;
  - the checks column: gates against the baseline, story criteria, commits;
  - steering: a note, and limits.

**Checkpoint**: the MVP (US1–US3) is complete. Run quickstart Q1–Q3 and Q5 with real agents, and do the visual check of screens 2, 3 and 5 against the mockup.

---

## Phase 6: User Story 4 - Questions go through the lead (Priority: P2)

**Goal**: `ask` during a step, answered, decided or relayed by the lead according to the autonomy level, with every answer committed and open to overruling.

**Independent Test**: a scripted agent asks two questions, one the spec answers and one it does not. The first is answered and committed, and the agent continues. The second reaches the engineer, and the loop waits for it (spec, US4).

### Tests for User Story 4

- [X] T059 [P] [US4] `tests/us4_ask.rs`:
  - an answer needs a basis;
  - `decided` happens only at Free, and only for a small question as FR-020a defines it;
  - Strict always relays, and reserved items or a change to a criterion are always relayed (CI gate II);
  - the answer is committed to `## Clarifications` and reaches the build branch at the next task boundary;
  - a relayed question makes the loop wait, while another loop goes on;
  - the step resumes with the answer;
  - overruling reopens the tasks built on the old answer and runs the checkpoint again;
  - one open ask per step;
  - the lead's 3-minute limit leads to a relay.

### Implementation for User Story 4

- [X] T060 [US4] Wire `ask` and `progress` to loop steps in `src/mcp/` and `src/run/questions.rs`: tokens per step, one open question, and the question's events.
- [X] T061 [US4] The lead's answer call in `src/run/questions.rs`: a fork of the spec session, the `answer` schema, and the rules of contracts/step-results.md, including the autonomy level and the reserved items.
- [X] T062 [US4] Commit answers in `src/run/questions.rs`: to the feature branch's `spec.md`, merged into the build branch at the next task boundary.
- [X] T063 [US4] Relay to the engineer: the step ends blocked, the loop waits (clarified 2026-10-30), the answer API, and the step resumes in its session.
- [X] T064 [US4] Overrule: the reversal commit, the tasks after the answer reopened as a repair, and the checkpoint run again.
- [X] T065 [US4] On the page, in `screens/loop.js` and the inbox: the question banner with options and the recommendation, answer and overrule buttons, and "lead answered" or "decided by the lead" entries with their commits in the feed.
  - Done on the loop page, with Confirm for a decision of the lead. The question's card in the dashboard's "Needs you" inbox comes with US5 (T068, T069).
  - Unlock edge case: while the feature is unlocked, a waiting question is not answered in the spec session; the engineer answers it on the loop page after the new lock (answers are not committed while the documents are open again).

**Checkpoint**: Q4 of quickstart.md works with real agents.

---

## Phase 7: User Story 5 - The dashboard (Priority: P2)

**Goal**: the dashboard of the mockup, with several loops, the "Needs you" inbox and desktop notifications.

**Independent Test**: two projects with a loop each. Both show with correct progress, questions from either reach the inbox, and the number of agents at once stays within the cap (spec, US5).

### Tests for User Story 5

- [X] T066 [P] [US5] `tests/us5_dashboard.rs`: two loops in two projects shown with their phase progress; needs-you items for a question, a ready feature and a stopped loop; "waiting for a slot" above the cap.
- [X] T067 [P] [US5] `tests/us5_notify.rs`, with a recording `platform::notify`: one notification per item; the text names only the project, the feature and what is needed (a marker test: no code or output); the off switch.

### Implementation for User Story 5

- [X] T068 [US5] The dashboard API and global events in `src/service/http.rs`: projects, needs-you, loops with phase progress and agents' activity in a few words, features, agents running, and cost today.
- [X] T069 [US5] The full dashboard screen in `screens/dashboard.js`, matching `mockup/Main.dc.html`: needs-you cards, running loops with phase bars, the features table, and the header count.
- [X] T070 [US5] Windows notifications in `src/platform/notify.rs` (research S-14):
  - `Shell_NotifyIconW` on a hidden message-only window, with the new `windows-sys` features;
  - balloons, and a click opens the item's URL;
  - a menu with Open, Notifications on/off and Stop;
  - a no-op on other platforms.
  - Built on a hidden window of its own rather than a message-only one (its menu needs a window that can come to the front), which also needs `windows-sys`'s `Win32_Graphics_Gdi` feature for the window class. Items already there when Fallow starts were notified before it stopped and are not raised again.
- [X] T071 [US5] `src/service/notify.rs`: needs-you items become notifications, with deduplication and the setting.
- [X] T072 [P] [US5] `tests/us5_data.rs`: the data view's sizes per project, loop, conversation and worktree; deleting a loop or conversation's data after confirmation; a running loop refused (FR-003b, constitution V).
- [X] T073 [US5] The data view: `GET /api/data` and `DELETE /api/data/<kind>/<id>` in `src/service/http.rs`, sizes computed in `src/project/data.rs`, and a "Data" panel reached from the dashboard in `screens/dashboard.js`, in the mockup's style, with the confirmation dialog.

**Checkpoint**: the dashboard matches screen 1, and notifications arrive with the page closed.

---

## Phase 8: User Story 6 - Agents and pipelines the engineer designs (Priority: P3)

**Goal**: the pipeline designer of the mockup, custom agents, and pipelines the lead proposes.

**Independent Test**: a custom checker that runs only when `api/` changes is added to a copy of Standard. It runs only after such changes, lists what it ran, and its failure leads to a repair (spec, US6).

### Tests for User Story 6

- [X] T074 [P] [US6] `tests/us6_defs.rs`:
  - the definitions and pipelines API: create, edit and delete, with errors at the line;
  - override order;
  - `.md.new` on update;
  - a custom checker with `paths` runs only on matching changes, its findings need `ran`, and its failure becomes a repair;
  - a proposed pipeline is never used before it is saved.

### Implementation for User Story 6

- [X] T075 [US6] The definitions, pipelines and runtimes API in `src/service/http.rs`, as contracts/web-api.md describes.
- [X] T076 [US6] The pipeline designer screen in `screens/pipelines.js`, matching `mockup/Pipeline.dc.html`:
  - the agent library with each agent's source;
  - the flow: add, remove and reorder steps; `when`, `at`, `paths` and `on_fail`; the "always" steps shown fixed;
  - the "not verifying" warning;
  - the definition editor with errors at their lines;
  - save.
- [X] T077 [US6] Pipelines the lead proposes: `proposed_pipeline` in a reply opens as a draft in the designer, used only after save. Choosing a pipeline per project, and per feature in the lock dialog.

Notes on Phase 8 as built:
- A definition or pipeline that does not pass is refused like other requests (`ok: false`, not a 422), one problem per line (contracts/web-api.md). `POST /api/pipelines/check` was added: the designer checks every change with it, and a pipeline the lead proposed opens through it as a draft.
- A path-limited checker that has not yet passed on a story compares against the commit before the story's first build, not the lock: it runs only when that story changed its paths (found by T074).
- What a checker's finding ran is carried into its reason, and the loop page lists every checker's latest outcome per story.
- Deleting an agent that a pipeline in force still uses, or a pipeline a project chooses, is refused. A default the engineer edited is reset, not deleted.
- "Ask the lead" in the designer opens the project conversation with the pipeline in the message box; the engineer sends it.

---

## Phase 9: User Story 7 - Agents from different providers (Priority: P3)

**Goal**: the runtime interface proven by a second runtime, and runtimes chosen per agent.

**Independent Test**: the full loop runs once with Claude and once with the `fake` runtime declared in the definitions, and behaves the same (spec, US7).

- [X] T078 [P] [US7] `tests/us7_runtime.rs`: a loop whose definitions name `runtime: fake` runs end to end through the registry; a runtime that is not installed fails the step, naming it, and the loop asks the engineer.
- [X] T079 [US7] The runtime registry in `src/agent/mod.rs`: availability, `GET /api/runtimes`, the definition's runtime and model used per step, and both shown on the agent cards.

Notes on Phase 9 as built:
- `agent::registry` builds the runtime a definition names from the settings' profiles: `claude` from `[agents.claude]` or the `claude` program on the PATH, `fake` only from `[agents.fake]`. Steps, the lead's answers in a loop and the lead's turns in chats all start through it.
- A runtime that is unknown, not configured or not installed is refused when its step would start: the loop asks the engineer, naming the agent and the runtime ("reviewer cannot run: the runtime \"fake\" is not configured…").
- The pipeline designer's agent library shows each agent's runtime and model, and says when the runtime is not installed.

---

## Phase 10: Polish & cross-cutting concerns

- [X] T080 Run records for loops in `src/measure/record.rs` (research S-20): restore 005/006's record format and field rules from git history; write a record when a loop ends, with its pipeline's steps, tasks, repairs, review rounds, questions by outcome, interventions, time, cost and outcome.
- [X] T081 [P] The record marker test in `tests/record_marker.rs`: marker text planted in a loop's task, documents, agent output and answers never appears in its record (constitution V).
- [X] T082 Update `.github/workflows/ci.yml` so the CI gates run: false claims and unverified results; reserved questions relayed; writes only where allowed (project install, discarded lead edits); loopback and egress; resume; the record marker test.
- [X] T083 [P] Benchmarks in `xtask/src/bench.rs`: dashboard ready within 500 ms; idle memory, and memory with 4 concurrent fake steps across two loops (constitution VIII).
- [X] T084 [P] Accessibility on the five screens:
  - every control reachable by keyboard;
  - a visible focus ring;
  - `aria-label` on icon buttons;
  - text contrast of at least 4.5:1 (FR-032).
- [ ] T085 The visual check (SC-007, SC-008): each screen next to its mockup at 1440 px and 390 px, with screenshots in `specs/007-fallow-studio/visual-check/`. The engineer approves.
- [ ] T086 Run quickstart Q1–Q5 with Sonnet in the sandbox (reset it first), and record the runs, their costs, the time from adding the folder to the lead's first reply (SC-001), and the problems found in this file.
- [X] T087 [P] Documentation: `README.md`; mark 006's superseded user stories in `specs/006-headless-orchestrator/tasks.md`; update the contracts with anything that changed during the build.
- [X] T088 A fresh-context review of the whole change, then fix its findings.

Notes on Phase 10 as built:
- T080: `src/measure/record.rs` writes `records/v1/loop-<id>.json` once, when a loop ends, from
  its state alone: the pipeline's steps, caps, level, agents by runtime and model, phases, tasks,
  repairs, review rounds, questions by outcome, interventions, locks, commits, time, cost and the
  outcome. Names the engineer chose become `custom`; `check()` refuses anything but fixed values,
  identifiers and numbers.
- T082: the 007 gates run as named steps in CI. 006's egress check is not restored: it needed an
  administrator's capture of the DNS log on Windows. Fallow's code has no client socket and no
  HTTP client crate; its one listener is the loopback page (`tiny_http`). Loopback, secret, Host and Origin are `service_http`.
- T083: `cargo xtask bench`. A loop runs one step at a time, and checkers marked `parallel_with`
  still run one after the other (T053's note), so the 4 concurrent steps come from four loops.
  On the debug build: page ready 160 ms (p95), idle 10 MB, running 15 MB.
- T084: every action is a button or a link; the focus ring is never removed; no icon-only button
  lacks a label. Every text colour pair is at least 4.5:1 except the document's line numbers
  (#A3A29B, 2.6:1), now the faint ink.
- T085 (open): the screenshots are in `visual-check/`, with what knowingly differs. The
  engineer's approval is pending. Taking them found two page bugs, now fixed: a screen left
  behind could write over the next one, and a stopped long poll kept its connection, so a
  screen opening after four others could wait for one.
- T086 (open): needs real agents (`claude`, Sonnet) in the sandbox and costs money; left for the
  engineer to run or to approve.
- T088: the review found one must-fix and seven should-fix findings, all fixed:
  - a relock while paused at ready went back to ready without checking the change: it now
    builds and checks it, a loop merges only a judged result, and an unjudged ready loop is
    never "Ready to merge" (test in `us2_unlock.rs`);
  - screens start their watchers only while still shown;
  - a project whose loops are still going cannot be removed;
  - a stopped loop's feature is "stopped";
  - a loop's data waits for its last step to end before it can be deleted;
  - an ended loop's record is tried once per start;
  - two known gaps are recorded in plan.md's Complexity Tracking (egress capture,
    `parallel_with`), with a source check in CI (`no_egress_no_push.rs`) and HTTP client crates
    banned in deny.toml; take-over is recorded there too.
  - The minor ones fixed: record model ids by form only; the marker test plants a model alias
    and checks the record after a later event; counts use only the lock in force; the README on
    waiting questions; the data view shows the run records.
  - Left as they are: "page ready" counts any answer of the page (a refusal without the secret
    is one); build worktrees stay registered until the loop's data is deleted, which the data
    view shows.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)** comes first. T001 and T002 can change T010 and T030.
- **Foundational (Phase 2)** depends on Setup, and blocks every story.
- **US1 (Phase 3)** depends on Phase 2.
- **US2 (Phase 4)** depends on Phase 2 and on US1's lock (T033). It can be tested on its own with locks made by tests.
- **US3 (Phase 5)** depends on US2's loop state (T044) and activity events (T010, T011).
- **US4 (Phase 6)** depends on US2, and on US1's spec session (the fork).
- **US5 (Phase 7)** depends on US1 to US3. Notifications (T070, T071) can start after Phase 2.
- **US6 (Phase 8)** depends on Phase 2's parsers. Its custom-checker test needs US2.
- **US7 (Phase 9)** depends on Phase 2 and US2.
- **Polish** comes last.

### Within a story

Tests first, written to fail; then the state and model, the logic, the API, and the screen.

### Parallel opportunities

- Setup: T003, T004, T005, T006 and T008 together, after T001 and T002 (which need real agents).
- Foundational: T012 to T017 together. T020 and T021 alongside them.
- Each story's test tasks marked [P] together.
- US5's notifications (T070, T071) and US6's designer (T076) can be built alongside US2 to US4 by another developer.

## Parallel Example: User Story 2

```text
Task: "T038 tests/us2_loop.rs"
Task: "T039 tests/us2_false_claims.rs"
Task: "T040 tests/us2_resume.rs"
Task: "T041 tests/us2_repairs.rs"
Task: "T042 tests/us2_unlock.rs"
Task: "T043 tests/us2_budget.rs"
```

## Implementation Strategy

### MVP first (US1 to US3, all P1)

1. Phases 1 and 2.
2. US1: spec sessions to a lock. Validate with Q1 and Q2.
3. US2: the loop to one merge. Validate with Q3 and Q5.
4. US3: the loop screen. Validate SC-004, and do the visual check of screens 2, 3 and 5.
5. **Stop and validate with the engineer** on a real feature in the sandbox.

### Incremental delivery

6. US4: questions through the lead (Q4).
7. US5: the dashboard and notifications.
8. US6: agents and pipelines.
9. US7: the second runtime.
10. Polish.

Each step is committed and pushed on `001-llm-aware-tui-editor`, with a fresh-context review at the end of the MVP and at the end.
