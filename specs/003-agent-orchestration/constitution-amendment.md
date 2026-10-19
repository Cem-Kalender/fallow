# Constitution Amendment Proposal: Agent Sessions and Orchestration

**Status**: PROPOSED, not ratified. `.specify/memory/constitution.md` is unchanged until the
project owner approves this proposal.

**Proposed version**: 3.0.1 → **4.0.0** (MAJOR: Principle IV is redefined to allow scoped standing
permissions for non-destructive input to Fallow's own sessions).

**Date**: 2026-10-19

**Required by**: feature 003 (agent orchestration). It also states explicitly what feature 002
(agent sessions) already relies on.

## Summary

| Principle | Change | Why |
|---|---|---|
| IV. Observability Over Control | Adds "Fallow's own sessions" as a class with its own rules: input may be delivered under a scoped, time-limited, visible, revocable standing permission. Destructive actions keep per-action confirmation everywhere. | Agents idle at their prompt act only on new input; "check in on each other" needs delivery without a prompt every time, under the user's limits. |
| V. Least Privilege | States that starting a process is a write capability, allowed only at the user's explicit request or under a standing permission with a session limit. | 002 starts agents; 003 lets agents request new sessions. |
| VII. Privacy by Construction | Allows one local, user-only channel between Fallow and its sessions. Network sockets of any kind, loopback included, stay forbidden. Orchestration content is memory-only. | 003 offers tools to agents over the Model Context Protocol and needs a channel that is not a network connection. |
| II. Instant Startup | Clarifies that the orchestration channel starts with the first session that uses it, never with Fallow. | Keeps startup and idle unaffected. |
| Workflow gates | Adds a test gate for the new delivery paths and the no-socket rule. | Mirrors the existing SC-008 control test. |

## Proposed text

### IV. Observability Over Control (replaces the current text)

- Fallow's job is to show what LLM processes are doing, and to run the agent sessions the user
  starts in it.
- For processes Fallow did not start, nothing changes: Fallow MUST NOT kill, signal, suspend,
  re-prioritize, inject into, or otherwise modify them without explicit, per-action user
  confirmation.
- For **Fallow sessions** (processes Fallow started at the user's request, and their children):
  - The user's own typing and pasting into a focused session is not an action by Fallow.
  - Text that another agent asks Fallow to deliver into a session is input on the user's behalf. It
    MUST be approved per action, **or** covered by a standing permission that the user granted and
    that is scoped by source, target and action, limited in time (at most 8 hours), visible at all
    times while active, revocable at once, and announced each time it is used.
  - Every delivered text MUST carry a sender label set by Fallow that an agent cannot forge.
- **Destructive actions** (kill, force kill, suspend, resume, re-prioritize) on any process,
  including Fallow sessions, MUST be confirmed per action. A confirmation MUST name the target and the
  action before it executes. There is no "don't ask again" and no standing permission for them.

Rationale: users must be able to trust that watching a process never changes its behavior, and that
agents can only influence each other within limits the user set and can see.

### V. Least Privilege, Read-Only by Default (adds one bullet)

- Starting a process is a write capability. Fallow MAY start a process only when the user explicitly
  starts or restarts a session, or when an agent's request is approved by the user or covered by a
  standing permission with a session limit (Principle IV). It never starts processes on its own, and
  never with more privileges than its own.

### VII. Privacy by Construction (adds two bullets; the rest is unchanged)

- Fallow MAY open **one local channel** for its own sessions to reach it (a Unix domain socket or a
  Windows named pipe, or the agent's standard input and output), restricted to the current user. It
  MUST NOT open any network socket for this, including on the loopback interface. Only processes
  Fallow started may use the channel, each only as itself.
- Messages, events, tasks, permissions and the activity log of orchestration, and the output of
  sessions, are kept in memory only. They MUST NOT be written to disk, logged or sent anywhere.

The existing rule stays: the only permitted outbound traffic is forward DNS resolution of the
configured LLM provider hostnames. Fallow never makes network requests on an agent's behalf.

### II. Instant Startup, No Implicit Background Work (adds one bullet)

- The orchestration channel and its state are created when the first session that uses them
  starts, never at Fallow's startup and never before the first frame.

### Development Workflow & Quality Gates (adds two bullets)

- Any code path that types into a Fallow session on another agent's behalf MUST be covered by a
  test proving it is unreachable without a per-action approval or a matching, unexpired standing
  permission (003 SC-003).
- CI MUST verify that Fallow never opens a listening network socket (003 SC-006), alongside the
  existing egress test.

## Impact

- **Existing features**: 001 is unaffected; its control rules are the "processes Fallow did not
  start" case and the destructive-action rule. 002 already complies; the amendment states its
  process starts under Principle V. 003 becomes plannable.
- **Budgets**: no budget changes. 002 and 003 must meet the existing idle (< 50 MB), loaded
  (< 150 MB) and startup (< 100 ms) budgets. Their success criteria define how the loaded budget is
  measured with sessions (002 SC-007, 003 SC-007).
- **Benchmarks and tests to add**: the delivery-path test and the no-listening-socket check above;
  the canary test of 001 SC-007 extended to session output and orchestration content.

## Alternatives considered

1. **No standing permissions**: every delivery is approved by hand. It's simpler and needs a smaller
   amendment (Principle VII only), but unattended workflows like "merge when the PR is merged" then
   stop until the user returns. It stays available as a stricter user setting either way.
2. **A network channel on loopback**: the most common way to host such tools, but any local process
   could reach it, and it breaks the "no network code paths" gate. Rejected.
3. **A new agent-to-agent protocol**: unnecessary; agent CLIs already speak the Model Context
   Protocol, and none speak agent-to-agent protocols natively.

## Decision needed

Approve as written, approve with changes (for example a shorter maximum duration, or alternative 1),
or reject. On approval, the text above is applied to `.specify/memory/constitution.md` with
`/speckit-constitution`, which updates the Sync Impact Report and the version to 4.0.0.
