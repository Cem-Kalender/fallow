# Contract: what the page receives (018)

**Amends** 007 contracts/web-api.md ("Agents and pipelines", the feature and loop views) and 008 contracts/loop-view.md and page.md. Routes are unchanged; only fields are added. Nothing is removed from the responses (the agents page and the loop page still use `verifying` and `missing`).

## `GET /api/pipelines?project=<p>`

Each pipeline, besides 007's fields:

```json
{
  "name": "parallel",
  "caps": { "task_repairs": 3, "review_rounds": 3, "builders": 3 },
  "steps": [ { "agent": "builder", "when": "task", "on_fail": "repair" }, "…" ],
  "verifying": true,
  "missing": [],
  "checks": [
    "Up to 3 builders work on [P] tasks at the same time, each in its own working copy.",
    "Fallow runs the gates after every task.",
    "The reviewer checks every user story's acceptance scenarios at its checkpoint, alongside the security reviewer."
  ]
}
```

The Pipelines tab shows `checks` and, when `missing` is not empty, "A result of this pipeline ends as "Done, not verified by Fallow": <missing>". It shows no "verifying"/"not verifying" text, pill or colour, in the list, the designer or "Based on" (spec FR-001).

## `POST /api/pipelines/check` and `PUT /api/pipelines/<name>`

As 007; the result carries `checks` too. Refusals name the step and the reason (contracts/pipeline.md "Checks").

## The feature view (the spec screen)

Each entry of `pipelines`:

```json
{ "name": "parallel", "verifying": true, "missing": [], "builders": 3,
  "statement": ["Phase 3 has 4 [P] tasks: up to 3 builders at a time."] }
```

`statement` follows research R-11; empty when `builders` is 1. The lock form shows the chosen pipeline's `statement` under the Pipeline field and updates it on change (spec FR-022). The option text keeps " (not verified)" for a pipeline whose results would not be verified.

## The loop view

- `pipeline.caps.builders`; `pipeline.steps[].at` is given for task steps too ("start" / "end"; the builder and gates: "end").
- `groups`:

```json
[{ "phase": 3, "done": false, "tasks": [
  { "id": "T010", "state": "building", "worktree": "task-T010", "merged": null, "overlaps": [] },
  { "id": "T011", "state": "repairing", "worktree": "task-T011", "merged": null,
    "overlaps": [{ "files": ["src/cart.rs"], "with": ["T010"] }] },
  { "id": "T012", "state": "merged", "worktree": "task-T012", "merged": "4f1c2e9…", "overlaps": [] }
]}]
```

`state`: `queued | building | repairing | accepted | merged | waiting` (a question to the engineer). The loop page shows, for each active group, one row per task with its builder's state, and for an overlap "T011 changed src/cart.rs, which T010 changed: repaired on top of T010" (spec FR-024). The history lists `TaskOverlap` and `TaskMerged` with the files and tasks.
