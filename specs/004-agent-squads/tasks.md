---

description: "Task list for 004 Agent Squads and Coordination Mode"
---

# Tasks: Agent Squads and Coordination Mode

**Input**: Design documents from `specs/004-agent-squads/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Included. The constitution requires them:
- tests for every path that types into a session or acts on a process;
- benchmarks for performance claims.

The spec's success criteria SC-002 to SC-009 are CI tests. Tests marked [P] are written first and
must fail before their implementation.

**Organization**: by user story, in the spec's priority order. US1 (coordination burst) is the MVP.
Each later story builds on the one before: US2 narrows a burst with roles, US3 declares them in a
squad, US4–US6 add gates, evidence and publishing to the squad.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1 … US6)
- Paths follow the plan's Project Structure (single crate `src/`, `tests/`, dev-only `xtask/`).

---

## Phase 1: Setup

- [ ] T001 Create the module skeletons with doc comments only and register them in `src/orchestration/mod.rs`: `squad.rs`, `roles.rs`, `burst.rs`, `briefing.rs`, `gates.rs`, `evidence.rs`, `repair.rs`, `publish.rs`, `git.rs` under `src/orchestration/`.

---

## Phase 2: Foundational (blocking)

**Purpose**: the squad model, roles and their flags, the sender with its role, git access, and the test agent's new steps.

**⚠️ CRITICAL**: no user story work starts before this phase is done. 003's tests and `cargo xtask orchestration` stay green.

- [ ] T002 [P] Write `tests/squad_file.rs` for the squad file (contracts/squad-file.md).
  - Parse every field.
  - Validate, quoting the rules:
    - role `name` "`[a-z][a-z0-9-]{0,31}`, unique";
    - `description` "1..=200 chars";
    - `level` "`read-only` | `edit` | `edit-run`";
    - `max` "`1..=16`";
    - `repair_cap` "`0..=10`";
    - `wait_notice_min` "`1..=120`";
    - gate `timeout_min` "`1..=240`";
    - "≤ 16 roles", "≤ 8 gates";
    - "must contain `lead`" (the built-in lead is added if missing);
    - `publish` "`auto` | `draft-pr` | `local-merge`", `merge` "`on-approval` | `automatic`";
    - merge conditions "`gate:<name>`, `done:<role>`, `signal:<event>@<role>`", referring to defined gates and roles.
  - Problems are reported as `.fallow/squad.toml:LINE: message`.
  - The built-in squad: lead, builder ×3, reviewer ×2; `publish = "auto"`; no gates; merge requires `done:builder` and `signal:review-approved@reviewer`.
  - `roles.toml` in the config folder merges with the squad's roles by name.
- [ ] T003 Implement `src/orchestration/squad.rs` so T002 passes (toml with `Spanned` lines, as `projects/store.rs`).
- [ ] T004 [P] Write unit tests in `src/orchestration/roles.rs` for the exact argument vectors of research R-2, per agent and level. Cover:
  - Claude Code:
    - read-only: `--permission-mode dontAsk --disallowedTools Edit Write NotebookEdit`;
    - edit: `--permission-mode acceptEdits`;
    - edit-run: `--permission-mode acceptEdits --allowedTools Bash`.
  - Codex:
    - read-only: `--sandbox read-only --ask-for-approval never`;
    - edit: `--sandbox workspace-write --ask-for-approval on-request`;
    - edit-run: `--sandbox workspace-write --ask-for-approval never`.
  - Gemini: read-only `--approval-mode plan`, edit `--approval-mode auto_edit`, edit-run: no mapping.
  - Model flags (`--model <name>`).
  - Bypass detection for the R-2 bypass flags.
  - A squad's `[role.flags]` overriding the mapping.
  - "An agent with no flags for a restricted level cannot take that role".
  - Merging `--allowedTools`/`--disallowedTools` with 003's `--allowedTools mcp__fallow__*` into one occurrence each.
- [ ] T005 Implement `src/orchestration/roles.rs` (levels, `Tool` set, signal patterns with `*`, flag resolution, bypass detection) and the level mappings per built-in profile in `src/session/profiles.rs`.
- [ ] T006 Extend `Party` in `src/orchestration/mod.rs` (data-model "Party"): `Party::Fallow`, and `role: Option<String>` on `Agent`. `json()` adds `"role"`; `label()` and `delivery::label` name the role (`agent #2 (builder, Claude Code, project api)`). Update 003's tests that compare `from` objects (`tests/orch_hub.rs`, `tests/orch_gate.rs`) for `"role": null`.
- [ ] T007 [P] Implement `src/orchestration/git.rs`: the read-only calls of research R-4 run on a worker thread, with results back on the `AppEvent` channel as `AppEvent::Git(..)`. Calls: `rev-parse HEAD`, `symbolic-ref --short HEAD`, `remote`, `status --porcelain`, `rev-list A..B`, `merge-base --is-ancestor`, `diff --name-only A..B`. Write `tests/squad_git.rs` against a real temporary repository.
- [ ] T008 [P] Add steps to the `mcp-agent` persona in `xtask/src/mcp_agent.rs`: `verdict <approved|changes-requested> <part> <notes>`, `report-evidence <task> <commit|HEAD> <file…>` (runs `git rev-parse HEAD` in its own folder for `HEAD`), `publish <url>`, `start-role <role> <first task>`.

**Checkpoint**: squads parse, roles resolve to exact flags, and every sender carries its role.

---

## Phase 3: User Story 1 - Coordination burst (Priority: P1) 🎯 MVP

**Goal**: one step lets a project's agents coordinate for up to 8 hours without per-action approval, led by the engineer's session, with a briefing for every session.

**Independent Test**: start a burst from a session for one hour; it starts a second session and hands it a task with no prompt; every use is announced; after the burst ends, the next request prompts again.

- [ ] T009 [P] [US1] Write `tests/squad_burst.rs`:
  - Start, with a Fake session table:
    - grants a 003 permission with source and target `Project(p)`, deliver + create tasks + start up to the allowance;
    - allowance default 3, at most `sessions.max_sessions`;
    - the lead is the starting session.
  - Duration limit: more than 8 hours is refused with "at most 8 hours"; no shorter burst starts.
  - End, by time or by `end()`:
    - revokes the permission, and the next delivery request is pending again (SC-007);
    - one "Coordination ended" notice and one board post `from {"kind": "fallow"}`.
  - Extending replaces the end time, with the new grant at most 8 hours.
  - A burst for project 1 leaves project 2's sessions untouched.
  - One burst per project.
  - The engineer can name another lead.
- [ ] T010 [US1] Implement `src/orchestration/burst.rs` (data-model "Burst", research R-1) and wire it into `Hub`: `start_burst`, `end_burst`, `extend_burst`, `set_lead`, and expiry in `tick`, with "ends in 10 min" and "ended" notices.
- [ ] T011 [P] [US1] Implement `src/orchestration/briefing.rs`.
  - Short text per role, under 3,500 characters (research R-3).
  - Full text per role covering FR-005's list: tools, rules, roles, gates, the check-in convention, sessions versus subagents, and the end time.
  - The Spec Kit pipeline when the project has `.specify/` (FR-006); otherwise plain steps.
  - The common board note.
  - Unit tests: lengths, and each FR-005 item present.
- [ ] T012 [US1] Deliver briefings, in `src/orchestration/channel.rs`, `rpc.rs` and `tools.rs`:
  - after the hello, the connection asks the hub for the session's short briefing, which becomes the `initialize` `instructions`;
  - `whoami` adds `role`, `role_description`, `burst {ends_at, lead, allowance_left}` and `briefing`;
  - starting a burst posts the common note;
  - sessions connected before the burst get a delivery, approved by the burst, asking them to call `whoami`.
- [ ] T013 [P] [US1] Add `--coordinate <DURATION> [PATH]` to `src/cli.rs`.
  - Duration format: `30m`, `4h`, `2h30m`. Over `8h`, exit 2 with "--coordinate: at most 8h".
  - The app keeps it as a pending burst. Nothing is created before the first session of that project (test: no hub after startup).
  - Tests go in `tests/cli_args.rs`.
  - Document it in `specs/001-llm-aware-tui-editor/contracts/cli.md`.
- [ ] T014 [US1] Add the ways in to `src/app/orchestration.rs` and `src/ui/orchestration.rs` (contracts/ui-commands.md "Starting a burst"):
  - The palette command "Coordinate this project…" opens the dialog: lead, squad summary, hours (max 8), the sessions allowance with the usage-cost note, publish mode.
  - The offer when the first session of a project starts: once per project per run; `d` stores "don't ask again for this project" in `projects.toml` (`src/projects/store.rs`).
  - The pending `--coordinate` burst starts with the first session as lead.
- [ ] T015 [US1] Add the Spec Kit offer (FR-007, research R-8). When a burst starts in a project without `.specify/`:
  - the dialog asks "Set up Spec Kit? The lead will run the official installer";
  - yes: a delivery to the lead (approved by the engineer's answer) with the installer command, saying to check for `uv` first and ask the engineer if it is missing;
  - Fallow downloads and writes nothing.
- [ ] T016 [US1] Build the burst UI, in `src/ui/hints.rs`, `src/ui/orchestration.rs` and `src/app/orchestration.rs`:
  - `● coordinating H:MM:SS left` in the hint bar in both views, also with a session focused;
  - a new first tab **Squad**: lead, sessions, time left, allowance used;
  - keys `+` (extend), `L` (make the selected session lead), `Shift-X` (end the burst);
  - palette entries;
  - render tests in `src/ui/workspace.rs`.
- [ ] T017 [US1] Create `xtask/src/squad.rs` and register `cargo xtask squad` in `xtask/src/main.rs`:
  - Fallow in the PTY harness; the lead is an `mcp-agent` session that starts a builder with a first task;
  - the harness starts the burst from the palette;
  - check: no prompt appears, the builder receives the task, and after `Shift-X` the next request is pending.
  - Add a CI step "Squads (004 SC-009)" in `.github/workflows/ci.yml` (timeout 15 min).

**Checkpoint**: quickstart Q1, Q2, Q8 and Q9 pass.

---

## Phase 4: User Story 2 - Roles enforced by Fallow (Priority: P2)

**Goal**: each session's role is fixed by Fallow, stamped on everything others receive, and narrows its tools and signals; read-only roles are checked by outcome.

**Independent Test**: a builder's `session_start` and `review-approved` signal are refused; a reviewer whose worktree changes is reported and its verdicts stop counting.

- [ ] T018 [P] [US2] Write `tests/squad_roles.rs`:
  - SC-002: every built-in role's `tools/list` matches contracts/mcp-tools.md "Tool list per role", and every tool outside it is refused with `not_allowed`.
  - Restricted events by pattern; `gate-*` refused for every role.
  - Role preconditions (`{"name": "review-approved", "role": "reviewer"}`) are met only by that role.
  - The role cannot be passed in any tool's arguments (`-32602`).
  - `from` carries the role.
  - `session_start` limits:
    - requires a role during a burst;
    - refused beyond the role's `max` or the allowance;
    - refused for an agent without flags for the role's level.
- [ ] T019 [US2] Enforce roles in `src/orchestration/tools.rs`, `events.rs` and `tasks.rs`: tool lists and refusals per role, restricted events, role preconditions, and `session_start` with `role`.
- [ ] T020 [US2] Start role sessions, in `src/app/workspace.rs` and `src/orchestration/inject.rs`:
  - resolve the role's profile and flags (T005) and merge them with 003's per-run flags;
  - `Session.role` is fixed for life;
  - a bypass confirmation dialog (contracts/ui-commands.md) before a role whose flags bypass the agent's checks;
  - session details (`i`) show role, level and the exact command line (FR-014).
- [ ] T021 [US2] Check read-only worktrees (FR-013):
  - the git worker runs `status --porcelain` and `rev-list <start>..HEAD` in each read-only session's worktree when it signals and every 2 minutes;
  - a change warns the engineer and marks the session's signals as not counting for gates and preconditions;
  - tests in `tests/squad_roles.rs` with a real repository.
- [ ] T022 [US2] Show roles: a role column in `src/ui/workspace.rs`, and `session_list` and `whoami` with roles (contracts/mcp-tools.md). Extend `cargo xtask squad` with the SC-002 refusals (a builder calls `session_start` and signals `review-approved`).

**Checkpoint**: quickstart Q3 and Q4 pass.

---

## Phase 5: User Story 3 - Squads declared in the project (Priority: P3)

**Goal**: the squad file's roles, gates and merge conditions are in force and frozen for the burst.

**Independent Test**: a squad whose merge needs backend done, frontend done and review approved keeps the merge closed until all three hold; editing the file mid-burst changes nothing.

- [ ] T023 [P] [US3] Write merge-condition tests in `tests/squad_publish.rs`:
  - `gate:<name>`, `done:<role>`, `signal:<event>@<role>` evaluated against the hub state;
  - an invalid squad file blocks the burst with `file:line`;
  - a squad file changed during a burst does not change its rules (FR-018);
  - the built-in squad's conditions;
  - SC-003: the merge condition is never "ready" before every condition holds.
- [ ] T024 [US3] Use the squad, in `src/orchestration/burst.rs` and `src/orchestration/publish.rs`:
  - load `.fallow/squad.toml` (or the built-in) when the burst starts, frozen in the burst;
  - role maxima;
  - `MergeReadiness` (data-model) with each condition and why;
  - the `squad_status` tool;
  - the Squad tab shows roles, conditions and readiness.

**Checkpoint**: the merge stays closed until every condition holds, in `xtask squad`.

---

## Phase 6: User Story 4 - Gates Fallow observes (Priority: P4)

**Goal**: build/test gates run as sessions; Fallow turns their exit status into signals and classifies the final run against the baseline.

**Independent Test**: with a baseline that fails, a final run with the same failures is "baseline only" and one with a new failure is "regression".

- [ ] T025 [P] [US4] Write `tests/squad_gates.rs`:
  - Failure names (research R-5): from cargo, pytest, Jest/Vitest, Go and .NET output samples; `regex` with one capture group; `exit-code`.
  - The classification table: Pass, BaselineOnly, Regression, Fail without names or baseline.
  - Gate behaviour:
    - `timeout_min` reached means failed "timed out";
    - the per-OS override chosen for the running OS;
    - `gate-*` events signalled only by `Party::Fallow` (SC-004).
- [ ] T026 [US4] Implement `src/orchestration/gates.rs`: gate runs, failure-name extraction from a session's scrollback text, classification, time limits.
- [ ] T027 [US4] Run gates in `src/app/orchestration.rs` (research R-6).
  - Baseline: in a temporary worktree at `start_commit` before builders' tasks are handed over (builders' first tasks wait for it), removed afterwards.
  - Final: in the lead's worktree when every task handed to a builder is reported done.
  - Both are command sessions through the shell profile, with role `gate`. They count against `sessions.max_sessions`, not the allowance.
  - On `SessionExited`, Fallow signals `gate-<name>-passed/-failed` as `Party::Fallow` and records the classification.
  - The Squad tab shows the runs.
- [ ] T028 [US4] Extend `cargo xtask squad` with a gate whose command passes or fails on a marker file: baseline recorded before the builder's task, final classified, and an agent's attempt to signal `gate-tests-passed` refused.

**Checkpoint**: quickstart Q5 passes.

---

## Phase 7: User Story 5 - Evidence and capped repair (Priority: P5)

**Goal**: reports carry evidence Fallow checks; failures go back to the owner of the part, up to a cap; the engineer hears when everyone waits.

**Independent Test**: a report naming a commit not on the session's branch is refused; a third repair round with cap 2 goes to the engineer.

- [ ] T029 [P] [US5] Write `tests/squad_evidence.rs` against a real temporary repository:
  - Reports:
    - `task_report done` during a burst without evidence is refused;
    - a commit not on the session's branch is refused (`merge-base --is-ancestor`);
    - files are compared with `diff --name-only`, and differences are noted, not refused;
    - limits "≤ 20" commands and "≤ 200" files.
  - Evidence is attached to tasks that depend on the reported one.
  - Repairs:
    - the owner is the session a part was handed to (FR-025a);
    - `review-changes-requested` or a failed gate creates a repair task for the owner;
    - a failure not tied to one part goes to the lead;
    - rounds count, and the round after `repair_cap` asks the engineer.
  - The deadlock notice: every session waiting for `wait_notice_min` gives one notice listing who waits for what, repeated at most every 30 minutes.
- [ ] T030 [US5] Implement `src/orchestration/evidence.rs` and the `task_report` changes in `src/orchestration/tools.rs`: evidence fields, git checks through the worker, `verified` facts in the result, evidence carried into dependent hand-overs.
- [ ] T031 [US5] Implement `src/orchestration/repair.rs` and wire it in: owners recorded at hand-over (`Task.owner`, `Task.part`), repair tasks on failures, the cap, and the deadlock check in `tick`. The Squad tab shows decisions for stopped loops: `r` hand back once more, `t` take over, `Delete` cancel the part.

**Checkpoint**: quickstart Q7 passes.

---

## Phase 8: User Story 6 - Independent review and publishing (Priority: P6)

**Goal**: reviewers give verdicts through a tool, two reviewers must both approve, and no agent merges: a draft pull request with a remote, or Fallow's local merge on the engineer's press.

**Independent Test**: one reviewer approves and one asks for changes, so the merge stays closed and the engineer sees both; local merge waits for the engineer and merges exactly the gated commit.

- [ ] T032 [P] [US6] Extend `tests/squad_publish.rs`:
  - `review_verdict` is reviewer-only.
  - Two reviewers: a disagreement keeps the merge closed and is shown with both notes. Same model: a warning.
  - Publish mode resolution: a remote means `draft-pr`, none means `local-merge`.
  - `publish` tool: records the url in draft-PR mode, lead only; refused in local-merge mode.
  - Local merge, on a real repository:
    - refused when the project folder is not on the target branch or not clean;
    - merges exactly the gated commit;
    - a foreign commit on the target branch during the burst warns (FR-028a);
    - automatic merge when the squad says so.
  - "No agent is ever handed a merge task" (SC-009).
- [ ] T033 [US6] Implement `review_verdict` in `src/orchestration/tools.rs` (the event for the part, notes, repair on changes requested) and disagreement handling in `src/orchestration/publish.rs`.
- [ ] T034 [US6] Implement publishing in `src/orchestration/publish.rs` and `src/app/orchestration.rs`:
  - draft-PR mode: the lead's briefing says to open a draft pull request for the gated commit and call `publish`;
  - local-merge mode: "ready to merge" in the Squad tab, and `m` asks to confirm, naming branch and commit; Fallow then runs `git merge --no-ff <commit>`;
  - the target-branch watcher every 2 minutes.
- [ ] T035 [US6] Complete `cargo xtask squad` for SC-009 on three OSes:
  - lead, two builders, reviewer and a gate, all scripted;
  - the run ends with "ready to merge", which the harness presses (`m`, `y`);
  - check that the target branch has the gated commit and no agent got a merge task;
  - print `os=… squad … result=PASS`.

**Checkpoint**: quickstart Q6 passes; SC-009 green in CI.

---

## Phase 9: Polish & Cross-Cutting Concerns

- [ ] T036 [P] Extend 003's gate and control tests in `tests/orch_gate.rs`:
  - every burst path still needs the permission;
  - a revoked burst prompts again;
  - no tool, role or permission can stop, pause or kill a session;
  - no merge task is ever handed to an agent (constitution quality gate).
- [ ] T037 [P] Update `src/ui/help.rs`: "Coordination and squads", "Roles and their limits" (read-only depends on the agent CLI; Fallow checks the outcome), and conflict rows for `+`, `L`, `m`, `r`, `t`. Update `tests/keymap_conflicts.rs`.
- [ ] T038 [P] Extend `xtask/src/bench_memory.rs`: `--loaded` runs a burst of 4 sessions plus a gate (SC-008). Check that `bench-startup` with `--coordinate` creates nothing before the first frame.
- [ ] T039 [P] Update the docs:
  - `README.md`: coordination mode, squads, the squad file, roles and levels, publishing, known gaps (Gemini edit-run needs flags; Codex has no "ask before every command");
  - `specs/001-llm-aware-tui-editor/plan.md`: release checklist item 7, 004 Q1–Q9 on macOS.
- [ ] T040 Resolve the research verify items and record them in `specs/004-agent-squads/research.md`:
  - Codex flags on the installed version, and a `codex --version` check when roles resolve;
  - the `uvx` form of the Spec Kit command.
- [ ] T041 Run quickstart Q1–Q9 on Windows with Claude Code and record the results in `specs/004-agent-squads/quickstart.md`. Needs the engineer at the terminal.

---

## Dependencies & Execution Order

### Phase dependencies

- Setup → Foundational → US1 → US2 → US3 → US4 → US5 → US6 → Polish.
- US2 needs US1 (roles narrow a burst).
- US3 needs US2 (a squad declares roles).
- US4–US6 need US3 (gates, repairs and publishing are squad rules).
- Polish needs the stories it covers.

### Key task-level dependencies

- T002 → T003; T004 → T005; T006 before T012 and T019.
- T007 → T021, T030, T034.
- T008 → T017, T022, T028, T035.
- T010 → T012, T014, T016; T026 → T027; T030 → T031; T033 → T034 → T035.

### Within each story

Tests marked [P] first, then the hub module, then tools, then the app and UI, then the xtask scenario.

## Parallel Example: Phase 2

```text
T002 tests/squad_file.rs   T004 src/orchestration/roles.rs tests   T007 src/orchestration/git.rs   T008 xtask/src/mcp_agent.rs
```

## Parallel Example: User Story 1

```text
T009 tests/squad_burst.rs   T011 src/orchestration/briefing.rs   T013 src/cli.rs
```

## Implementation Strategy

### MVP first (User Story 1)

1. Phases 1 and 2: squads parse and roles resolve to exact flags, with 003 unchanged.
2. Phase 3: a burst led by the engineer's session with briefings. **Stop and validate** with
   quickstart Q1, Q2 and Q8 against Claude Code, and `xtask squad` in CI.

### Incremental delivery

US1 (burst) → US2 (roles) → US3 (squad file) → US4 (gates) → US5 (evidence, repair) → US6 (review,
publishing). Each increment keeps every constitution gate green in CI.

## Notes

- [P] tasks touch different files and have no dependency on unfinished tasks.
- Commit after each task or logical group; push to `001-llm-aware-tui-editor`.
- Never write burst, role, gate, evidence or briefing content anywhere (FR-029); never write into a
  project except the engineer's confirmed local merge.
