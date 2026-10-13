<!--
Sync Impact Report
==================
Version change: 2.0.0 → 3.0.0
Bump rationale: MAJOR. Principle VII is redefined. The blanket ban on reading environment
variable values becomes a ban on parsing, storing, displaying or logging them, with a bounded
exception for platforms where a command-line read unavoidably returns them (macOS
KERN_PROCARGS2; see specs/001-llm-aware-tui-editor/research.md R-2). Under this constitution's
versioning policy, redefining a principle is a MAJOR change.

Modified principles:
  - VII. Privacy by Construction: "environment variable values" removed from the list of secret
    values that must not be read. New rule: never parse, store, display or log environment
    values. Where a command-line read returns them, wipe the buffer before the call returns and
    run the SC-007 canary test on that platform.

Added sections: none
Removed sections: none

Previous amendment (2.0.0): Principle VII allowed forward DNS resolution of configured provider
hostnames as the only outbound traffic.

Follow-up TODOs (carried over from 1.0.0; research.md R-13 proposes resolutions, to be written
in once CI exists):
  - TODO(LARGE_PROJECT_FIXTURE): The "large project" used for the 150 MB budget must be pinned
    as a concrete, versioned fixture in the benchmark suite (file count, total size, languages).
  - TODO(REFERENCE_HARDWARE): The CI runner class on which budgets are measured must be named
    once CI exists.
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

Rationale: a tool that is slow to open stops being opened; hidden background work silently
erodes both the startup and memory budgets.

### III. Terminal-Only

- Fallow MUST run entirely inside a terminal emulator.
- No GUI toolkit, no Electron, no webview, and no bundled browser runtime may be added as a
  dependency, direct or transitive.

Rationale: the terminal is where agentic workflows already live, and avoiding browser
runtimes is a precondition for meeting Principles I and II.

### IV. Observability Over Control

- Fallow's job is to show what LLM processes are doing.
- Fallow MUST NOT kill, signal, suspend, re-prioritize, inject into, or otherwise modify any
  process without explicit, per-action user confirmation.
- A confirmation MUST name the target process and the action before it executes. There is
  no "don't ask again" for destructive actions.

Rationale: users must be able to trust that watching a process never changes its behavior.

### V. Least Privilege, Read-Only by Default

- Fallow's default mode MUST be read-only with respect to the system: it does not write to
  files, processes, or settings outside its own config and cache directories.
- Fallow MUST NOT require root, administrator, or elevated privileges for any core feature.
  When data is unavailable without elevation, Fallow MUST degrade gracefully and say so,
  not prompt for elevation.
- Any write capability MUST be opt-in and scoped to an explicit user action.

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
| Startup | < 100 ms | Process launch to first interactive frame |

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

**Version**: 3.0.0 | **Ratified**: 2026-10-13 | **Last Amended**: 2026-10-13
