# Implementation Plan: Merge conflicts between loops

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is `011-merge-conflicts`) | **Date**: 2026-11-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/011-merge-conflicts/spec.md`

## Summary

011 makes a loop whose target moved go through verification again instead of a dead end:

- **Knowing the target moved** by ancestry, and whether a merge would conflict, by an in-memory trial
  merge (`git merge-tree`), never touching the target or the engineer's folder (R-1).
- **An update**: the target merged into the loop's own branch in its build worktree; a conflict goes
  to a builder as a repair step on its own part; the feature's locked documents changed on the
  target stop it for the engineer ("Lock again" / "Stop") (R-2). Then every story is reopened and the
  gates run against the target merged in, reusing the relock machinery (R-3).
- **Who starts it** follows the level: Free by itself, Normal and Strict ask (R-4).
- **A merge line** per target, rebuilt from the loops' logs, so loops do not overtake each other
  while one holds its turn (R-5); **overtaken updates** counted against a limit set at the lock
  (default 3), after which the engineer chooses "Verify again on the latest <target>" or "Merge /
  Publish without the latest check" (labelled, never as verified) (R-6).
- **Pull requests**: conflicts and "behind" read from the hosting tool; the update uses the remote
  target; the new commit is pushed forward on the loop's own branch, never forced, never to a second
  name (R-7). A recheck of the remote target precedes every push (FR-011a).
- **Overlap warnings** at the lock and while running, with "Run after" and "Wait for" (R-8).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as before).

**Primary Dependencies**: no new crates, no JavaScript libraries. git as a subprocess (2.38+ for
`merge-tree --write-tree`, with a scratch-worktree fallback for older git, R-1); the engineer's `gh`
for pull requests, as 010.

**Storage**: the loop's event log gains `Updated`, `UpdateMerged`, `UpdateDropped`, `Overtaken`,
`Lined`, `Unlined`, `Overlap`, `WaitsFor`, `WaitEnded`; `Merged`/`Published` gain `unchecked`;
`LoopCreated` gains `update_limit` and `run_after`; new `Steering` kinds. `LockRec` gains
`update_limit`, `run_after`. All optional with defaults (data-model.md); logs from before 011 load
unchanged.

**Testing**: `cargo test` without network or model (quickstart Q1–Q6): the fake runtime plays the
conflict repair; two features in one project (a generalized `lock_calc_in` and a second calc
feature); a bare remote and `fallow-fake-host` with `mergeable`/`mergeStateStatus` and `strict`.
New `tests/us11_update.rs`, `us11_line.rs`, `us11_publish_update.rs`, `us11_overlap.rs`; unit tests
for the trial merge, the conflict-marker check, `push_forward`'s refusals, the cycle check, path
extraction; `no_egress_no_push.rs` allows the second push site and still forbids force. Page checks
with headless Chrome over CDP (the loop page's offers and labels, the lock form's overlap and limit).
Every test from 007 to 010 stays green.

**Target Platform**: Windows 10/11 first; also Linux and macOS.

**Project Type**: local service with a browser page (one crate plus `xtask`).

**Performance Goals**:
- The target watch is local git only (`rev-parse`, `merge-base --is-ancestor`, `merge-tree`), on a
  thread, every 30 s per ready loop and on reading a loop (at most every 10 s); never on the engine
  thread.
- SC-006: a waiting loop starts within 10 s of the other being merged or stopped (it is driven on
  the next 1 s tick).
- 007's budgets hold: idle < 60 MB; the new state is a few hundred bytes per loop.

**Constraints**:
- The target branch and the engineer's folder are never used for an update or a check (FR-004,
  SC-003); updates happen in the loop's build worktree only.
- Builders never edit the locked documents (VI): a change to them on the target is not merged in.
- Never a push to the target, never forced, never a second pull request (II, FR-012); the only new
  push is `push_forward`, refusing unless the remote branch is at Fallow's last pushed commit.
- "Without the latest check" results are never shown in the words or colors of a verified one (I).
- A loop whose pull request is open is never stopped or paused (FR-016b).

**Scale/Scope**: a handful of loops per project; one line per target branch; overlaps per pair of
loops.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

Checked against v6.5.0.

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Only a commit whose gates ran on top of the target and whose stories were all checked again is offered (R-3). The builder's conflict resolution is checked by Fallow (no unmerged path, no marker, committed). "Without the latest check" is the engineer's choice, recorded with the commits it was not verified with, and labelled as not verified everywhere (R-6). | PASS |
| II. The Engineer Decides | Updates start by the level the engineer chose; at a limit or a cap the engineer decides; every choice is a recorded steering action. Never a push to the target, never forced, never a second pull request; a published loop is never stopped while its pull request is open. | PASS |
| III. Coordination in Code | The line, the waits and the updates are the engine's code and the loops' logs; agents do not coordinate. Every project is a git repository (6.5.0): updates and checks are git. | PASS |
| IV. Simplest Setup First | No new agent kind: the conflict goes to the pipeline's builder as a repair; checking again reuses the relock path. | PASS |
| V. Local and Private | Local checks are local git. For a published loop, `git fetch` of the target and `gh pr view` are the engineer's own tools publishing a run, as V lists. Nothing new leaves the machine. | PASS |
| VI. Least Privilege | Updates write only to the loop's own working copy and branch; builders never touch locked documents (R-2). | PASS |
| VII. Runs Survive | Every new state is an event; a merge in progress is on disk and its repair re-queued; the line is rebuilt on load (R-9). | PASS |
| VIII. Small and Measured | No dependencies; local git on a thread, bounded frequency; no new budget claims beyond SC-006, which a test times. | PASS |
| IX. Runtime-Agnostic | Decided from git, the gates and the checker's verdict; the repair prompt is plain text; all of it tested on the fake runtime. | PASS |

**Re-check after Phase 1 design**: PASS. The contracts add six requests, fields on the loop read and
the lock, and events; the only network is the engineer's tools for published loops, as V lists.

## Project Structure

### Documentation (this feature)

```text
specs/011-merge-conflicts/
├── plan.md              # this file
├── research.md          # R-1 to R-10
├── data-model.md
├── quickstart.md        # Q1 to Q7
├── contracts/
│   └── web-api.md       # the loop's target/update/line/waits/overlaps, requests, the lock
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/verify/git.rs        # merge_tree (trial merge, R-1) with the scratch fallback; unmerged paths; conflict markers
src/run/
├── events.rs            # Updated, UpdateMerged, UpdateDropped, Overtaken, Lined, Unlined, Overlap, WaitsFor, WaitEnded; unchecked; update_limit, run_after; Steering kinds
├── state.rs             # LoopState.update, updates, update_limit, overtaken, base_commit, lined_at, touched, task_paths, overlaps, waits_for; folding Updated like Relocked for every story
├── update.rs            # NEW: starting an update (R-2), the repair prompt, verifying the resolution, docs_changed
├── feature.rs           # step(): an update in progress; baseline at base_commit; waits_for blocks starting steps
├── machine.rs           # merge(): recheck (FR-003) and without_latest_check; publish(): recheck remote (FR-011a); drive in Publishing during an update; Done::Update*
├── engine.rs            # target watch thread; lines; who starts (R-4); overtaken (R-6); overlap at lock and while running (R-8); Request::Update/LeavePr/Wait/StopWaiting; lock's update_limit, run_after
└── dashboard.rs         # needs-you: update, update_limit, docs_changed, overlap
src/publish/
├── push.rs              # push_forward (R-7)
├── mod.rs               # fetching the remote target; behind/required
└── host/{mod.rs,github.rs}   # mergeable, mergeStateStatus; protection's strict
src/speckit/tasks.rs     # paths_in (moved from run/relock.rs, public)
src/service/
├── http.rs              # the new routes; lock's update_limit/run_after; overlap read
└── notify.rs            # the three notices
src/service/web/screens/
├── loop.js              # the target/update area: offers and labels, line, waits, overlaps
├── spec.js              # the lock form: overlap, Run after, update limit
└── dashboard.js         # needs-you items
src/bin/fallow-fake-host.rs   # mergeable, mergeStateStatus, strict
tests/
├── us11_update.rs, us11_line.rs, us11_publish_update.rs, us11_overlap.rs   # NEW
├── no_egress_no_push.rs # push_forward allowed in publish/push.rs; no force
└── common/mod.rs        # a second feature; lock_calc_in by name; conflict repair scenarios
```

**Structure Decision**: one crate, as before. The update lives in a new `src/run/update.rs` beside
`relock.rs`, whose machinery it reuses; pushing stays in `src/publish/push.rs`.

## Complexity Tracking

No violation to justify.
