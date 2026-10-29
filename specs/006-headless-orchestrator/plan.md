# Implementation Plan: Headless Orchestrator

**Branch**: `001-llm-aware-tui-editor` (work stays on this branch; the spec directory is
`006-headless-orchestrator`) | **Date**: 2026-10-29 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/006-headless-orchestrator/spec.md`

## Summary

Fallow becomes a local service that takes a coding task from an approved plan to a result it has
verified itself. The engineer is involved for the plan, the merge, and the decisions their autonomy
level reserves.

How it works:
- **One service** (`fallow serve`) holds every run. A thin command line starts it, starts runs and
  opens pages (R-1).
- **A run** is an append-only event log on disk, so it survives a crash or a restart and resumes
  without repeating completed steps (R-2).
- **Each step** is a non-interactive `claude -p` call with:
  - a typed result (`--json-schema`), its cost and a resumable session;
  - rights enforced by the agent's own permission rules, an allowlist per role (R-3, R-4).
- **Fallow verifies every claim itself**: the commit on the branch, the files changed, the gates it
  runs against a baseline, and verdict completeness per `part/id` (R-7).
- **Agents never message each other.** During a step they read the plan and ask for decisions
  through MCP tools on 003's named-pipe bridge. Decisions are commits on the run's plan branch, and
  every affected part is reopened (R-5, R-9).
- **The page** is served on loopback with a per-launch secret: the run view of the mockup, with
  live activity over SSE and steering as plain requests (R-6).
- **Publishing:** a local merge of the verified commit, or a pull request through the engineer's
  own `git` and `gh`, never bypassing protection (R-11).
- **Measurement:** plain, solo and squad on task sets with hidden tests, carried over from 005
  (R-13).
- **The terminal app is removed**, with about 25 of its crates. One crate is added (R-14).

## Technical Context

**Language/Version**: Rust stable, edition 2024, MSRV 1.85 (as before).

**Primary Dependencies**:
- **Kept:** `serde`, `toml`, `serde_json`, `globset`, `windows-sys` (features trimmed).
- **Added:** `tiny_http` (R-6).
- **Removed:** `crossterm`, `ratatui`, `ropey`, the `tree-sitter` crates, `notify`, `grep-*`,
  `ignore`, `arboard`, `vt100`, `portable-pty`, `ipnet`, and the rest of the editor and terminal
  crates (R-14).
- **Run as external tools:**
  - `claude` 2.1.x, with `-p`, `--json-schema`, stream-json output, `--resume`, `dontAsk`,
    `--allowedTools`, `--mcp-config` and `--max-budget-usd` (verified, R-3);
  - `git`;
  - `gh` 2.x, for publishing only.

**Storage**:
- Run directories under `%LOCALAPPDATA%\fallow\` with an event log per run, step activity, run
  records and measured copies (contracts/storage.md).
- Plans and decisions live in the repository, on the run's branches.

**Testing**:
- `cargo test` with `fallow-fake-agent`, a scripted stand-in for `claude -p` (R-15), covering:
  - the stage machine and resume at every stage;
  - verification and false claims;
  - decisions and reopening;
  - integration conflicts;
  - steering actions;
  - the page's authentication and request checks;
  - publishing against a fake `gh`;
  - run records (the marker test).
- Benchmarks through `cargo xtask bench` (R-16).
- Manual runs with real agents per quickstart.md.

**Target Platform**: Windows 10/11 x64 (clarified 2026-10-29). Nothing Windows-specific leaks
outside `platform` modules, so Linux and macOS can follow.

**Project Type**: a local service with a command line and a browser page (a single Rust crate, plus
`xtask`).

**Performance Goals** (constitution VIII): the page ready within 500 ms of launch, and agent
activity on the page within 2 s.

**Constraints**:
- **Memory:** under 60 MB idle and under 200 MB with 4 concurrent steps, for Fallow's process
  only.
- **Network:** no outbound connections from Fallow's process, and the listener on loopback only.
- **Rights:** the engineer's rights only.
- **Runs:** $10 and 2 h per run by default.

**Scale/Scope**:
- one engineer, a handful of concurrent runs across repositories, up to 4 concurrent agent steps;
- runs of tens of steps and thousands of activity lines.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | How the design meets it | Status |
|---|---|---|
| I. Verified, Not Trusted | Every step result is a claim checked by R-7. Gates are run only by Fallow. The reviewer is fresh, check-only, and its worktree is verified. Verdict completeness per `part/id`. CI injects false claims | PASS |
| II. The Engineer Decides | No agent step before plan approval, except planning turns (FR-033). Autonomy levels. Questions block only their part. Merge and pull-request merge are the engineer's. No `--force`, `--admin`, self-approval or target push (CI-checked call sites). Destructive actions are confirmed on the page. All steering is recorded | PASS |
| III. Coordination in Code, Decisions in the Repo | The stage machine decides the next step. No agent-to-agent or agent-to-engineer messaging tools (mcp-tools.md). Decisions are commits on `fallow/r<id>/plan`. Affected parts are reopened (R-9). `part/id` everywhere | PASS |
| IV. Simplest Setup First | `build = "builder"` (one part) by default. Parts only with owned files, no overlap and contracts. Components switch off in measured runs | PASS |
| V. Local and Private | The page on `127.0.0.1` with a per-launch secret, Host and Origin checks, CSP `default-src 'self'`, and no external resources (R-6). Fallow's process makes no outbound connections; publishing uses the engineer's `git` and `gh` (R-11), and the egress test covers this. No secrets are read. Run data is user-only on disk. Run records keep 005's field rules | PASS |
| VI. Least Privilege | No elevation. Role allowlists under `dontAsk` (R-4). `unrestricted` only per run with confirmation. Writes are limited to Fallow's directories, the run's worktrees and branches, and autostart is printed, not written (R-1) | PASS |
| VII. Runs Survive | An event log flushed before acting. Resume by fold and by session (R-2). A resume test at every stage | PASS |
| VIII. Small and Measured | One added crate and about 25 removed. Four CI benchmarks with a pinned reference run (R-16). Effectiveness claims only from US7 series | PASS |

Re-check after Phase 1 design: PASS. The contracts add no network access, no writes outside the
directories above, and no messaging path between agents.

## Project Structure

### Documentation (this feature)

```text
specs/006-headless-orchestrator/
├── plan.md                  # this file
├── research.md              # R-1 to R-16
├── data-model.md
├── quickstart.md            # V1 to V8
├── constitution-proposal.md # ratified 2026-10-29 (v6.0.0)
├── contracts/
│   ├── cli.md
│   ├── web-api.md
│   ├── step-results.md
│   ├── mcp-tools.md
│   ├── config.md
│   └── storage.md
├── checklists/requirements.md
└── tasks.md                 # /speckit-tasks
```

### Source Code (repository root)

```text
src/
├── main.rs              # command line: fallow, serve, run, status, stop, autostart, mcp-bridge, bench
├── lib.rs
├── service/             # NEW: the one service
│   ├── mod.rs           #   start, single instance (named pipe), resume runs, shutdown
│   ├── http.rs          #   tiny_http server: loopback, secret cookie, Host/Origin checks, CSP
│   ├── api.rs           #   JSON reads and steering actions (contracts/web-api.md)
│   ├── sse.rs           #   live events with Last-Event-ID
│   └── web/             #   index.html, run.html, app.js, app.css (embedded, no external resources)
├── run/                 # NEW: runs
│   ├── events.rs        #   event types, append + fsync, fold (contracts/storage.md)
│   ├── state.rs         #   Run, Part, Step, Decision, Verdict, Publication (data-model.md)
│   ├── machine.rs       #   the stage machine: what step is next
│   ├── decisions.rs     #   ask/decide/needs_engineer, autonomy, reopening (R-9)
│   ├── steering.rs      #   every engineer action and its effects
│   └── limits.rs        #   cost and time limits, concurrency and memory (R-12)
├── agent/               # NEW: agent steps
│   ├── mod.rs           #   the Adapter trait: call, stream, result, resume
│   ├── claude.rs        #   claude -p with the flags in R-3
│   ├── stream.rs        #   stream-json to activity lines
│   ├── prompts.rs       #   instructions per step kind and role
│   └── allow.rs         #   role allowlists (R-4)
├── verify/              # NEW, from orchestration/{evidence,gates,git}.rs
│   ├── claims.rs        #   commit on branch, files within owns, takes contained (R-7)
│   ├── gates.rs         #   gate processes, baseline classification (adapted from 004)
│   └── verdict.rs       #   completeness per part/id (from 005)
├── plan/                # from orchestration/manifest.rs: the plan format and its checks (R-8)
├── integrate.rs         # merging parts, conflicts to repairs (R-10)
├── publish/             # local merge (from 004), pull requests and checks via gh (R-11)
├── measure/             # from orchestration/{runs,record}.rs: task sets, copies, hidden tests, records
├── mcp/                 # from orchestration/{bridge,channel,rpc,identity}.rs: the step tools
├── config/              # kept: paths, settings, squad file (contracts/config.md)
└── platform/            # Windows: named pipe, Job Objects, ACLs, random, memory sampling
xtask/src/
├── fake_agent.rs        # NEW: fallow-fake-agent scripts (R-15)
├── bench.rs             # NEW: the constitution VIII benchmarks
├── egress.rs            # kept, retargeted to the service
└── record.rs            # kept: the run-record marker test
tests/                   # integration tests per area, with the fake agent
```

**Removed**: `src/app`, `src/ui`, `src/editor`, `src/session` (except Job Objects and the memory
sampler, moved to `platform`), `src/procmon`, `src/projects`, `src/clipboard.rs`, `src/dns.rs`,
003–005 code that has no place above (board, deliveries, permissions, bursts, signals, lead,
roles, inject, the old tools), and their tests and benchmarks.

**Structure Decision**: a single Rust crate, as before, organised by the new areas above. Code
from 003–005 moves into `verify`, `plan`, `publish`, `measure`, `mcp` and `platform` where it is
reused, and everything else is deleted. The order of the work is: remove first, then the run
machine and solo (US1), then decisions (US2), the page (US3), steering (US4), parts (US5),
publishing (US6), and measurement (US7).

## Complexity Tracking

No violations to justify.
