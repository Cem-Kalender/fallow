---

description: "Tasks for 006, the headless orchestrator"
---

# Tasks: Headless Orchestrator

**Input**: Design documents from `specs/006-headless-orchestrator/`: plan.md, spec.md, research.md,
data-model.md, contracts/, quickstart.md.

**Tests**: included. Constitution v6.0.0 makes several tests CI gates:
- false claims (I);
- no target push, force-push or self-approval (II);
- the loopback, secret, Host and Origin checks (V);
- egress (V);
- resume at every stage (VII);
- the run-record marker test (V).

The success criteria SC-003 to SC-006 and SC-009 also need tests. Unless a task says otherwise,
tests use the fake agent (research R-15).

**Organization**: by user story (spec.md US1 to US7). Work stays on branch `001-llm-aware-tui-editor`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story the task serves (US1 to US7)

---

## Phase 1: Setup (removing the terminal app, the new skeleton)

**Purpose**: clear out what 006 drops, keep what it reuses, and get a compiling skeleton of the
new layout (plan.md Project Structure).

- [X] T001 Verify research R-4 on Windows:
  - run `claude -p` under `--permission-mode dontAsk` with `--allowedTools "Bash(git status:*)" "Bash(python -m unittest:*)"` in a scratch repo, asking it to run one allowed and one refused command;
  - record in `specs/006-headless-orchestrator/research.md` R-4 how `permission_denials` reports the refusal, and how prefixes with arguments match under PowerShell and cmd.
- [X] T002 Move the reused code to its new places, keeping its tests green:
  - `src/orchestration/{evidence,gates,git}.rs` → `src/verify/`;
  - `src/orchestration/manifest.rs` → `src/plan/`;
  - `src/orchestration/{runs,record}.rs` → `src/measure/`;
  - `src/orchestration/{bridge,channel,rpc,identity}.rs` → `src/mcp/`;
  - the Job Object and memory sampling code from `src/session/platform` and `src/orchestration/memory.rs` → `src/platform/`;
  - 004's local merge from `src/orchestration/publish.rs` → `src/publish/local.rs`.
  - Done for what Phases 1-3 use. The task sets, run records and memory budget (`measure`, the memory sampler) move with US5 and US7 (T061, T069 to T072), restored from git history then.
- [X] T003 Delete the terminal app:
  - **folders:** `src/app`, `src/ui`, `src/editor`, `src/session` (what is left after T002), `src/procmon`, `src/projects`;
  - **files:** `src/clipboard.rs`, `src/dns.rs`, `src/cli.rs`;
  - **the rest of `src/orchestration`:** board, delivery, permissions, burst, signals, lead, roles, inject, delegation, briefing, tools, usage, activity, events, tasks, repair, squad's role parts;
  - **tests and benchmarks:** their tests in `tests/` and their benchmarks and canaries in `xtask/src/` (`bench_startup`, `bench_refresh`, `bench_new_process`, `bench_sessions`, `bench_search`, `canary_sessions`, `pty`, `projects`, `squad`, `mcp_agent`, `orchestration`, `diag`).
- [X] T004 Update `Cargo.toml` (research R-14):
  - remove `crossterm`, `ratatui`, `ropey`, every `tree-sitter*`, `notify`, `grep-*`, `ignore`, `arboard`, `vt100`, `portable-pty`, `ipnet`, `compact_str`, `smallvec`, and `regex` if unused;
  - add `tiny_http`, pinned to an exact version;
  - trim the `windows-sys` features to those still used;
  - `cargo build` and `cargo clippy --all-targets` must be clean.
- [X] T005 Write the new command line in `src/main.rs`:
  - `fallow`, `serve`, `run`, `status`, `stop`, `autostart`, `mcp-bridge` and `bench`, with stubs, as contracts/cli.md;
  - `fallow autostart` prints the Startup-folder shortcut command and writes nothing (research R-1).
- [X] T006 [P] Create the module skeletons with doc comments naming their contract: `src/service/`, `src/run/`, `src/agent/`, `src/integrate.rs`, `src/publish/`, as in plan.md.
  - `src/integrate.rs` and `src/measure/` arrive with US5 and US7; the other modules exist.
- [X] T007 [P] Write `fallow-fake-agent` in `xtask/src/fake_agent.rs`. It accepts `claude -p`'s flags (research R-3) and plays a scenario file (TOML: a list of tool events, edits, commits, an `ask_decision` call, the final `structured_output`, an optional crash or delay). It writes stream-json in the verified format, ending with a `result` event that has `structured_output`, `total_cost_usd`, `session_id` and `permission_denials`.
  - Built as a second binary of the crate, `src/bin/fallow-fake-agent.rs`, so integration tests find it through `CARGO_BIN_EXE_fallow-fake-agent` (research R-15).
- [X] T008 [P] Remove the old jobs from `.github/workflows/ci.yml`:
  - startup, idle and loaded memory, refresh, new-process, sessions and search benchmarks, the session canary;
  - the macOS and Linux build matrix, since Windows is the target (clarified 2026-10-29);
  - keep build, clippy, fmt, test and egress jobs on Windows.

**Checkpoint**: `cargo build`, `cargo clippy --all-targets` and `cargo test` pass on the skeleton.
The reused tests still pass.

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: the event log, the service, the page server, the agent adapter and the config that
every story builds on.

- [X] T009 Implement the event types of contracts/storage.md in `src/run/events.rs`:
  - each line is `{"seq","at","type",…}`, and `seq` increases by 1;
  - appending is followed by flush and `fsync` before returning;
  - the run directory is created with a current-user-only ACL;
  - `fold(events) -> RunState`.
- [X] T010 [P] Implement the entities of data-model.md in `src/run/state.rs`: Run, Plan, Part, Step, Decision, GateRun, Verdict, SteeringAction, Publication. Include the stage transitions ("planning → building → integrating → gating → reviewing → ready → publishing → done", "paused" remembers its stage, "done" and "stopped" are final) and Part's states, with the transitions checked in `fold`.
  - Stage transitions are checked in `fold`; Part transitions are not yet (any state may follow any other).
- [X] T011 [P] Add `uuid_v4()` and `secret_256()` from `BCryptGenRandom` to `src/platform/random.rs`, and a helper that creates directories and files readable by the current user only in `src/platform/acl.rs`.
- [X] T012 Implement the service in `src/service/mod.rs`:
  - `fallow serve` claims the single-instance named pipe, refusing if it is taken;
  - writes `service.json` (pid, port, start time, never the secret);
  - loads every run whose stage is not final, through `fold`;
  - answers command-line requests over the pipe;
  - shuts down by ending steps' process trees (Job Objects) and leaving runs to resume.
- [X] T013 Implement the page server in `src/service/http.rs`:
  - `tiny_http` bound to `127.0.0.1:0`;
  - the per-launch secret exchanged for an `HttpOnly; SameSite=Strict` cookie;
  - 401 without it;
  - 403 unless `Host` is `127.0.0.1:<port>` or `localhost:<port>`, and on a `POST` whose `Origin` is not the page's;
  - headers `Content-Security-Policy: default-src 'self'`, `X-Frame-Options: DENY`, `Cache-Control: no-store` (research R-6).
- [X] T014 [P] Implement Server-Sent Events in `src/service/sse.rs`: `GET /api/runs/<id>/events`, resuming after `Last-Event-ID`, one thread per stream, plus `activity` events.
  - Replaced by long polling (`GET /api/runs/<id>/wait`, research R-6): `tiny_http` never flushed the event stream, found in the browser on 2026-10-29. Test: `tests/run_wait.rs`.
- [X] T015 [P] Implement the `Adapter` trait in `src/agent/mod.rs`: start a call (instruction, schema, allowlist, MCP file, session new or resumed, model, budget, working directory, time limit), stream activity, and return the result or the failure reason.
  - The adapter is `agent::Call` with `agent::claude::run`; other agents get their own `run` behind the same `Call` when they arrive.
- [X] T016 Implement `src/agent/claude.rs`, which builds the exact `claude -p` command of research R-3, and `src/agent/stream.rs`, which parses stream-json into activity lines (reads, edits, commands with results, progress) and the final `result`. A missing or invalid `structured_output`, `is_error`, or a time-out (process tree ended) makes a failed step.
- [X] T017 [P] Implement `src/agent/allow.rs`: the planner, builder and reviewer allowlists of research R-4, plus the squad file's `[allow]` additions and each declared gate command.
- [X] T018 [P] Implement the config of contracts/config.md in `src/config/`:
  - settings `[defaults]` (autonomy, `limit_usd = 10`, `limit_min = 120`, `step_min = 30`, `repair_cap = 2`, `max_parallel = 2`, `memory_budget_mb`), `[roles.*]` and `[agents.*]`;
  - the squad file changes: role tables and `[merge] requires` are ignored with a notice, plus the new `autonomy`, limits, `[roles.*]` and `[allow]`;
  - resolution order: settings, then the squad file, then the run.
- [X] T019 [P] Add run worktrees and branches to `src/verify/git.rs`:
  - create `fallow/r<id>/plan`, `fallow/r<id>/<part>` and `fallow/r<id>/result` from the start commit;
  - one worktree per step under `runs\<id>\worktrees\`;
  - removal when the run is deleted.
- [X] T020 Test: `tests/run_events.rs`:
  - append, then fold;
  - a torn last line is ignored with a notice;
  - the ACL is user-only;
  - illegal stage transitions are refused.
- [X] T021 [P] Test: `tests/service_http.rs` (constitution V gate):
  - the listener's address is `127.0.0.1`;
  - no cookie gives 401;
  - a wrong `Host` gives 403;
  - a `POST` with a foreign `Origin` gives 403;
  - the CSP header is present;
  - the embedded page files contain no `http://` or `https://` URL.
- [X] T022 [P] Test: `tests/agent_claude.rs`, with the fake agent:
  - the command line matches research R-3;
  - activity lines and the result are parsed;
  - an invalid result, a crash and a time-out are each a failed step;
  - `permission_denials` are reported.
  - `tests/agent_claude.rs`, plus unit tests in `src/agent/claude.rs`.

**Checkpoint**: a service that starts, serves an authenticated empty page, holds runs in event logs,
and can run one fake-agent step.

---

## Phase 3: User Story 1 - One task to completion without steering (Priority: P1) 🎯 MVP

**Goal**:
1. The engineer plans with the planner on the page and approves.
2. Then one builder, Fallow's verification, gates, a review and capped repairs run unattended to
   "ready to merge".
3. The engineer merges the verified commit locally.
4. A restart resumes the run.

**Independent Test**: quickstart V1 (real agent) and V3, and `tests/us1_*.rs` (fake agent). On the
`search` task: 2 interventions, the merged commit is the gated one, and the hidden tests pass.

- [X] T023 [US1] Update the plan format in `src/plan/` (research R-8, data-model.md Plan):
  - `build` is `"builder"` (one part named `main`) or `"parts"`;
  - criterion ids are unique within their part and addressed as `part/id`;
  - at least one `must_fail` per part;
  - `approvable()` requires the plan to parse, pass the checks, and be committed on `fallow/r<id>/plan`;
  - problems are named with file and line.
- [X] T024 [US1] Write the instructions per step kind and role in `src/agent/prompts.rs`:
  - `plan_turn` and `decide` (planner), `build`, `take` and `repair` (builder), `review` (reviewer);
  - each states the goal, the relevant plan sections and criteria, the rules (no messaging, ask with `ask_decision`, commit before reporting), and the result schema;
  - Spec Kit projects get the `/speckit-specify` and `/speckit-plan` pointers (research R-8).
- [X] T025 [P] [US1] Write the JSON schemas of contracts/step-results.md (build, take, repair, decide, plan_turn, review) in `src/agent/schemas.rs`.
- [X] T026 [US1] Implement the planning conversation in `src/run/machine.rs`:
  - `POST /api/runs/<id>/plan/say` queues a `plan_turn` step that resumes the planner session with the engineer's text;
  - its `reply` is shown;
  - its `commit` updates `plan_committed` with `approvable` and `problems`;
  - `POST …/plan/approve` succeeds only when approvable (FR-002, FR-003, FR-033).
- [X] T027 [US1] Implement the solo stage machine in `src/run/machine.rs`: building (one `build` step for part `main`) → gating → reviewing → ready, with repairs. It decides the next step only from folded state, and writes `step_queued` before starting a step.
- [X] T028 [US1] Implement claim verification in `src/verify/claims.rs` (research R-7):
  - the commit is on the step's branch (`merge-base --is-ancestor`) and newer than its base;
  - changed files are within `owns` and `shared`;
  - every `takes` commit is contained;
  - the outcome is written as `verified` (accepted or rejected, with reasons).
- [X] T029 [US1] Adapt 004's gate session into a process in `src/verify/gates.rs`:
  - run each declared command in a temporary worktree of the commit, with `timeout_min`;
  - capture output into the step's activity;
  - classify against the baseline taken at building start (`clean`, `pre-existing failures only`, `regression`);
  - write a `gate_run` event.
- [X] T030 [US1] Implement verdict completeness in `src/verify/verdict.rs` (from 005, with ids qualified by part):
  - every criterion in scope must be answered with `ran` and `result`, and every `must_fail` with a `failing_case`;
  - partial verdicts in one review add up;
  - an incomplete verdict resumes the reviewer naming the missing `part/id`s (FR-007);
  - a reviewer whose worktree changed stops counting.
- [X] T031 [US1] Implement repairs in `src/run/machine.rs`:
  - a rejected claim, a failed gate or a failing verdict queues a `repair` for the part, with the failed checks in its instruction;
  - the count is kept per part against `repair_cap` (default 2);
  - at the cap, the engineer is asked (FR-008).
- [X] T032 [US1] Implement run limits in `src/run/limits.rs` (FR-038):
  - cost is the sum of reported `total_cost_usd`, and each step gets `--max-budget-usd` of what is left;
  - time counts from building start;
  - per-step time comes from `step_min`;
  - when a limit is reached: a `limit_reached` event, then pause before the next step, offering to raise the limit, continue once, or stop;
  - when an agent reports no cost, the page says only the time limit applies.
  - Built in `RunState::limit_hit` and `src/run/machine.rs`, not a `limits.rs`. Paused time does not count against the time limit. "Continue once" lets one more step start (`limits_set` with `once`).
  - A step whose agent reports no cost shows "cost not reported" on the page; the page does not yet say that only the time limit then applies.
- [X] T033 [US1] Implement the local merge in `src/publish/local.rs` (004's, adapted): only from `ready`, only exactly the verified integration commit, onto the target branch, without touching the engineer's working tree, then a `merged` event (FR-009).
  - When the engineer's repository folder has the target branch checked out, the merge happens there (it needs no changes to tracked files; untracked files are ignored), so the working tree does move to the merged commit. Otherwise it happens in a temporary worktree.
- [X] T034 [US1] Implement resume in `src/service/mod.rs` and `src/run/machine.rs` (research R-2):
  - on start, a step with `step_started` and no `step_finished` is resumed with `--resume <session>` and the "you were interrupted" note, or redone if the session cannot be resumed;
  - completed steps are never queued again.
  - A resumed step that fails without any activity, with Claude Code saying it found no conversation for the session, is a lost session (`session_lost`): a new session gets the whole instruction (with the repair reasons or the engineer's answer it was resuming with), and the attempt costs no repair round. Other failures of a resumed step (an overloaded API, the budget) count as failures. The exact text Claude Code prints for an unknown session is assumed, not yet seen on a real run. A question that only a transient error raised is cleared on restart, and the engineer can "Try again" (`POST /api/runs/<id>/retry`).
- [X] T035 [US1] Implement `fallow run "<task>"` and the setup check (contracts/cli.md, FR-032) in `src/main.rs` and `src/service/api.rs`:
  - check the repository, uncommitted changes, the agent CLI and its sign-in, the gate commands, the remote and the hooks list;
  - create the run (`run_created`) and open `/runs/<id>/plan`;
  - also implement `fallow status` and `fallow stop`.
  - The check covers the repository, uncommitted files, the agent and gate programs on the path. Whether the agent is signed in and which hooks will run are not checked yet; a step that fails for either says so.
- [X] T036 [US1] Write the first page in `src/service/web/`:
  - `index.html`: the runs list;
  - `run.html`, `app.js` and `app.css`: the planning conversation with the plan, its problems and Approve; the stage; the bar for what needs the engineer (limit reached, repair cap, ready to merge with Merge);
  - no framework and no external resources, with the mockup's tokens and layout.
  - Checked in Chrome on 2026-10-29 with a scripted run: planning, approval, live progress, review evidence, the merge. Three display bugs fixed there.
- [X] T037 [US1] Implement the read endpoints of contracts/web-api.md in `src/service/api.rs` (`GET /api/runs`, `GET /api/runs/<id>`, step activity), and the US1 actions (`POST /api/runs`, `plan/say`, `plan/approve`, `limits`, `merge`).
- [X] T038 [P] [US1] Test `tests/us1_solo.rs`, with fake agent scenarios for a good run (V1), checking:
  - the stages in order;
  - exactly 2 steering actions recorded (approve, merge);
  - the merged commit is the gated one;
  - nothing on the target branch before the merge.
- [X] T039 [P] [US1] Test `tests/us1_false_claims.rs` (constitution I gate, SC-004): each of these is rejected, followed by a repair, with no question to the engineer:
  - a missing commit;
  - a commit without changes in the owned files;
  - a file outside `owns`;
  - a failing gate;
  - an incomplete verdict.
  - Also: a review whose commands were refused does not count (found on a real run).
- [X] T040 [P] [US1] Test `tests/us1_resume.rs` (constitution VII gate, SC-003): end the service at each stage (planning, building mid-step, gating, reviewing, ready), start it again, and check that the run completes without a completed step repeated and with the history intact.
  - Covered: planning, building mid-step, gating (the baseline), reviewing and ready. "Integrating" does not exist as a separate step for one builder. A stop or merge cut in half by a crash is handled on restart but not tested.
- [X] T041 [P] [US1] Test `tests/us1_limits.rs` (FR-038): the cost limit and the time limit each pause before the next step, and raising the limit continues the run.
  - The cost limit and "continue once" run end to end; the time limit, with paused time left out, is a `RunState` test in `tests/run_events.rs`.

**Checkpoint**: the MVP. A solo run from the plan to the merge with the fake agent in CI, and with
Claude Code by hand (quickstart V1, V3).

Reached on 2026-10-29:
- **With the fake agent:** every US1 test passes.
- **With the real Claude Code (Haiku), `tests/real_agent.rs`:** a solo run went from plan to merge with one step of each kind, in 90 s, for $0.016.
- **What the real runs changed:** Fallow commits each step's work itself, rules are mirrored for the PowerShell tool, agents run one command per call, and a review with refused commands does not count (research R-4).
- **What a fresh review found, and was fixed the same day:**
  - gates now run in a new worktree per run, checked to be at the commit;
  - files are counted from the part's last accepted commit, so a refused change stays counted;
  - a pause without a question asks again or ends;
  - each decision is derived again from the log, so a crash between two events repeats it, never skips it: the build, gate, review and repair decisions, a stop or merge cut in half (checked on restart), and an engineer's line without a planner turn. A step processed again after a crash adds no second reply or verdict;
  - lost sessions are taken over by a new session;
  - "Try again" exists for questions Fallow raised itself;
  - Stop is refused during a merge, and ends running gates;
  - builders and reviewers get the exact gate commands, not the bare program (`python`), and no git rights;
  - `--target` is the branch the run starts from, and it is checked;
  - answers from a review with refused commands are left out of the verdict;
  - failing test names are collected from the whole gate output;
  - a repair cap of 0 holds the part (`at_cap`) for the engineer, after a failing gate or a failing review (a second review found the review case looped on every pass, fixed the same day);
  - a gate worktree that cannot be made is a question with "Try again", not a gate failure;
  - only a retry of the review question starts a new review round, and interrupted reviews use none of the 3 tries;
  - review failures are routed from the latest answer to each criterion of the plan.

- **The third real run (Sonnet, the sandbox, the engineer on the page):** "a `total` command" went from plan to merge in 3.5 minutes for $0.31, with two interventions (approve, merge) and no refused commands. The planner noticed that the task's example product does not exist. The reviewer ran all 8 criteria, including the 4 that must fail.
- **The fourth real run (Sonnet, a restart mid-build):** for "a cart", the engineer stopped Fallow 46 s into the build and started it again. The build step ended as interrupted and was resumed one second later in the same session ("resumed after Fallow stopped"). It finished with 11 passing tests. The review passed and the run merged, with no repair round. The interrupted step reports no cost, because Claude Code reports cost only at the end of a step.
- **What the second real run found (Sonnet, the sandbox):** the reviewer could not run the program it checks, so the plan now names the commands agents may run (`commands`, approved with the plan); and the service opened a console window for every git and gate command, now hidden (research R-4).

Known gaps: time spent waiting for the engineer outside a pause (a builder's question, the repair cap) counts against the time limit; a gate thread that panics leaves the run busy until Fallow restarts.

---

## Phase 4: User Story 2 - Decisions during a step, without a chat (Priority: P2)

**Goal**:
- builders ask through `ask_decision`;
- the planner decides and commits;
- the step continues;
- affected parts are reopened;
- the engineer is asked only per autonomy level.

**Independent Test**: quickstart V4, `tests/us2_*.rs`.

- [ ] T042 [US2] Adapt `src/mcp/` (003's bridge, channel and identity) to step tokens:
  - `fallow mcp-bridge --step <token>` reaches the service over the named pipe;
  - the service accepts the token only from that step's process tree and only while it runs;
  - there are no messaging tools (FR-012).
- [ ] T043 [US2] Implement the tools of contracts/mcp-tools.md in `src/mcp/tools.rs`: `plan_read`, `criteria`, `contract_read`, `progress` (an activity line) and `ask_decision`. `ask_decision` waits up to 3 minutes, then returns `decided` (answer and commit) or `waiting` (question id).
- [ ] T044 [US2] Write each step's MCP config file (`--mcp-config`, `--strict-mcp-config`) in `src/agent/claude.rs`, with `mcp__fallow__*` added to every role's allowlist.
- [ ] T045 [US2] Implement decisions in `src/run/decisions.rs`:
  - a question becomes a `decide` step (planner, resuming the planner session);
  - its result is checked: the commit is on the plan branch, only plan files changed, and the plan still validates;
  - `needs_engineer` applies when the planner says so, or when `changes` is `criteria` or `contract` and autonomy is Medium or Low;
  - a decision is `open`, `deciding`, `needs_engineer`, `answered` or `overruled`, as in data-model.md.
- [ ] T046 [US2] Implement reopening in `src/run/decisions.rs` and `src/run/machine.rs` (research R-9):
  - compute the affected parts of a decision commit (criteria changed for the part, or a contract it is `between`);
  - add the commit to each part's `takes`;
  - queue a `take` step for parts already done, resuming their sessions with "merge <commit> and check your work against it";
  - refuse any result that lacks a `takes` commit, naming it (FR-013).
- [ ] T047 [US2] Implement the blocked-and-resumed path in `src/run/machine.rs`: a builder that ends `blocked` with a question id waits only for that part, and resumes in the same session with the answer when the decision is taken (research R-5).
- [ ] T048 [P] [US2] Test `tests/us2_decisions.rs`:
  - at High, `ask_decision` is answered by the planner within the step, with the commit taken;
  - at Medium, a contract change waits for the engineer with the planner's recommendation, and only that part waits;
  - the decisions log has who decided and the commit.
- [ ] T049 [P] [US2] Test `tests/us2_reopen.rs` (SC-005): a criteria or contract change reopens parts already done, they take the commit and are verified again before "ready", and a stale result is refused naming the commit.
- [ ] T050 [P] [US2] Test `tests/us2_mcp.rs`: a token from another process or a finished step is refused, and no tool lets an agent message another agent or the engineer.

**Checkpoint**: a solo run whose plan is incomplete still finishes without the engineer at High
autonomy.

---

## Phase 5: User Story 3 - See what the agents are doing (Priority: P3)

**Goal**: the run view of the mockup ("Fallow Run View"): pipeline, agents, live activity, decisions,
gates, criteria per part, cost and time, all live.

**Independent Test**: quickstart V5. Someone who did not watch explains the run from the page.

- [ ] T051 [US3] Build the run view in `src/service/web/run.html`, `app.js` and `app.css`:
  - the pipeline with stage states;
  - agent cards with role, state, current activity, branch, cost, and the warning when the reviewer's agent and model equal the builder's (FR-037);
  - the activity feed with a filter per agent, Orchestrator and You;
  - the decisions log with commit links;
  - gates against the baseline;
  - the criteria per part with evidence;
  - repairs, time and cost;
  - built from `GET /api/runs/<id>` plus long polling, with no external resources.
- [ ] T052 [US3] Stream activity in `src/agent/claude.rs` and `src/run/engine.rs`: each activity line is appended to `steps/<n>.jsonl` and wakes the pages waiting on the run as it is parsed, within 2 s (FR-015).
- [ ] T053 [US3] Complete the runs list in `src/service/web/index.html`: stage, what needs the engineer, cost, time, how many runs Fallow holds and their size on disk (constitution V), and the start form.
- [ ] T054 [US3] Add notifications in `src/service/web/app.js`: the browser Notification API for "needs you", permission asked on first visit, and the service's console line (FR-016, research R-6).
- [ ] T055 [P] [US3] Test `tests/us3_page.rs`:
  - reading a run mid-step returns the whole history;
  - a page that waits after event N gets every later event, without gaps;
  - an activity line reaches the stream within 2 s of the fake agent writing it (SC-006);
  - the run continues with no page open.

**Checkpoint**: the run is understandable from the page alone.

---

## Phase 6: User Story 4 - Steer the run, not the agents (Priority: P4)

**Goal**: every steering action of FR-017 and FR-018, applied through the run's state and never
typed into an agent, and recorded.

**Independent Test**: quickstart V6, `tests/us4_steering.rs`.

- [ ] T056 [US4] Implement steering in `src/run/steering.rs`:
  - `answer`, `overrule` (a decide step with the engineer's ruling, then reopening), `add_criterion` and `change_criterion` (a planner commit, reopen parts needing work, and the reviewer checks it), `note` (added to that agent's next instruction), `pause`, `resume`, `stop` (confirmed), `set_autonomy` (from the next decision), `set_limits`, `request_changes` (a repair with the comment, within the cap) and `reject`;
  - each one writes a `steering` event and counts as an intervention, except `approve_plan` and `merge` (FR-020).
- [ ] T057 [US4] Implement take over and hand back in `src/run/steering.rs`:
  - `take_over` ends the part's running step at once (its process tree), marks the part `with_engineer`, and returns `claude --resume <session>` and the worktree path;
  - `hand_back` verifies the part's branch as a step result (R-7), and uncommitted changes are reported and not accepted.
- [ ] T058 [US4] Add the steering endpoints of contracts/web-api.md to `src/service/api.rs` (answer, overrule, criteria, note, pause, resume, stop, take-over, hand-back, autonomy, request-changes, reject), returning 409 with the reason when not allowed.
- [ ] T059 [US4] Add the steering controls to `src/service/web/`:
  - the autonomy control;
  - the bar for what needs the engineer, with options;
  - Overrule on answered decisions;
  - Add a criterion;
  - Note, Pause and Take over on agent cards, and Hand back;
  - Merge, Request changes and Reject;
  - Stop the run, with an on-page confirmation naming what is affected;
  - as in the mockup.
- [ ] T060 [P] [US4] Test `tests/us4_steering.rs`: each action's effect and its `steering` event. In particular:
  - overrule reopens the affected parts;
  - a criterion added after review is checked before "ready";
  - a note appears in the next instruction;
  - hand back verifies;
  - request changes repairs within the cap;
  - no action writes to an agent's input.

**Checkpoint**: the engineer can intervene precisely, and every intervention is on record.

---

## Phase 7: User Story 5 - Parallel parts where the plan declares them (Priority: P5)

**Goal**: builders for declared parts run concurrently within caps. Fallow integrates their branches,
and conflicts become repairs.

**Independent Test**: quickstart V4 with two parts, `tests/us5_parts.rs`.

- [ ] T061 [US5] Schedule parts in `src/run/machine.rs` and `src/run/limits.rs`:
  - one `build` step per part, at most `max_parallel` at once;
  - 005's memory budget (measured memory plus the expected size of unmeasured starts, research R-12);
  - a step that does not fit is `waiting_capacity`, with the reason on the page, and starts by itself.
- [ ] T062 [US5] Implement integration in `src/integrate.rs` (research R-10):
  - merge the parts' branches into `fallow/r<id>/result` in plan order with `--no-ff`;
  - on a conflict, abort and queue a repair for the part being merged, naming the files and the other part's commit;
  - attribute an integration gate failure to the part owning the failing test file, or ask the engineer when unclear (FR-023).
- [ ] T063 [P] [US5] Test `tests/us5_parts.rs`:
  - a plan with overlapping `owns`, or without a committed contract per boundary, is not approvable and the problem is named (FR-021);
  - two parts run concurrently;
  - a scripted conflict becomes a repair for the right part;
  - the capacity wait starts by itself.

**Checkpoint**: squad mode works where the plan splits.

---

## Phase 8: User Story 6 - Publishing that respects the remote (Priority: P6)

**Goal**: with a protected target branch:
- a pull request with the run summary;
- CI known from the branch rules, then from the reported checks;
- repairs for failing required checks;
- the merge through GitHub.

**Independent Test**: quickstart V7, `tests/us6_publish.rs` with a fake `gh`.

- [ ] T064 [US6] Read the remote's rules in `src/publish/github.rs` (research R-11):
  - `gh api repos/{owner}/{repo}/rules/branches/{target}`, and `…/branches/{target}/protection` when allowed;
  - from them: required checks by name, required reviews, and whether the branch must be up to date;
  - `rules_source` is `branch_rules`, `protection`, `checks_only` or `none`.
- [ ] T065 [US6] Publish in `src/publish/github.rs`:
  - from `ready`: `git push origin fallow/r<id>/result`, then `gh pr create --base <target> --head fallow/r<id>/result --body-file <summary>`, where the summary has the plan, the criteria with evidence, the gates, the decisions with their commits, the repairs and the cost;
  - poll `gh pr checks --json name,state,bucket` every 30 s;
  - a failing required check becomes a repair round on the same branch;
  - a branch behind the target is merged up to date and gated again;
  - with nothing required and nothing reporting for 5 minutes, say "No CI found" (FR-024 to FR-026).
- [ ] T066 [US6] Merge the pull request in `src/publish/github.rs`: the engineer's Merge runs `gh pr merge --merge`. The page shows "Required by <target>: …", "Needs N approving reviews" and the rules source (FR-025, FR-027).
- [ ] T067 [P] [US6] Test `tests/us6_publish.rs`, with a fake `gh` and a bare remote:
  - a pull request opens with the summary;
  - the run waits for the required checks;
  - a failing required check repairs;
  - a check that is not required only warns;
  - "No CI found" after the wait.
- [ ] T068 [P] [US6] Test `tests/us6_never.rs` (constitution II gate, SC-009): scan the source for every git and gh call site and fail on `--force`, `--force-with-lease`, `--admin`, `pr review --approve`, or a push whose refspec names the target branch. At run time, the fake remote records no push to the target branch.

**Checkpoint**: Fallow fits repositories with protected branches.

---

## Phase 9: User Story 7 - Measure it on the engineer's own tasks (Priority: P7)

**Goal**: plain, solo and squad on task sets with hidden tests, records within constitution V, and
components switched off.

**Independent Test**: quickstart V8, `tests/us7_measure.rs`.

- [ ] T069 [US7] Implement the plain mode in `src/measure/` (research R-13): one `claude -p` call with only the task description, the builder's allowlist, no MCP tools and no Fallow instruction, in a fresh copy of the project, then the hidden tests.
- [ ] T070 [US7] Implement series in `src/measure/` and `fallow bench` in `src/main.rs`:
  - 005's task sets and copies, one run at a time, in shuffled order and with repeats;
  - solo and squad run at autonomy High with automatic plan approval, and the description as the planner's only input;
  - hidden tests are applied after the run's agents end;
  - a `hidden_tests` event is written;
  - copies are removed afterwards.
- [ ] T071 [US7] Implement switching components off in `src/run/machine.rs` and `src/measure/` (FR-030):
  - reviewer off: no review, and the merge needs only the gates;
  - gates off: no gate steps, but hidden tests still run;
  - planner decisions off: every question goes to the engineer, which in measured runs counts as stopped;
  - parallel parts off: everything is built as one part;
  - the record names them.
- [ ] T072 [US7] Write run records in `src/measure/record.rs`, keeping 005's field rules (constitution V): the agent and model per step, counts (steps, repairs, decisions, interventions, hidden tests), durations and reported cost. Add the bench page `src/service/web/bench.html` (grouped by task and mode: pass rate, median time, cost, interventions, spread).
- [ ] T073 [P] [US7] Test `tests/us7_measure.rs`, plus the marker test retargeted in `xtask/src/record.rs` (constitution V gate):
  - plain, solo and squad each produce a record;
  - hidden tests are never in a copy before the agents end;
  - records group by task and mode, and apart when components are switched off;
  - no marker text appears in a record.

**Checkpoint**: the comparison T051 asked for can be run on the engineer's task set.

---

## Phase 10: Polish & cross-cutting

- [ ] T074 [P] Write the four constitution VIII benchmarks in `xtask/src/bench.rs`, with a pinned reference project and a fake-agent script of 4 concurrent steps:
  - idle RSS under 60 MB;
  - running RSS under 200 MB;
  - page ready under 500 ms (p95 of 30 after 3 warm-ups);
  - activity latency under 2 s (p95);
  - add them to `.github/workflows/ci.yml` with one automatic re-run.
- [ ] T075 [P] Retarget the egress test in `xtask/src/egress.rs`: during a fake run that publishes through a fake `gh`, Fallow's own process opens no outbound connection, and its only listener is on `127.0.0.1` (constitution V).
- [ ] T076 [P] Rewrite `README.md` for the new Fallow: what it does, `fallow run`, the page, autonomy, steering, the allowlists and `[allow]`, publishing and protected branches, measurement, limits, and what is out of scope (Windows only for now).
- [ ] T077 Run `cargo clippy --all-targets`, `cargo fmt --check` and `cargo test`, then a fresh-context review of the whole of 006 against the spec and constitution v6.0.0, and fix its findings.
- [ ] T078 Manual: quickstart V1 to V8 with Claude Code on Windows. Record the results in `specs/006-headless-orchestrator/quickstart.md`. This needs the engineer.
- [ ] T079 Manual: the comparison series (plain vs solo, then squad) on the engineer's task set (SC-001, SC-002, SC-008). Report the results in `specs/006-headless-orchestrator/results.md`. This needs the engineer and real agents.

---

## Dependencies & Execution Order

**Phases**:
- **Setup (1):** nothing before it. T002 comes before T003, and T003 before T004.
- **Foundational (2):** after Setup. It blocks every story.
- **US1 (3):** after Foundational. It is the MVP.
- **US2 (4):** after US1, since it needs the machine and steps.
- **US3 (5):** after US1, since it needs the page skeleton and steps. It can run alongside US2.
- **US4 (6):** after US2 (overrule and criteria use decisions) and US3 (controls on the run view).
- **US5 (7):** after US1. It can run alongside US2 to US4, but integration conflicts need repairs from US1.
- **US6 (8):** after US1, since it publishes from "ready". It is independent of US2 to US5.
- **US7 (9):** after US1 (solo) and US5 (squad mode).
- **Polish (10):** after the stories it covers. T078 and T079 are last and need the engineer.

**Within a story**: the state and verification code before the machine, the machine before the API,
the API before the page. Tests marked [P] run alongside the tasks after the code they test.

## Parallel examples

- **Setup**: T006, T007 and T008, together after T005.
- **Foundational**: T010, T011, T014, T015, T017, T018 and T019 together, after T009. Then the tests T020 to T022 together.
- **US1**: T025 together with T023 and T024. The tests T038 to T041 together, once T027 to T037 are done.
- **Across stories**: US3 (T051 to T055) and US6 (T064 to T068) can be built alongside US2 once US1 is done.

## Implementation Strategy

1. **MVP**: Setup, Foundational and US1. Then stop and check quickstart V1 and V3 with Claude Code:
   one task from plan to merge with two interventions. This is also the solo mode for the comparison.
2. **Then**:
   - US2 (decisions), the fix for what stalled T050 Q2;
   - US3 and US4 (seeing and steering);
   - US5 (parts);
   - US6 (publishing);
   - US7 (measurement).
3. **Each story** ends at its checkpoint with its tests green and a commit. A fresh-context review
   follows each phase, as with 005.
