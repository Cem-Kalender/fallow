# Contract: the work manifest `.fallow/work.toml`

Written and committed by the lead; read by Fallow at a commit, never written by Fallow. Errors are
reported with file and line, and a manifest with errors counts as absent (every piece of work is then
built by the lead).

```toml
# Prose lives in these documents; Fallow reads only this file.
docs = ["specs/012-shop/spec.md", "specs/012-shop/plan.md", "specs/012-shop/tasks.md"]
# Files any part may change.
shared = ["Cargo.lock", "package-lock.json"]
# Optional: overrides the squad's test patterns.
# test_patterns = ["tests/**", "**/*_test.go"]

[[work]]
id = "cart"
build = "parts"                       # lead | builder | features | parts
reason = "API and page meet only at the cart contract; separate files"

[[part]]
id = "cart-api"
work = "cart"
owns = ["src/api/cart/**", "tests/api/cart/**"]
criteria = [
  { id = "add", text = "POST /cart adds an item and returns the cart" },
  { id = "bad-qty", text = "a quantity of 0 is rejected with 400", must_fail = true },
]

[[part]]
id = "cart-page"
work = "cart"
owns = ["web/cart/**", "web/tests/cart/**"]
criteria = [{ id = "shows", text = "the page lists the cart's items and total" }]

[[contract]]
path = "contracts/cart.openapi.yaml"
between = ["cart-api", "cart-page"]
tests = "contract"                    # a gate in the squad file

[[work]]
id = "login-copy"
# build omitted: the lead builds it (FR-001)
criteria = [{ id = "text", text = "the login page shows the new wording" }]

[[review]]
focus = "security"
work = ["cart"]
```

## Rules Fallow checks

| Rule | When | Result |
|---|---|---|
| `id`s unique; `work` of a part exists | every read | error with line |
| `reason` present unless `build = "lead"` | every read | error |
| at least one criterion per work (or each part) | before it is built | the build or hand-over is refused (FR-027) |
| `build = "lead"`: no builder | `session_start` with this work | refused (FR-003) |
| `build = "builder"`: one builder | a second `session_start` | refused (FR-003) |
| `parts`: `owns` non-empty, no overlap, a contract per boundary, committed | `session_start` with a part | refused, naming what is missing (FR-004) |
| a part owns a file outside test patterns and `docs` | `session_start` | refused (FR-006) |
| `features`: no contract needed | | (FR-005) |
| changes by a non-lead session to this file, `docs` or a contract | each done report and gate | reported to lead and engineer (FR-024) |
| a builder's files outside `owns` and `shared` | each done report and gate | reported to the lead (FR-025) |

## Squad file additions (`.fallow/squad.toml`)

```toml
max_parallel_builders = 2      # FR-010; default: the burst's allowance
signal_unanswered_min = 15     # FR-035
run_idle_end_min = 3           # R-13
test_patterns = ["tests/**", "**/*_test.*", "**/*.test.*", "**/test_*.py"]

[merge]
requires = ["work:done", "review:approved"]   # both new (research R-16)
```

New role level `check` (run commands, no edits) for reviewers and investigators (research R-3).
Builders started for a piece of work get the branch `fallow/<work>-<part>` (research R-18).
