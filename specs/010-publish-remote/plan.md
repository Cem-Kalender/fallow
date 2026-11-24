# Implementation Plan: Publishing that respects the remote

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is `010-publish-remote`) | **Date**: 2026-11-02 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/010-publish-remote/spec.md`

## Summary

010 gives a finished loop a second way out, besides the local merge:

- **The project's remote**, found with git (the remote the target branch tracks, its configured
  address, R-1) and, for its protection, through a hosting adapter (R-2, R-3). GitHub is the one
  adapter in this version, running the engineer's own `gh`. The project shows it next to its
  branch.
- **Publishing**: push exactly the verified commit to the loop's own branch on the remote, never the
  target, never forced (R-4), and open a pull request whose text Fallow derives from the loop's
  record (R-5). With a protected target, it is the only way; otherwise it comes first and the local
  merge stays.
- **Following the pull request** in the existing stage Publishing: its state and checks every 60 s
  and when the loop is read; merged on the remote ends the loop; closed puts it back to Ready (R-6).
- **"When verified"**, a per-loop choice at the lock, preset by the level Free: a verified loop
  publishes itself (R-7).
- **Restart, notifications and the CI gate**: publishing survives a restart without a second pull
  request (R-8); "pull request opened" and "merged on the remote" notices (R-9); the CI scan narrowed
  to what the constitution says, with a behavioural test that the target never moves (R-10).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as 007 to 009).

**Primary Dependencies**: no new crates, no JavaScript libraries. git as a subprocess, as
everywhere; the engineer's `gh` as a subprocess (constitution V). No HTTP client (deny.toml).

**Storage**:
- The loop's event log gains `Published`, `PublishFailed`, `PullRequestState`, and
  `when_verified` on `LoopCreated`; `Steering` gains `Publish`. All optional with defaults, so logs
  from before 010 load unchanged.
- `projects.json` gains `remote_seen` (the last known remote, with its time) and `LockRec.when_verified`.
- `settings.toml` may name the hosting tool: `[hosting.github] command`.
- The pull request's body is written to `<run>/publish/body.md`.

**Testing**: `cargo test` without network: a local bare repository behind a `github.com` address
(git's `insteadOf`, R-1) and a new test binary `fallow-fake-host` in place of `gh`
(contracts/host-adapter.md). New `tests/us10_remote.rs`, `tests/us10_publish.rs`; unit tests for
address parsing, the refspec guard and the body; `tests/no_egress_no_push.rs` reworked (R-10). The
page is checked with headless Chrome over CDP as in 009 (the project's remote pill; the loop page's
publish area in each state; the lock form's preset). Every test from 007 to 009 stays green. One
check by hand on a real GitHub repository (quickstart Q7).

**Target Platform**: Windows 10/11 first (`gh.exe` found on the PATH like the agents' programs);
also Linux and macOS.

**Project Type**: local service with a browser page (one crate plus `xtask`).

**Performance Goals**:
- SC-003: publishing within 30 s on a normal connection (a push and one `gh pr create`); every
  external call has a time limit (R-11) and runs on a thread, never on the engine thread.
- SC-005: a merge on the remote seen within 2 minutes: one `gh pr view` a minute per publishing
  loop, and on reading the loop.
- Opening a project never waits for the network: the git part is shown at once, the protection when
  the adapter answers.
- 007's budgets hold: idle < 60 MB, page ready < 500 ms; the remote's state is a few hundred bytes
  per project.

**Constraints**:
- Never push to the target branch, never force, never approve, review or merge a pull request
  (II); a guard refuses a destination equal to the target before git runs.
- Fallow's own process opens no connection; only the engineer's git and `gh`, run by Fallow, reach
  the network, and only to publish and to read protection and pull request state (V).
- No credential is read, stored or shown; never `gh auth token` or `--show-token`.
- No prompt or sign-in window ever waits: `GH_PROMPT_DISABLED=1`, `GIT_TERMINAL_PROMPT=0`, `GCM_INTERACTIVE=never`, SSH in batch mode unless the engineer set their own SSH command (R-11).
- The rules live outside the GitHub adapter (FR-017; IX by analogy).

**Scale/Scope**: one remote per project; a handful of loops publishing at once; pull requests
followed until merged or closed.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.3.1 (V clarified on 2026-11-02 to cover reading the target's protection to show it on a project, finding D1 of /speckit-analyze).

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Only the verified commit is pushed; the pull request's text is derived from Fallow's record, not written by an agent; the remote's state (merged, checks) is read from the remote, never claimed. A loop that ended not verified is never published on its own. | PASS |
| II. The Engineer Decides | Publishing is the engineer's action, confirmed, or the engineer's choice at the lock ("when verified"); the merge of the pull request is always the engineer's, on the remote. Never a push to the target, a force, an approval or a merge by Fallow; a guard and a CI test hold it. With a protected remote target, only a pull request is offered. | PASS |
| III. Coordination in Code | Publishing, following and restarts are the engine's code and the loop's record; no agent takes part. | PASS |
| IV. Simplest Setup First | One adapter (GitHub), no waiting for checks or repairs in this version (clarified); a project without a remote works exactly as today. | PASS |
| V. Local and Private | Fallow's process opens no connection: the engineer's own git and `gh` publish, read branch rules and pull request state, and read the target's protection to show it on the project, exactly the traffic V lists (6.3.1). They run as the engineer with their own sign-in; no credential is read, no token requested, no prompt waited on (R-11, R-12). Nothing is sent but the branch and the pull request the engineer asked for. | PASS |
| VI. Least Privilege | The tools run with the engineer's rights; Fallow asks them for no more than reading protection, pushing one new branch and opening one pull request. | PASS |
| VII. Runs Survive | Stage Publishing and the new events are in the log; a restart mid-publication reconciles from the remote, never opening a second pull request (R-8). | PASS |
| VIII. Small and Measured | No dependencies; one poll a minute per publishing loop; timed claims (SC-003, SC-005) have tests with the fake (polling interval, time limits). The idle memory benchmark still runs. | PASS |
| IX. Runtime-Agnostic | No agent runtime is involved. By analogy for hosting services: the rules are outside the adapter; GitHub's specifics live in its adapter; other services are named and merge locally. | PASS |

The CI list's "no code path pushes to a target branch, force-pushes or approves a pull request" is
kept, as a narrower scan plus a behavioural test (R-10); the old blanket ban on any push predates
publishing and is replaced, not loosened beyond what II and V allow.

**Re-check after Phase 1 design**: PASS. The contracts add one request (`publish`), fields on two
reads, and three events; the only network is the engineer's tools, as V lists.

## Project Structure

### Documentation (this feature)

```text
specs/010-publish-remote/
├── plan.md              # this file
├── research.md          # R-1 to R-12
├── data-model.md
├── quickstart.md        # Q1 to Q7
├── contracts/
│   ├── host-adapter.md  # the Host trait, GitHub through gh, the fake
│   └── web-api.md       # remote on the project, publish on the loop, the lock's when_verified
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/publish/
├── mod.rs               # Remote (R-1), offer rules (FR-005..007), when_verified rule (FR-020)
├── local.rs             # the local merge, unchanged
├── run.rs               # NEW: running git / the hosting tool with a time limit, no prompts (R-11)
├── push.rs              # NEW: the verified commit to the loop's branch; the target guard (R-4)
├── body.rs              # NEW: the pull request's text from the loop's record (R-5)
└── host/
    ├── mod.rs           # NEW: the Host trait (contracts/host-adapter.md), choosing the adapter
    └── github.rs        # NEW: gh
src/run/
├── events.rs            # Published, PublishFailed, PullRequestState, when_verified, Steering::Publish, Publishing → Ready
├── state.rs             # LoopState.publication, when_verified
├── machine.rs           # publish() beside merge(); Done::Publish / Done::PrState; restart reconciling (R-8)
├── feature.rs           # finish(): publishing when verified (R-7)
├── engine.rs            # Request::Publish; the 60 s poll on the tick; project remote refresh on a thread
└── dashboard.rs         # needs-you for a failed automatic publication; merged features
src/service/
├── http.rs              # POST /api/loops/<id>/publish; when_verified on the lock
└── notify.rs            # "pull request opened", "merged on the remote"
src/service/web/screens/
├── project.js           # the remote pill
├── loop.js              # the publish area: offer, confirm, pull request, checks, failure
└── spec.js              # the lock form's "When verified", preset by the level
src/project/mod.rs       # remote_seen, LockRec.when_verified
src/config/settings.rs   # [hosting.github] command
src/bin/fallow-fake-host.rs  # NEW (tests): plays gh from a JSON state file, logs what it was asked
tests/
├── us10_remote.rs       # NEW: Q1
├── us10_publish.rs      # NEW: Q2 to Q5
├── no_egress_no_push.rs # reworked (R-10)
└── common/mod.rs        # a bare remote behind a github.com address; the fake host's settings
```

**Structure Decision**: one crate, as before. Publishing stays in `src/publish/` (local merge and
pull requests side by side), the only place allowed to push or run the hosting tool; the run engine
calls it on threads as it already does for the local merge.

## Complexity Tracking

No violation to justify.
