# Research: Chat commands and lead choice (009)

Decisions for [spec.md](./spec.md), on top of 008's code. Each has a decision, why, and what else was considered. File references are to the code as of commit `ece6607`.

## R-1 Which Spec Kit steps the menu offers, and how the lead runs one

**Decision**:
- A step is a Spec Kit skill installed in the conversation's working copy: `.claude/skills/speckit-<step>/SKILL.md`. Fallow scaffolds these into every lead worktree (`project::install::scaffold`).
- The menu offers `constitution`, `specify`, `clarify`, `plan`, `tasks`, `analyze`, `checklist` and `converge`, each with the skill's `description` and `argument-hint` from its frontmatter.
- `implement` and `taskstoissues` are not offered:
  - `implement` is the loop's job, and the lead may not write code (constitution VI).
  - `taskstoissues` publishes to GitHub, which a spec session never does.
- In the project conversation only `specify` is offered (clarify answer 1).
- When a step is sent, the lead's prompt says: run the Spec Kit step `<step>` by reading and following `<path of SKILL.md>` in your working copy, with the engineer's input in place of `$ARGUMENTS`.
- The prompt does not contain the skill's text: the lead reads the file. The file is the step's own, and Spec Kit's other files (templates, scripts) are next to it.

**Why**:
- Any runtime that can read a file can follow it. Nothing depends on a runtime's built-in slash commands (FR-006).
- The skills are already in the working copy at every turn, so Fallow points at them instead of copying 10–20 KB into every prompt.

**Alternatives**:
- Pasting the skill's text into the prompt: rejected. It is large and duplicates a file the lead can read.
- Spec Kit's agent-neutral `templates/commands/*.md`: Fallow does not ship them (only the Claude skills are in `assets/spec-kit`). Adding them would mean a second copy of every step to keep in step with the first.
- Sending `/speckit-plan` to the runtime: rejected. That works only for Claude Code, and only interactively.

## R-2 The project's own commands

**Decision**: Fallow reads the project folder (`p.path`, the engineer's own copy, where uncommitted commands live too) in these places, in this order:

| Runtime | Place | Name | Instructions |
|---|---|---|---|
| Claude Code | `.claude/commands/**/*.md` | path below `commands`, `/` as `:` (`frontend:review`) | the body after the frontmatter |
| Claude Code | `.claude/skills/<name>/SKILL.md`, except `speckit-*` | `<name>` | the body |
| Gemini CLI | `.gemini/commands/**/*.toml` | path below `commands`, `/` as `:` | the `prompt` value |
| GitHub Copilot | `.github/prompts/*.prompt.md` | file name without `.prompt.md` | the body |
| Cursor | `.cursor/commands/*.md` | file name | the body |

How Fallow reads each command:
- **Description**: the frontmatter's `description` (TOML `description`), else the first non-empty line.
- **Input**: the engineer's text replaces `$ARGUMENTS` and `{{args}}`. If neither appears, the input follows the instructions under "The engineer's input:".
- **Settings that grant rights are not applied** (FR-009), for example `allowed-tools` or `model`. The lead turn runs with every lead turn's rights.
- **Size**: a file over 64 KiB is listed as too large and cannot be sent.
- **Same name twice**: Spec Kit's step is picked first, then the places in the table's order. The menu shows each with where it came from (edge case).

When sent, the lead's prompt carries the command's instructions as the engineer's request. They are quoted, with where they came from. They are not given as system instructions.

**Why**:
- These are the places where these runtimes keep a project's commands today.
- Sending the text makes them work with any lead (FR-008).
- Reading the engineer's folder, rather than the lead's worktree, finds commands that are not committed yet. Spec Kit is handled the same way, through `scaffold`.

**Alternatives**:
- User-level folders (`~/.claude/commands`): left out, because the spec says "the project's own". They can be added later as another place.
- Running the command through its own runtime: rejected (not model-agnostic).

## R-3 Where a message becomes a command

**Decision**: the server decides, in `say()`:
- A message whose first word is `/<name>`, with `<name>` in the conversation's catalogue, is a command; anything else is plain text (FR-005).
- `TurnStarted.said` keeps the text as typed (FR-012).
- `LeadTurn` gains `command: Option<Command>`. The prompt adds a short block for it (R-1, R-2) before "The engineer says:".
- In the project conversation, `/specify <title>` does not start a turn there. `say` creates the feature (as `NewFeature`, with the session's lead choice, R-4), then sends `/specify <title>` in the new spec session. It answers `{feature, conversation}`, and the page opens that session (clarify answer 1).
- The catalogue is computed per request from the files, so it is never stale. It is cheap: a few directory reads.
- `GET /api/conversations/<c>/commands` gives the page the same catalogue (contracts/web-api.md).
- Fallow's own actions are the page's (R-9). They are not sent to the server as messages.

**Why**: one place decides what is a command, so the menu and a typed `/plan` agree, and the rule has tests (`tests/us9_commands.rs`).

**Alternatives**: the page expanding commands into instructions (rejected: untestable in Rust, and the chat would show the expansion instead of `/plan`).

## R-4 The lead of a session

**Decision**:
- `Project` gains `lead: Option<String>`; `None` means `lead`. It is set by `PATCH /api/projects/<p>` with `{lead}`, which checks that the name is an agent of kind Lead in the project's library.
- `patch_project` now checks every key before changing any. Today an error on a later key leaves earlier keys changed in memory but not saved.
- `ConversationCreated` gains `#[serde(default)] lead: Option<String>`, set from the project's default when the session is created.
- A new event, `LeadSet { lead }`, records a change before the first message. It is refused once a turn has started that was not withdrawn and has a session.
- `State` gains:
  - `lead: Option<String>` (`None` means `lead`, which covers sessions from before 009, FR-018);
  - `lead_runtime` and `lead_model`, taken from the first `TurnStarted` of the current agent session.
- `TurnStarted` gains `#[serde(default)] runtime, model`.
- `say()` looks up `state.lead` instead of `"lead"`. When the definition's runtime or model differs from the one the session ran on, the turn starts a `Session::New`. The engineer sees a notice before sending (FR-019a), from the view's `lead.changed`.
- A spec session forks the project conversation's agent session only when both use the same lead on the same runtime and model; otherwise it starts a new one. Today it always forks, which would cross runtimes.
- When the lead is gone, the view says so and `say` refuses with that reason (FR-019). Gone means the definition is deleted, it is no longer kind Lead, or its runtime is not available.
- Only the shipped `lead` cannot disappear: deleting a user or project override falls back to the shipped definition.

**Why**:
- The lead is recorded where the session is: in its own log, so it survives restarts (VII).
- Recording the runtime and model at the first turn is what makes "the session cannot move" checkable.

**Note**: an older Fallow build truncates a conversation log at `lead_set`, as it does at 008's `cancel_asked`. This only matters if the engineer goes back to an older build. `ConversationCreated`'s new field is ignored by older builds.

**Alternatives**:
- Storing the lead on the `Feature` record: rejected. The project conversation has no feature, and the log is the source of truth for a conversation.
- Allowing a switch mid-session with a summary: rejected by the engineer (2026-11-02).

## R-5 The loop's lead

**Decision**:
- `run::questions::Lead` gains `agent: String`.
- `Studio::lead_of` fills it from the feature's spec session (`state.lead`, or `lead`).
- `start_lead` looks up `lib.agent(&lead.agent)` instead of `"lead"`. If that lead is gone, the question goes to the engineer with the reason, as today when the lead cannot answer.
- Loops locked before 009 resolve to `lead` (FR-017, FR-018).
- The feed's "lead answered" line names the agent (`by: "careful-lead"`). The page's `q.by === "lead"` checks become "a lead answered".

**Why**: the loop already finds the spec session through `project.features[f].conversation` on every question. Adding the name there costs nothing and needs no new loop event.

## R-6 The working indicator

**Decision**:
- `#chat` gets a sibling indicator element, `#working`, outside the re-rendered message list. It holds three dots and "The lead is answering…", and `syncComposer` shows or hides it.
- Each live update re-renders the messages, but not the indicator, so its animation does not restart.
- The dots move only under `@media (prefers-reduced-motion: no-preference)`, as 008's graph pulse does. Otherwise they are still (FR-022).
- `#working` is `role="status"`, so "The lead is answering" is announced once. Today the chat has no live region.

**Why**: the chat is re-rendered on every `conv:` event (`messages()`). An indicator inside it would restart its animation every few seconds.

## R-7 Leaving with unsaved changes

**Decision**:
- A screen may define `dirty()`: agents.js already has one, pipelines.js has `s.dirty`, and spec.js gains one (the editor's text differs from the document's).
- The router keeps the last address shown. On `hashchange`, if the current screen is dirty and the new address leaves it, the router:
  - puts the old address back with `history.replaceState`;
  - asks "Leave your changes?" with the existing `confirm()`;
  - goes on only on "Leave them".
- The screens' own `update()` guards stay, for changes within a screen.
- A `beforeunload` listener calls `preventDefault()` when the current screen is dirty, which brings up the browser's own prompt (FR-025).

**Why**: every navigation is a hash change (links, nav, Back and Forward, `location.hash =`), so one check in the router covers them all (FR-023).

**Alternatives**: intercepting clicks on links (rejected: misses Back and Forward and `location.hash` writes).

## R-8 The file views

**Decision**: three read-only endpoints (contracts/web-api.md), all in a new module `src/studio/files.rs`.

**Versions**:
- `folder`: the project folder as it is.
- `feature:<f>`: the feature's live working copy. That is the loop's build worktree (`runs/<id>/wt/build`) when the feature has a loop and the worktree exists; otherwise the spec session's worktree; otherwise the branch, read with `git show`.
- The base is `merge-base(HEAD of that copy, feature.base_branch)`.

**Tree**:
- In git: `git ls-files --cached --others --exclude-standard` in that copy (the project's ignore rules).
- Marks come from `git diff --name-status <base>` (the working tree against the base, uncommitted changes included) plus untracked files as added.
- Files under the feature's folder are tagged `spec`.
- A plain folder is walked directly, leaving out `.git` and the cache folders git already skips (`git::CACHES`). It has no marks.
- The page gets a flat list and builds the tree; 5,000 paths are about 200 KB.

**File**:
- Text up to 512 KiB. Larger files show their first 512 KiB, marked as cut.
- A file with a NUL byte in its first 8 KiB is binary: Fallow sends only its size.
- The lines added against the base, as ranges, from `git diff -U0`.
- `changes`: the unified diff against the base.

**Who changed it**:
- `git log -1 --format=%H%x1f%s%x1f%b%x1f%ct <base>..HEAD -- <path>`.
- The subject's `<kind> <task>:`, plus the body's `Fallow loop r<id>, step <n>`, give the task and, through the loop's step record, the agent definition.
- Spec sessions' `spec: <step> (turn n)` gives "the lead".
- An uncommitted change in a running loop's worktree is "being changed", with the agent at work.

**Safety (FR-035, FR-036, FR-036a)**:
- A path must be relative, with no `..`, `\`, drive letter or leading `/`.
- The joined path is canonicalized, must stay under the canonical root, and must not be a symlink (`symlink_metadata`).
- The secrets list is matched on the file name or its folders: `.env` and `.env.*` (except `.example`, `.sample` and `.template`), `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.keystore`, `id_rsa*`, `id_ed25519*`, `id_ecdsa*`, `.npmrc`, `.pypirc`, `.netrc`, `.git-credentials`, `credentials*`, `secrets.*`, and anything under `.aws/`, `.ssh/` or `.gnupg/`.
- These files are listed with `secret: true`; their content is never read.
- No endpoint writes.

**Live updates**: while a file view is shown, the page asks again every 3 s, only while the window is visible. It also asks on a `loop:` or `conv:` event it already watches. A reply with an unchanged `fingerprint` (a hash of the list and the marks) changes nothing on the page (FR-034).

**Why**:
- git already knows what is ignored, what changed and what was committed. Reading the live working copy shows a running loop's work before it is committed, which is what the engineer wants to watch.
- The commit messages already carry the task (`build T009: …`) and the step.

**Alternatives**:
- `git ls-tree` at a commit only: rejected, because it misses uncommitted work in a running loop.
- A file watcher: rejected (a new dependency against VIII).

## R-9 The `/` menu on the page

**Decision**:
- The message box becomes an ARIA combobox while the text starts with `/` and has no space yet. Its options form a listbox with four groups (`role="group"`, labelled), and `aria-activedescendant` follows the highlight.
- A `role="status"` line announces the number of matches (FR-004).
- Matching is case-insensitive on name, then description. Name prefixes come first.
- Tab completes, Enter picks (or sends when the menu is closed), Escape closes, and a click picks.
- The catalogue comes from `GET /api/conversations/<c>/commands` when the box first gets `/`, and again on each new `/`.
- Fallow actions are the page's own list. Each has an `available()` and a `reason()` from the state the screen already has, and calls the same handler as its button. That includes the button's confirmations, through `data-act`. Unavailable actions are listed last in their group, greyed out, with `aria-disabled` (FR-010).

**Why**: no library and no new pattern beyond what WAI-ARIA describes for a combobox with a listbox. Only the catalogue needs the server.

## R-10 Ask the lead about a file, and Open in my editor

**Ask the lead**:
- The page opens the project page and fills the box with `@<path>`, adding `(on <feature>)` for a feature's version, through the existing `fallow.prefill`.
- When it is sent, `say()` finds `@` references to files in the project's views. It adds each file's text to the lead's prompt as context, with its version, applying the same limits and the secrets rule as the file view.
- The lead does not need git access to a branch, so this works for any lead.

**Open in my editor**:
- A preference in Fallow's data directory, `prefs.json`, holds `editor: {command}`. It is Fallow's own data (VI). `settings.toml` stays read-only to Fallow.
- `POST /api/files/open` with a version, a path and an optional confirmed `command`:
  - It opens the file with the preferred editor, launching the program with the file's absolute path as one argument.
  - Without a preference it answers `{choose: [found editors]}`. The editors looked for are `code`, `code-insiders`, `cursor`, `windsurf`, `zed`, `subl`, `idea` and `pycharm`, plus `notepad++` on Windows. Fallow looks on `PATH` and in their usual install folders.
  - The page asks the engineer to confirm one, or to type a command. Confirming saves it.
- It never asks the system to open the file by its type (FR-032). A `.cmd` launcher on Windows goes through `cmd /c` with the path quoted. Paths with `"`, `%`, `^`, `&`, `|`, `<` or `>` are refused there, because `cmd` would read them.

## R-11 Tests

**Rust, with the fake runtime**:
- `tests/us9_leads.rs`: the default lead, a pick before the first message, refusal after it, a fork only with the same lead, a definition changed between turns, a lead gone, and a loop's question answered by the session's lead.
- `tests/us9_commands.rs`: the catalogue per conversation and place, a step's prompt naming the file, a project command's text and input, an unknown `/word` sent as text, `/specify` in the project conversation, and settings in a command not applied.
- `tests/us9_files.rs`: versions, marks, ignore rules, binary and large files, the secrets list, paths outside, links, who changed what in which task, and the editor's choice. The editor is a fake program that records its arguments.
- The fake agent's scenario matches on prompt text (`when`), so tests can check what the lead was told.

**Page**: headless checks with Chrome, as in 008:
- the menu by keyboard;
- the indicator, and the absence of motion under reduced motion;
- the leave prompt on every way out;
- the Files tab and drawer against the mockup, at 1440 px and 390 px.
