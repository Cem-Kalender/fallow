---

description: "Task list for 005 Adaptive Delegation"
---

# Tasks: Adaptive Delegation

**Input**: Design documents from `specs/005-adaptive-delegation/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Included. The constitution requires them:
- a test for every path that types into a session or acts on a process;
- the run-record marker test (new in v5.0.0);
- benchmarks for performance claims.

SC-005 to SC-009 are CI tests or `xtask` scenarios. SC-001 to SC-004 are the engineer's comparison
results on their own task set (quickstart Q5). Tests marked [P] are written first and must fail before
their implementation.

**Organization**: by user story, in the order the plan sets: measure first. The MVP is Phases 1–4:
- the lead builds by default (US1 core);
- plain and solo runs compared on hidden tests (US2).

Prepared parallel work (US1 rest, US3), verification (US4), switching components off (US2, FR-017), the
lead (US5) and memory (US6) follow.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1 … US6)
- Paths follow the plan's Project Structure (single crate `src/`, `tests/`, dev-only `xtask/`).

---

## Phase 1: Setup

- [X] T001 Create the module skeletons with doc comments only and register them in
  `src/orchestration/mod.rs`: `manifest.rs`, `signals.rs`, `delegation.rs`, `runs.rs`, `record.rs`,
  `usage.rs`, `memory.rs` under `src/orchestration/`.

---

## Phase 2: Foundational (blocking)

**Purpose**: the manifest, git additions, the `check` level, the new merge conditions and the built-in
squad.

**⚠️ CRITICAL**: no user story work starts before this phase is done. 004's tests and
`cargo xtask squad` stay green.

- [X] T002 [P] Write `tests/squad_manifest.rs` for `.fallow/work.toml` (contracts/work-manifest.md):
  - parse `docs`, `shared`, `test_patterns`, `[[work]]`, `[[part]]`, `[[contract]]`, `[[review]]`;
  - errors as `.fallow/work.toml:LINE: message`:
    - duplicate ids;
    - `build` not one of `lead` | `builder` | `features` | `parts`;
    - `reason` missing when `build` is not `lead`;
    - a part's `work` undefined;
    - a contract's `between` naming an undefined part;
    - duplicate review focuses;
  - a manifest with errors counts as absent;
  - limits: at most 64 pieces of work and 32 parts;
  - criteria with `must_fail`.
- [X] T003 Implement `src/orchestration/manifest.rs`:
  - the parser and validation of T002;
  - `Manifest::at(commit)`, reading `git show <commit>:.fallow/work.toml` through the git worker;
  - the absent case.
- [X] T004 [P] Extend `tests/squad_git.rs` with temporary repositories for:
  - show at commit;
  - commits between two commits touching given paths;
  - `merge-base --is-ancestor`;
  - `hash-object --stdin` with CRLF and LF input giving one fingerprint after normalising;
  - fetching one commit into a new repository, so that only its history is present and other branches
    are absent.
- [X] T005 Implement the T004 calls in `src/orchestration/git.rs`, read-only on the engineer's
  repository, on the existing worker thread.
- [X] T006 [P] Extend `tests/squad_roles.rs` for the `check` level (research R-3):
  - the exact flag vectors for Claude Code and Codex;
  - `None` for Gemini;
  - a `check` session's tracked change or commit voids its verdicts like `read-only`;
  - ignored build output does not.
- [X] T007 [P] Extend `tests/squad_file.rs`:
  - `level = "check"`;
  - `max_parallel_builders` "`1..=16`", `signal_unanswered_min` "`1..=240`", `run_idle_end_min`
    "`1..=60`", `test_patterns`;
  - merge conditions `work:done` and `review:approved`;
  - the new built-in squad: lead, builder ×3, reviewer (`check`) ×2, merge `work:done` and
    `review:approved`.
- [X] T008 Implement T006 and T007:
  - `Level::Check` and its flags in `src/orchestration/roles.rs`;
  - the squad additions, the new conditions and the built-in squad in `src/orchestration/squad.rs`;
  - the conditions' labels in `src/orchestration/publish.rs` (readiness logic comes in T011).

---

## Phase 3: User Story 1 core: the lead builds by default (Priority: P1) 🎯 MVP part 1

**Goal**: a burst builds each piece of work in the lead's own session unless the manifest delegates
it.

**Independent Test**: quickstart Q1.

- [X] T009 [P] [US1] Write `tests/squad_delegation.rs`:
  - with no manifest, the lead's `whoami` says it builds the work itself;
  - `session_start` for a builder is refused, naming the reason and what a split needs (FR-003);
  - with `build = "builder"`, the first builder starts and a second is refused;
  - Fallow creates the lead's task from the burst description (no manifest), or one per piece of
    work built by the lead;
  - `work:done` holds once those tasks are reported done with evidence;
  - `review:approved` without focuses follows 004's rule.
- [X] T010 [US1] Implement the start refusals for `lead` and `builder` decisions in
  `src/orchestration/delegation.rs`, and the `work` argument of `session_start` in
  `src/orchestration/tools.rs`.
- [X] T011 [US1] Implement:
  - the lead's tasks at burst start in `src/orchestration/burst.rs`;
  - `work:done` and `review:approved` readiness in `src/orchestration/publish.rs`.
- [X] T012 [US1] Rewrite the lead, builder and reviewer briefings and the `whoami` additions in
  `src/orchestration/briefing.rs` and `tools.rs` (contracts/mcp-tools.md "Briefings"):
  - build yourself unless the work has clean boundaries;
  - use your own subagents to investigate.
- [X] T013 [US1] Add the Work section to the Squad tab in `src/ui/orchestration.rs`: each piece of work
  with its decision, reason, builder and state, and refused starts with their reason (FR-007).
- [X] T014 [US1] Add a `solo-loop` run to `xtask/src/squad.rs`:
  - the built-in squad with no manifest;
  - the lead commits and reports, and a builder start is refused;
  - the gate and the reviewer run;
  - the merge becomes ready on `work:done` and `review:approved`.

**Checkpoint**: one loop is the default, enforced, and visible.

---

## Phase 4: User Story 2: measure and compare (Priority: P2) 🎯 MVP end

**Goal**: plain and solo runs of a task set, isolated, scored by hidden tests, recorded and compared.

**Independent Test**: quickstart Q5 with plain and solo.

- [X] T015 [P] [US2] Write the task set tests in `tests/squad_runs.rs` (contracts/task-set.md):
  - parsing and errors with line numbers;
  - fingerprints of the description and of the hidden tests stable under CRLF/LF (FR-016);
  - the run copy holds only history up to `start`, has the `setup` files, and leaves the engineer's
    repository's refs unchanged (FR-011a).
- [X] T016 [US2] Implement task sets in `src/orchestration/runs.rs`:
  - parsing and fingerprints;
  - registration in the setting `orchestration.task_sets`, with the hidden-tests notice.
- [X] T017 [US2] Implement run copies in `src/orchestration/runs.rs` and `src/session/worktree.rs`:
  - a new repository under the cache, fetching only `start`, with the setup files;
  - worktrees from the copy;
  - removal after the run.
- [X] T018 [P] [US2] Write the run lifecycle tests in `tests/squad_runs.rs`:
  - plain: the session starts with the `edit-run` flags and a usage-only link, and has no MCP tools
    (FR-012);
  - the description and the closing line are typed;
  - the idle end does not fire while a gate runs, a start is pending or waiting for memory, or a
    hand-over is typed (FR-013);
  - the time limit ends a run;
  - an engineer's end counts as an intervention;
  - the close confirmation names every session, and nothing closes without it;
  - after closing, the hidden files go into the lead's worktree and run as gate sessions, and counts
    are recorded;
  - a build failure counts every hidden test as failed;
  - a run in progress at exit is discarded (FR-019a).
- [X] T019 [US2] Add run sessions in `src/session/mod.rs` (without orchestration tools for plain runs,
  with the usage status line for recorded runs; research R-10 reads the report from the screen, so
  no link is needed), and plain starts in `src/app/runs.rs`.
- [X] T020 [US2] Implement the run lifecycle in `src/orchestration/runs.rs` and
  `src/app/orchestration.rs`:
  - running → ended → testing → recorded | discarded;
  - the end rule in `tick`;
  - the close confirmation (Principle IV: one dialog naming the sessions and the action);
  - hidden tests as gate sessions in the lead's worktree;
  - solo runs as a burst with the run's settings and an allowance that needs no approval.
- [X] T021 [US2] Count interventions in `src/app/orchestration.rs` and `src/app/workspace.rs` (FR-014):
  - each line the engineer sends into a run session;
  - each approval or decision during a run;
  - not the close confirmation.
- [X] T022 [P] [US2] Write the usage tests in `tests/squad_runs.rs` (research R-10):
  - `usage-report` parses `cost.total_cost_usd`, `model.id` and `version` and forwards them;
  - cost and tokens are unknown when missing;
  - `--settings` comes before the list flags;
  - no status line when the profile passes its own `--settings`;
  - the command path uses forward slashes and is quoted;
  - only run sessions get it.
- [X] T023 [US2] Implement:
  - `src/orchestration/usage.rs` (the marker, reading it from the screen, the status line
    arguments);
  - `fallow usage-report` in `src/cli.rs` and `src/main.rs`;
  - the status line for run sessions in `src/session/mod.rs`.
- [X] T024 [P] [US2] Write the record tests in `tests/squad_runs.rs` (contracts/run-record.md):
  - only the listed fields;
  - parts and roles as numbers;
  - fixed outcome values;
  - the file under `runs/v1/` with mode 0600 on Unix;
  - list and delete;
  - **the constitution's marker test**: markers in the task name and description, the part, role and
    focus names, messages, signals, reviews, test names, paths, session output and a custom model
    alias never appear in the record.
- [X] T025 [US2] Implement `src/orchestration/record.rs`:
  - collect per part and per role the time working and blocked (FR-014's definition), signals, repair
    rounds, gate and review outcomes, and cost;
  - write the record once the hidden tests finish;
  - list and delete.
- [X] T026 [US2] Implement series in `src/orchestration/runs.rs`, with tests in `tests/squad_runs.rs`
  (FR-018):
  - a shuffled order seeded from the clock;
  - repeats per task and mode (default 3);
  - one run at a time;
  - waiting at each end for the close confirmation.
- [X] T027 [US2] Add the Runs tab and the palette commands in `src/ui/orchestration.rs` and
  `src/app/orchestration.rs` (contracts/ui-commands.md):
  - commands: Register a task set, Start a run, Start a series, End the run, Open runs;
  - grouping by task fingerprint, durations and mode, showing pass rate, median time, interventions,
    cost per passing run and spread (FR-019);
  - a delete confirmation;
  - the count and size of records;
  - task names by fingerprint match.
- [X] T028 [US2] Remove the copy of a discarded run lazily, when the Runs tab opens or a run starts,
  never at startup (Principle II), in `src/orchestration/runs.rs`.
- [X] T029 [US2] Add a `runs` run to `xtask/src/squad.rs` with a tiny task set:
  - one plain and one solo run, recorded, with two hidden test commands (one passes, one fails);
  - check the records and the counts;
  - check that the project's branches are unchanged and that the copies are removed.

**Checkpoint (MVP)**: plain vs solo can be compared on the engineer's tasks (SC-004).

---

## Phase 5: User Story 1 rest and User Story 3: prepared parallel work, decisions in the repo (Priorities: P1, P3)

**Goal**: parallel parts and independent features only when prepared; builders and the lead
coordinate through signals and commits.

**Independent Test**: quickstart Q2.

- [X] T030 [P] [US1] Extend `tests/squad_delegation.rs` for parts and features (FR-004 to FR-006,
  FR-010):
  - refusals:
    - `owns` empty;
    - owned globs overlapping, naming the files;
    - a boundary without a committed contract;
    - a part owning only test files or only `docs`;
    - `max_parallel_builders` reached;
  - features start without contracts;
  - builder branches are named `fallow/<work>-<part>` (research R-18).
- [X] T031 [US1] Implement T030 in `src/orchestration/delegation.rs` and `src/session/worktree.rs`.
- [X] T032 [P] [US3] Write `tests/squad_signals.rs` (contracts/mcp-tools.md):
  - `signal_raise` by builders only, to the lead only;
  - `change` needs a diff;
  - `signal_answer`:
    - a `commit` must be on the lead's branch and touch the manifest, docs or a contract;
    - without a commit, only an answer stating no change;
    - the raiser receives the answer and the commit;
  - `signal_list`;
  - `deliver` from a builder only to the lead;
  - `consult_lead` is gone;
  - an unanswered signal is told to the engineer once after `signal_unanswered_min`.
- [X] T033 [US3] Implement `src/orchestration/signals.rs` and the tools in
  `src/orchestration/tools.rs`; remove `consult_lead`.
- [X] T034 [P] [US3] Test with temporary git repositories (in `tests/squad_signals.rs`, which has the
  repository fixture):
  - the plan-commit check (research R-5):
    - a change to the part's criteria or contract must be in the reported commit, and the refusal
      names it;
    - a change to another part's criteria does not count;
  - the ownership check: files outside `owns` and `shared` are reported to the lead with their owner;
  - a non-lead change to the manifest, docs or a contract becomes a Fallow signal, and the work counts
    after `accept` (R-19).
- [X] T035 [US3] Implement T034 in `src/orchestration/delegation.rs`, called from `task_report` and
  gate runs.
- [X] T036 [US3] Add squad runs to `src/orchestration/runs.rs`, and a `prepared-split` run to
  `xtask/src/squad.rs`:
  - two parts and a contract;
  - a signal answered by a commit;
  - a done report refused without that commit, then accepted;
  - an edit outside the part reported.

**Checkpoint**: parallel work only when prepared; decisions travel as commits.

---

## Phase 6: User Story 4: verification that checks the right thing (Priority: P4)

**Goal**: criteria before building, verdicts that answer them, a reviewer per focus.

**Independent Test**: quickstart Q3.

- [X] T037 [P] [US4] Extend `tests/squad_publish.rs`:
  - a hand-over to a builder is refused without committed criteria (FR-027);
  - lead-built work without criteria uses the description as its one criterion;
  - a verdict without a check per criterion and one with `expect: "fail"` is `incomplete`, does not
    count, and the reply says what is missing (FR-029);
  - `review:approved` needs one approval per manifest focus (FR-031);
  - a squad using `check` tells the engineer that writes outside the worktree are not seen (FR-030).
- [X] T038 [US4] Implement T037:
  - `checks` in `review_verdict` and the reviewer's `focus` at `session_start` in
    `src/orchestration/tools.rs`;
  - readiness in `src/orchestration/publish.rs`;
  - the notice in `src/orchestration/burst.rs`.

---

## Phase 7: User Story 2: switching components off (Priority: P2)

**Goal**: every component can be switched off for a recorded run, with the effect in the spec's FR-017
table.

- [X] T039 [P] [US2] Write a test per component in `tests/squad_runs.rs`:
  - reviewer;
  - gates (the hidden tests still run);
  - contracts;
  - plan-commit check;
  - criteria precondition;
  - ownership check;
  - verdict check;
  - lead updates (nothing typed after the description);
  - parallel builders.

  Each test also checks that the record names the components that were off.
- [X] T040 [US2] Implement the switches (research R-17):
  - the burst's `switched_off` set in `src/orchestration/burst.rs`, read by each check in
    `delegation.rs`, `publish.rs` and `lead.rs`;
  - the choice in the Start a run dialog in `src/app/orchestration.rs`.

---

## Phase 8: User Story 5: a replaceable lead, told only what needs a decision (Priority: P5)

**Goal**: decision-only updates; a new lead continues from the repo and Fallow's status.

**Independent Test**: quickstart Q4.

- [ ] T041 [P] [US5] Extend `tests/squad_lead.rs`:
  - no quiet-time status, and no deadlock notice to the lead;
  - each event FR-035 lists gives one line;
  - a lead named with `L` gets the briefing;
  - `squad_status` gives the manifest commit, each piece of work with its builder, base, branch,
    latest commit and evidence, open signals, switched-off components and the memory state (FR-033);
  - starting a burst on a project with open manifest work shows each branch and what was lost
    (FR-036).
- [ ] T042 [US5] Implement T041 in `src/orchestration/lead.rs`, `repair.rs`, `tools.rs` and
  `burst.rs`:
  - remove 004's quiet-status and lead-deadlock paths;
  - keep the everyone-waiting notice to the engineer.
- [ ] T043 [US5] Retire the 004 tests that assert quiet-time status to the lead, and update 004's spec
  "Changes to earlier features" cross-reference in `specs/004-agent-squads/spec.md` (FR-037, FR-040
  superseded by 005).

---

## Phase 9: User Story 6: fits a small machine (Priority: P6)

**Goal**: starts are capped by measured memory.

**Independent Test**: quickstart Q6.

- [ ] T044 [P] [US6] Write `tests/squad_memory.rs` (research R-14):
  - the sum of the session footprints, gate sessions included, plus the expected size (the largest
    seen for the profile, else `orchestration.session_size_mb`);
  - a start that does not fit waits, and the lead and the engineer get the shortfall;
  - it proceeds when a sample shows room;
  - SC-008.
- [ ] T045 [US6] Implement `src/orchestration/memory.rs`:
  - the settings `orchestration.memory_budget_mb` and `orchestration.session_size_mb` in
    `src/config/schema.rs`;
  - the sampler's footprints in `src/app/orchestration.rs`.

---

## Phase 10: Polish & cross-cutting

- [ ] T046 [P] Add the help and conflicts rows for the new commands and keys in
  `src/app/commands.rs` and `src/app/keymap.rs` (FR-041).
- [ ] T047 [P] Add a section to `README.md`: one loop by default, the manifest, runs and comparisons, and what
  "hidden" means.
- [ ] T048 Run `bench-memory --loaded` with a burst and `bench-startup` (`xtask/src/bench_memory.rs`,
  `bench_startup.rs`) to confirm no regression (Principles I, II).
- [ ] T049 Resolve the Verify items and record the results in `research.md`:
  - Codex and Gemini flags for `check`;
  - `--settings` with `statusLine` on Windows;
  - the footprint of a Claude Code session on Linux, which replaces the 700 MB default.
- [ ] T050 Run quickstart Q1–Q7 manually on Windows and on the Linux VPS, and record the results in
  `specs/005-adaptive-delegation/quickstart.md`.
- [ ] T051 Run the comparison series on the engineer's task set (SC-001 to SC-004), and report the
  results as measured on that set in `specs/005-adaptive-delegation/results.md`.

---

## Dependencies & Execution Order

### Phase dependencies

- Setup → Foundational → US1 core → US2 (MVP) → US1 rest/US3 → US4 → components off → US5 → US6 →
  Polish.
- US2 needs US1 core: solo runs are a burst where the lead builds.
- Phase 5 needs Phase 4 only for squad runs (T036); T030–T035 need only Phase 3.
- Phase 7 needs the checks it switches off: Phases 5 and 6.

### Key task-level dependencies

- T002 → T003; T004 → T005; T006/T007 → T008.
- T003, T005 → T010, T030, T034.
- T011 → T020 (solo runs), T038.
- T015 → T016 → T017 → T020; T018 → T019, T020, T021.
- T022 → T023; T024 → T025; T020, T025 → T026 → T027.
- T032 → T033 → T035; T031, T035 → T036.

### Within each story

Tests marked [P] first, then the hub module, then tools, then the app and UI, then the `xtask`
scenario.

## Parallel Example: Phase 2

```text
T002 tests/squad_manifest.rs   T004 tests/squad_git.rs   T006 tests/squad_roles.rs   T007 tests/squad_file.rs
```

## Parallel Example: User Story 2

```text
T015 task set tests   T018 lifecycle tests   T022 usage tests   T024 record tests   (all tests/squad_runs.rs: write in sequence, or split the file)
```

## Implementation Strategy

### MVP first (Phases 1–4)

1. Foundation: manifest, git additions, `check`, new merge conditions.
2. The lead builds by default.
3. Plain and solo runs with hidden tests, records and the Runs tab. **Stop and validate**: run a
   small series on the engineer's tasks. If solo does not beat plain (SC-004), the gates and reviewer
   are not paying for themselves, and that decides what comes next.

### Incremental delivery

MVP → prepared parallel work and signals (squad runs) → verification → switches → lead → memory.
Each increment keeps every constitution gate green in CI, and each new component can be measured
against the previous runs.

## Notes

- [P] tasks touch different files and have no dependency on unfinished tasks.
- Commit after each task or logical group; push to `001-llm-aware-tui-editor`.
- Never write messages, tasks, signals, briefings or session output anywhere. Write run records only
  with the allowed fields, and never write into the engineer's repository (runs use their own copy).
