# Contract: the verification in the loop view

`GET /api/loops/<l>` (007's contracts/web-api.md) keeps every field it has. Each entry of `checks.stories[]` gains a summary, and its criteria gain `expected`:

```json
{
  "id": "US2",
  "title": "See the cart and its total",
  "checked_by_pipeline": true,
  "total": 5, "checked": 4, "failed": 1, "findings": true,
  "state": "failed",
  "criteria": [
    { "id": "US2/5", "text": "Given `cart.json` is not valid …", "must_fail": true,
      "state": "failed", "ran": "python -m shop cart", "result": "exit 0, the file was rewritten",
      "expected": "the command prints a message, exits non-zero and leaves the file untouched" }
  ],
  "checkers": [ { "agent": "reviewer", "passed": false, "reasons": ["…"], "step": 14 } ],
  "reviewer": "reviewer"
}
```

- `state` is `checked`, `failed`, `progress` or `not_checked` (data-model.md).
- A criterion's `state` is `passed`, `failed` or `not yet` (007's values, unchanged).
- `findings` is true when any checker of the story failed in the current round; `state` is then `failed` even if no criterion failed (constitution I: never green while a checker failed).
- `expected` is present for a failed criterion: the scenario's **Then** part, without the markdown emphasis.
- `checked_by_pipeline` is false when no checker of the pipeline answers the criteria (no scenario checker). Every story is then `not_checked`, and the page says that no checker in this pipeline verifies these criteria.

The view's `agents[]` gains nothing. An agent's `status` is `working` while any of its steps runs, so two parallel checkers both show as working. `busy` lists the running steps, for example `"steps 14, 15"`, or `"gates"`, `"merge"` or `""`.
