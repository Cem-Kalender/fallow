# Contract: what Fallow reads and writes in Spec Kit files

Fallow reads `tasks.md` and `spec.md` at the lock and refuses a lock whose files it cannot read, naming the file, the line and what was expected (research S-8).

## `tasks.md`

| Element | Pattern | Use |
|---|---|---|
| Phase | `## Phase <n>: <title>` | Phases in order |
| Task | `- [ ] T<nnn> [P]? [US<n>]? <text>` (also `[X]` or `[x]` when done) | Tasks in order within their phase; ids must be unique |
| Story phase | A phase whose tasks carry `[US<n>]`, or whose title contains `User Story <n>` | Its end is that story's checkpoint |
| Checkpoint line | `**Checkpoint**: ...` | Shown in the task tree; not required |

**Rules**:
- Tasks before the first phase heading, or phases without tasks, are refused.
- A story phase whose story is not in `spec.md` is refused.
- When Fallow accepts a task, it rewrites that task's `- [ ]` as `- [X]` in the task's commit. This is the only change Fallow makes to a locked document besides committing answers.

## `spec.md`

| Element | Pattern | Use |
|---|---|---|
| User story | `### User Story <n> - <title> (Priority: P<k>)` | Stories and their order |
| Acceptance scenario | A numbered item under `**Acceptance Scenarios**:` in that story | The criterion `US<n>/<k>` |
| Must fail | A scenario whose text ends with `(must fail)` | Its verdict must show the case that was refused |
| Open marker | `[NEEDS CLARIFICATION` | Highlighted in the editor, and blocks the lock |
| Engineer's items | A `**Reserved for the engineer**:` list under a story or in Requirements | Questions about them always go to the engineer (FR-020a) |
| Clarifications | `## Clarifications`, with `### Session <date>` or `### Loop <date>` | Where answers are committed |

**Lock rules**:
- every story has at least one scenario;
- every story has at least one scenario marked `(must fail)`;
- no open marker.

## Analyze

The lock needs the analyze step done. The lead records the outcome of `/speckit-analyze` in the spec session as structured data: findings with severity. Any CRITICAL finding blocks the lock until it is fixed or the engineer dismisses it, which is recorded.

## Handovers

`specs/NNN-name/handover/phase-<n>.md`, with the sections `## Built`, `## Decisions` and `## For the next phase`. They are written by the builder and committed by Fallow (FR-010a).
