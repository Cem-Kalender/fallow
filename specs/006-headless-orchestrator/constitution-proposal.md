# Constitution Proposal: Fallow, the Orchestrator

**Status**: RATIFIED 2026-10-29 by the project owner, as written. Applied to
`.specify/memory/constitution.md` (v6.0.0).

**Proposed version**: 5.0.0 → **6.0.0** (MAJOR: the constitution is replaced. Every principle is
redefined or removed, and the budgets change.)

**Date**: 2026-10-29

**Required by**: feature 006 (headless orchestrator), FR-034. On 2026-10-29 the owner decided that
the orchestrator replaces Fallow completely and that the constitution is recreated for it.

## What changes, in short

| v5.0.0 | v6.0.0 | Why |
|---|---|---|
| Fallow is a terminal IDE that shows what LLM processes do | Fallow is an orchestrator: it takes a task from an approved plan to a verified result | The product changed (FR-034) |
| I. Memory Efficiency First: 50 MB idle, 150 MB loaded | VIII. Small and Measured: budgets for the orchestrator itself, not its agents | Still a value, but the old fixture (a large project in an editor) no longer applies |
| II. Instant Startup, under 100 ms to the first frame | VIII: the page reachable within 500 ms; activity on the page within 2 s | There is no terminal frame any more |
| III. Terminal-Only | Removed. A local page, served only to the engineer (V) | The run page is the interface (US3, US4) |
| IV. Observability Over Control | Replaced by I. Verified, Not Trusted, and II. The Engineer Decides | Fallow now acts; what matters is that it checks and that the engineer keeps the decisions |
| V. Least Privilege | Kept and narrowed to agents and roles (VI) | Agents run with fixed, minimal rights |
| VI. Benchmarked Performance Claims | Kept, and extended to claims about effectiveness (VIII) | "Better than one session" must be measured |
| VII. Privacy by Construction | Kept, rewritten for a program that stores run data and publishes through the engineer's own tools (V) | Runs must survive a restart; publishing needs the remote |
| (new) | III. Coordination in Code, Decisions in the Repo | The lesson of T050 Q2 |
| (new) | IV. Simplest Setup First | One loop by default; splits only with evidence |
| (new) | VII. Runs Survive | A crash or restart never loses a run |

## Proposed text

The full text that would replace `.specify/memory/constitution.md`:

---

# Fallow Constitution

Fallow takes a coding task from a plan the engineer approved to a result it has verified itself,
running AI coding agents as non-interactive steps. It involves the engineer only where the
decision is theirs, shows everything the agents did on a local page, and runs on the engineer's
own machine with the engineer's own rights.

## Core Principles

### I. Verified, Not Trusted (NON-NEGOTIABLE)

- An agent's claim (done, a commit, files changed, tests passing, a verdict) MUST NOT count until
  Fallow has checked it itself: the commit on the step's branch, the files changed, the gates run
  by Fallow against a baseline, and the verdict complete for every criterion, including the cases
  that must fail.
- Gates are commands the project declares. Only Fallow runs them, and only Fallow reports their
  outcome.
- A reviewer never reviews its own work, and runs with rights that do not allow edits. A reviewer
  whose working copy changes stops counting.
- What Fallow cannot verify (for example, a command that writes outside the working copy) MUST be
  stated where it matters, never implied to be checked.

Rationale: agents report success they have not achieved. Fallow's value is that a result it
offers to merge has been checked by something other than the agent that produced it.

### II. The Engineer Decides

- Nothing reaches the target branch without the engineer's explicit merge. With a protected
  remote branch, Fallow only opens a pull request: it never pushes to the target branch,
  force-pushes, bypasses protection or approves its own pull request.
- Agent steps start only after the engineer approved the plan, apart from the planning
  conversation the engineer started. The approval covers that run's steps, within its caps.
- The engineer sets how much is decided without them (the autonomy level). Questions reserved for
  the engineer are never answered on their behalf, and a waiting question blocks only the work that
  depends on it.
- The engineer can always see the run, pause or stop any part of it, overrule a decision, and take
  over a part. Every steering action is recorded.
- Destructive actions (stopping a run, deleting run data, discarding branches) MUST be confirmed
  per action, naming what is affected.

Rationale: autonomy is acceptable only when the engineer keeps the decisions that matter and can
step in precisely.

### III. Coordination in Code, Decisions in the Repo

- Fallow, not the agents, decides which step runs next, from state it has verified.
- Agents MUST NOT message each other or the engineer. During a step an agent may read the plan,
  report progress, and ask Fallow for a decision.
- Decisions MUST be recorded as commits to the plan, the criteria or a contract. A message may
  point to the commit, never replace it.
- When a decision changes what a part builds against, every affected part MUST be reopened and
  verified again, including parts already done.
- Acceptance criteria are named by part (`part/id`), so no answer is ambiguous.

Rationale: in T050 Q2, agents that coordinated by messages stayed isolated, and the engineer
became the router. State that lives in code and in the repository needs no one to carry it.

### IV. Simplest Setup First

- A run builds with one builder by default, with gates and an independent reviewer.
- Work is split into parallel parts only when the approved plan declares it: owned files with no
  overlap, each part owning its code and its tests, and a committed contract for each boundary.
- Investigation happens inside a step (the agent's own subagents), never as extra steps.
- Every orchestration component MUST be possible to switch off for a recorded run, so its effect
  can be measured.

Rationale: more agents cost more and fail in more ways. They are added only where the plan says
the work divides cleanly and measurement says it pays.

### V. Local and Private by Construction

- Fallow runs on the engineer's machine, for the engineer only. It sends no telemetry, crash
  reports, update checks or remote logs.
- The run page MUST be served only on the loopback interface, to the engineer only:
  - a secret per launch, required on every request;
  - requests from other origins and other host names rejected.

  The page MUST load nothing from outside Fallow.
- Fallow itself MUST NOT make network requests. The traffic of a run is:
  - the agents' own traffic to their providers;
  - the engineer's own git and remote tools, which Fallow runs with the engineer's credentials only
    to publish a run (push its branch, open its pull request, read branch rules and check results).
- Fallow MUST NOT read secret values (credential files, keychains, tokens, API keys), and never
  stores or displays environment variable values.
- Run data (state, steps, agent activity, decisions, steering actions) MAY be kept on disk in
  Fallow's own data directory, readable by the current user only, until the engineer deletes it.
  Fallow shows what it holds and how large it is.
- Run records used for comparisons hold only numbers, identifiers, fingerprints and fixed-value
  outcomes, never text, paths or output, as in v5.0.0.

Rationale: Fallow handles source code, agent output and credentials-adjacent data; it earns trust
by keeping all of it on the machine, and by publishing only through tools and rights the engineer
already has.

### VI. Least Privilege

- Fallow MUST NOT require elevated privileges, and runs every agent with the engineer's rights or
  fewer.
- Each role's rights are fixed by Fallow and enforced by the agent's own options: builders edit
  and run commands, reviewers run commands without editing, and the planner edits only the plan.
- Options that make an agent skip its own permission checks MUST NOT be used unless the engineer
  turned them on explicitly for that run.
- Fallow writes only to its own data and config directories, the working copies and branches of
  its runs, and the files its agents are allowed to change there.

Rationale: a program that starts agents on its own must give each of them no more power than its
step needs.

### VII. Runs Survive

- A run's state MUST be kept on disk after every change.
- A stopped or crashed Fallow resumes every run where it was, without repeating a completed step
  and without losing history.
- A run never depends on the page being open.

Rationale: autonomous runs take long; anything that loses them on a restart forces the engineer
back into watching.

### VIII. Small and Measured

- Fallow's own process (not its agents) MUST stay under **60 MB** resident memory when idle, and
  under **200 MB** with a run of 4 concurrent agent steps.
- The page MUST be reachable within **500 ms** of Fallow starting, and an agent's activity MUST
  appear on it within **2 s**.
- Every performance claim MUST be backed by an automated benchmark that runs in CI and fails the
  build when its budget is exceeded.
- Every claim about effectiveness ("solo beats plain", "a split pays") MUST come from recorded runs
  on a task set with hidden tests, naming the task set and the number of runs.

Rationale: Fallow runs next to memory-hungry agents and must stay cheap, and claims about agent
orchestration are easy to make and hard to earn.

## Budgets & Measurement

| Metric | Budget | Measured as |
|---|---|---|
| Idle memory | < 60 MB RSS | Fallow's process, settled, with no run active |
| Running memory | < 200 MB RSS | Fallow's process during the reference run with 4 concurrent steps (agents excluded) |
| Page ready | < 500 ms | Process launch to the page answering; p95 of 30 launches after 3 warm-ups |
| Activity latency | < 2 s | An agent's output event to its appearance on the page; p95 over the reference run |

- The reference run is a pinned, versioned fixture with scripted agents. Changing it changes the
  budgets and requires an amendment.
- A failing benchmark job on a shared CI runner is re-run once automatically; the second result
  is final.

## Development Workflow & Quality Gates

- Every specification and plan MUST include a Constitution Check against these principles.
- Every pull request MUST pass the CI benchmark gates (VIII).
- Adding a dependency MUST be justified against V, VI and VIII, including its effect on memory,
  startup and what it sends over the network.
- CI MUST verify that:
  - injected false claims are never accepted (I): a missing commit, unchanged files, failing gates
    and an incomplete verdict;
  - no code path pushes to a target branch, force-pushes or approves a pull request (II);
  - the page listens on loopback only, rejects requests without the secret or from another origin
    or host name, and loads no external resources (V);
  - Fallow's own process opens no outbound connection (V);
  - a run interrupted at each stage resumes without repeating a completed step (VII);
  - a run record holds only the fields V allows, using marker text that must not appear (V).

## Governance

- This constitution supersedes all other project practices. Where a spec, plan, or pull request
  conflicts with it, the constitution wins.
- Amendments require a written proposal stating the change, its rationale, and its impact on
  existing features and benchmarks, recorded in this file with an updated version.
- Versioning follows semantic versioning: MAJOR when a principle is removed or redefined or a
  budget is loosened; MINOR when a principle or section is added or guidance is materially
  expanded; PATCH for clarifications.
- Complexity or exceptions to a principle MUST be justified in the plan's Complexity Tracking
  section; unjustified violations block merge.
- Compliance is reviewed at every pull request and at every release.

**Version**: 6.0.0 | **Ratified**: 2026-10-13 | **Last Amended**: (on approval)

---

## Impact on earlier features

- **001 to 005:** their specs stay as history. Their code is kept only where the orchestrator uses
  it (gates and baselines, manifest checks, verdict completeness, task sets and run records, the
  memory cap). The rest (editor, process monitor, terminal session hosting, typing into sessions,
  MCP messaging) is removed during 006.
- **The old budgets and benchmarks** (the 50 MB, 150 MB and 100 ms gates, the large-project
  fixture) are replaced by the ones above.
- **T050 and T051:** T050 is closed with the results so far (Q1 passed, Q2 passed with findings).
  The comparison T051 asked for moves to 006 (US7).
