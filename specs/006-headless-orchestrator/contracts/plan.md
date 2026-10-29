# Contract: the plan, `.fallow/work.toml`

The planner writes the plan and commits it on the run's plan branch. Fallow reads it at a commit,
never from a worktree, and never writes it (research R-8). It replaces 005's work manifest. A run
is one task, so there is no list of pieces of work.

## One builder (the default)

```toml
docs = ["specs/012-search/spec.md"]     # optional: where the prose is
shared = ["Cargo.lock"]                  # optional: files any part may change
# owns = ["src/**", "tests/**"]          # optional: by default the builder may change
#                                        # everything except the plan's own files
criteria = [
  { id = "find", text = "find ignores case and surrounding spaces" },
  { id = "unknown", text = "an unknown name returns None", must_fail = true },
]
```

## Parallel parts

```toml
build = "parts"
reason = "the cart and the receipt meet only at the Cart interface"

[[part]]
id = "cart-core"
owns = ["shop/cart.py", "tests/test_cart.py"]          # its code and its tests
criteria = [{ id = "add", text = "…" }, { id = "over-stock", text = "…", must_fail = true }]

[[part]]
id = "receipt"
owns = ["shop/receipt.py", "tests/test_receipt.py"]
criteria = [{ id = "total", text = "…" }, { id = "empty", text = "…", must_fail = true }]

[[contract]]
path = "contracts/cart.md"
between = ["cart-core", "receipt"]
# tests = "contract"                                   # optional: a gate of the squad file
```

## Rules Fallow checks

| Rule | Result |
|---|---|
| The file parses, with only the keys above | Problems named with file and line (the T050 lesson) |
| Each part has criteria, at least one `must_fail`, ids `[a-z][a-z0-9-]{0,31}` unique **within the part** | Problem |
| Criteria are addressed as `part/id`; one builder's part is `main` | |
| `build = "parts"`: a reason, 2 to 8 parts, `owns` non-empty per part | Problem |
| No two parts own the same file; each part owns code, not only tests or plan documents | Not approvable, naming them |
| Each part has a contract with another part, and every contract file is committed | Not approvable, naming them |
| The plan files (this file, the contracts, `docs`) are changed only by the planner | A builder's change to them is refused |
