# Data Model: Fallow Studio redesign (008)

008 stores nothing new. Everything below is derived from 007's records (the loop's event log, the pipeline TOML, the agent definitions) when the page asks.

## Story verification (derived, per user story of a loop)

| Field | Type | Rule |
|---|---|---|
| id | `USn` | The story, as in `spec.md` |
| title | string | From `spec.md` |
| checked_by_pipeline | bool | True when the pipeline has a checker that answers the criteria (the scenario checker: the first end-of-checkpoint checker without `paths`, feature.rs `scenario_checker`); false gives FR-001's "not checked" |
| total | int | The story's acceptance scenarios |
| checked | int | Criteria whose latest verdict is `passed` |
| failed | int | Criteria whose latest verdict is `failed` |
| findings | bool | Some checker of the story failed in the current round (its reasons in `checkers[]`), whether or not a criterion failed |
| state | `checked \| failed \| progress \| not_checked` | `not_checked` if not checked_by_pipeline; else `failed` if failed > 0 or findings; else `checked` if checked = total; else `progress` |
| criteria[] | list | Per criterion: `id` (`USn/k`), `text`, `must_fail`, `state` (`passed \| failed \| not yet`, as 007 writes it), `ran`, `result`, `expected` (the scenario's Then part, for a failure) |
| checkers[] | list | As in 007: each checker of the story, passed or not, with its reasons |

**Rules**: review rounds start at each repair of the story and at a new lock that reopened it (`queued_seq` bounds, as `checkpoint()` uses). The view shows the latest round that has a verdict; "latest verdict" is the last verdict in it. While a repair or a new lock waits to be checked, that round stays shown with `rechecking = true` (the page marks it "being repaired" or "checking again"), so a failure stays visible while it is repaired. The view keeps listing every story; the page shows no pill for a story without criteria.

| Field | Type | Rule |
|---|---|---|
| rechecking | bool | A repair or a new lock came after the round shown; its result is not in yet |

## Parallel group (derived, per story checkpoint)

| Field | Type | Rule |
|---|---|---|
| members | agent names | The checkers of the checkpoint's end that are connected by `parallel_with`; a checker alone is a group of one |
| state | `pending \| running \| judged` | `judged` only when every member has a verdict in the round |

**Transitions**:
1. pending → running: the members are queued and started while slots are free.
2. running → judged: the last member's verdict arrives.
3. judged:
   - if failing: one repair with every failing member's reasons, then pending again (the round count grows);
   - if incomplete: the incomplete member alone is asked again (up to 3 tries).

## Running step (in memory, per loop)

| Field | Rule |
|---|---|
| n | The step's number |
| stop | Its own stop flag; Stop and shutdown signal every running step; an unlock lets them finish, then the loop halts (007 FR-009) |
| group | Its process group, for memory sampling and the slot cap |
| tool | Its token for Fallow's MCP tools |
| asking | Its question waiting for the lead, if any |

**Rules**: not stored. On a restart, every step without an end is recorded as cut short (007), and each one is taken up again in its own session.

## Agent field change (request, per save)

| Field | Rule |
|---|---|
| fields | A map of frontmatter keys to new values (`runtime`, `model`, `kind`, `may_run`, `returns`, `description`); other keys are refused |
| scope | `user` or `project:<p>`, as 007 |

**Rule**: the server rewrites only those keys' lines of the current definition text and runs 007's checks on the result.
