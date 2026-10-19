<!--
Sync Impact Report
==================
Version change: 3.0.1 → 4.0.0
Bump rationale: MAJOR. Principle IV is redefined: input delivered into Fallow's own agent
sessions may be covered by a scoped, time-limited, visible, revocable standing permission instead
of a per-action confirmation. Destructive actions keep per-action confirmation everywhere.
Approved by the project owner on 2026-10-19. Source: specs/003-agent-orchestration/
constitution-amendment.md.

Modified principles:
  - II. Instant Startup, No Implicit Background Work: the orchestration channel starts with the
    first session that uses it, never at startup.
  - IV. Observability Over Control: redefined (Fallow sessions, standing permissions,
    destructive actions).
  - V. Least Privilege, Read-Only by Default: starting a process is a write capability, allowed
    only on explicit request or under a standing permission with a session limit.
  - VII. Privacy by Construction: one local, user-only channel for Fallow's sessions; no network
    socket, loopback included; orchestration content and session output are memory-only.

Added sections: none (two new quality gates in Development Workflow & Quality Gates).
Removed sections: none

Templates requiring updates: none. plan-template.md, spec-template.md and tasks-template.md read
the constitution at runtime.

Previous amendment (3.0.1): PATCH. The startup budget statistic (p95 of 30 runs after 3
warm-ups, one automatic retry).

Previous amendment (3.0.0): MAJOR. Principle VII: environment values are never parsed, stored,
displayed or logged; macOS KERN_PROCARGS2 exception with a wiped buffer and the SC-007 canary.

Follow-up TODOs: none. TODO(LARGE_PROJECT_FIXTURE) and TODO(REFERENCE_HARDWARE) are resolved in
specs/001-llm-aware-tui-editor/research.md R-13 (fixture "fallow-fixture v1", 2026-10-19).
-->

# Fallow Constitution

Fallow is a terminal-based IDE for agentic workflows. It shows developers what LLM-driven
processes on their machine are doing, while staying small, fast, local, and unprivileged.

## Core Principles

### I. Memory Efficiency First (NON-NEGOTIABLE)

Memory efficiency is the primary design constraint and outranks feature completeness.

- Resident set size (RSS) MUST stay under **50 MB** when Fallow is idle.
- RSS MUST stay under **150 MB** with the reference large project open.
- A feature that pushes either measurement over budget MUST be rejected, or shipped as
  opt-in and disabled by default. Opt-in features MUST document their memory cost.
- When a design trade-off pits memory against speed or convenience, memory wins unless a
  constitution amendment says otherwise.

Rationale: Fallow runs alongside memory-hungry agents and toolchains; its value depends on
being cheap enough to leave open permanently.

### II. Instant Startup, No Implicit Background Work

- Time from process launch to first interactive frame MUST be under **100 ms**.
- Fallow MUST NOT perform background indexing, crawling, or scanning unless the user has
  explicitly enabled it. Such features are opt-in and off by default.
- Work needed at startup MUST be lazy: deferred until the user requests the view that
  needs it.
- The orchestration channel and its state are created when the first session that uses them
  starts, never at Fallow's startup and never before the first frame.

Rationale: a tool that is slow to open stops being opened; hidden background work silently
erodes both the startup and memory budgets.

### III. Terminal-Only

- Fallow MUST run entirely inside a terminal emulator.
- No GUI toolkit, no Electron, no webview, and no bundled browser runtime may be added as a
  dependency, direct or transitive.

Rationale: the terminal is where agentic workflows already live, and avoiding browser
runtimes is a precondition for meeting Principles I and II.

### IV. Observability Over Control

- Fallow's job is to show what LLM processes are doing, and to run the agent sessions the user
  starts in it.
- For processes Fallow did not start, nothing changes: Fallow MUST NOT kill, signal, suspend,
  re-prioritize, inject into, or otherwise modify them without explicit, per-action user
  confirmation.
- For **Fallow sessions** (processes Fallow started at the user's request, and their children):
  - The user's own typing and pasting into a focused session is not an action by Fallow.
  - Text that another agent asks Fallow to deliver into a session is input on the user's behalf.
    It MUST be approved per action, **or** covered by a standing permission that the user granted
    and that is scoped by source, target and action, limited in time (at most 8 hours), visible at
    all times while active, revocable at once, and announced each time it is used.
  - Every delivered text MUST carry a sender label set by Fallow that an agent cannot forge.
- **Destructive actions** (kill, force kill, suspend, resume, re-prioritize) on any process,
  including Fallow sessions, MUST be confirmed per action. A confirmation MUST name the target and
  the action before it executes. There is no "don't ask again" and no standing permission for
  them.

Rationale: users must be able to trust that watching a process never changes its behavior, and that
agents can only influence each other within limits the user set and can see.

### V. Least Privilege, Read-Only by Default

- Fallow's default mode MUST be read-only with respect to the system: it does not write to
  files, processes, or settings outside its own config and cache directories.
- Fallow MUST NOT require root, administrator, or elevated privileges for any core feature.
  When data is unavailable without elevation, Fallow MUST degrade gracefully and say so,
  not prompt for elevation.
- Any write capability MUST be opt-in and scoped to an explicit user action.
- Starting a process is a write capability. Fallow MAY start a process only when the user
  explicitly starts or restarts a session, or when an agent's request is approved by the user or
  covered by a standing permission with a session limit (Principle IV). It never starts processes
  on its own, and never with more privileges than its own.

Rationale: a monitoring tool with broad privileges is an attack surface; staying unprivileged
keeps it safe to install anywhere.

### VI. Benchmarked Performance Claims

- Every performance claim (in docs, release notes, specs, or UI) MUST be backed by an
  automated benchmark that runs in CI.
- The budgets in Principles I and II MUST each have a CI benchmark that fails the build when
  the budget is exceeded.
- A claim without a corresponding benchmark MUST be removed or reworded as unmeasured.

Rationale: performance budgets that are not measured continuously regress silently.

### VII. Privacy by Construction

- No user data, file contents, process details or network payloads ever leave the machine:
  no telemetry, no crash upload, no update ping, no remote logging.
- The only permitted outbound traffic is forward DNS resolution of the configured LLM provider
  hostnames. It MUST be possible to disable it with a configuration setting. Fallow MUST NOT
  perform reverse lookups of the user's connections or resolve any other hostname.
- Fallow MUST NOT read secret values (credential files, keychains, tokens, API keys).
- Fallow never parses, stores, displays or logs environment variable values. On platforms where
  a command-line read unavoidably returns them, the buffer is wiped before the call returns, and
  the SC-007 canary test runs on that platform.
- Fallow MUST NOT inspect network payloads.
- Fallow MAY open **one local channel** for its own sessions to reach it (a Unix domain socket or a
  Windows named pipe, or the agent's standard input and output), restricted to the current user.
  It MUST NOT open any network socket for this, including on the loopback interface. Only processes
  Fallow started may use the channel, each only as itself. Fallow never makes network requests on
  an agent's behalf.
- Messages, events, tasks, permissions and the activity log of orchestration, and the output of
  sessions, are kept in memory only. They MUST NOT be written to disk, logged or sent anywhere.
- Detection of LLM processes MUST rely only on process metadata (name, PID, executable path,
  command line, parent/child relationships, resource usage) and connection endpoints
  (remote host, port, protocol).

Rationale: Fallow observes processes that handle source code and credentials; it earns trust
only by being structurally unable to leak them. Resolving a fixed public list of provider
hostnames reveals nothing about the user beyond the fact that Fallow is running.

## Performance Budgets & Measurement

| Metric | Budget | Measured as |
|---|---|---|
| Idle memory | < 50 MB RSS | Peak RSS after startup with no project open, settled |
| Loaded memory | < 150 MB RSS | Peak RSS with the reference large project open |
| Startup | < 100 ms | Process launch to first interactive frame; p95 of 30 runs after 3 warm-ups |

- Startup statistic: the budget is met when the **95th percentile of 30 measured launches**,
  taken after 3 unmeasured warm-up launches, is under 100 ms. Shared CI runners are noisy, so a
  failing benchmark job is re-run **once** automatically; the second result is final.
- Budgets are measured on the CI benchmark runner, with background features at their
  defaults (off).
- The reference large project MUST be a pinned, versioned fixture in the benchmark suite.
  Changing the fixture is a change to the budget and requires a constitution amendment.
- Opt-in features MUST be benchmarked separately and their cost published; they never
  count against the default budgets but MUST NOT be enabled by default to dodge them.

## Development Workflow & Quality Gates

- Every specification and plan MUST include a Constitution Check that states its expected
  impact on memory, startup, privileges, and data access.
- Every pull request MUST pass the CI benchmark gates (Principle VI) before merge.
- Adding a dependency MUST be justified in the PR against Principles I–III and VII,
  including its effect on binary size, memory, and startup.
- Any code path that acts on another process MUST be covered by a test proving it is
  unreachable without user confirmation (Principle IV).
- Any code path that types into a Fallow session on another agent's behalf MUST be covered by a
  test proving it is unreachable without a per-action approval or a matching, unexpired standing
  permission.
- CI MUST verify that Fallow never opens a listening network socket, alongside the egress test.
- Any code path that opens a network connection MUST be treated as a constitution violation,
  except forward DNS resolution of configured provider hostnames as permitted by Principle VII.
  Any further exception requires an amendment.

## Governance

- This constitution supersedes all other project practices. Where a spec, plan, or PR
  conflicts with it, the constitution wins.
- Amendments require a written proposal stating the change, its rationale, and its impact
  on existing features and benchmarks, recorded in this file with an updated version.
- Versioning follows semantic versioning:
  - MAJOR: a principle is removed, redefined, or a budget is loosened.
  - MINOR: a principle or section is added, or guidance is materially expanded.
  - PATCH: clarifications and wording changes with no change in meaning.
- Complexity or exceptions to a principle MUST be justified in the plan's Complexity
  Tracking section; unjustified violations block merge.
- Compliance is reviewed at every PR and at every release.

**Version**: 4.0.0 | **Ratified**: 2026-10-13 | **Last Amended**: 2026-10-19
