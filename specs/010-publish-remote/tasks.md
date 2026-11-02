# Tasks: Publishing that respects the remote

**Input**: Design documents from `specs/010-publish-remote/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: requested, as in 007 to 009. Every behaviour is tested without network: a local bare
repository behind a `github.com` address (git's `insteadOf`, research R-1) and the test binary
`fallow-fake-host` in place of `gh` (contracts/host-adapter.md). The page is checked with headless
Chrome over CDP on a demo service.

**Organization**: one phase per user story, in the spec's order:
- US1, the project's remote (P1)
- US2, a finished loop becomes a pull request, and publishing when verified (P1)
- US3, following the pull request to its end (P2)

After every phase, an independent fresh-context reviewer verifies it, and the implementer fixes
the confirmed findings before the next phase. That is done outside this list.

**Build rules for this machine**:
- Debug builds only.
- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, `-j 2`.
- Tests with `--test-threads=2` and `CARGO_TARGET_DIR=target/wip`, so the engineer's running
  Fallow is not locked.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1–US3)

---

## Phase 1: Setup

**Purpose**: the test doubles every story needs.

- [ ] T001 [P] Create `src/bin/fallow-fake-host.rs` (registered in `Cargo.toml` like `fallow-fake-agent`), per contracts/host-adapter.md "The fake". It reads a JSON state file named by the environment variable `FALLOW_FAKE_HOST`:
  - `signed_in: bool`;
  - `rules: {branch: [rule types]}` and `protected: {branch: bool}` (absent branch → answer 404);
  - `prs: [{number, head, base, title, state, merged_at, checks: [{name, conclusion, status, url}]}]`;
  - `fail: {"<subcommand>": "<message>"}` to inject failures (`auth`, `api`, `pr create`, `pr view`, `pr list`).
  - `sleep_ms: {"<subcommand>": ms}` to make a subcommand slow (for "answers at once" and restart tests).
  It answers exactly the command lines of the GitHub table (`auth status`, `api repos/…/rules/branches/…`, `api repos/…/branches/…`, `pr create …`, `pr view … --json …`, `pr list … --json …`) with `gh`'s output shapes; `pr create` appends a PR (state open, the body file's text kept) and prints its address `https://github.com/{o}/{r}/pull/{n}`; anything else exits 1 with "unknown command". It appends every command line (one JSON array per line) to `<state file>.log`, so tests can check what was asked. A test can change the state file between calls (merge, close, checks).
- [ ] T002 [P] In `tests/common/mod.rs`, add:
  - `bare_remote(root, project_repo, address)`: `git init --bare <root>/remote.git`; in the project repo `git remote add origin <address>`, `git config url.<bare>.insteadOf <address>`, push the target branch with `-u` (research R-1);
  - `fake_host(root, state: serde_json::Value) -> PathBuf` writing the state file, and `fake_host_log(path) -> Vec<Vec<String>>`;
  - a way for `start_studio` to add `[hosting.github] command = "<fallow-fake-host>"` and `FALLOW_FAKE_HOST` for the engine (the settings text through `settings.extra.toml`; the environment variable set for the process by the test, as the tests are their own processes);
  - `remote_head(root, branch) -> Option<String>`: `git rev-parse` in the bare repository.

**Checkpoint**: the fake answers its commands (a unit test in the binary's module, or a small test calling it); a bare remote can be made in a test.

---

## Phase 2: Foundational

**Purpose**: the runner, the adapter, the records. Blocks all stories.

- [ ] T003 Create `src/publish/run.rs` (research R-11): `run(program, args, dir, env, limit) -> Result<String, String>` running a command with `platform::command`, `GIT_TERMINAL_PROMPT=0` and `GH_PROMPT_DISABLED=1` set, capturing stdout and stderr, killing the process when `limit` passes ("<program> took longer than <n> s"), and returning stderr's last lines as the error. Limits: 20 s for reads, 60 s for push and pull request creation. Unit tests with a command that sleeps (`git` is not needed: use `cmd /C ping -n` on Windows, `sleep` elsewhere) and one that fails.
- [ ] T004 Create `src/publish/host/mod.rs` with the `Host` trait, `Repo { owner, name }`, `PullRequest { number, url }`, `PrState { state: Open|Merged|Closed, merged_at, checks: Vec<Check { name, state: Waiting|Passed|Failed|Skipped, url }> }`, exactly as contracts/host-adapter.md, and `host_for(service, settings) -> Option<Box<dyn Host>>` (only `"github"` in this version). Add `[hosting.github] command` (default `"gh"`) to `src/config/settings.rs` with a parse test.
- [ ] T005 Create `src/publish/host/github.rs`: the GitHub adapter through `run.rs`, with the command lines and the `statusCheckRollup` mapping of contracts/host-adapter.md. `protected`: rules endpoint first (a `pull_request` rule → true), then the branch's `protected` flag; a 404 for the branch → Err "<branch> is not on <owner/repo> yet". `signed_in` uses the exit code of `auth status --hostname github.com` only. Never `auth token`, never `--show-token`, no method that merges, approves, reviews, closes or edits a pull request. Unit tests: the argument lists built (no process), the JSON parsing of `pr view` and `pr list` sample outputs, the check mapping.
- [ ] T006 Records, in `src/run/events.rs` and `src/run/state.rs` (data-model.md "Publication of a loop", "Loop: when verified"):
  - `WhenVerified { Wait, PullRequest }` (serde `wait` / `pull_request`, default `Wait`); `LoopCreated.when_verified` with `#[serde(default)]`;
  - events `Published { remote, service, repo, branch, commit, target, pr_number, pr_url, by }` (`by`: `engineer` | `when_verified`), `PublishFailed { step, why, pushed: Option<(String, String)> }` (`step`: `check` | `push` | `pull_request`), `PullRequestState { state, checks, merged_at }`; `Steering` kind `Publish`;
  - the stage transition Publishing → Ready (in the allowed list beside Ready → Publishing, Publishing → Done);
  - `LoopState.when_verified`, `LoopState.publication` (the last `Published` with its latest `PullRequestState`) folded from the events.
  Unit tests: an old log without these fields reads; the fold of Published → PullRequestState(merged).
- [ ] T007 [P] In `src/project/mod.rs`, `LockRec.when_verified` and `Project.remote_seen: Option<RemoteSeen>` (the last known remote of data-model.md "Remote of a project", with `checked_at`), both `#[serde(default)]`; a round-trip test with an old `projects.json`.

**Checkpoint**: `cargo test --lib` green; nothing visible changes yet.

---

## Phase 3: User Story 1 - Fallow shows the project's remote (Priority: P1) 🎯 MVP

**Goal**: every project shows whether it has a remote, which one, the target there and whether it is protected; never a guess.

**Independent Test**: quickstart Q1.

### Tests for User Story 1

- [ ] T008 [P] [US1] Write `tests/us10_remote.rs` (quickstart Q1), each case a project with `bare_remote` and `fake_host` as needed, reading `remote` from the project's view (`Req`/`Request` the page uses) and waiting for `protected` to leave `checking`:
  - no remote → `state: none`;
  - `https://github.com/acme/shop.git` with a `pull_request` rule on the target → `known`, `service: github`, `repo: acme/shop`, `protected: yes`;
  - the classic `protected: true` only → `yes`; neither → `no`;
  - `signed_in: false` → `unknown` with a reason naming sign-in; the fake's log holds no `auth token` and no `--show-token`;
  - the branch absent on the fake (404) → `unknown`, "not on acme/shop yet";
  - `git@gitlab.com:acme/shop.git` → `unsupported`, service `gitlab.com`, no host command run;
  - two remotes, the target tracking `upstream` → `remote: upstream`;
  - the project view answers at once (before the host replies) with `protected: checking` (time it: under 500 ms with the fake's `sleep_ms` set to 3 s for `api`).

### Implementation for User Story 1

- [ ] T009 [US1] In `src/publish/mod.rs`, the git part of R-1: `find_remote(repo, target) -> Remote` (upstream of the target, else `origin`, else `no_target`; the configured address with `git config --get remote.<r>.url`; parse `https://`, `ssh://git@` and `git@host:` GitHub forms into `Repo`; any other host → `unsupported` named by its host). Unit tests for the address forms (with and without `.git`, `www.`, upper case), and a test on a real temporary repository for the upstream choice.
- [ ] T010 [US1] In `src/run/engine.rs`: the project view (`project_view`, ~line 1043) gains `remote` (contracts/web-api.md): the git part computed at once; `protected` from an in-memory cache per project (`checking` until known). Opening a project (reading its view) starts a refresh on a thread when the last check is older than 60 s, through the adapter (`signed_in`, then `protected`), reporting back with a new `Msg`/`Done` variant on the engine's channel; the result is kept in the cache and in `projects.json` `remote_seen`. The same refresh runs when a loop of the project reaches Ready (`finish()` in `src/run/feature.rs`). Publish a `project:<id>` bus event when it changes, so the page updates.
- [ ] T011 [US1] In `src/service/web/screens/project.js`, the remote next to the `git · <branch>` pill (line ~12): `github · acme/shop · main · protected` / `not protected`, `no remote`, `protection unknown` with the reason as its title and a "?" text, `checking…`, `gitlab.com: Fallow merges locally there`. Styles in `src/service/web/app.css` (an existing pill style; protected in the needs/orange tone, not protected grey). Refresh on the `project:<id>` event.
- [ ] T012 [US1] Check with headless Chrome on a demo service (a variant of `tests/visual_009.rs` with a bare remote and the fake host, or new `tests/visual_010.rs`): the pill in the five states at 1440 px and 390 px, screenshots in `specs/010-publish-remote/checks/`.

**Checkpoint**: Q1 green; the project page shows the remote.

---

## Phase 4: User Story 2 - A finished loop becomes a pull request (Priority: P1)

**Goal**: push exactly the verified commit to the loop's own branch and open a pull request with the loop's summary; the only way with a protected target; also on its own when verified, if chosen at the lock.

**Independent Test**: quickstart Q2, Q3, Q5.

### Tests for User Story 2

- [ ] T013 [P] [US2] Write `tests/us10_publish.rs` (quickstart Q2, Q3, Q5) on `locked_calc` loops run to Ready with `calc_loop_ok()`, with `bare_remote` and `fake_host`:
  - protected: the loop's `publish.offer == ["pull_request"]`; `Request::Merge` refused naming the protection; `Request::Publish{id}` → `remote_head(loop branch) == verified commit`, `remote_head(target)` unchanged, the fake's log has one `pr create` with `--base <target> --head <loop branch>`, the title the feature's, the body containing every criterion id with its verdict and "the merge is the engineer's"; stage Publishing; `publication.pr_url` set;
  - unprotected: both offered; `Request::Merge` merges locally and the bare remote is unchanged;
  - the loop branch already on the remote at another commit → published as `<branch>-2`, the other branch untouched;
  - `fail: {"pr create": …}` → `PublishFailed{step: pull_request, pushed: Some(..)}`, stage Ready, the reason on the loop; after clearing the failure, publishing again opens exactly one pull request (the fake's state has one);
  - a second `Request::Publish` while a pull request is open → refused, showing it;
  - Free without `when_verified` at the lock → `when_verified: pull_request`; at Ready (verified) it publishes without a request, `by: when_verified`, and a notice "pull request opened" is raised (the test notifier of `start_studio_notify`); the same loop ending not verified (a scenario with a failed criterion) is not published; with `signed_in: false` (protection unknown) it waits and `publish.why` says why;
  - restart between push and pull request (stop the engine while the fake sleeps in `pr create`, `sleep_ms`): after starting again, the fake has no PR → stage Ready with "publishing was interrupted"; a variant where the fake has the PR → recorded and followed, still one PR.
- [ ] T014 [P] [US2] A unit test in `src/publish/push.rs` (T016): a destination equal to the target is refused before git runs; no refspec ever starts with `+`; no argument `--force` / `--force-with-lease`.

### Implementation for User Story 2

- [ ] T015 [P] [US2] Create `src/publish/body.rs` (research R-5): the pull request's Markdown from the loop's record, the same facts as "What was verified" (find the code that builds it for `src/service/web/screens/loop.js` `showVerified`, and reuse its data source): feature and spec path; every acceptance criterion with its evidence and verdict; gates against the baseline; decisions with who took them; repairs; steps with agents and models; cost and time; the closing line "Verified by Fallow. The merge is the engineer's." (or "Not verified by Fallow: …" when the outcome is not verified). Unit tests on a recorded run.
- [ ] T016 [US2] Create `src/publish/push.rs` (research R-4): `push(repo, remote, commit, branch, target) -> Result<String /* branch used */, String>`: refuse `branch == target`; `git ls-remote <remote> refs/heads/<branch>`: absent → `git push <remote> <commit>:refs/heads/<branch>`; same commit → nothing; other → try `<branch>-2`, `-3`, … (up to 9, then an error). Through `run.rs` with the 60 s limit.
- [ ] T017 [US2] In `src/publish/mod.rs`, the offer rules (FR-005 to FR-007, data-model.md validation rules): `offer(remote, stage, outcome) -> (Vec<Offer>, why)`; and `publish_when_verified(when_verified, remote, outcome) -> Result<(), why>` (FR-020: only a verified outcome, `state: known`, `protected` yes or no). Unit tests for every combination.
- [ ] T018 [US2] In `src/run/machine.rs`, `publish(run, ctx, by)` beside `merge()` (~line 595): checks Ready, an outcome, not busy, the offer contains a pull request, no open publication; appends `Steering{Publish}`; `busy = Busy::Publish`; a thread: refresh the protection (step `check`), `push`, write `<run>/publish/body.md` with `body.rs`, `host.open(...)` → `Msg::Done{ Done::Publish(Result<..>) }`. In `on_done`: Ok → `Published` + Stage Publishing; Err → `PublishFailed` (stage stays Ready) and, when `by == when_verified`, a needs-you item. Refuse `merge()` when the offer lacks `local_merge`. Add `Done::Publish`, `Busy::Publish` in `src/run/engine.rs`.
- [ ] T019 [US2] Restart (research R-8): in `machine::resume` (the code that reconciles an interrupted merge, ~line 115), a run in Publishing without `publication`, or with a `Steering{Publish}` after the last `Published`/`PublishFailed`: on a thread, `host.find(repo, branch)`: found → `Published` (by as recorded) and followed; none → `PublishFailed{step: pull_request, why: "publishing was interrupted; try again"}`, stage Ready.
- [ ] T020 [US2] "When verified" (research R-7): the lock request takes `when_verified` (`src/service/http.rs` ~538, `Request::Lock`, `Engine::lock` ~1675: absent → `pull_request` for `Free`, else `wait`), records it in `LoopCreated` and `LockRec`. In `src/run/feature.rs` `finish()` (~2141): after `Stage{Ready}` with a verified outcome and `when_verified == PullRequest`, call `machine::publish(.., by: when_verified)` when `publish_when_verified` allows; otherwise record why on the loop (shown by the page). Show "When verified" in `project_view`/`loop view` for the page.
- [ ] T021 [US2] Requests and views: `Request::Publish { id }` and `POST /api/loops/<id>/publish` with `confirm` (contracts/web-api.md), refusing with the reasons; the loop view (`src/run/activity.rs` view or wherever `GET /api/loops/<id>` is built) gains `when_verified` and `publish { offer, why, remote, publication, failed }`.
- [ ] T022 [US2] Notices (research R-9): in `src/service/notify.rs` `what()`, kinds `pr_opened` ("<feature>: pull request opened") and `pr_merged` ("<feature>: merged on <repo>"); raised once per pull request (key `pr:<run>:<number>:opened|merged`), through the engine's notifier outside `needs_items`, honouring the project's notifications switch. A failed automatic publication is a needs-you item (`src/run/dashboard.rs`), kind `publish_failed`.
- [ ] T023 [US2] The loop page (`src/service/web/screens/loop.js`): where Merge is today (~line 746):
  - protected → only "Push and open a pull request", with the reason; unprotected → that first, then "Merge locally"; unknown → both, with "protection unknown: <why>";
  - a confirm dialog naming the remote, the branch to push, the target and the commit (FR-008);
  - after publishing: the pull request's number and link, its state, "by you" or "when verified, as chosen at the lock";
  - a failure: the step, the reason, "Try again";
  - while busy: the working state, no second click (as the merge does).
  Activity lines for the new events in `src/run/activity.rs` (FR-018).
- [ ] T024 [US2] The lock form (`src/service/web/screens/spec.js` ~84, ~392): "When verified" beside Autonomy (Wait for me / Open a pull request), preset from the level and following it until the engineer changes it; a note "only when the project has a remote"; sent as `when_verified`.
- [ ] T025 [US2] Check with headless Chrome on the demo service: the lock form's preset (Free → Open a pull request, back to Normal → Wait), the loop page's publish area protected / unprotected / failed / published, at 1440 px and 390 px; screenshots in `specs/010-publish-remote/checks/`.

**Checkpoint**: Q2, Q3, Q5 green; a verified loop reaches the remote as a pull request.

---

## Phase 5: User Story 3 - Fallow follows the pull request to its end (Priority: P2)

**Goal**: the pull request's state and checks on the loop; merged on the remote ends the loop; closed puts it back to Ready.

**Independent Test**: quickstart Q4.

### Tests for User Story 3

- [ ] T026 [P] [US3] Add to `tests/us10_publish.rs` (quickstart Q4): after publishing, the fake's PR set to `merged` → within one poll (the test sets the poll interval through a test setting or reads the loop, which asks at once) the loop is Done with `Merged{how: "pull request"}`, the feature's `ended` is merged, the local target branch unchanged (`git rev-parse <target>` before and after), a `pr_merged` notice once; set to `closed` → stage Ready with the closed pull request shown and Publish offered again; checks changing from waiting to passed/failed → one `PullRequestState` per change, none when nothing changed; `pr view` failing → the last state kept and "could not check now" with its time.

### Implementation for User Story 3

- [ ] T027 [US3] In `src/run/engine.rs` on `Msg::Tick` (~538): for each run in Publishing, at most every 60 s (and at most every 10 s when its view was read, research R-6), on a thread `host.state(repo, number)` → `Done::PrState(Result<PrState, String>)`. In `machine::on_done`: append `PullRequestState` only when the state or a check changed; merged → `Merged{commit: published commit, how: "pull request"}`, Stage Done, the feature ended merged (as the local merge does), `pr_merged` notice; closed → Stage Ready (keeping the closed publication shown); Err → keep the last state and note the time of the failed check in memory for the view. The interval is a constant with a test override (as `START_WAIT` in `src/agent/claude.rs`).
- [ ] T028 [US3] The loop page and the dashboard: the pull request's checks with their states and links; "merged on <repo> at <time>" when Done this way; "pull request closed" with Publish again / Stop; "could not check now (last at <time>)". The dashboard and the project's feature list show the feature merged (FR-014); nothing pulls or changes the local target branch, and the page says the merge is on the remote (FR-016).

**Checkpoint**: Q4 green; the loop ends when its pull request is merged on GitHub.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T029 Rework `tests/no_egress_no_push.rs` (research R-10): pushing (`"push"`) and the hosting tool's program allowed only in files under `src/publish/`; anywhere in `src/`: no `--force`, `--force-with-lease`, a refspec starting with `+`, `"merge"` with `pr`, `"review"`, `--approve`, `auth token`, `--show-token`; `TcpStream::connect` / `UdpSocket::bind` still banned everywhere. Keep its comment pointing at constitution II and V. Add the behavioural check that T013's protected case asserts (`remote_head(target)` unchanged) as part of the CI gate's test list in its header.
- [ ] T030 [P] Update `specs/007-fallow-studio/contracts/storage.md` (or wherever the run's events and `projects.json` are documented) with the new events and fields, (`contracts/runtime.md` is untouched: no runtime change). Mark 006 US6 as delivered by 010 for FR-024 and FR-027, with FR-025/026 (waiting for checks, repairs, updating) left for later, in `specs/006-headless-orchestrator/spec.md` notes.
- [ ] T031 Run the whole suite (`cargo test -j 2 -- --test-threads=2`, CARGO_TARGET_DIR=target/wip) and `cargo clippy --all-targets -j 2 -- -D warnings`; fix what fails. Re-run any known flaky test alone before calling it a failure, and say so.
- [ ] T032 Quickstart Q7 by hand, with the engineer, on a real GitHub repository whose main branch requires pull requests (the engineer's own); record the result in `specs/010-publish-remote/checks/real-github.md`.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: none. T001 ∥ T002.
- **Foundational (Phase 2)**: T003 → T004 → T005 (the adapter uses the runner and the trait); T006 and T007 in parallel with them. Blocks every story.
- **US1 (Phase 3)**: needs Phase 2. T008 first (failing), then T009 → T010 → T011 → T012.
- **US2 (Phase 4)**: needs Phase 2 and US1's T009 and T010 (the remote and its protection). T013 ∥ T014 ∥ T015 first; T016 → T017 → T018 → T019 → T020 → T021 → T022 → T023 → T024 → T025.
- **US3 (Phase 5)**: needs US2's publishing (T018, T021). T026 first, then T027 → T028.
- **Polish (Phase 6)**: T029 after T016 (the push exists to be scanned); T030 any time after the contracts; T031 after all; T032 with the engineer, last.

### Within each story

The test file comes first and fails, then the module, then requests and routes, then the page, then the headless check.

### Parallel opportunities

- T001 ∥ T002.
- T006 ∥ T007 beside T003 → T005.
- T008 ∥ T009's unit tests (US1).
- T013 ∥ T014 ∥ T015 (US2: a test file, a unit test, a separate module).
- T030 at any time after the contracts are final.

## Parallel Example: User Story 2

```text
Task: "Write tests/us10_publish.rs (T013)"
Task: "Unit tests of the target guard in src/publish/push.rs (T014)"
Task: "Create src/publish/body.rs with unit tests (T015)"
```

## Implementation Strategy

### MVP first

1. Phases 1 and 2.
2. US1: every project shows its remote and protection. Already useful: the engineer knows what
   Merge will do, and that a protected project needs a pull request.
3. Stop and check Q1; review.

### Then

4. US2: publishing, by hand and when verified (the feature's core).
5. US3: following the pull request to its merge.
6. Polish: the CI gate, the docs, the whole suite, and the check on a real repository with the
   engineer.
