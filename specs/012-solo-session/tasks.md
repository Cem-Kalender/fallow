# Tasks: Solo sessions

**Input**: Design documents from `specs/012-solo-session/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/web-api.md, contracts/runtime.md, quickstart.md

**Tests**: requested, as in 007 to 011. Each story has one test file on the fake runtime, without
network or a model. Hooks that write marker files prove that Fallow's git commands run no hook. A
local bare repository stands in for a remote. The page is checked with headless Chrome over CDP on a
demo service. Only T001 uses the real `claude`.

**Organization**: one phase per user story, P1 first, then P2, then P3:
- US1 (P1): a coding session with the assistant
- US2 (P1): several sessions, picked from one dropdown
- US8 (P1): every project is a git repository
- US3 (P2): seeing and checking what the session changed
- US4 (P2): git by asking, every change shown
- US7 (P2): from a discussion to a feature
- US5 (P3): everything a coding assistant needs
- US6 (P3): closing a session

After every phase, an independent fresh-context reviewer verifies it, and the implementer fixes
the confirmed findings before the next phase. That is done outside this list.

**Build rules for this machine**:
- Debug builds only, with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0` and `-j 2`.
- Tests run with `--test-threads=2` and `CARGO_TARGET_DIR=target/wip`, so the engineer's running
  Fallow is not locked.
- When memory is short, run test files one at a time.
- If a build is blocked by a security policy, stop and report it; do not work around it.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1–US8)

---

## Phase 1: Setup

**Purpose**: confirm what the plan assumes about the installed `claude`, and build the test doubles
and fixtures every story needs.

- [ ] T001 **The check on the installed `claude`** (research R-6, R-8; contracts/runtime.md, "Which settings apply"). Run it first; **if any point fails, stop and report to the engineer before any other task**.
  - **Where**: an ignored test `check_012_claude` in `tests/agent_claude.rs`, run by hand with `--ignored`. Each probe is a short `claude -p` call in a scratch repository, with the engineer's normal login and the cheapest model, and the whole check spends at most $0.50.
  - **The settings points** (R-6), each confirmed or refuted:
    1. `--setting-sources` with an empty list is accepted, and the login and `--model` still work.
    2. `--setting-sources user` is accepted.
    3. An allow rule in the working copy's `.claude/settings.json` does not apply.
    4. Under `dontAsk` with an empty source list, a user-level allow rule does not apply. The probe adds no file to the engineer's settings: it uses the user rules already there, read only through `claude`'s own behaviour. If no user rule can be found that way, it says so and the engineer confirms by hand.
    5. A whole-tool `permissions.ask` rule given through `--settings <json>` beats a user allow rule, an `--allowedTools` allow and an `additionalDirectories` path, and reaches `--permission-prompt-tool`.
    5b. No other tool that can write files or run commands is open to a user allow rule (the ask rules cover `Bash`, `PowerShell`, `Edit`, `Write`, `MultiEdit` and `NotebookEdit`; any other writing tool the installed `claude` has is added to them, or the check fails).
    6. `disableAllHooks: true` through `--settings` stops a user hook.
    7. Whether the project's `CLAUDE.md` is still loaded, and whether any plugin or user MCP tool loads with `--setting-sources user --strict-mcp-config`.
  - **The other points** (R-8): a call with `--input-format stream-json` and a base64 png image block is answered about the image; `WebFetch` and `WebSearch` in `--disallowedTools` are refused.
  - **The record**: the results go in `specs/012-solo-session/checks/claude-check.md`, with the `claude --version`, each point's outcome and the exact flags used. Fallow never reads the engineer's settings file.
- [ ] T002 [P] Extend the fake runtime in `src/bin/fallow-fake-agent.rs` and `src/agent/fake.rs`, and document each new key in the binary's header.
  - Its profile declares capabilities: `images`, `web_off` and `control_files`.
  - A scenario step may carry `expect_images = n`. The step fails when it receives a different number of images; it reads them from a stream-json stdin message the way the Claude adapter writes it.
  - A step may carry `web = ["<url>"]`, which emits `tool_call` events of kind `web`.
  - A step's `output` may carry `proposed_feature {title, brief}` and `proposed_pipeline`.
  - A step may carry `run = [...]` commands that move git refs. It runs them in its working directory through the permission prompt tool, so `tool_call` sees them, and it waits for the answer.
  - When `FALLOW_FAKE_ARGS_LOG` is set, every invocation's argv goes to that file, so tests can check the adapter's flags.
  - Unit tests for each new key.
- [ ] T003 Add helpers to `tests/common/mod.rs`:
  - `start_session(tx, project, agent) -> session` and `say_solo(tx, session, text, attachments)`;
  - `wait_turn(tx, session, n)` and `approve_next(tx, session, answer)`;
  - `repo_with_markers(root)`: a project repository whose `pre-commit`, `prepare-commit-msg`, `post-commit`, `post-checkout` and `reference-transaction` hooks each append their name to `<root>/hooks.log`, plus `hooks_ran(root) -> Vec<String>`;
  - `solo_scenario(steps)`: a fake profile of kind assistant;
  - `bare_remote(repo)`, reused from 010.

  It runs after T002.
- [ ] T004 [P] Add the Assistant kind.
  - `src/defs/agent.rs`: `Kind::Assistant` (`kind: assistant`), which returns `assistant_reply`.
  - `assets/agents/assistant.md`: the shipped default, `runtime: claude`, with a plain-text body.
  - `src/defs/mod.rs`: `DEFAULT_AGENTS` installs it, and `install_defaults` copies it.
  - `src/defs/pipeline.rs`: refuses an assistant as a step, as it refuses a lead.
  - `screens/agents.js`: the kind list shows it.
  - Unit tests: parsing it, and the pipeline refusing it.

**Checkpoint**: T001's record shows every point confirmed, or the engineer has decided. The fake
runtime plays images, web calls and suggestions. The assistant kind exists.

---

## Phase 2: Foundational

**Purpose**: the runtime contract, settings and instructions for every kind, git helpers, the
session's records and the command judge. These block all stories.

- [ ] T005 Extend the runtime contract in `src/agent/mod.rs` (R-8, contracts/runtime.md).
  - `Capabilities { images, web_off, control_files }` and `Runtime::capabilities()`.
  - `Call.attachments: Vec<Attachment{path, media_type}>` and `Call.web: bool`, both default empty or false for every kind but the assistant.
  - `ToolKind::Web`, with `activity_of` showing "Fetched <url>" or "Searched <query>".
  - Permission requests in the contract's terms (analyze D1, IX): `agent::Request { kind: Run | Edit | Web | Other, command, path, url, query, tool }`. The Claude adapter maps its tool names and input fields into it (`Bash`/`PowerShell` to `Run`; `Edit`/`Write`/`MultiEdit` with `file_path` and `NotebookEdit` with `notebook_path` to `Edit`; `WebFetch`/`WebSearch` to `Web`; anything else to `Other`). The MCP bridge hands `tool_call` this shape, and the lead's existing Bash/PowerShell check in `src/studio/mod.rs` moves to it.
  - The Claude adapter in `src/agent/claude.rs`:
    - maps `WebFetch` and `WebSearch` to `Web`;
    - puts them in `--allowedTools` when `web` is true and in `--disallowedTools` when it is false;
    - writes the instruction as a stream-json user message with base64 image blocks when there are attachments (`--input-format stream-json`).
  - `GET /api/runtimes` gains `capabilities`.
  - Unit tests for the adapter's arguments with each combination.
- [ ] T006 Settings for every kind (FR-042), in `src/agent/claude.rs` `args()` and `src/agent/rights.rs` (R-6).
  - The lead, makers and checkers: `--setting-sources` with an empty list, and `--model` always (the definition's, or the runtime profile's default).
  - The assistant:
    - `--setting-sources user`;
    - `--settings <json>` with whole-tool `permissions.ask` rules for `Bash`, `PowerShell`, `Edit`, `Write`, `MultiEdit` and `NotebookEdit`, and `disableAllHooks: true`.
  - The runtime's `control_files` are added to every call's `protect`.
  - `--tools` with the fixed list per kind (research R-6, "A fixed tool list"): `Read`, `Glob`, `Grep`, `Bash`, `PowerShell`, `Task` and Fallow's MCP tools; `Edit`, `Write`, `NotebookEdit` for makers, the lead and the assistant; `WebSearch`, `WebFetch` for the assistant with web on. The instruction goes on stdin, never after a list flag.
  - Unit tests check the arguments per kind.
  - Run every test from 007 to 011 (lead, loops, publishing) and keep them green. When a test relied on settings, the change is noted in this task.
- [ ] T007 Hand the project's instruction files to every kind (FR-007, FR-042; R-7).
  - **The reader**: a new `src/agent/instructions.rs` reads `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.github/copilot-instructions.md`, `.cursorrules`, `.cursor/rules/*.mdc` and `[instructions] extra` from a working copy.
    - The `extra` list is read from the project folder's `HEAD` with `git show`.
    - Each file is cut at 64 KiB, and the cut is marked.
    - Symlinks are resolved. Files outside the copy, and secrets (`files::is_secret`), are skipped and named.
    - It returns `[{path, sha, text, cut, skipped}]`.
  - **The config**: `src/config/squad.rs` gains `[instructions] extra = [..]` in `KNOWN`, with a parse test.
  - **The prompts**: `prompts::lead_turn` includes the files on the lead's first turn, and on a later turn when a sha changed (the shas are kept per turn, in `turn_started.instructions`). The loop's step prompts in `src/run/feature.rs` include them, read from the step's own worktree.
  - **Tests** in `tests/us1_conversation.rs` and `tests/us2_loop.rs`: a lead turn and a builder step carry an `AGENTS.md` text, and a `.env` file named in `extra` is never read.
- [ ] T008 [P] Add git helpers to `src/verify/git.rs` (R-2, R-3, R-10, R-18).
  - `NOHOOKS`: the empty directory `<data>/nohooks`.
  - `git_nohooks(dir)`: a `Command` with `-c core.hooksPath=<nohooks>` and `GIT_LFS_SKIP_SMUDGE=1`.
  - `snapshot(wt, branch, message, ident) -> Result<Option<sha>, String>`:
    - it first checks that `git symbolic-ref HEAD` is `refs/heads/<branch>`, and otherwise fails with "the working copy is not on the session's branch (it is on …)";
    - then `add -A`, `write-tree`, and nothing when the tree equals `HEAD^{tree}`;
    - then `commit-tree -p HEAD --no-gpg-sign` with `-c commit.gpgSign=false`, and `update-ref refs/heads/<branch> <new> <old>`.
  - `identity(dir) -> Option<(name, email)>` from `git config user.name/user.email`, and the `GIT_AUTHOR_*`/`GIT_COMMITTER_*` fallback to Fallow's author.
  - `worktree_add_new(repo, dir, branch, start)`, without hooks.
  - `ref_snapshot(repo) -> BTreeMap<ref, sha>` with `--no-optional-locks for-each-ref refs/heads refs/remotes`.
  - `folder_status(dir)` with `--no-optional-locks status --porcelain=v1`.
  - `blob_of(dir, path)`.
  - `git_check() -> GitCheck{found, ok}` with a minimum of 2.38, reusing `git_version`.
  - Tests in `tests/verify_git.rs`:
    - a snapshot in a repository with marker hooks writes no marker, and the commit carries no `gpgsig` even with `commit.gpgSign=true`;
    - a no-op snapshot makes no commit, and ignored files are not committed;
    - a snapshot refuses a detached or other `HEAD`;
    - `worktree_add_new` fires no hook;
    - `ref_snapshot` sees local and remote-tracking refs.
- [ ] T009 Add the session's records to `src/conversation/events.rs`, exactly as in data-model.md, all `#[serde(default)]`.
  - `Kind::Solo`.
  - The new events:
    - creating: `session_creating`, `session_created`, `session_renamed`, `branch_renamed`;
    - turn commits: `turn_committed`, `fold_next`, `uncommitted_discarded`, `engineer_edits`, `base_moved`;
    - undo: `undo_started`, `turns_undone`;
    - waiting messages: `message_queued`, `queue_edited`, `queue_removed`, `queue_sent`;
    - observed git and gates: `web_set`, `git_observed` with `commits[]`, `folder_state`, `squashed`, `gates_run`;
    - features: `feature_suggested`, `feature_starting`, `feature_started`;
    - closing: `close_started`, `session_closed`, `copy_removed`.
  - The new fields:
    - `turn_started`: `attachments`, `instructions`, `slot_waited_ms`;
    - `turn_finished`: `commit`, `commits`, `commit_failed`, `changed[]`, `cut`, `folded_into`;
    - `approval_asked`: `reasons`, `merge{…, control_changes}`;
    - `approval_answered`: `auto`, `why`;
    - `conversation_created`: `agent`, `origin`;
    - `mode_set`: the values `ask` and `until_switch` (`until_switch` is never written).
  - The fold into `State`, per data-model:
    - the turns, with undone and shared commits;
    - `base`, the last `base_moved` of a turn that is not undone, else the start commit;
    - `pending_commit`, `queue`, `web`, `gates` and its label, `suggestions`, `features`, `observed`, `closed`.
  - Unit tests for each fold, including that undoing a turn moves the base back, and that a log from 009 folds unchanged.
- [ ] T010 [P] Write the command judge in a new `src/studio/judge.rs` (R-6).
  - `split(cmd)`, on `&&`, `||`, `;`, `|` and newlines, handling one level of quotes.
  - `OPAQUE`: the "cannot tell" forms and the inline interpreters, exactly as R-6 lists them, plus environment assignments and dumps, `[Type]::` calls, provider drives (matched without regard to case) and UNC paths.
  - `assess(cmd, ctx{wt, branch, folder, protect}) -> Allow | Ask(Vec<reason>)`, covering every form R-6 lists:
    - pushes: force, `--prune`, deletes, `+` refspecs;
    - branch rewrites outside the session's own branch;
    - commands that discard uncommitted changes;
    - `HEAD` moves (checkout or switch to another branch, a detached commit, `-b`/`-c`/`-B`/`-C`/`--detach`, `symbolic-ref`, `worktree`);
    - paths outside the copy or matching `protect`, after resolving symlinks and counting `--opt=value` and `-oVALUE` as paths;
    - writes to `git config`;
    - output options (`--output`, `-o`, `tee`), which are allowed inside the copy and outside `protect`;
    - redirections, under the same rule as output options.
  - `prefix_safe(prefix)`.
  - `runs_unasked(cmd, gates, approved)`: true only when every part is a gate or a `prefix_safe` approved prefix and there is no "cannot tell" form.
  - Every request carries the fixed note "Fallow judges the command as written; it cannot see what a script does inside".
  - A table-driven unit test holds every example of research R-6 and quickstart Q5, with each verdict: `npm test && rm -rf ../x`, `git -c core.pager=x log`, `GIT_DIR=../p/.git git merge x`, `printenv`, `cp x .fallow/squad.toml`, `git diff --output=.fallow/squad.toml`, and so on.
- [ ] T011 [P] Amend 007's contracts.
  - `specs/007-fallow-studio/contracts/runtime.md` points to 012's `contracts/runtime.md` (capabilities, attachments, `web`, settings, the assistant's schema).
  - `specs/007-fallow-studio/contracts/storage.md` gains a "012 adds" section: the session events, `attachments/`, `projects/<p>/git.jsonl` and `git-seen.json`, and `refs/fallow/solo/<id>/undone/*`.

**Checkpoint**: the helpers and the judge pass their unit tests. Old logs load. Every test from 007 to
011 is green with the new settings arguments. The lead and the builders get the instruction files.

---

## Phase 3: User Story 1 - A coding session with the assistant (Priority: P1) 🎯 MVP

**Goal**: start a session, send a message, see the assistant work, and find the turn committed on the
session's own branch, with the engineer's folder untouched.

**Independent Test**: quickstart Q1.

- [ ] T012 [US1] Write `tests/us12_session.rs` (Q1). It fails first.
  - A draft's first send creates the session and branch `fallow/solo/fix-the-sunday-bug`, and says the folder's uncommitted change is not in it.
  - Turn 1 edits `picker.js`, and there is exactly one commit, named `Fallow solo <id>, turn 1: …`.
  - `hooks_ran` is empty after creation, renaming and the commit.
  - The commit is not signed.
  - The folder's files, branch and status are as they were.
  - A turn that changes nothing makes no commit.
  - A stopped turn's edits are committed with `(cut short)`.
  - A taken branch name gets `-2` (FR-015).
  - An LFS-attributed file stays a pointer, and the session says so (`lfs`).
  - The activity shows edits and commands.
  - A folder on a detached commit: the session starts from that commit and says so (analyze C7).
  - The assistant commits by itself during a turn: its commit stays, and Fallow's snapshot holds only what is left (analyze C12).
  - The assistant's runtime missing: the session says why and what to do, keeps its working copy, and the message can be sent again (analyze C5).
  - The first turn's system text holds the squash instruction, the rule against force-pushing and when to suggest a feature (analyze C8).
- [ ] T013 [US1] Create sessions, in a new `src/studio/solo.rs` (R-2).
  - `create(project, agent, first_message)`:
    1. writes `session_creating`;
    2. makes `worktree_add_new` at the project folder's `HEAD` under `worktrees/<p>/_solo/<id>`;
    3. checks the start branch, whether the folder is dirty, and whether `.gitattributes` names LFS;
    4. writes `session_created`;
    5. names the branch after the first message's slug (`branch_renamed`, with `-2`… when the name is taken).
  - The session is named after its first message.
  - `Studio` keeps the project's `sessions` and `last_session` in `projects.json`, through `src/project/mod.rs`.
  - `Req::NewSession{project, agent, text, attachments}` lives in `src/studio/mod.rs`.
  - The project's default assistant: `Project.assistant`, set through `PATCH /api/projects/<p>` and checked as `lead` is.
- [ ] T014 [US1] A session's turn, in `src/studio/mod.rs` `say`, for `Kind::Solo` (R-6, R-7, R-13).
  - **What the call is built from**:
    - the assistant's definition and runtime;
    - `rights::assistant(mode, approved, web)`: in-copy edits with `protect` (`CONTROL_GLOBS` and the control files), and an allow-list of only the gates and the `prefix_safe` approved prefixes, never `cd`;
    - `prompts::assistant_system` and `assistant_turn`, built from T007's instruction files, the undone turns, a failed commit and the engineer's edits;
    - `schemas::assistant_reply`;
    - `Call.web` from the session.
  - **The slot**: a turn takes one (`Ctx.slots.take()`). When none is free, the message waits as `message_queued` with "waiting for a free agent slot".
  - **`tool_call`'s assistant branch**, judging T005's `Request` only, never a runtime's tool names:
    - `Web` is allowed when web access is on;
    - `Edit`: its path is resolved, and denied outside the copy or matching `protect`;
    - `Run` goes to `judge::runs_unasked` with the live `approved_commands` and the gates (each matched as a whole command). Otherwise, under `until_switch`, it goes to `judge::assess`, and the rest is asked;
    - `Other` is denied.
  - **`prompts::assistant_system`** holds R-7's content: work only in the current directory, Fallow commits each turn, bring work into another branch as one squashed commit in one command line unless asked for a plain merge, never force-push unless asked, suggest a feature in `proposed_feature` and never create it, put a designed pipeline in `proposed_pipeline` (analyze C8).
  - **Waiting messages, minimal** (analyze F5): the engine's 1 s tick sends the oldest `message_queued` once a slot is free and the turn before ended normally. T045 adds editing, removing and the restart rule.
  - **A runtime that cannot start**: the turn ends with the reason and what to do (`agent.problem`), the working copy stays, and the message can be sent again (analyze C5).
  - **"Always"** is refused unless the prefix is `prefix_safe`.
  - **The mode** is `Mode::Ask`, or `until_switch`, kept in memory in `Conv.allow_until_switch`.
- [ ] T015 [US1] The end of a session's turn, in `src/studio/mod.rs` `turn_done` and `solo.rs` (R-3).
  - `snapshot` on the session's branch, with the identity.
  - `TurnFinished.commit` and `commits` come from `rev-list <before>..<after>`, which keeps the assistant's own commits.
  - When the commit fails: `commit_failed` with `changed[{path, blob}]`, and the changes stay in the working copy.
  - Cancel commits the turn as `(cut short)`.
  - The working-copy reset of 009's withdrawal never applies to a session.
- [ ] T016 [US1] Add the session routes in `src/service/http.rs` and the engine dispatch (`Engine::studio_request` in `src/run/engine.rs`), per contracts/web-api.md:
  - `POST /api/projects/<p>/sessions`;
  - `GET /api/sessions/<s>`, long-polled on `conv:<s>`;
  - `POST …/say`, `…/cancel`, `…/mode` and `…/approve`.

  The session view gains the fields that exist so far.
- [ ] T017 [US1] Build the page, in a new `src/service/web/screens/solo.js` and in `screens/project.js` (R-16, FR-001, FR-004, FR-006).
  - The middle column is the session open last, or a draft: an empty composer with the assistant picker.
  - The session header shows the branch, the start branch and commit, web access, "Open in my editor" and "Close session" (the last is a placeholder until US6).
  - The chat, `/` menu, working line, activity and stop come from `app.js`.
  - The approval card shows the judge's reasons and the fixed note.
  - The mode chip reads "Ask for commands" or "until I switch back". Turning "until I switch back" on shows the notice that Fallow judges commands as written and cannot see what scripts do inside (FR-008; analyze C11).
- [ ] T018 [US1] Run the headless page check: a demo service, `serve_the_012_demo` (ignored) in `tests/us12_session.rs`, and screenshots `specs/012-solo-session/checks/solo-*.png` of a draft, a working turn, a finished turn and an approval card.

**Checkpoint**: Q1 passes. The MVP works: a session edits code on its own branch, and every turn is
committed by Fallow.

---

## Phase 4: User Story 2 - Several sessions, picked from one dropdown (Priority: P1)

**Goal**: several sessions at once, each on its own branch, picked from one dropdown, surviving a
restart.

**Independent Test**: quickstart Q2.

- [ ] T019 [US2] Write `tests/us12_many.rs` (Q2). It fails first.
  - Two sessions edit line 3 of `a.txt` at the same time, and each branch holds only its own change (SC-005).
  - The list shows both, with their working marks and changed-file counts.
  - A rename shows in the list.
  - A restart during a turn: the turn is cut short, its changes are committed, nothing is repeated, and a pending approval is cancelled ("Fallow restarted") (SC-008).
  - A process left running is killed before the snapshot.
  - A half-done `session_creating` is reconciled.
  - With `max_parallel = 1`, a second session's message waits with "waiting for a free agent slot" and is sent when the first turn ends (FR-027; analyze C4).
  - A session's branch deleted outside Fallow: the session is marked `gone` and can only be closed (analyze C12).
- [ ] T020 [US2] Add the list and renaming.
  - `GET /api/projects/<p>/sessions` (open, closed and earlier conversations, with `records_bytes`) and `POST /api/sessions/<s>/rename`, in `src/service/http.rs` and `src/studio/solo.rs`.
  - Each item carries its name, a working mark and its changed-file count (`git diff --name-only base.mark HEAD`, plus uncommitted changes).
  - `last_session` is updated on open.
- [ ] T021 [US2] Handle restarts, in `src/studio/solo.rs` and `Studio::load` (R-19).
  - `turn.pid` holds the root pid and its start time. Load kills the tree first, checking the start time; on Windows it walks the tree with Toolhelp.
  - Then the cut turn is snapshotted.
  - Pending approvals are cancelled (`why: "Fallow restarted"`).
  - The half-done intents are reconciled through one hook point, `solo::reconcile`. This task handles `session_creating`; T030, T049 and T040 each add their own (`undo_started`, `close_started`, `feature_starting`) (analyze F6).
  - "Until I switch back" is off after a restart, and the session says so when it was on.
  - A session whose working copy or branch is missing is marked `gone`.
- [ ] T022 [US2] Build the dropdown in `screens/solo.js` (FR-002).
  - Clicking it only opens the list.
  - The open session is ticked and on top, with working dots, counts and "+ New session".
  - "Closed sessions…" and "Earlier conversations with the lead…" sit at the bottom.
  - There are no sessions anywhere else.
  - Screenshot `checks/solo-dropdown.png`.

**Checkpoint**: Q2 passes.

---

## Phase 5: User Story 8 - Every project is a git repository (Priority: P1)

**Goal**: git is checked at start; a folder without git is made a repository on the engineer's click;
plain folders are retired.

**Independent Test**: quickstart Q9.

- [ ] T023 [US8] Write `tests/us12_repo.rs` (Q9). It fails first.
  - A folder holding `node_modules/` and `.env`: the scan names them, the file count, the size and a proposed `.gitignore`; nothing changes before the click; afterwards there is one commit without them.
  - A subfolder of a repository is refused, with `inside`.
  - A subfolder project added before, with a feature, moves to the top folder (`NeedsMove`). Its feature gets a branch, its documents stay where they were, and it can be locked.
  - A plain-folder project from before gets the question.
  - A fake `git` on `PATH` reporting 2.30, or none at all: `GET /api/git` names the problem, every project request answers 409, and "Check again" passes once git is back.
  - With no identity, the first commit is refused, saying how to set one (SC-012).
- [ ] T024 [US8] The git check, in `src/run/engine.rs` and `src/service/http.rs` (FR-040).
  - `git_check` runs at start and when a project is added.
  - `GET /api/git` and `POST /api/git/check`.
  - While git is not ok, project requests answer 409 "Fallow needs git 2.38 or later".
  - The start page (`screens/dashboard.js`) shows the message, the minimum version, the installation hint for the system and "Check again".
- [ ] T025 [US8] Projects without git, and the retirement of plain folders, in `src/project/mod.rs` and `src/studio/mod.rs` (FR-039, FR-041).
  - `Kind::NeedsGit` replaces `Kind::Folder`.
  - `GET /api/projects/<p>/git-init` returns the scan: the dependency and build folders and the likely secrets of R-18, the size, and the proposed `.gitignore`.
  - `POST …/git-init {gitignore?}` checks the identity, then `git init`, the ignore file, `add -A` and the first commit (the engineer's git as set up), then the existing feature moves.
  - Plain-folder handling is retired everywhere: `undo`, `say`, `turn_done`, `lock_check`, `files::versions`, the lead's plain-folder instructions, and the page notices in `project.js`, `spec.js` and `dashboard.js`.
  - `tests/us1_projects.rs` and other tests that built plain folders move to the `NeedsGit` flow, with each change noted.
- [ ] T026 [US8] Projects inside a bigger repository: `Kind::NeedsMove {top}` and `Project.spec_root` (R-18).
  - `GET`/`POST /api/projects/<p>/move` moves the project to its top folder. The features' `dir` gets the subfolder as a prefix, and each feature gets a branch and a worktree, with a `spec:` commit when its documents are not committed.
  - `.specify/` and `.fallow/` stay where they are, read under `spec_root`.
  - The code that assumes `specs/` at the project's root uses the spec root instead: `rights::SPEC_GLOBS`, `SPEC_DIRS` and `CONTROL_GLOBS` per project, `next_dir`, `spec_dirs`, `update.rs` `doc_dir`, `files` `feature_dir`, `lock_prepare`, `SQUAD_FILE`, the project library in `defs`, and the `.specify/` paths in `say` and `new_feature`.
- [ ] T027 [US8] Build the pages: "Make it a git repository" and "Use the repository's top folder", with the scan and an editable `.gitignore`, in `screens/project.js` and `screens/dashboard.js`. Screenshots `checks/git-*.png`.

**Checkpoint**: Q9 passes. Every test from 007 to 011 is green without plain folders.

---

## Phase 6: User Story 3 - Seeing and checking what the session changed (Priority: P2)

**Goal**: the session's changes, files beside the chat, gates on demand with honest labels, and undo
back to before any turn.

**Independent Test**: quickstart Q3.

- [ ] T028 [US3] Write `tests/us12_check.rs` (Q3, as numbered there). It fails first.
  - Changes lists the files and the turns that changed them.
  - The Files tab's version is `solo:<id>`, with its marks.
  - The engineer's edit is committed before turn 4 as their own.
  - The gates go from "Not verified" to "passed on <short>, no independent check" (never the verified wording), and back to stale after a later turn. They are refused during a turn.
  - Undoing turn 2 names turns 2–4 and the edits, the branch is at turn 1's commit, the undone ref exists, and the next prompt names what was undone.
  - A failed commit: undo and the next turn wait; "Fix it in the next turn" makes one commit for turns n–m; an edit made after the failure gets its own commit, marked as the engineer's.
  - `base_moved` when a turn merges the starting branch in, and again on undo.
  - A restart between `undo_started` and `turns_undone` finishes the undo (analyze C3).
  - Opening a file with "Open in my editor" during a turn sets `editor_during_turn`, and the session says those edits count as the turn's (analyze C12).
  - The Changes panel and the undo confirmation say that they cover only the working copy (analyze C1).
- [ ] T029 [US3] Settling the working copy, `solo::settle` (R-4).
  - A pending commit is retried, comparing paths by blob: what the turn left goes in `turn_committed`, and what changed since goes in `engineer_edits`.
  - The engineer's between-turn edits are snapshotted as `engineer_edits`.
  - `fold_next` and `uncommitted_discarded` need a confirmation naming the files.
  - `POST /api/sessions/<s>/pending`.
  - `say`, undo and close wait while a commit is pending.
- [ ] T030 [US3] Undo and the base (R-5).
  - `GET …/undo?turn=k` returns what the confirmation names: shared commits, the engineer's edits, changes made elsewhere, and whether the base moves.
  - `POST …/undo` follows: `undo_started`, then `refs/fallow/solo/<id>/undone/<k>`, then `reset --hard` without hooks and with no LFS download, then `turns_undone`.
  - `confirmed` is required when the undo goes back more than the last turn, past the engineer's edits, or over a turn that merged or pushed elsewhere.
  - At the end of each turn, `base_moved` is detected for a merge and for a rebase or fast-forward onto the starting branch, as R-5 decides.
  - `start_moved` is computed here from the starting branch (`rev-list --count base.gates..S`) when the session is read. The ref watcher (T035) refreshes it (analyze F7).
  - `solo::reconcile` gains `undo_started` (analyze F6).
- [ ] T031 [US3] Changes and files.
  - `GET /api/sessions/<s>/changes`: files against `base.mark`, with their turns and who changed them.
  - FR-020's details (analyze C6): folders carry the number of changes inside; an "only changed files" filter; a changed file opens with a choice between its changes and the whole file, naming the turns that changed it and by whom. Deleted files are listed in Changes only.
  - In `src/studio/files.rs`: the `solo` version kind, `who_of` for session subjects ("assistant, turn n" or "you"), the version list per open session, and `POST /api/files/open` setting `editor_during_turn` while a turn works.
- [ ] T032 [US3] Gates on a session, `solo::run_gates` (R-12).
  - Refused during a turn.
  - Settle first, then a worktree `-gates-<ms>` at the head, without hooks or LFS download.
  - The gates are read from `HEAD:<spec_root>/.fallow/squad.toml`, never the session's copy.
  - A baseline at `base.gates`, cached per commit.
  - `gates::classify`, then `gates_run`, then the worktree is removed.
  - `POST /api/sessions/<s>/gates`.
  - The view's label: `not_run`, `passed`, `failed`, `stale` or `no_gates`.
- [ ] T033 [US3] Build the page.
  - The rail's Changes and Gates panels, with neutral colors for a pass and the label's exact words (FR-022).
  - Undo on each turn, with its confirmation, and undone turns marked.
  - The pending-commit notice with its two ways on.
  - The start branch moved notice.
  - The notice "Undo and Changes cover only this session's working copy: what a command wrote elsewhere is not seen or undone", on the Changes panel and in every undo confirmation (analyze C1).
  - In the Files tab: change counts on folders, the "only changed" filter, and the changes/whole-file choice (analyze C6).
  - A file opening in the middle with the chat beside it, in `project.js`, `solo.js` and `files.js`; the same layout for a feature's spec session, in `spec.js` (FR-029).
  - Screenshots `checks/solo-changes.png`, `solo-gates.png`, `solo-undo.png` and `spec-file-beside.png`.

**Checkpoint**: Q3 passes.

---

## Phase 7: User Story 4 - Git by asking, every change shown (Priority: P2)

**Goal**: merges, pushes and pull requests happen only through commands the engineer approves; every
change to the project's branches shows, with a proven source.

**Independent Test**: quickstart Q4 and Q5.

- [ ] T034 [US4] Write `tests/us12_git.rs` (Q4 and Q5). It fails first.
  - A squash merge (`git -C <folder> merge --squash … && git -C <folder> commit …`) is one approval. The card shows the snapshot count, "squashes: yes", the folder's branch, uncommitted changes and `.fallow/` changes. After it, `main` moved by 1 with the session named as the source, `squashed` is recorded, the base moved, and `folder_state` is `changed` (SC-010).
  - The base mode: `npm test` approved; `ls`, `npm test; git push --force` and `npm test > ../f` are asked.
  - Under "until I switch back": every case of Q5 step 1 is asked, and `npm test` and `git status` run.
  - "Always" for `git push origin x` is refused.
  - A commit made in the folder from a "terminal" shows as "outside Fallow" within 5 s (SC-004), including one made while an approval card waits.
  - A push to the bare remote shows `refs/remotes/origin/…` created, from the session.
  - A loop and a spec session at the same time: none of their changes shows as "outside Fallow" or as the session's, and they go in the quiet group.
  - A `.claude/settings.json` allowing `Bash(git:*)` and `Bash(rm:*)` in the copy: `git push --force` and `rm -rf <folder>` are still asked. The fake's argv log shows the settings flags.
  - Only the session's branch changed when nothing about git was asked.
  - A non-merge command that changes the folder (`git -C <folder> reset --hard`) carries the folder's branch and uncommitted changes in its request (analyze C2).
  - A merge that conflicts in the folder shows `folder_state: conflicted` in the session and on the project (analyze C12).
  - After a restart, "until I switch back" is off (Q5 step 7; analyze C11).
- [ ] T035 [US4] The ref watcher, in a new `src/studio/refwatch.rs` and `src/run/engine.rs` (R-10).
  - A thread per project, every 2 s: `ref_snapshot`, the folder's `HEAD` and status.
  - The diff gives `created`, `deleted`, `commits(n)`, `rewound` or `rewritten`.
  - Sources come from records only: a session, a loop or a spec session, by commits their logs hold; otherwise "outside".
  - Changes of Fallow's own work on its own branches are marked `quiet`.
  - `<data>/projects/<p>/git.jsonl`, capped at 1,000, and `git-seen.json`, so changes made while stopped show after a restart.
  - A session's branch or working copy missing marks it `gone`.
  - Unit tests for the diff and the source rules.
- [ ] T036 [US4] Commands and what they moved (R-10, R-11, R-6).
  - The opening snapshot is taken in `tool_call` when it answers "allow", and the closing one at `tool_result`.
  - Attribution is per ref, by the parsed command's target: the refspec for a push, the folder's branch for `-C <folder> merge/commit`.
  - `git_observed` with first-parent `commits[]`, and `folder_state` (`changed`, `conflicted`, `staged_only` or `unchanged`).
  - Squash recognition by comparing trees, then `squashed` and `base_moved{why: squash}`.
  - Unit test of the tree comparison.
- [ ] T037 [US4] Merge facts, in `judge.rs` and `tool_call`.
  - For a `merge`: the snapshot commits it would bring (`rev-list <target>..<head>`, by subject), "the assistant's own commits: unknown", whether it squashes, the folder's branch and uncommitted changes (`--no-optional-locks`), and the `.fallow/` paths it would change.
  - They go in `approval_asked.merge`.
  - Every request the judge flags as changing the engineer's folder, not only a merge, carries the folder's checked-out branch and uncommitted changes (FR-016; analyze C2).
  - The approval card in `app.js` shows them.
- [ ] T038 [US4] Show the changes: `GET /api/projects/<p>/git-changes` and the long-poll scope `project:<p>`; a "Branches" list on the project page with the quiet group folded; a session's own changes in its conversation. Screenshot `checks/solo-git.png`.

**Checkpoint**: Q4 and Q5 pass.

---

## Phase 8: User Story 7 - From a discussion to a feature (Priority: P2)

**Goal**: the assistant suggests a feature, the engineer confirms a brief, and the lead writes its
spec from it; the project conversation with the lead is retired.

**Independent Test**: quickstart Q8.

- [ ] T039 [US7] Write `tests/us12_feature.rs` (Q8). It fails first.
  - A fake turn outputs `proposed_feature`, and "Make this a feature" shows.
  - Confirming with an edited brief: the feature exists from the engineer's branch, `brief.md` is committed with exactly that text, and the spec session's first turn runs `/speckit-specify` with the brief. The links go both ways (SC-011).
  - Cancelling creates nothing.
  - With a template constitution, the constitution step comes first, then specify on the brief by itself.
  - A taken title is refused.
  - `/feature` asks for a suggestion.
  - A restart between `feature_starting` and `feature_started` is reconciled.
  - Refusals: the folder on no branch (saying why), an empty brief or title; with Spec Kit missing the feature is created and its spec session says specify could not start; a second suggestion of the same discussion shows the feature already started (analyze C10).
  - An earlier project conversation is read-only, and its unstarted proposal opens the same dialog; a proposal already started shows "Started" with its link (FR-037; analyze C10).
  - "Ask about it" from a file puts the reference in the session open last, and a `proposed_pipeline` shows "Open in the designer" (FR-036; analyze C10).
- [ ] T040 [US7] Suggestions and the handover, in `src/studio/solo.rs` and `src/studio/mod.rs` (R-15).
  - `feature_suggested` comes from `proposed_feature`.
  - `/feature` in a session's `/` menu, in `src/studio/commands.rs`, together with `/close`.
  - `GET /api/sessions/<s>/feature-draft`.
  - `POST /api/sessions/<s>/feature` and `POST /api/conversations/<c>/feature` write `feature_starting`, then call `new_feature(…, origin)`.
  - `new_feature` no longer forks from a project conversation, for "+ New feature" too.
  - `brief.md` is committed with `commit_all` as `spec: brief from solo session <name>`.
  - The first turn is `/speckit-specify` with the brief as `$ARGUMENTS`, or the constitution step first and specify after it.
  - `feature_started` is written, `Feature.origin` set, and the spec session's `conversation_created.origin` set.
  - `solo::reconcile` gains `feature_starting` (analyze F6).
- [ ] T041 [US7] Retire the project conversation (FR-035 to FR-038).
  - `POST /api/projects/<p>/conversation` answers 409, and `project_view` no longer has `conversation`.
  - Earlier conversations are listed through `…/sessions` and open read-only; `say` answers 409.
  - Their unstarted proposals offer "Make this a feature".
  - "Ask about it" in `files.js` goes to the session open last, or a draft.
  - The pipeline designer's "Ask the lead" becomes "Ask the assistant", in `pipelines.js`.
  - A `proposed_pipeline` card in a session shows "Open in the designer".
  - The tests from 009 that used the project conversation (`us1_conversation.rs`, `us9_commands.rs`, `us9_files.rs`, `us9_leads.rs`) move to sessions or to spec sessions, with each change noted.
- [ ] T042 [US7] Build the page.
  - The confirmation dialog in the middle of the screen: title, brief and lead picker; refusals shown inline.
  - The session's links to its features, and the spec session's link back (`spec.js`).
  - "Earlier conversations with the lead…".
  - Screenshots `checks/solo-feature.png`.

**Checkpoint**: Q8 passes.

---

## Phase 9: User Story 5 - Everything a coding assistant needs (Priority: P3)

**Goal**: screenshots, messages typed while the assistant works, the project's instruction files,
and web access.

**Independent Test**: quickstart Q6.

- [ ] T043 [US5] Write `tests/us12_extras.rs` (Q6). It fails first.
  - A png attached: `expect_images = 1` passes. With `images = false`, the attachment is refused before sending.
  - A wrong type, or a file over 5 MiB, is refused.
  - Two messages sent during a turn wait, can be edited and removed, and are sent in order. After a restart they wait, with "Send now".
  - `AGENTS.md` is handed on the first turn and again after a change, and the session lists it.
  - A `web` step shows "Fetched <url>".
  - Web access off: the call carries `web: false`. With `web_off = false`, switching it off is refused.
- [ ] T044 [US5] Attachments (R-8).
  - `POST /api/sessions/<s>/attachments`: checked by magic bytes (png, jpeg, gif or webp), up to 5 MiB, at most 4 per message.
  - Stored at `conversations/<s>/attachments/<sha>.<ext>`, and served by `GET …/attachments/<sha>`.
  - `Call.attachments`.
  - Refused when the runtime lacks `images`.
- [ ] T045 [US5] Waiting messages (R-9).
  - `message_queued` while a turn works or no slot is free.
  - `POST /api/sessions/<s>/queue/<id>` edits, removes or sends a message.
  - T014's tick (sending the oldest waiting message when a turn ends normally and a slot is free) gains its exceptions: nothing is sent by itself after a failure, an approval refusal or a restart; each waiting message has "Send now".
- [ ] T046 [US5] Web access and instruction files in the session.
  - `POST /api/sessions/<s>/web` sets `web_set`, refused without `web_off`.
  - The instruction files are re-handed when a sha changed, and shown in the view as `instructions`.
- [ ] T047 [US5] Build the page: paste or attach an image in the composer, waiting messages under the chat, the web switch in the header, and the list of instruction files. Screenshots `checks/solo-extras.png`.

**Checkpoint**: Q6 passes.

---

## Phase 10: User Story 6 - Closing a session (Priority: P3)

**Goal**: closing never loses work that is not merged without the engineer's choice.

**Independent Test**: quickstart Q7.

- [ ] T048 [US6] Write `tests/us12_close.rs` (Q7). It fails first.
  - A merged and clean session closes without a question (SC-010).
  - A merged and clean session with undone turns asks once, naming the undone turns and their commits; confirming closes it as `merged`, with the undone refs gone (constitution II; analyze D2).
  - One with a `.env` in the copy asks, naming it.
  - An unmerged session with an uncommitted change asks. "Keep" commits the change, keeps the branch and names it.
  - "Discard" names the commits and files, then the branch is gone.
  - A failed "keep" commit keeps the copy, and "Remove working copy" follows.
  - `/exit` is sent as a message.
  - Closing during a turn asks to stop it.
  - A closed session reads under "Closed sessions…", and its record can be deleted.
  - The undone refs are gone.
  - A restart in the middle of a close finishes it (SC-007).
- [ ] T049 [US6] Close, in `src/studio/solo.rs` (R-14, R-11).
  - "Merged" means every commit that is not undone is contained in another local or remote-tracking ref, or a `squashed` covers the last turn.
  - `POST /api/sessions/<s>/close`: the dry answer (`stop_first`, or `ask` with commits, files, edits, ignored files and undone turns); a merged and clean session asked only about its undone turns closes with `{confirmed: true}`, then `close_started`, then keep, discard or (when only undone turns were asked about) merged, then `session_closed`.
  - The undone refs are deleted, after the close named them (analyze D2), and the `-gates-*` worktrees pruned, all without hooks.
  - `solo::reconcile` gains `close_started` (analyze F6).
  - `POST …/remove-copy` and `DELETE /api/sessions/<s>`, both confirmed.
  - `records_bytes`.
- [ ] T050 [US6] Build the page: "Close session" and `/close`, the close dialog naming what is lost, and the read-only "Closed sessions…" list with "Remove working copy" and "Delete record". Screenshots `checks/solo-close.png`.

**Checkpoint**: Q7 passes.

---

## Phase 11: Polish & Cross-Cutting Concerns

- [ ] T051 Sessions and 011's loops (quickstart Q10, R-10).
  - **Code**: in `src/run/update.rs`, `MovedCommit.from` becomes `From { Feature | Session{id, name} | Outside }`, a plain string reading as `Feature`. A commit in a session's `git_observed.commits` is from that session. The loop view gains `from_kind`, and `loop.js` shows "from solo session <name>".
  - **Test** in `tests/us12_with_loops.rs`: a session's squash into a ready loop's target is named as the session's on the loop page. 011's rules apply (Free updates by itself, Normal asks), and the session never joins the merge line.
- [ ] T052 [P] Add three benches to `xtask/src/bench.rs`, run in CI (R-17):
  - `solo_start`: from the first send to the turn's start under 5 s, on 10,000 files (SC-001);
  - `solo_four`: 4 scripted sessions at once, under 200 MB resident, with the activity shown within 2 s at p95 (SC-009);
  - `refwatch_latency`: a moved branch shown within 5 s (SC-004).

  Each bench fails the build when its budget is exceeded (constitution VIII; analyze C13).
- [ ] T053 [P] Audits and notes.
  - `tests/no_egress_no_push.rs` still finds no push, fetch or network in Fallow's own code for sessions (FR-018).
  - `specs/005-adaptive-delegation/contracts/task-set.md` gets the note that `setup/settings.local.json` no longer reaches run agents.
  - `specs/009-chat-commands-leads/spec.md` gets a note that the project conversation is retired by 012.
- [ ] T054 Run the whole suite, file by file, and `cargo clippy --all-targets -- -D warnings`. Mark the tasks done, and record any test changed by the retirements (plain folders, the project conversation, settings).
- [ ] T055 Quickstart Q11 by hand with the engineer, on the real `claude`:
  - a session from the start to a squash into the branch;
  - a pasted screenshot;
  - a web lookup;
  - making a feature;
  - the wording of the dropdown, the approval cards and the close and feature dialogs.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: T001 first. **If any of its points fails, stop for the engineer.**
  - T002 and T004 can then run in parallel.
  - T003 comes after T002.
- **Foundational (Phase 2)**: needs Phase 1. It blocks every story.
  - T005, then T006 (both in `claude.rs`), then T007.
  - T008, T010 and T011 can run in parallel with them.
  - T009 after T008.
- **US1 (Phase 3)**: needs Phase 2. T012 first (failing), then T013 → T014 → T015 → T016 → T017 → T018.
- **US2 (Phase 4)**: needs US1. T019, then T020 → T021 → T022.
- **US8 (Phase 5)**: needs Phase 2. It is independent of US1 and US2 in behaviour, but it edits `project/mod.rs`, `studio/mod.rs` and the pages, so it runs after them. T023, then T024 → T025 → T026 → T027.
- **US3 (Phase 6)**: needs US1. T028, then T029 → T030 → T031 → T032 → T033.
- **US4 (Phase 7)**: needs US1 and US3's base (T030). T034, then T035 → T036 → T037 → T038.
- **US7 (Phase 8)**: needs US1. T039, then T040 → T041 → T042.
- **US5 (Phase 9)**: needs US1 and T005. T043, then T044 → T045 → T046 → T047.
- **US6 (Phase 10)**: needs US3 (settle) and US4 (squash recognition). T048, then T049 → T050.
- **Polish (Phase 11)**:
  - T051 after US4.
  - T052 and T053 any time after US1.
  - T054 after all the others.
  - T055 with the engineer, last.

### Within each story

The test file comes first and fails. Then come the records and the module, then the requests and
routes, then the page, then the headless check.

### Parallel opportunities

- **Phase 1**: T002 ∥ T004, after T001.
- **Phase 2**:
  - T008 ∥ T010 ∥ T011, beside the `claude.rs` chain T005 → T006;
  - T007 after T006, because it touches the prompts that T006's tests run.
- **Polish**: T052 ∥ T053.
- **Stories**: they share `src/studio/mod.rs`, `solo.rs` and the pages, so they run one after
  another.

## Implementation Strategy

- **MVP**: Phase 1, Phase 2 and US1. A session edits code on its own branch, each turn committed,
  commands judged. Then US2, for several sessions and restarts.
- **Then**: US8, which retires plain folders and is the riskiest change to earlier tests. After it
  come US3 (checking and undo), US4 (git by asking, the watcher), US7 (features), US5 and US6.
- Each phase is committed and pushed on `001-llm-aware-tui-editor` after its reviewer approves it.
