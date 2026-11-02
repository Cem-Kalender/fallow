# Tasks: Chat commands and lead choice

**Input**: Design documents from `specs/009-chat-commands-leads/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: requested, as in 007 and 008. Every behaviour is tested with the fake runtime. The page is checked with headless Chrome over CDP on a demo service, and the file views are compared with `mockup/files.html`.

**Organization**: one phase per user story, in the spec's order:
- US1, the `/` menu (P1)
- US2, the lead (P1)
- US3, the indicator (P2)
- US4, unsaved changes (P2)
- US5, the files (P3)

After every phase, an independent fresh-context reviewer verifies it, and the implementer fixes the confirmed findings before the next phase. That is done outside this list.

**Build rules for this machine**:
- Debug builds only.
- `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, `-j 2`.
- Tests with `--test-threads=2` and `CARGO_TARGET_DIR=target/wip`, so the engineer's running Fallow is not locked.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1–US5)

---

## Phase 1: Setup

**Purpose**: test support and the git building blocks that several stories use.

- [X] T001 [P] Add test helpers to `tests/common/mod.rs`:
  - `second_lead(repo, name, runtime, model, marker)` writes `.fallow/agents/<name>.md` as a copy of the shipped `lead` definition, with `kind: lead` and `returns: reply` kept. It changes the name, runtime and model, and puts `marker` in its instructions, so a scenario's `when` can tell which lead was called.
  - `fake_editor(root) -> (command, log)` writes a small program that appends its arguments to `log`: a `.cmd` file on Windows, a shell script elsewhere.
- [X] T002 [P] Add git helpers to `src/verify/git.rs`, each with a unit test in its test module (research R-8):
  - `listed(dir) -> Vec<String>`: `ls-files --cached --others --exclude-standard -z`.
  - `name_status(dir, base) -> Vec<(char, String)>`: the working tree against `base` with `diff --name-status --no-renames -z`, plus untracked files from `ls-files --others --exclude-standard` as `A`.
  - `last_change(dir, base, path) -> Option<(commit, subject, body, unix_time)>`: `log -1 --format=%H%x1f%s%x1f%b%x1f%ct <base>..HEAD -- <path>`.
  - `added_ranges(dir, base, path) -> Vec<(u32, u32)>`: from the `+c,n` of `diff -U0 <base> -- <path>`.
  - `diff_worktree(dir, base, path) -> String`: the unified diff of the working tree against `base` for one path.

---

## Phase 2: Foundational

**Purpose**: the session-lead fields every lead and command change builds on. ⚠️ Must be done before US1 and US2.

- [X] T003 In `src/conversation/events.rs`:
  - add `#[serde(default)] lead: Option<String>` to `ConversationCreated`;
  - add the new event `LeadSet { lead: String }`;
  - add `#[serde(default)] runtime: Option<String>, model: Option<String>, new_session: bool` to `TurnStarted` (`new_session` is true for `Session::New`; its `session` string alone cannot tell a new session from a resume or fork);
  - add `lead`, `lead_runtime` and `lead_model` to `State`, with the fold rules of data-model.md:
    - `lead` is the last `LeadSet`, else the value from creation;
    - `lead_runtime`/`lead_model` are set by the `TurnStarted` with `new_session: true`;
    - they are cleared when a withdrawn turn leaves the conversation without a session;
  - add `State::lead_fixed()`: a `TurnStarted` exists that was not withdrawn (answered, answering or failed).

  Unit tests: an old log without the fields folds to `lead: None`; `LeadSet` after creation; runtime and model kept across resumed turns and replaced by a new session; `lead_fixed` true while the first turn is busy and after a failed first turn, false after a withdrawn one.
- [X] T004 Add `#[serde(default)] lead: Option<String>` to `Project` in `src/project/mod.rs`. Give `new_conv` in `src/studio/mod.rs` a `lead: Option<String>` parameter that it records in `ConversationCreated` and in the `State` it builds by hand (lines ~385-408). Update its callers: `project_conversation` passes the project's default lead. Give `new_feature` a `lead: Option<String>` parameter (default: the project's default lead) that it passes on; `Req::NewFeature` passes `None` until T015.
- [X] T005 Rewrite `patch_project` in `src/studio/mod.rs` so that every key is checked before any is applied (today an error on a later key leaves earlier ones changed in memory). Add the `lead` key: it must name an agent of kind Lead in `Library::load(config, Some(&p.path))`, whose runtime `agent::registry::runtime` accepts. Otherwise the error is "there is no lead <name> for this project", or "<name>'s runtime is not available: <reason>". Add `lead: {name, problem?}` to `project_view` in `src/run/engine.rs`. `problem` is set when the stored name no longer resolves to an available Lead, in which case new sessions use `lead`.

**Checkpoint**: the build and every existing test pass; nothing visible has changed yet.

---

## Phase 3: User Story 1 - Commands from the keyboard (Priority: P1) 🎯 MVP

**Goal**: the `/` menu, with Spec Kit steps sent as "read and follow this skill file", project commands sent as their text, Fallow actions, and `/specify` in the project conversation.

**Independent Test**: in a spec session, `/pl` then Enter twice sends `/plan`, and the fake lead's scenario only answers when the prompt names `.claude/skills/speckit-plan/SKILL.md`. `/review-copy x` sends the command's text with `x` in place of `$ARGUMENTS`. `/nothing` is plain text.

### Tests for User Story 1

- [X] T006 [P] [US1] Write `tests/us9_commands.rs`, with scenarios whose `when` matches the prompt text:
  - The catalogue (`Req::Commands`):
    - a spec session lists `constitution, specify, clarify, plan, tasks, analyze, checklist, converge` with the SKILL.md `description` and `argument-hint`, and not `implement` or `taskstoissues`;
    - the project conversation lists only `specify`;
    - a step whose SKILL.md is missing from the working copy is `available: false` with a reason.
  - Project commands from each place in research R-2's table (`.claude/commands/frontend/review.md` named `frontend:review`, a non-`speckit` skill, `.gemini/commands/x.toml` with `prompt`, `.github/prompts/y.prompt.md`, `.cursor/commands/z.md`):
    - each has its `source` and `runtime`;
    - `speckit-*` skills are not listed twice;
    - a 65 KiB file is `available: false`;
    - two commands of the same name are both listed, and the first in R-2's order is the one run.
  - `say "/plan keep it small"`:
    - the turn's `said` is `/plan keep it small`;
    - the lead's prompt names `.claude/skills/speckit-plan/SKILL.md` and quotes "keep it small";
    - the scenario step's `when` matches only that prompt.
  - A project command:
    - its body, with the input in place of `$ARGUMENTS` (and `{{args}}` for the Gemini one), is in the prompt;
    - a command without a placeholder gets the input under "The engineer's input:";
    - its frontmatter's `allowed-tools` line is not in the prompt.
  - `/nothing here` is sent as plain text (the prompt has no command block).
  - In the project conversation:
    - `say "/specify A receipts page"` answers `{feature, conversation}`, the feature exists, and the new spec session's first turn is `/specify A receipts page` with the step prompt;
    - `say "/specify"` with no title is refused with "Give the feature a name";
    - the new feature's session starts with the project's default lead.
  - SC-002 (CI): the catalogue of a project with 200 command files is answered within 100 ms.

### Implementation for User Story 1

- [X] T007 [P] [US1] Create `src/studio/commands.rs`, with unit tests, per research R-1 and R-2 and data-model.md "Command":
  - `catalogue(project_path, workdir, kind) -> Vec<Command>`:
    - Spec Kit steps from `workdir/.claude/skills/speckit-<step>/SKILL.md`, for the eight steps of R-1, or only `specify` when `kind` is `Project`;
    - then the project's commands from `project_path`, in R-2's order;
    - frontmatter `description` and `argument-hint`, or the first non-empty line;
    - "over 64 KiB" makes a command unavailable.
  - `resolve(catalogue, text) -> Option<(Command, input)>`: the first word `/<name>`; the first in the list wins.
  - `instructions(cmd, input) -> String`: the body (TOML `prompt`), with `$ARGUMENTS` and `{{args}}` replaced, or the input appended under "The engineer's input:". Frontmatter is never included.
- [X] T008 [P] [US1] In `src/agent/prompts.rs`, add `command: Option<CommandUse>` to `LeadTurn`. `CommandUse` is either a `Step { step, path, input }` or a `Project { name, source, instructions }`. Write the two blocks of contracts/web-api.md "Commands" before "The engineer says:", with the input and the instructions quoted with `> `. Unit tests on the exact text.
- [X] T009 [US1] In `src/studio/mod.rs` `say()`:
  - Compute the catalogue for the conversation; when `resolve` finds a command, fill `LeadTurn.command`.
  - Keep `said` as typed.
  - In a project conversation, `/specify <title>` calls `new_feature(project, title, None)` (the project's default lead, T004), then `say(new_conv, "/specify <title>")`, and answers `{feature, conversation}`; an empty title gives "Give the feature a name: /specify <name>".
  - Add `Req::Commands { conv }` and handle it in `src/run/engine.rs`.
  - Add the route `GET /api/conversations/<c>/commands` in `src/service/http.rs` `studio_route`.
- [X] T010 [US1] Build the menu in `src/service/web/app.js`, per contracts/page.md "The `/` menu":
  - The `#say` box becomes a combobox while its text matches `^/\S*$`.
  - A `#cmd-menu` listbox shows the groups "Spec Kit steps", "Fallow actions", "Agents" and "Project commands" (`role="group"` with `aria-labelledby`). Options carry ids for `aria-activedescendant`.
  - A `role="status"` line gives the count.
  - Matching is case-insensitive, name prefixes first.
  - Keys:
    - ↑/↓ wrap and skip unavailable options;
    - Tab completes;
    - Enter picks (steps and project commands become `/<name> `; actions and agents act);
    - Escape closes;
    - a click picks.
  - The catalogue comes from `GET …/commands` at each new `/`.
  - Agents come from `/api/agents?project=<p>`, grouped with their kind. Picking one opens `#/agents/<p>/<name>`; a lead's picking behaviour is wired in T019.
  - Fallow actions come from `Fallow.actions(c)`, which screens provide (T011).
  - Add the styles in `src/service/web/app.css` (no inline `style=`; the CSP test must pass).
- [X] T011 [US1] Provide `actions(c)` for the menu:
  - in `src/service/web/screens/spec.js`: Lock the spec (unavailable with the stepper's "Not ready to lock: …" reason), Open spec.md / plan.md / tasks.md (unavailable before the file exists), Go to the loop (unavailable without a loop);
  - in `src/service/web/screens/project.js`: New feature (focus `#feature-title`), and Lock unavailable with "Open a feature's spec session to lock it";
  - shared in app.js: Cancel the answer (unavailable when not busy), Expand / Narrow the chat, and Files (opens the project page's Files tab, or the drawer in a spec session, once US5 is built; until then it is listed unavailable with "Coming with the file views").

  Each calls the same handler as its button, including its confirmations. In project.js, when `say` answers `{feature, conversation}`, open `#/p/<p>/f/<feature>`.
- [X] T012 [US1] Check the menu with headless Chrome on the demo service (the scenario gets steps whose `when` matches the step prompts). Test by keyboard only:
  - `/pl` Enter Enter sends `/plan`;
  - Escape keeps the text;
  - Lock is greyed out with its reason;
  - `aria-activedescendant` follows the highlight;
  - the status line counts;
  - `/specify A receipts page` from the project conversation opens the new session.

  Fix what fails.

**Checkpoint**: US1 works with the agent named `lead`; `cargo test --test us9_commands` passes.

---

## Phase 4: User Story 2 - Choosing the lead (Priority: P1)

**Goal**: a default lead per project, a pick before the first message, a fixed lead afterwards, a new agent session when its runtime or model changed, and the loop's questions answered by the session's lead.

**Independent Test**: with `careful-lead` (on the fake runtime, with its own marker) as the project's lead, a new feature's turns match only scenario steps with careful-lead's marker. A loop question from that feature is answered by careful-lead, with `by: "careful-lead"`.

### Tests for User Story 2

- [X] T013 [P] [US2] Write `tests/us9_leads.rs`, using T001's `second_lead`:
  - `PatchProject {lead}`:
    - refused for an unknown name and for a non-lead agent;
    - refused for a lead whose runtime is not installed;
    - a patch with a valid `lead` and an invalid `autonomy` changes nothing.
  - A new feature starts with the project's lead (`v.lead.name`), and its prompt has that lead's marker.
  - `POST …/lead`:
    - accepted before the first message;
    - refused after it, with "A session keeps its lead";
    - refused while the first message is being answered (busy, "Wait until the lead has answered", and fixed);
    - refused after a first turn that failed;
    - still accepted after a first turn that was cancelled and withdrawn without a session.
  - Fork rule:
    - the spec session forks the project conversation's agent session when both use `lead` (`session` starts with the parent's);
    - it starts a new one when its lead differs.
  - Changing careful-lead's `model` between turns:
    - `v.lead.changed` is true;
    - the next turn starts a new session (its `session` is not the previous one);
    - `lead_model` is updated.
  - Deleting careful-lead:
    - `v.lead.problem` says it is gone;
    - `say` is refused with it;
    - the project's `lead.problem` is set, and a new feature uses `lead`.
  - A conversation log written without the new fields reads `lead.name = "lead"`.
  - After a restart (shutdown and `start_studio` again), the session's lead is kept.
  - A locked feature whose session used careful-lead: a builder's question that the lead answers (`lead_writes_calc`/`locked_calc` helpers) is answered by a scenario step with careful-lead's marker, and the loop view's question has `by: "careful-lead"`, `by_lead: true`. The engineer can overrule that answer (questions' overrule request succeeds). After careful-lead's model is changed, the next loop question starts a new agent session instead of forking the spec session's.
  - SC-006 (CI): with a scenario step that sleeps (`sleep_ms`), a `Req::Conversation` sent right after `say` returns shows `busy: true`.

### Implementation for User Story 2

- [X] T014 [US2] In `src/studio/mod.rs` `say()`:
  - Look up `state.lead.unwrap_or("lead")` instead of `"lead"`, and refuse with the problem when it is gone, not kind Lead, or its runtime is not available (FR-019).
  - Record `runtime`, `model` and `new_session` on `TurnStarted`.
  - Start `Session::New` when the definition's runtime or model differs from `lead_runtime`/`lead_model` (FR-019a).
  - Fork the parent's session only when the parent conversation's lead, runtime and model are the same; otherwise start a new session.
  - The "first message" test counts only turns that were not withdrawn.
  - Label the lead's messages with the session's lead name (`messages()` in `src/service/web/app.js` uses `c.lead.name` instead of the word "lead").
- [X] T015 [US2] Add `Req::SetLead { conv, lead }`, which appends `LeadSet` when the lead is not fixed (`State::lead_fixed()`), the conversation is not busy, and the lead is an available Lead. Otherwise refuse with "A session keeps its lead; start a new feature or session to use another", "Wait until the lead has answered", or the availability reason. Route `POST /api/conversations/<c>/lead`. Give `Req::NewFeature` an optional `lead`, taken from the body of `POST /api/projects/<p>/features`. Files: `src/studio/mod.rs`, `src/run/engine.rs`, `src/service/http.rs`.
- [X] T016 [US2] In `src/studio/conv.rs` `view()`, add:
  - `lead: {name, runtime, model, fixed, changed, problem?}`;
  - `leads: [{name, runtime, model, available, reason?}]` for every Lead agent of the project's library, with availability from `agent::registry::runtime`.

  Rules as in data-model.md "Derived in the view".
- [X] T017 [US2] In `src/run/questions.rs`:
  - add `agent: String` to `Lead`;
  - fill it in `Studio::lead_of` from the feature's spec session (`src/studio/mod.rs`); `lead_of` gives `session: None` when the lead's definition no longer runs on the session's `lead_runtime`/`lead_model` (research R-12);
  - look up `lib.agent(&lead.agent)` in `start_lead`. When it is gone, the question goes to the engineer with the reason, as when the lead cannot answer today;
  - record the answer's `by` as the agent's name, and store `#[serde(default)] by_lead: bool` with the question entry (entries before 009 read it from `by == "lead"`);
  - the overrule check (~line 896, `entry.by == Some("lead")`) uses `by_lead`, so the engineer can overrule any lead's answer (constitution II).

  In `src/run/activity.rs`, replace the `by == "lead"` checks (lines ~220-234, ~733) with `by_lead`. In `src/service/web/screens/loop.js`, show "<name> answered" for `by_lead` (replacing `q.by === "lead"`, ~line 466), and replace `e.agent === "lead"` (~line 274) with the feed entry's lead flag.
- [X] T018 [P] [US2] On the Agents tab (`src/service/web/screens/agents.js`), give a Lead agent:
  - "Use for this project" (`PATCH /api/projects/<p>` `{lead}`), disabled with the reason when its runtime is not available;
  - a "this project's lead" pill.

  Show the project's `lead.problem` on the project page.
- [X] T019 [US2] Show the lead in the chat heads of `src/service/web/screens/project.js` and `spec.js`, through a shared `leadHead(c)` in `app.js`: "Lead: <name> · <runtime> · <model>".
  - Before the session's lead is fixed, the name is a button opening a picker: available leads can be chosen, unavailable ones are greyed out with the reason. Choosing calls `POST …/lead`.
  - After it is fixed, the name is plain text, titled "A session keeps its lead".
  - `lead.changed` shows its notice above the box; `lead.problem` shows the reason and disables the box.
  - "+ New feature" gets a lead select, defaulting to the project's lead.
  - In the menu (T010), picking a lead before the lead is fixed sets it, and afterwards opens it in Agents. "Choose the lead" in Fallow actions opens the picker (unavailable when fixed).
- [X] T020 [US2] Check with headless Chrome on the demo service with a second lead:
  - the head, the picker, the refusal after the first message, the changed-model notice;
  - "Use for this project";
  - "careful-lead answered" in a loop's feed.

  Fix what fails.

**Checkpoint**: `cargo test --test us9_leads --test us9_commands` and 007/008's conversation and loop tests pass.

---

## Phase 5: User Story 3 - A chat that shows it is working (Priority: P2)

**Goal**: a moving indicator that survives re-renders, and stays still under reduced motion.

**Independent Test**: while a fake turn sleeps, the `#working` element is visible, and it is the same DOM node across three live updates. Under emulated `prefers-reduced-motion: reduce`, its computed `animation-name` is `none`.

- [X] T021 [US3] In `src/service/web/app.js`:
  - wrap the messages in `#chat-list` inside `#chat`, and re-render only `#chat-list` in `project.js` and `spec.js`;
  - add `#working` (`role="status"`: three `span.dot` and "The lead is answering…") after `#chat-list`;
  - show it in `sendSay` as soon as the message is sent (before the request returns), hide it again if the send is refused, and otherwise show or hide it in `syncComposer` from `c.busy`;
  - remove the `.typing` span from `messages()`, keeping "working" in the meta line.

  In `src/service/web/app.css`, add the dots, with the animation only under `@media (prefers-reduced-motion: no-preference)` (research R-6). Scroll-to-end keeps `#working` in view.
- [X] T022 [US3] Check with headless Chrome:
  - shown before the `say` request returns (hold the request back with CDP `Fetch` interception);
  - node identity across updates;
  - the animation under no preference, none under reduce (`Emulation.setEmulatedMedia`);
  - hidden after the answer and after a cancel.

  Fix what fails.

---

## Phase 6: User Story 4 - Unsaved work is never lost silently (Priority: P2)

**Goal**: one guard in the router for every way out of an unsaved agent, pipeline or document, plus the browser's prompt on close or reload.

**Independent Test**: with an agent changed, every route of quickstart Q4 asks. Staying keeps the address, the selection and the text; leaving goes where the engineer asked.

- [X] T023 [US4] In `src/service/web/app.js`:
  - keep `lastHash` at every shown address;
  - on `hashchange`, when the current screen's `dirty?.()` is true and the new route leaves the screen (another screen, or a project the screen's `update()` would not take), put the old address back without rewriting the history: for a link or a `location.hash` write with `history.replaceState(lastHash)`, for Back or Forward with `history.go(+1)` or `history.go(-1)` (the router keeps a position counter in `history.state` to tell which), ignoring the `hashchange` this causes; then `confirm({title: "Leave your changes?", …, ok: "Leave them"})`. On yes, mark the screen as leaving and set `location.hash` to the asked address; on no, stay.
  - A save under way finishes (or fails) before the screen is asked whether it is dirty.
  - Add a `beforeunload` listener that calls `preventDefault()` (and sets `returnValue`) while the current screen is dirty.
  - The screens' own `update()` guards stay for changes within a screen.
- [X] T024 [US4] Expose `dirty()`:
  - `src/service/web/screens/agents.js`: the existing function;
  - `src/service/web/screens/pipelines.js`: `() => s.dirty`;
  - `src/service/web/screens/spec.js`: the editor is open and its text differs from `doc.text`.

  In spec.js, "Cancel" of an edit with changes also asks "Leave your unsaved edit?".
- [X] T025 [US4] Check with headless Chrome, for an unsaved agent, pipeline and spec document:
  - the nav links, a project link, Back, a feature link, and `location.hash` from code all ask;
  - staying keeps the address and the text, and after staying on Back, Back and Forward still lead where they did before (the history is not rewritten);
  - leaving right after Save (while it is under way) does not ask once the save succeeded;
  - the `beforeunload` event is cancelled (through CDP `Page.javascriptDialogOpening`);
  - nothing is asked without changes.

  Fix what fails.

---

## Phase 7: User Story 5 - The project's files, read-only (Priority: P3)

**Goal**: the Files tab on the project page (A) and the Files drawer on the loop page and spec session (B1/B2), read-only, with marks, who changed what in which task, Ask the lead, and Open in my editor.

**Independent Test**: on a fake-runtime loop that changed `shop/cart.py` in T002:
- the feature's version marks it `changed`, with `last.who = "builder"` and `last.task = "T002"`;
- `.env` is `secret` and its content is never sent;
- a path with `..` is refused;
- "Open in my editor" with the fake editor records the file's absolute path.

### Tests for User Story 5

- [X] T026 [P] [US5] Write `tests/us9_files.rs`:
  - Versions:
    - `folder` first;
    - a feature with only a spec session is `kind: spec`;
    - a feature with a loop is `kind: loop` and `running` while it runs;
    - a feature whose worktrees are gone but whose branch exists is `kind: branch`, with entries, marks and files read from the branch;
    - a feature whose copy and branch are both gone is not listed, and asking for it answers "This version is gone".
  - Entries:
    - ignored files are absent (`.gitignore` with `build/`);
    - marks `changed`/`added`/`deleted` against the base, including an uncommitted change in the loop's worktree;
    - files under `specs/<f>/` are `spec: true`;
    - the `fingerprint` changes only when the list or a mark changes, and right after a file is written in the loop's worktree (FR-034, CI);
    - a plain folder has no marks and no `.git`.
  - File:
    - text with `added` ranges;
    - `view=changes` gives a unified diff;
    - a file with a NUL byte is `binary` with its `size`;
    - a 600 KiB file is `cut`;
    - a deleted file shows its last content;
    - `last` names `builder` and the task from the loop's commit (`build T002: …` and "Fallow loop r<id>, step <n>"), and the session's lead name for a spec session's commit.
  - Safety:
    - `../x`, `/etc/x`, `C:x`, `a\\b` and a symlink to outside are all refused with "That path is outside the project";
    - every name on research R-8's secrets list is listed `secret: true`, and its file comes back `kind: secret` with no content;
    - `.env.example` is not secret.
  - Open:
    - without prefs it answers `choose` (the list may be empty on CI) and the file's absolute `path` to copy;
    - with the fake editor as `editor: {program, args: ["--flag"]}` it saves `prefs.json`, and the editor's log holds `--flag` then the absolute path as the last argument;
    - the next open uses it without asking;
    - `POST /api/files/editor {clear: true}` removes it, and the next open asks again;
    - a program that does not exist is refused when saved;
    - a typed command `"C:\Program Files\x\x.exe" -r` splits into the program and `-r`; a command holding `& del x` is not run through a shell (the fake editor receives it as plain arguments or the save is refused, and nothing else runs);
    - a path with `&` is refused for a `.cmd` editor.
  - Read-only (FR-035, SC-009): after every file request of this test file, the repository's `git status --porcelain`, its HEAD and the hashes and modification times of its files are unchanged.
  - `@` references: `say "@shop/cart.py (on 001-cart) why?"` in the project conversation puts the file's text in the prompt (scenario `when`), and `@.env` puts only its name.
  - SC-008: a generated repository of 5,000 files lists in under 2 s.

### Implementation for User Story 5

- [X] T027 [P] [US5] Create `src/studio/files.rs`, with unit tests:
  - `safe_path(root, rel)`: relative, with no `..`, `\`, drive or leading `/`; canonicalized under the canonical root; not a symlink.
  - `is_secret(path)` per research R-8's list.
  - `versions(p, features, runs)`: the `folder` version, then per feature the loop's `wt/build` if it exists, else the spec conversation's workdir, else the branch if it exists (`kind: branch`); a feature with none of these is left out. The base is `git::merge_base` with `feature.base_branch`.
- [X] T028 [US5] In `src/studio/files.rs`, implement `entries(version)` with T002's `listed` and `name_status` (a walk for plain folders, skipping `.git` and `git::CACHES`), with `spec` and `secret` and a `fingerprint` (a hash of paths and marks); a `branch` version uses `git::files_at(branch)` and `git diff --name-status <base> <branch>`, and its files `git::show(branch, path)`. Then implement `file(version, path, view)`:
  - text up to 512 KiB with `cut`;
  - binary when there is a NUL in the first 8 KiB;
  - `secret` without reading the file;
  - deleted files via `git::show(base, path)`;
  - `added` from `added_ranges`, and `changes` from `diff_worktree`;
  - `last`: parse `last_change`'s subject `^(\w+) (T\d+|[\w-]+):` and body `Fallow loop r(\d+), step (\d+)`, then map the step to its agent definition through the run's state (`steps[n].def`); a spec session's commit is the session's lead name. While the loop runs and the file differs from its last commit, `live: true` with the agent at work.
- [X] T029 [US5] In `src/studio/files.rs`, implement the editor (research R-10):
  - `prefs.json` in the data directory, read and written atomically, holding `editor: {program, args}`;
  - `parse_command(text) -> (program, args)`: split on spaces with double quotes grouping, never through a shell; the program must exist (on `PATH` or as a full path) when saved;
  - `found_editors()`: R-10's names on `PATH`, plus their usual install folders on Windows (`%LOCALAPPDATA%\Programs\Microsoft VS Code\bin\code.cmd` and similar);
  - `open(path, editor)`: starts the program directly with `args` and then the absolute path as the last argument. A `.cmd` or `.bat` launcher runs through `cmd /c` with every argument quoted, and paths or arguments containing `"%^&|<>` are refused there. It never opens the file by its type;
  - `set_editor` / `clear_editor`.
- [X] T030 [US5] Add `Req::FileVersions`, `Req::Files`, `Req::File` and `Req::OpenFile` in `src/studio/mod.rs`, handle them in `src/run/engine.rs`, and route them in `src/service/http.rs` per contracts/web-api.md "Files":
  - `GET /api/projects/<p>/files/versions`
  - `GET /api/projects/<p>/files?version=`
  - `GET /api/projects/<p>/file?version=&path=&view=`
  - `POST /api/files/open`
  - `GET` and `POST /api/files/editor`
- [X] T031 [US5] Implement `@` references in `say()`:
  - find `@<path>` tokens, with an optional `(on <feature>)` after them;
  - read each through `files::file` (secret: name only; over 64 KiB: the beginning, marked as cut);
  - pass them as `LeadTurn.refs`.

  In `src/agent/prompts.rs`, write the "The engineer refers to these files:" block (contracts/web-api.md), with unit tests.
- [X] T032 [US5] Create `src/service/web/screens/files.js`: a component `Fallow.files.mount(el, {project, version, mode})` with:
  - the version picker, the filter and "Only changed";
  - the tree (`role="tree"`, ↑↓→← and Enter) with the marks M/A/D, "spec" and a lock;
  - the file view: a header with the mark and "by <who> in <task>", Changes / Whole file, line numbers with added lines highlighted, and nothing editable;
  - a refresh every 3 s while the document is visible, applied only when `fingerprint` changes and keeping the open file, the folded folders and the scroll position;
  - a version that is gone: the open file says so and offers the project folder's version;
  - "Editor: <program> · change" once an editor is saved, which opens the same choice as the first time, with "Remove".

  Add it to `SCREENS` in `src/service/http.rs` and to `src/service/web/index.html`, and add its styles to `app.css`, following `mockup/files.html`.
- [X] T033 [US5] In `src/service/web/screens/project.js`:
  - the left column's tabs Features / Files (`role="tablist"`);
  - an open file takes the chat column's place, with "Changes vs <base>", "Ask the lead about it" (back to the chat with `@<path> (on <feature>) ` in the box), "Open in my editor" and "← Conversation";
  - the route `#/p/<p>/files?v=<version>&f=<path>` in `app.js` `route()`.

  "Open in my editor" posts `/api/files/open`. On `choose`, it asks with `confirm()`: the found editors as choices, or an input for a command. It then posts again with the chosen `editor`, or with `command` (the text the engineer typed, which the server splits). Declining shows the file's absolute `path`, selected, to copy. A secret file's view offers only that button.
- [X] T034 [US5] Add the Files drawer:
  - in `src/service/web/screens/loop.js`: a "Files" button in the header;
    - B1: a narrow drawer in place of the side column, with "Changed · n" / "All files" and "Open full";
    - B2: opening a file widens it over the agent cards and the feed, keeping the header and the task tree, with "← Back to the loop";
  - in `src/service/web/screens/spec.js`: the same drawer next to the documents, widening over them.

  "Ask the lead" from the loop page goes to the project conversation (clarify answer 2); from the spec session's drawer it goes into that spec session's own message box (decided at analyze). The loop page's `render()` keeps the drawer's state across live updates.
- [X] T035 [US5] Check with headless Chrome on the demo service with a running loop:
  - the tab, versions, marks, filter, "Only changed", the open file and its live refresh (every 3 s, only while visible);
  - a secret's view;
  - "Ask the lead" prefill;
  - "Open in my editor" with the fake editor;
  - B1 and B2 on the loop page and the drawer in the spec session;
  - the keyboard in the tree;
  - no file view holds an editable element (no `textarea`, `input` other than the filter, or `contenteditable`);
  - from a loop page, a changed file's content is reached in at most 2 clicks (SC-007): Files, then the file.

  Fix what fails.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T036 Adjust 007/008 tests that assert `by == "lead"`, the conversation view's keys, or the chat's `.typing` text, and run the whole suite (`cargo test --test-threads=2`). Every test passes.
- [X] T037 [P] Update `specs/007-fallow-studio/contracts/web-api.md` with rows pointing to 009's contracts for the new and changed endpoints.
- [X] T038 Visual check:
  - the project page's Files tab and the loop page's drawer (B1 and B2) against `mockup/files.html`, and the menu, lead head and indicator for consistency with 008's look;
  - at 1440 px and 390 px, nothing scrolls sideways at 390 px;
  - screenshots in `specs/009-chat-commands-leads/checks/`.

  Fix what differs.
- [X] T039 Whole-change review by an independent fresh-context reviewer against the spec, plan, contracts and constitution, especially V (secrets, nothing outside the project) and VI (no rights from project commands, the editor never "opens by type"). Fix the confirmed findings and run the suite again.
- [ ] T040 Real-agent validation with `claude` (Sonnet) in `C:\Repos\FallowSandbox`, with the budget agreed with the engineer before starting:
  - quickstart Q1 steps 2 and 5 (a `/plan` and a project command);
  - Q2 steps 1 and 5 (a second lead answering in its session and a loop question);
  - Q5 step 3 (Ask the lead about a file).

  Record what happened, and the cost, in `specs/009-chat-commands-leads/checks/real-run.md`.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: none. T001 and T002 run in parallel.
- **Foundational (Phase 2)**: does not need Phase 1. T003 → T004 → T005 run in order (they share files). It blocks US1 and US2.
- **US1 (Phase 3)**: needs Phase 2. Its lead picking in the menu is wired by T019 (US2).
- **US2 (Phase 4)**: needs Phase 2. It is independent of US1 except for T019's menu wiring, which needs T010.
- **US3 (Phase 5)** and **US4 (Phase 6)**: page only. They can start after Phase 2, or in parallel with US1 and US2, apart from the shared `app.js`; do them one after another to avoid conflicts there.
- **US5 (Phase 7)**: needs T002. T031 needs T008's `LeadTurn` change. Otherwise independent.
- **Polish (Phase 8)**: after the stories.

### Within each story

The test file comes first and fails, then the Rust module, then requests and routes, then the page, then the headless check.

### Parallel opportunities

- T001 ∥ T002.
- T006 ∥ T007 ∥ T008 (US1: a test file and two separate modules).
- T013 ∥ T018 (US2).
- T026 ∥ T027 (US5).
- T037 at any time after the contracts are final.

## Parallel Example: User Story 1

```text
Task: "Write tests/us9_commands.rs (T006)"
Task: "Create src/studio/commands.rs with unit tests (T007)"
Task: "Add LeadTurn.command and its prompt blocks in src/agent/prompts.rs (T008)"
```

## Parallel Example: User Story 5

```text
Task: "Write tests/us9_files.rs (T026)"
Task: "Create src/studio/files.rs: safe_path, is_secret, versions (T027)"
```

## Implementation Strategy

### MVP first

Phases 1–3: the `/` menu with Spec Kit steps and project commands, working with the agent named `lead`. That is the engineer's first remark, and it is usable on its own.

### Incremental delivery

1. US1, the menu. It is reviewed, then committed and pushed.
2. US2, the lead. Reviewed, committed, pushed.
3. US3 and US4, the indicator and the guard: small, reviewed together, then committed.
4. US5, the files. Reviewed, committed, pushed.
5. Polish: the visual check, the whole-change review and the real-agent run.

Each phase ends with its fresh-context review and a commit on `001-llm-aware-tui-editor`. The engineer's running Fallow is restarted only when they ask, and only after checking `agents_running == 0`.
