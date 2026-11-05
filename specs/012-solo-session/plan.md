# Implementation Plan: Solo sessions

**Branch**: `001-llm-aware-tui-editor` (the work stays on this branch; the spec directory is
`012-solo-session`) | **Date**: 2026-11-05 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/012-solo-session/spec.md`

## Summary

012 adds a coding assistant inside Fallow, the project page's only conversation. Each session has
its own working copy and branch, and Fallow keeps what it always keeps.

- **A session is a third kind of conversation** (`Kind::Solo`), which reuses 007 and 009's turn
  machinery. It has a new agent kind, **Assistant**, and a shipped `assistant` (R-1, R-6).
- **A worktree and a branch per session**, `fallow/solo/<slug>`, made from the engineer's `HEAD`.
  The engineer's folder is never touched (R-2).
- **A snapshot commit per turn** with plumbing and an empty `core.hooksPath`: no hook from any
  source, no signing (R-3). The working copy is settled before a turn, an undo or a close: a failed
  commit is retried, and the engineer's own edits are committed as theirs (R-4).
- **Undo** to before any turn. Undone commits stay recoverable under `refs/fallow/solo/<id>/undone`
  until close, and the base is derived from the turns that are not undone (R-5).
- **Commands.** The runtime always runs in Ask mode. Fallow's command judge allows commands by
  itself under "until I switch back", except the destructive ones, which are asked per command and
  named. A merge request carries the merge facts, and a squash is one approval (R-6).
- **Plain-text prompts.** The project's instruction files are handed as text, and a result schema
  carries `proposed_feature` (R-7). The runtime contract gains capabilities (`images`, `web_off`),
  attachments and a `web` tool kind (R-8). A runtime never applies the working copy's settings. Unattended kinds
  read no settings at all. The assistant reads the engineer's user settings, but every command and
  edit still reaches Fallow's judge. The project's instruction files are handed to every kind as
  text (R-6, R-7). Messages typed during a turn wait (R-9).
- **A ref watcher** per project shows every change to local and remote-tracking branches, with a
  source only when Fallow's records prove it (R-10). Squashes into a branch are recognized from the
  trees (R-11).
- **Gates on demand** in a separate worktree, against the session's base. "Not verified by Fallow"
  shows until they pass on the head (R-12). A session's turn takes an agent slot (R-13). Closing
  keeps or discards unmerged work on the engineer's word (R-14).
- **From a discussion to a feature**: a confirmed brief is committed as `brief.md`, and specify
  runs on it in a spec session with a lead. Project conversations are retired and stay readable
  (R-15).
- **Every project is a git repository**: a git check on the start page, "Make it a git repository"
  with a scan and a proposed `.gitignore`, and plain folders retired (R-18).
- **With 011**: a session's merge into a loop's target is a target move like any other, named as
  coming from the session. It is never part of the merge line (R-10, quickstart Q10).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as before).

**Primary Dependencies**:
- No new crates and no JavaScript libraries.
- git as a subprocess, 2.38 or later, now checked at start (R-18).
- The engineer's `claude` (or any runtime that implements the contract) for the assistant.

**Storage**:
- **Session logs**: a session is a conversation of kind `solo` in `conversations/<id>/events.jsonl`,
  with the new events in data-model.md. Attachments are in `conversations/<id>/attachments/`.
- **Per project**: `<data>/projects/<p>/git.jsonl` and `git-seen.json`.
- **In `projects.json`**: `sessions`, `last_session`, `assistant`, and the features' `origin`.
- **In the repository**: the undone refs `refs/fallow/solo/<id>/undone/<n>`.
- **Compatibility**: everything is optional with defaults, so logs from before 012 load unchanged.

**Testing**:
- `cargo test` without network or model (quickstart Q1–Q10), on the fake runtime. It gains
  `images`/`web_off` in its profile, `expect_images` and `web` steps, and `proposed_feature`
  output.
- Project repositories with hooks that write a marker file prove R-3. A bare remote is used for
  pushes.
- The new test files are `tests/us12_session.rs`, `us12_many.rs`, `us12_check.rs`,
  `us12_git.rs`, `us12_extras.rs`, `us12_close.rs`, `us12_feature.rs`, `us12_repo.rs` and
  `us12_with_loops.rs`.
- Unit tests cover the judge, the snapshot, the base fold, the squash comparison, the ref diff, the
  instruction files and the git-init scan.
- The page is checked with headless Chrome over CDP: the dropdown, the file beside the chat, the
  approval card, and the close and feature dialogs.
- The benches `solo_start`, `solo_four` and `refwatch_latency` run in CI (R-17).
- Every test from 007 to 011 stays green. Those that relied on plain folders or on the project
  conversation are moved to the new flows, and the review audits each such change.

**Target Platform**: Windows 10/11 first; also Linux and macOS.

**Project Type**: local service with a browser page (one crate plus `xtask`).

**Performance Goals**:
- SC-001: a session is ready within 5 s of its first message on 10,000 files (`git worktree add`).
- SC-004: a branch change shows within 5 s. The ref watch runs every 2 s for every project while
  Fallow runs, and after every command a session runs. It is local git only, on a thread.
- SC-009: with 4 working sessions, Fallow stays under 200 MB, and the activity shows within 2 s
  (p95).
- 007's budgets hold: idle under 60 MB. A session's state is its log; the activity is kept as
  today.

**Constraints**:
- **The engineer's folder and the other branches**: never changed by Fallow. Only the assistant
  changes them, on the engineer's word, through commands the engineer approved (FR-012, FR-016,
  SC-002).
- **Fallow's own commits**: run no hook and are never signed. Fallow never pushes, fetches or
  contacts a remote for a session (FR-018).
- **Destructive commands**: always asked per command, whatever the mode (II, FR-008). An approved
  prefix that could cover one is never put in the runtime's allow-list (R-6).
- **No hooks and no locks**: no hook runs in any git command Fallow makes on a session, and every
  read in the engineer's folder runs with `--no-optional-locks` (R-2).
- **"Until I switch back"**: never survives a restart.
- **A session's result**: never shown in the words or colors of a verified loop (I, FR-022).
- **Sources of branch changes**: never guessed (FR-017).

**Scale/Scope**: a handful of sessions per project, and up to 4 at work with the default slots.
Sessions live for hours to days.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.5.1.

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | A session's result is "Not verified by Fallow", naming what was not checked, until Fallow's own gate run passes on its head. Even then it says "no independent check" and never uses a verified loop's words or colors (R-12). The assistant's own test runs are commands, never gate outcomes. What Fallow cannot see is said where it matters: inside scripts, writes outside the working copy, the engineer's edits during a turn, squashes on a remote. | PASS |
| II. The Engineer Decides | A merge, push or pull request happens only through a command the engineer asked for and approved. Destructive commands are asked per command even under "until I switch back" (R-6). Every observed branch change is shown, with its proven source (R-10). Closing never drops unmerged work without the engineer's choice, and discarding is confirmed, naming what is lost (R-14). A feature starts only on the engineer's confirmation (R-15). | PASS |
| III. Coordination in Code | Every project is a git repository (R-18). The per-turn commits, the base, the squash recognition and "merged" come from git and Fallow's logs. The feature's brief is committed in its folder as its starting point (R-15). | PASS |
| IV. Simplest Setup First | One assistant per session, with no subagents as steps. The ref watcher and the judge are small modules. The session can be measured on its own (the benches), and it does not change loops. | PASS |
| V. Local and Private | Fallow makes no network request for a session (FR-018). The assistant's web and git-remote traffic is on by default and shown in the activity (V since 6.3.0). Web access can be switched off where the runtime supports it. Instruction files that look like secrets are never read (R-7). Attachments stay in Fallow's data folder. | PASS |
| VI. Least Privilege | The assistant edits only its working copy, never the squad file, agents or pipelines under `.fallow/` (protected), and is held to the gates of the project's committed `HEAD` (not its own copy) and to the approved commands only the engineer's approvals set. Commands beyond the gates and the approved commands are asked, and permission checks are never skipped (R-6). Fallow writes only to the session's working copy and branches (its branch, and the refs under `refs/fallow/solo/<id>/` that keep its undone commits, which VI's "the working copies and branches of … solo sessions" covers), its data folder, and a folder's repository, ignore file and first commit on the engineer's click (VI, 6.5.0) (R-18). | PASS |
| VII. Runs Survive | Every session state is an event. On a restart, a turn at work is committed as cut short and never repeated, approvals are cancelled, and waiting messages wait (R-19). | PASS |
| VIII. Small and Measured | No new dependencies. SC-001, SC-004 and SC-009 are backed by CI benches (R-17). The watch is local git on a thread, at a bounded rate. | PASS |
| IX. Runtime-Agnostic | Images and web-off are capabilities in the contract that every runtime declares. A feature suggestion is a structured field of the result schema. The prompts and instruction files are plain text. Commands are judged by Fallow, not by the runtime. Everything is tested on the fake runtime (R-7, R-8). | PASS |

**Re-check after Phase 1 design**: PASS.
- **The contracts add**: the session requests, attachments, the project's git changes, the git
  check and git-init scan, and the feature from a session.
- **They change**: the runtime contract (capabilities, attachments, `web`, the assistant's schema)
  and the project view (the conversation is gone).
- **They remove**: the project conversation's start.
- **No new network traffic**: there is none for Fallow itself.

## Project Structure

### Documentation (this feature)

```text
specs/012-solo-session/
├── plan.md              # this file
├── research.md          # R-1 to R-20
├── data-model.md
├── quickstart.md        # Q1 to Q11
├── contracts/
│   ├── web-api.md       # sessions, attachments, git changes, git check and init, feature from a session
│   └── runtime.md       # capabilities, attachments, web, the assistant's result schema
├── decisions.md, mockup/solo.html
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/conversation/events.rs   # Kind::Solo; the session events; Turn fields; state (base fold, queue, pending commit)
src/studio/
├── solo.rs              # NEW: create (R-2), settle and snapshot (R-3, R-4), undo (R-5), queue (R-9), gates (R-12), close (R-14), restart (R-19)
├── judge.rs             # NEW: assess(command, ctx) -> Allow | Ask(reasons); merge facts (R-6)
├── refwatch.rs          # NEW: snapshots, diff, sources, squash recognition (R-10, R-11)
├── mod.rs               # say/turn_done per kind; tool_call with the judge; new_feature without a fork, with origin and brief (R-15); project conversation retired; git_init with the scan (R-18)
├── files.rs             # Version kind solo; who_of for session subjects
└── commands.rs          # /feature, /close in a session's menu
src/agent/
├── mod.rs               # Capabilities, Call.attachments, Call.web, ToolKind::Web
├── claude.rs            # stream-json input with images; web off; WebFetch/WebSearch mapping; --setting-sources (empty, or user for the assistant), --settings ask rules and disableAllHooks, --model always
├── rights.rs            # rights::assistant
├── prompts.rs           # assistant_system, assistant_turn; instruction files for every kind (lead_turn too)
└── schemas.rs           # assistant_reply
src/defs/{agent.rs,mod.rs,pipeline.rs}   # Kind::Assistant; the shipped default; not a pipeline step
assets/agents/assistant.md               # NEW
src/verify/git.rs        # snapshot (plumbing, empty hooksPath), worktree with a new branch, for-each-ref snapshot, git check
src/project/mod.rs       # Kind::NeedsGit (Folder retired); sessions, last_session, assistant; moving a subfolder project to its top folder
src/run/engine.rs        # session slots; the 1 s tick sends waiting messages; refwatch threads; GitObserved into 011's MovedCommit.from; scopes project:<p>; studio requests
src/run/update.rs        # MovedCommit.from names a solo session
src/run/feature.rs       # the loop's step prompts carry the project's instruction files (R-7)
src/service/http.rs      # the routes in contracts/web-api.md
src/service/web/
├── app.js               # the approval card: reasons and merge facts; the attachments in messages
├── app.css
└── screens/
    ├── solo.js          # NEW: dropdown, header, queue, undo, close, feature dialog, Changes and Gates
    ├── project.js       # the middle column is the session; file beside the chat; git notices
    ├── spec.js          # file beside the chat (FR-029); the origin link
    ├── files.js         # solo versions; "Ask about it" to the session
    ├── pipelines.js     # "Ask the assistant"
    └── dashboard.js     # the git check; NeedsGit projects
src/bin/fallow-fake-agent.rs   # expect_images, web, capabilities from the profile
xtask/                   # benches solo_start, solo_four, refwatch_latency
tests/
├── us12_*.rs            # NEW, one per story plus with_loops
├── common/mod.rs        # sessions, hooks that write markers
└── us1_projects.rs, us1_conversation.rs, us9_*.rs   # moved off plain folders and the project conversation
specs/007-fallow-studio/contracts/{runtime.md,storage.md}   # amended (capabilities, settings; session storage)
specs/005-adaptive-delegation/contracts/task-set.md         # note: setup settings no longer reach run agents
```

**Structure Decision**: one crate, as before. Sessions live in the studio beside the conversations,
whose machinery they reuse. The judge and the ref watcher are their own modules, so they can be
unit-tested.

## Complexity Tracking

No violation to justify.
