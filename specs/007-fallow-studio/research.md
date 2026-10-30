# Research: Fallow Studio

Decisions for the plan. Each one has its rationale and the alternatives that were considered. 006's research (R-1 to R-16) still holds where it is not replaced here.

## S-1 One service, one page, five screens

**Decision**: keep 006's service: one process per user, a loopback HTTP server with a secret per launch, the event log, and the engine thread.
- `fallow` with no arguments starts the service if needed and opens the dashboard.
- The page becomes a single-page app with hash routes, one per screen of the mockup:
  - `#/` the dashboard;
  - `#/p/<project>` the project conversation;
  - `#/p/<project>/f/<feature>` the spec session;
  - `#/p/<project>/f/<feature>/loop` the loop;
  - `#/pipelines` the pipeline designer.

**Rationale**: the service, its authentication and its resume already work (006 Phases 1–3, four real runs). One page keeps the navigation of the mockup and loads once.

**Alternatives**:
- A desktop shell (WebView2, Tauri). Rejected: a new runtime and dependencies, and the browser already does the job.
- A separate page per screen. Rejected: the screens share the header, the inbox count and live updates.

## S-2 The page: plain HTML, CSS and JavaScript, built from the mockup

**Decision**: write the screens by hand in plain JavaScript, with no framework and no build step, as 006 did.
- **The look**: the mockup's styles become `app.css`, with the colors as CSS custom properties named after their role (`--ground #F6F6F3`, `--panel`, `--line #E3E2DC`, `--ink`, `--muted`, `--accent #2F52C9`, `--needs #B4470F`/`--needs-bg #FBEDE3`, `--ok #226640`/`--ok-bg #E6F3EA`, `--spec #5B3BB0`/`--spec-bg #F3EEFB`). The mockup's class names are kept: `card`, `pill`, `btn`, `btn2`, `nav`, `stage`, `task`, `ev`, `row`.
- **Rendering**: each screen is a module that renders its panels from JSON with small template functions, and re-renders only the panel whose data changed.
- **The logo** is the mockup's inline SVG.
- **The mockup files are the reference**: `specs/007-fallow-studio/mockup/*.dc.html`. A visual check compares each screen with them at 1440 px and 390 px (SC-007, SC-008).

**Rationale**: the mockup is already HTML and CSS, so porting it is direct and exact. No framework means no build tooling, no dependency to justify against constitution VIII, and a small page.

**Alternatives**: React, Svelte or Preact with a bundler. Rejected: a toolchain and dependencies for five screens, and the risk of drifting from the mockup's markup.

## S-3 Fonts shipped with Fallow

**Decision**:
- **Which files**: Inter (400, 500, 600, 700) and JetBrains Mono (400, 500), as Latin-subset `woff2` files under `assets/fonts/`, with their SIL Open Font License files.
- **How they are served**: embedded in the binary and served by the page's own server (`/fonts/...`), so the CSP stays `default-src 'self'`.

**Rationale**: constitution V says the page loads nothing from outside Fallow, so Google Fonts cannot be used at run time. The OFL allows embedding and redistribution. The six files are about 150 KB in all.

**Alternatives**:
- The system font stack. Rejected: the engineer asked for the mockup's fonts exactly.
- Loading the fonts from Google. Rejected by constitution V.

## S-4 Live updates

**Decision**: keep 006's long polling (`GET /api/events?after=<seq>`), extended to two scopes:
- **The global scope** serves the dashboard and the inbox. It covers project and feature changes, loop progress and items that need the engineer.
- **A scope per loop or conversation** serves its feed.

A request returns as soon as there is something newer than `after`, or after 25 seconds with nothing.

**Rationale**: it already meets the 2-second activity budget in 006's tests, and it works through tiny_http without holding a thread per stream.

**Alternatives**: server-sent events or WebSockets. Rejected for now: tiny_http holds one thread per open response, and long polling is already measured.

## S-5 The lead's conversations are `claude -p` turns

**Decision**: every message the engineer sends to the lead is one agent call, resumed in the conversation's session, as 006's planning turns were.
- **The reply** streams into the chat from the call's stream-json, with partial messages (`--include-partial-messages`).
- **The project conversation** has one session per project.
- **A spec session** starts as a fork of the project conversation (`--resume <project session> --fork-session`), so the lead carries over what was discussed (FR-034). After that, it has its own session.
- **Answers during a loop**: each time the lead answers an agent's question, Fallow forks the spec session again (`--fork-session`). The lead knows everything the engineer said, and the spec session itself is not changed by the loop.
- **The lead's result** is structured: `{reply, step?, documents_changed[], proposed_feature?, proposed_pipeline?, question?}`.

**Verified**: `claude` 2.1.314 has `--fork-session`, `--include-partial-messages` and `--resume`.

**Alternatives**:
- One long-lived interactive process per conversation (stdin streaming). Rejected: it dies with Fallow, a restart loses it, and 006's resumable calls already survive restarts.
- Pasting the conversation into each new call. Rejected: it costs more and loses the agent's own context.

## S-6 Where the lead works

**Decision**:
- **A git project:**
  - Starting a feature creates its branch `NNN-name` from the project's current branch and a worktree for it under Fallow's data directory. The spec session works in that worktree.
  - The engineer's edits from the page are written into that worktree.
  - After each turn, Fallow commits the changed documents (`spec: <step> turn <n>`), and discards changes anywhere else, saying so in the chat.
  - The project conversation works in a worktree checked out at the project branch's head, refreshed before each turn, and discarded changes the same way.
- **A plain folder:** the lead works in the folder itself. Only `specs/`, `.specify/` and `.fallow/` may be written, which the lead's permission rules enforce. Fallow cannot check what a command wrote elsewhere, and says so in the session (constitution I: state what cannot be verified). At the first lock, Fallow offers `git init` with an initial commit of what is there.

**Rationale**: worktrees keep the engineer's own working copy untouched, even in the "Run anything" mode. Committing every turn gives history and a clean base for the lock.

## S-7 Chat modes through permission rules and a prompt tool

**Decision**: the four modes of FR-007a map onto Claude Code's own permission controls:
- **All modes**:
  - reading: `Read`, `Glob`, `Grep`;
  - writing: `Edit`/`Write` only on `specs/**`, `.specify/memory/**` and `.fallow/**`;
  - everything else is denied (`dontAsk`, or prompts routed as below).
- **Read only**: no command rules.
- **Approved commands**: plus `Bash(<cmd>:*)` and the PowerShell mirror for each approved command, as 006 does for the gates and the plan's commands (006 research R-4).
- **Ask**: plus `--permission-prompt-tool mcp__fallow__approve`.
  - A command outside the rules calls Fallow's `approve` tool, which shows the exact command in the chat with "Allow once", "Always in this project" and "Deny", and waits.
  - The tool gives up after 10 minutes and denies.
  - "Always" adds the command's program prefix to the project's approved commands, after the engineer confirms the prefix.
- **Run anything**: plus `Bash` and `PowerShell` with no restriction. Edits outside the spec paths stay denied, and S-6's commit-or-discard still applies after the turn.

**To verify on a real run** (as R-3 did in 006): that `--permission-prompt-tool` is called for an unlisted command under `--print`. The help of 2.1.314 lists `--permission-prompts host` and names the tool option. If it does not work, Ask falls back to ending the turn with the refused command and asking the engineer to allow it, then resuming the turn.

**Rationale**: the rights are enforced by the agent's own permission system, never by asking it to behave (constitution VI). No mode uses `--dangerously-skip-permissions`.

## S-8 What Fallow reads from Spec Kit files

**Decision**: Fallow parses the documents it needs, and the lock refuses documents it cannot read, naming the line (contracts/spec-kit-files.md).
- **`tasks.md`**:
  - phases are `## Phase N: Title`;
  - tasks are `- [ ] T### [P]? [USn]? description`;
  - a phase is a user story's phase when its tasks carry the same `[USn]` label, or its title names `User Story n`;
  - its checkpoint is the end of that phase.
- **`spec.md`**: for each user story (`### User Story n - Title (Priority: Pn)`), its numbered acceptance scenarios. They become the criteria `USn/k`. A scenario whose text ends with `(must fail)` is a must-fail case.
- **Lock checks**:
  - no open clarification markers;
  - analyze done, with no findings marked CRITICAL;
  - every task id unique;
  - every user story phase matching a story in the spec;
  - every user story having at least one must-fail scenario.

  The default lead definition writes these markers itself during clarify and analyze.
- **Progress**: Fallow ticks a task (`- [X]`) in `tasks.md` in the task's own commit, so the repository shows progress the way Spec Kit's own `/speckit-implement` does.

**Rationale**: the loop must know the phases, tasks and criteria to schedule and verify them. Spec Kit's formats are already regular, so a strict reader with clear refusals is enough.

**Alternatives**: a separate machine-readable plan (006's `.fallow/work.toml`). Rejected: it duplicates `tasks.md`, and the engineer locks Spec Kit's own documents.

## S-9 The loop: one step per task, one session per phase

**Decision**: 006's stage machine is reused with a new unit of work:
- **Builder steps**:
  - The builder's step is one task.
  - The first task of a phase starts a new session from the locked documents and the handovers (FR-010a). Each next task of that phase resumes the session with the next task's instruction.
  - After the last task of a phase, a short `handover` step in the same session writes `specs/NNN-name/handover/phase-N.md`.
- **What Fallow does after each task**:
  - it checks the claim, as in 006 R-7, with "owned files" replaced by "not the locked documents";
  - it ticks the task and commits;
  - it runs the gates against the baseline.

  A failing gate becomes a repair of that task in the same session.
- **The pipeline's checkpoint steps** (reviewer, security reviewer, and others) run at the end of a user story's phase, each in a fresh session on the integrated commit. A failing verdict becomes a repair in the phase's builder session, then the checkpoint runs again.
- **Repair caps** (FR-013): 3 per task for gate failures, and 3 review rounds per checkpoint. Both are settable per pipeline. Steps cut short (an interruption or a lost session) do not count, as in 006.
- **The baseline**: the gates run once on the lock commit. After an unlock and a new lock, they run again on the new lock commit.

**Rationale**: a step per task gives Fallow a checkpoint per task (FR-011) without restarting the agent's context. A session per phase keeps the context focused (clarification of 2026-10-30).

## S-10 Questions through the lead (`ask`)

**Decision**: 006's MCP bridge (`fallow mcp-bridge`, the named pipe, tokens checked against the step's Job Object) is reused. The tools are:
- `ask {question, options?}`, for every agent in a loop;
- `approve`, the permission prompt tool, for the lead's chats only;
- `progress {text}`.

When an agent calls `ask`:
1. **Fallow records the question** and starts a lead call, forked from the spec session (S-5). The call gets the question, the locked documents, the autonomy level and the reserved items.
2. **The lead returns one of three things**:
   - `answer`: Normal or Free, and the documents or conversations settle it;
   - `decided`: Free only, a small open question the lead decided itself;
   - `relay`: everything else, and every reserved item, with options and a recommendation.
3. **For `answer` or `decided`**:
   - Fallow commits the answer to `spec.md`'s Clarifications on the feature branch;
   - the tool call returns `{state: "answered", answer, commit}`, and the agent carries on;
   - the commit reaches the build branch at the next task boundary (a merge of the documents-only commit).
4. **For `relay`**:
   - the tool returns `{state: "waiting", question}`;
   - the agent's instruction says to end the step with `status: blocked`;
   - only the phases that depend on that user story wait;
   - when the engineer answers, the answer is committed the same way, and the step resumes in its session with it.
5. **Overruling** commits the reversal. The tasks committed after the overruled answer, in the phases of the affected story, are reopened: their phase's builder session gets a repair with the new answer, and the story's checkpoint runs again.

**Limits**:
- one open question per step;
- the lead's call has a 3-minute limit; if it hits the limit, the question is relayed to the engineer;
- the lead's answers count against the loop's budget.

**Rationale**: it reuses what 006 built and verified (the bridge and its identity checks), and keeps every answer as a commit (constitution III).

## S-11 Agent definitions: markdown with a small frontmatter

**Decision**: the format is in contracts/agent-definition.md.
- **The frontmatter** sits between `---` lines. Fallow reads a strict subset of YAML: `key: value`, `key: [a, b]`, and comments. Anything else is refused with the line number.
- **The body** is the agent's standing instruction, given as its system prompt addition.
- **Where definitions come from**: the defaults ship inside Fallow, are copied to `~/.fallow/agents/` on first start, and are never overwritten after that. An update that changes a default writes it as `<name>.md.new` and tells the engineer. A project's `.fallow/agents/<name>.md` overrides by name.
- **The fields**: `name`, `description`, `runtime`, `model`, `kind` (maker or checker), `may_run`, `returns`, `limits`.
  - `may_run` lists commands, plus the words `gates` and `plan.commands`.
  - `returns` must match the kind: `work` for a maker, `verdict` for a checker. The lead returns `reply`.
- **The lead** is a definition too, with kind `lead`. Its rights come from the chat mode (S-7), not from `may_run`.

**Rationale**: it is close to Claude Code's subagent files, which the engineer already knows. A strict subset needs no YAML dependency and gives exact errors.

**Alternatives**:
- TOML frontmatter. Rejected: further from what the engineer writes elsewhere.
- A full YAML crate. Rejected: a dependency for six fields.

## S-12 Pipelines: TOML files from fixed building blocks

**Decision**: the format is in contracts/pipeline.md.
- **Where**: `~/.fallow/pipelines/<name>.toml`, overridable by `.fallow/pipelines/<name>.toml` in a project. The defaults are Standard, Test first and Solo.
- **Steps**: each `[[step]]` names:
  - `agent`, or the built-in `fallow:gates`;
  - `when`: `task`, `phase`, `checkpoint`, or `once` with `at = "start"` or `"end"`;
  - optionally `paths`, globs the changes must touch;
  - `on_fail`: `repair` or `engineer`;
  - optionally `cap`.
- **A pipeline is valid when**:
  - exactly one maker step is `when = "task"` (the builder);
  - every agent it names exists;
  - checkers have `on_fail` set.
- **What Fallow derives from it**: whether the result can be "verified". That needs `fallow:gates` per task, and at least one checker at every checkpoint. Otherwise the result is "Done, not verified by Fallow", listing what is missing (FR-015).
- **A pipeline the lead proposes** arrives as a TOML draft in the chat. The designer opens it, and only the engineer's Save writes it (FR-028).

**Rationale**: a fixed vocabulary that the stage machine can schedule, and that the designer can draw as the mockup does.

## S-13 One runtime interface

**Decision**: 006's `agent::Call` and `agent::claude` become a `Runtime` trait (contracts/runtime.md).
- `start(call) -> Stream` and `resume(session, call) -> Stream`;
- a stream yields events: `message`, `message_delta`, `tool_call {kind, …}`, `tool_result`, `permission_request`, `result {value, cost_usd, session}`, and `failed {why}`;
- `available() -> {installed, version, models}`.

Activity on the page is built from these events, never from a provider's raw output. Claude Code is the first runtime. The fake agent becomes a second runtime (`fake`), so tests exercise the interface itself (US7's independent test).

**Rationale**: everything above the trait (the loop, the feed, the checks) stays provider-agnostic (FR-029).

## S-14 Desktop notifications from the service

**Decision**: on Windows, the service adds a notification-area icon (`Shell_NotifyIconW`, through `windows-sys`'s `Win32_UI_Shell` and `Win32_UI_WindowsAndMessaging`, on a hidden message-only window) and shows a balloon for each new "Needs you" item. Windows 10 and 11 show it as a toast.
- **Clicking it** opens the item's page in the default browser.
- **The icon's menu** has "Open Fallow", "Notifications on/off" and "Stop Fallow".
- **What a notification says**: the project, the feature and what is needed, never code or output.
- **Each item raises one notification.**
- **Other platforms**: a no-op behind `platform::notify` for now.

**Rationale**: it needs no new crate (only `windows-sys` features), no network, and works when the page is closed (FR-003a).

**Alternatives**:
- The browser's Notification API. Rejected as the only way: it needs the page open.
- WinRT toasts through the `windows` crate. Rejected: a large dependency.
- Running PowerShell for each notification. Rejected: a process per notification, and it is fragile.

## S-15 Spec Kit shipped with Fallow

**Decision**: Fallow embeds Spec Kit 1.0.12, the same files this repository was initialised with:
- `.specify/scripts/bash/*`, `.specify/templates/*` and `.specify/memory/constitution.md` (the template);
- `.claude/skills/speckit-*/SKILL.md`;
- `.specify/init-options.json`.

Adding a project copies each of them only if it does not exist. If the project has Spec Kit at another version, Fallow says so and copies nothing over it. Spec Kit's license (MIT) ships with the files.

`xtask spec-kit` refreshes the embedded copy from a Spec Kit checkout that a developer chooses. Fallow itself never downloads it.

**Rationale**: FR-002 (no download) and constitution V. The files are small, about 120 KB.

## S-16 Projects, conversations and the dashboard's data

**Decision**:
- **Projects**: `%LOCALAPPDATA%\fallow\projects.json` holds `id`, `path`, `name`, `added`, `pipeline`, `autonomy`, `approved_commands[]`, `budget_per_task`. It is written atomically, readable by the user only.
- **Features** are discovered from the project's `specs/NNN-*` folders, merged with what Fallow knows: stage, lock, loop id.
- **Conversations** (project and spec sessions) are event logs like runs (`conversations\<id>\events.jsonl`), with their turns, edits, approvals and the session id.
- **Loops** are 006's runs with new event types (contracts/storage.md).
- **Settings and definitions**: `~/.fallow/` holds `settings.toml`, `agents/` and `pipelines/`. 006's `%APPDATA%\fallow\settings.toml` is read once and moved there.

**Rationale**: the engineer chose `~/.fallow/` for definitions (2026-10-30). Keeping the settings beside them gives one place to look. Run data stays in the data directory, as constitution V requires.

## S-17 Unlock and the affected stories

**Decision**:
1. **Unlocking** records `unlocked`. The loop pauses at the end of its current step, and the spec session opens again with its history.
2. **The new lock** commits the changes. Fallow then works out which stories are affected, by comparing the documents at the two lock commits:
   - a user story whose section changed in `spec.md`;
   - a story whose tasks changed in `tasks.md`;
   - and, for a change to `plan.md` or a contract, every story whose tasks name a file the change mentions. When nothing is named, every story is affected.
3. **What reopens**: the affected stories' phases that were done are reopened. Their builder gets a repair with the change, in a fresh phase session with the handovers, and their checkpoints run again. Stories not affected keep their results.

**Rationale**: a mechanical rule that the engineer can see and predict. The page lists the affected stories before the engineer confirms the new lock.

## S-18 What 007 replaces from 006

**Decision**:
- **Replaced**:
  - the `fallow run "<task>"` command and the planner's turns;
  - the plan format `.fallow/work.toml`, with its parts, owned files and contracts;
  - the run page `run.html`.
- **Kept**: the service, the event log and its fold, the stage machine's verification, gates, repairs, resume and limits, the Claude adapter, the bridge, and the fake agent.
- **The plan's `commands`** become the project's approved commands, plus the commands that the plan of a feature names in a `## Commands` section.
- **006's remaining user stories** are superseded: decisions (US2), page steering (US4), parts (US5), publishing (US6) and measurement (US7). Parts and publishing return later as their own features. Measurement keeps its record format, and is re-planned for loops.

**Rationale**: two ways to start work would split the product. The spec says the old flow is replaced (FR-033).

## S-19 Concurrency and memory

**Decision**: keep 005/006's cap. By default, at most 4 agent steps run at once across all loops and conversations. A turn of the lead in a chat the engineer is waiting on goes ahead of loop steps. Above the cap, a step waits with "waiting for a slot". The memory budget is kept.

**Rationale**: constitution VIII's reference run is 4 concurrent steps.
