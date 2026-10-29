# Contract: step results

Each agent step is called with `--json-schema` for its kind (research R-3). The agent's
`structured_output` must match the schema. Otherwise the step failed (spec edge cases). Fallow
treats every field as a **claim** and verifies it (R-7) before anything counts.

## build, take, repair (builder)

```json
{
  "type": "object",
  "required": ["status", "summary"],
  "properties": {
    "status": { "enum": ["done", "blocked", "failed"] },
    "commit": { "type": "string", "description": "Required when done: the commit with the work" },
    "files": { "type": "array", "items": { "type": "string" } },
    "commands": {
      "type": "array",
      "items": { "type": "object", "required": ["command", "result"],
                 "properties": { "command": { "type": "string" }, "result": { "type": "string" } } }
    },
    "criteria": {
      "type": "array", "description": "How each of the part's criteria is met",
      "items": { "type": "object", "required": ["criterion", "how"],
                 "properties": { "criterion": { "type": "string", "description": "part/id" },
                                 "how": { "type": "string" } } }
    },
    "question": { "type": "string", "description": "When blocked: the question id from ask_decision, or the question" },
    "summary": { "type": "string" }
  }
}
```

**Fallow's checks:**
- **When `done`:** the commit is on the part's branch and newer than the step's base; the files
  changed are within `owns` and `shared`; every `takes` commit is contained; the gates pass, with
  no regression against the baseline.
- **When `blocked`:** the question becomes a decision (FR-011), or the refused commands from
  `permission_denials` are named (FR-036).

## decide (planner)

```json
{
  "type": "object",
  "required": ["outcome", "answer"],
  "properties": {
    "outcome": { "enum": ["decided", "needs_engineer"] },
    "answer": { "type": "string" },
    "changes": { "enum": ["none", "plan", "criteria", "contract"] },
    "commit": { "type": "string", "description": "Required unless changes is none" },
    "recommendation": { "type": "string", "description": "When needs_engineer: what the planner would choose and why" },
    "options": { "type": "array", "items": { "type": "string" } }
  }
}
```

**Fallow's checks:**
- the commit is on the plan branch, changes only plan files, and the plan still validates;
- if `changes` is `criteria` or `contract` and the autonomy level reserves those for the engineer,
  the decision becomes `needs_engineer`, with the planner's answer as the recommendation.

## plan_turn (planner)

```json
{
  "type": "object",
  "required": ["reply"],
  "properties": {
    "reply": { "type": "string", "description": "Shown to the engineer in the planning conversation" },
    "commit": { "type": "string", "description": "When the plan was written or changed" },
    "ready": { "type": "boolean", "description": "The planner considers the plan complete" }
  }
}
```

## review (reviewer)

```json
{
  "type": "object",
  "required": ["answers"],
  "properties": {
    "answers": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["criterion", "ran", "result", "passed"],
        "properties": {
          "criterion": { "type": "string", "description": "part/id" },
          "ran": { "type": "string" },
          "result": { "type": "string" },
          "passed": { "type": "boolean" },
          "failing_case": { "type": "string", "description": "For must-fail criteria: the case that must fail, and that it did" }
        }
      }
    },
    "summary": { "type": "string" }
  }
}
```

**Fallow's checks:**
- the verdict is complete (data-model.md Verdict): an incomplete verdict reruns the review, as
  `--resume` with the missing `part/id`s named (FR-007);
- the reviewer's worktree is unchanged.

## plain (measured runs)

No schema. Plain mode is a single call with the task description and nothing else (R-13).
