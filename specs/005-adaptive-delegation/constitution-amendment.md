# Constitution Amendment Proposal: Run Records

**Status**: RATIFIED 2026-10-28 by the project owner, as written. Applied to
`.specify/memory/constitution.md` (v5.0.0).

**Proposed version**: 4.0.0 → **5.0.0** (MAJOR: Principle VII is redefined to allow one kind of
orchestration data on disk).

**Date**: 2026-10-28

**Required by**: feature 005 (adaptive delegation), FR-016: comparisons of plain, solo and squad
runs span several runs and Fallow restarts.

## Summary

| Principle | Change | Why |
|---|---|---|
| VII. Privacy by Construction | Allows run records, fingerprints, identifiers, numbers and fixed-value outcomes only, to be kept on disk in Fallow's own cache directory, for runs the engineer chose to record. Any text written by the engineer or an agent, session output, file contents, paths and command lines stay memory-only. | The comparison against a single session is the purpose of 005; it needs records that outlive one Fallow process. |
| V. Least Privilege | No change: the records live in Fallow's own config or cache directories, which V already allows. | Stated for completeness. |
| Workflow gates | Adds a test gate: a run record never contains text from messages, tasks, signals, session output, files or command lines. | Makes the boundary checkable, like the other privacy gates. |

## Proposed text

### VII. Privacy by Construction (replaces the bullet on orchestration data)

Current:

- Messages, events, tasks, permissions and the activity log of orchestration, and the output of
  sessions, are kept in memory only. They MUST NOT be written to disk, logged or sent anywhere.

Proposed:

- Messages, events, tasks, signals, permissions and the activity log of orchestration, and the
  output of sessions, are kept in memory only. They MUST NOT be written to disk, logged or sent
  anywhere.
- One exception: **run records**. For a run the engineer chose to record, Fallow MAY keep a record
  on disk in its own cache directory, readable by the current user only. A record holds only:
  - the run's mode and the components switched off, as fixed values;
  - fingerprints (one-way hashes) of the task description and of the hidden tests, never the text;
  - identifiers of the agent, model and version of each session, and of the starting and final
    commits;
  - the start time and durations;
  - per part and per role, referred to by number, the counts (signals, repair rounds,
    interventions, hidden tests passed and failed) and gate and review outcomes as fixed values;
  - token and cost figures as agents reported them.

  A run record MUST NOT contain any text written by the engineer or an agent (task names or
  descriptions, part, role or focus names, messages, tasks, signals, reviews, notes), session
  output, file contents, test names, paths of any kind, command lines or environment values. Run
  records are never sent anywhere. Fallow writes none unless the engineer chose to record the run;
  they are kept until the engineer deletes them, and Fallow shows how many it holds and their size.

### Development Workflow & Quality Gates (adds one bullet)

- CI MUST verify that a run record holds only the fields Principle VII allows, using a run whose
  task name and description, part, role and focus names, messages, tasks, signals, reviews, test
  names, paths and session output contain marker text that must not appear in the record.

## Impact

- **Existing features**: none. 003 and 004 keep all orchestration data in memory.
- **Benchmarks**: none. Records are written only for recorded runs, after the run; startup and idle
  memory are unaffected.
- **Risk**: a record reveals when runs happened, with which agents and models, and how they went,
  on the engineer's own machine. It holds no text, names, paths, code, prompts or agent output; the
  comparison view shows task names by matching fingerprints against the engineer's task set, which
  Fallow reads but does not copy.
