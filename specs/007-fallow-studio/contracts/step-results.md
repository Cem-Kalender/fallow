# Contract: step results

Every step returns a structured result that follows a JSON Schema (given with `--json-schema` for Claude). It is a claim, and Fallow verifies it (constitution I). 006's schemas are kept where they are named.

## `work` (maker: one task, a repair, the test-writer, the docs-writer)

```json
{ "status": "done | blocked | failed",
  "task": "T014",
  "files": ["shop/cart.py", "tests/test_cart.py"],
  "commands": [{"command": "python -m unittest tests.test_cart", "result": "Ran 6 tests, OK"}],
  "criteria": [{"criterion": "US1/3", "how": "..."}],
  "question": "q-12 (when blocked by a waiting ask)",
  "resolutions": [{"path": "shop/old.py", "how": "kept the deletion from main"}],
  "summary": "..." }
```

`resolutions` (011, optional, plain JSON, part of the contract every runtime implements): in a repair
of an update's conflicts, how each non-text conflict (a file deleted on one side, a binary file, a
rename) was resolved. Fallow checks that every non-text conflict is listed, and shows the list to the
engineer; it is never evidence: the gates and the checker verify the result.

**What Fallow checks**:
- `files` against the worktree's real changes (006 R-7);
- no change to the locked documents;
- `task` is the task it was given;
- a `blocked` result needs a waiting question, or a `question` text, which Fallow turns into one.

## `handover` (maker: the builder, at the end of a phase)

```json
{ "status": "done", "path": "specs/007-cart/handover/phase-3.md", "summary": "..." }
```

**What Fallow checks**: the file exists in the worktree, has the three sections (data-model.md, Handover), and changed nothing else. Fallow commits it.

## `verdict` (checker)

006's verdict, with criteria `USn/k`:

```json
{ "answers": [{"criterion": "US1/3", "ran": "python -m shop cart add apple 99",
               "result": "Refused: only 5 in stock", "passed": true,
               "failing_case": "adding 99 when 5 are in stock"}],
  "findings": [{"severity": "high", "where": "api/tags.ts:40", "ran": "...", "result": "..."}],
  "summary": "..." }
```

**What Fallow checks**:
- every criterion of the story has an answer with `ran` and `result`;
- every must-fail scenario has its `failing_case`;
- `findings` count only with `ran` and `result`;
- the checker's worktree is unchanged;
- refused commands make the verdict incomplete (006).

A checker other than the reviewer (for example the security reviewer) answers its own checks in `findings`. "The reviewer" is the pipeline's first checker at the checkpoint's end without `paths`, the one that makes the pipeline verifying. A checker at a phase's end or `once` also answers in `findings`. A finding of severity `high` or `critical` fails the step.

## `reply` (the lead, in chats)

```json
{ "reply": "markdown for the chat",
  "step": "clarify",
  "documents_changed": ["specs/008-checkout/spec.md"],
  "question": {"text": "...", "options": [{"label": "A", "text": "...", "recommended": true}]},
  "proposed_feature": {"title": "discount codes", "summary": "...", "after": "008-checkout"},
  "proposed_pipeline": "<TOML text>" }
```

`question` is how a clarify question is shown as choices (FR-007).

## `answer` (the lead, answering an `ask` during a loop)

```json
{ "outcome": "answer | decided | relay",
  "answer": "...",
  "basis": "spec.md FR-006 | the spec session, turn 14 | none",
  "options": [{"label": "...", "recommended": true}],
  "reserved": false,
  "changes_criterion": false,
  "touches_interface": false,
  "adds_dependency": false }
```

**Fallow's rules**:
- `answer` needs a `basis` other than `none`;
- `decided` is accepted only at the Free level, and only for a small question: `touches_interface` and `adds_dependency` false (a missing flag counts as true);
- whatever the lead says, the outcome becomes `relay` when the level is Strict, when `reserved` or `changes_criterion` is true or missing, or when the question or the answer names an item the spec reserves for the engineer: Fallow checks this itself, the lead's flags being claims (FR-020a);
- a lead's call that fails or passes its limit (`lead_answer_sec` in `[defaults]`, 180 by default) relays the question;
- when relayed, the lead's answer, if it gave one, is its recommendation.
