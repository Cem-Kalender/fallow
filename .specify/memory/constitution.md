<!--
Sync Impact Report
==================
Version change: 7.0.0 → 7.0.1
Bump rationale: PATCH. A clarification of Budgets & Measurement: budgets are measured on each
platform Fallow supports, not on "each of the three platforms". 7.0.0 wrongly assumed three; since
006 (clarified 2026-10-29) the first version supports Windows only, and Linux and macOS come later,
each in its own feature. Nothing added, removed or loosened. Decided by the project owner on
2026-11-14 ("Let's do windows first"), during /speckit-clarify for feature 014.

Modified sections: Budgets & Measurement (where budgets are measured).

Added sections: none. Removed sections: none.

Templates requiring updates: none.

Follow-up TODOs: none.

Previous amendments: 7.0.0 (MAJOR, 2026-11-14) Fallow as a standalone desktop app: the Studio
only in Fallow's own window through the system's web view, the browser page retired, combined
memory budgets (< 250 MB idle, < 400 MB running) and window ready < 1 s, a per-user install
without elevated privileges, releases built by CI from a tagged commit with checksums and no
self-update; 6.5.1 (PATCH) V clarified for fetching the target branch; 6.5.0 (MINOR) every project
is a git repository (III, VI); 6.4.0 (MINOR) VI names the assistant; 6.3.1 (PATCH) V clarified for
showing a project's remote; 6.3.0 (MINOR) solo sessions in II and V; 6.2.0 (MINOR) IX.
Runtime-Agnostic; 6.1.0 (MINOR) feature 007; 6.0.0 (MAJOR) the constitution replaced for the
headless orchestrator.
-->

# Fallow Constitution

Fallow takes a coding task from a plan the engineer approved to a result it has verified itself,
running AI coding agents as non-interactive steps. It involves the engineer only where the
decision is theirs, shows everything the agents did in its own window, and runs on the engineer's
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
- A result is offered as verified only when the gates ran on every accepted task and an
  independent checker checked every user story. A result without either MUST be shown as not
  verified, naming what was not checked, and never in the colors or words of a verified one.

Rationale: agents report success they have not achieved. Fallow's value is that a result it
offers to merge has been checked by something other than the agent that produced it.

### II. The Engineer Decides

- Nothing reaches the target branch without the engineer's explicit merge. With a protected
  remote branch, Fallow only opens a pull request: it never pushes to the target branch,
  force-pushes, bypasses protection or approves its own pull request.
- In a solo session the assistant acts on the engineer's word: a merge, commit, push or pull
  request it makes when the engineer asks for it counts as the engineer's decision. Fallow shows
  every change to the project's branches and remotes it observes, from any session.
- Agent steps start only after the engineer approved the plan, apart from the planning
  conversation the engineer started. The approval covers that run's steps, within its caps.
- The engineer sets how much is decided without them (the autonomy level). The lead may answer an
  agent's question only from what the locked documents or the engineer's own conversations settle
  (or, at the level the engineer chose, a small open question, listed for the engineer to
  confirm). Every such answer is committed and can be overruled. Questions reserved for the
  engineer, and questions that would change an acceptance criterion, are never answered on their
  behalf. A waiting question blocks only the work that depends on it.
- The engineer can always see the run, pause or stop any part of it, overrule a decision, and take
  over a part. Every steering action is recorded.
- Destructive actions (stopping a run, deleting run data, discarding branches) MUST be confirmed
  per action, naming what is affected.

Rationale: autonomy is acceptable only when the engineer keeps the decisions that matter and can
step in precisely.

### III. Coordination in Code, Decisions in the Repo

- Every project is a git repository: git is how Fallow's agents coordinate and how work is handed
  over.
- Fallow, not the agents, decides which step runs next, from state it has verified.
- Agents MUST NOT message each other or the engineer. During a step, an agent may read the locked
  documents, report progress, and ask a question. Fallow routes the question to the lead or the
  engineer, and records it with its answer.
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
- The Studio is shown only in Fallow's own window, using the operating system's web view. It is
  not offered to browsers or other programs:
  - whatever carries the Studio to the window MUST be reachable from that window only. A local
    connection is on the loopback interface, needs a secret per launch that is never shown,
    logged or written to disk, and rejects requests from other origins and host names;
  - the window MUST load nothing from outside Fallow, and MUST NOT navigate away from the Studio.
    A link to anywhere else opens in the engineer's browser, only when they click it.
- The web view's own network features (safe-browsing lookups, spell-check services, telemetry,
  update checks) MUST be switched off wherever the platform allows. What a platform does not let
  Fallow switch off MUST be stated in the release notes and the plan, never implied to be off.
- Fallow's notifications are shown by Fallow's own process on the engineer's machine. They name
  the project, the feature and what is needed, and never include code or command output.
- Fallow itself MUST NOT make network requests. The traffic of a run is:
  - the agents' own traffic to their providers;
  - the engineer's own git and remote tools, which Fallow runs with the engineer's credentials only
    to publish a run (push its branch, open its pull request, read branch rules and check results,
    and fetch the target branch's latest commit to bring the run up to date or to check it before a
    push), and to show on a project what publishing would do (read the target branch's protection);
  - the assistant's own requests to the web and to the project's git remotes, in a solo session;
    on by default for the assistant, and off for every other kind unless its definition allows it.
    Each request shows in the activity like any other tool call.
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
  fewer. On every platform there MUST be a way to install Fallow for the current user only,
  without elevated privileges.
- Each agent's rights are fixed by Fallow from its kind, whatever its definition says. A maker
  edits only its own working copy, and never the locked documents. A checker runs commands without
  editing. The lead writes only spec documents (`specs/`, the constitution, `.fallow/`). The
  assistant edits only its solo session's working copy, and the project's branches only on the
  engineer's word (II). Commands are limited to the gates, the plan's commands and the project's
  approved commands.
- Options that make an agent skip its own permission checks MUST NOT be used. In a conversation
  the engineer is present for, they may let the lead or the assistant run commands that are not
  approved, either per command or until they switch back. The lead's edits stay limited to spec
  documents either way.
- Fallow writes only to these places:
  - its own data and config directories;
  - the working copies and branches of its loops, conversations and solo sessions;
  - in a folder the engineer adds, on their click: the git repository, an ignore file they
    accepted, and its first commit;
  - in a project the engineer added: the Spec Kit files and `.fallow/` files it installs where
    they are missing, and the spec documents and handovers of that project's features.

Rationale: a program that starts agents on its own must give each of them no more power than its
step needs.

### VII. Runs Survive

- A run's state MUST be kept on disk after every change.
- A stopped or crashed Fallow resumes every run where it was, without repeating a completed step
  and without losing history.
- A run never depends on the window being open. Closing the window MUST NOT stop a run without
  the engineer's confirmation (II).

Rationale: autonomous runs take long; anything that loses them on a restart forces the engineer
back into watching.

### VIII. Small and Measured

- Fallow, counting its own process and the web view processes of its window (not its agents),
  MUST stay under **250 MB** resident memory when idle, and under **400 MB** with a run of 4
  concurrent agent steps.
- The Studio MUST be drawn in the window within **1 s** of Fallow starting, and an agent's
  activity MUST appear in it within **2 s**.
- Every performance claim MUST be backed by an automated benchmark that runs in CI and fails the
  build when its budget is exceeded.
- Every claim about effectiveness ("solo beats plain", "a split pays") MUST come from recorded runs
  on a task set with hidden tests, naming the task set and the number of runs.

Rationale: Fallow runs next to memory-hungry agents and must stay cheap, window included, and claims about agent
orchestration are easy to make and hard to earn.

### IX. Runtime-Agnostic

- Fallow's rules (the loop, the studio, the window) MUST hold for every runtime that implements the
  runtime contract. They are decided from what Fallow observes itself (its own records of what it
  started and received, git, the gates), never from how one runtime or model is known to behave.
- What is particular to one runtime (its command line, output format, how it is installed) lives
  only in that runtime's adapter. Anything Fallow needs from a runtime is part of the contract
  every runtime implements.
- Instructions to agents are plain text any model can follow: no runtime's built-in commands,
  tools or file conventions are relied on.
- Fallow's behaviour is tested on the fake runtime, through the same contract.

Rationale: Fallow is meant to run any supplier's agents. A rule that only holds for one runtime
breaks silently on the next, and ties the engineer to one supplier.

## Budgets & Measurement

| Metric | Budget | Measured as |
|---|---|---|
| Idle memory | < 250 MB RSS | Fallow's process plus its window's web view processes, summed, settled with the window open and no run active |
| Running memory | < 400 MB RSS | The same sum during the reference run with 4 concurrent steps (agents excluded) |
| Window ready | < 1 s | Process launch to the Studio drawn in the window; p95 of 30 launches after 3 warm-ups |
| Activity latency | < 2 s | An agent's output event to its appearance in the window; p95 over the reference run |

- Budgets are measured on the installed app, on each platform Fallow supports.
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
  - the Studio is reachable from Fallow's window only: a local connection listens on loopback
    only and rejects requests without the secret or from another origin or host name; the window
    loads no external resources and does not navigate away from the Studio (V);
  - Fallow's own process opens no outbound connection (V);
  - a run interrupted at each stage resumes without repeating a completed step (VII);
  - a run record holds only the fields V allows, using marker text that must not appear (V);
  - the per-user install runs without elevated privileges (VI).
- A release is built by CI from a tagged commit on the main branch, for each platform, with a
  published checksum for every download. Fallow does not update itself: the engineer downloads and
  installs a new version (V).

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

**Version**: 7.0.1 | **Ratified**: 2026-10-13 | **Last Amended**: 2026-11-14
