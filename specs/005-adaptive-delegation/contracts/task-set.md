# Contract: the task set

A directory the engineer chooses, outside every project. Fallow reads it and never writes it.

> **Note (012, 2026-11-05):** Fallow's agents no longer read settings from a working copy (012 FR-042), so a `setup/settings.local.json` copied into a run's copy does not reach its agents. Their permissions come from Fallow's rights.

```text
my-tasks/
├── taskset.toml
├── setup/
│   └── settings.local.json     # copied into every run's copy, all modes alike
├── cart-add/
│   ├── description.md          # the fixed description typed into the run's first session
│   └── hidden/                 # hidden tests, placed only after the run's sessions are closed
│       └── cart_hidden_test.rs
└── login-copy/
    ├── description.md
    └── hidden/check.sh
```

```toml
setup = [{ from = "setup/settings.local.json", to = ".claude/settings.local.json" }]

[[task]]
name = "cart-add"
project = "C:/Repos/shop"          # where the starting commit is fetched from
start = "4f2a9c1"                   # starting commit
description = "cart-add/description.md"
several_components = true           # marked in advance, for SC-002
time_limit_min = 120
hidden = [{ from = "cart-add/hidden/cart_hidden_test.rs", to = "tests/cart_hidden_test.rs" }]
tests = [
  { command = "cargo test --test cart_hidden_test add_item" },
  { command = "cargo test --test cart_hidden_test rejects_zero" },
]
```

## How a run uses it

1. **Copy**: Fallow creates a new repository under its cache, fetches `start` from `project` (only
   that commit's history), checks it out on `main` and copies the `setup` files in. Every worktree of
   the run comes from this copy; the engineer's repository is only read.
2. **Run**: the first session gets `description` plus the closing line "When you are finished, commit
   your work." (research R-13).
3. **End**: Fallow asks once to close the run's sessions; after that it copies the `hidden` files
   into the lead's worktree (the plain session's in plain mode) as left, runs each `tests` command there (pass = exit 0 within
   the time limit; `windows`/`macos`/`linux` overrides as for gates), records the counts and removes
   the copy.

## Fingerprints (research R-12)

- **Task**: the description, with line endings normalised. The time limit and idle setting are
  recorded as durations; runs are grouped only when fingerprints and settings match.
- **Hidden**: each hidden file's normalised content and target path, plus the commands.

A change makes later runs a separate group.

## Notice

Registering a task set shows: "Hidden tests are kept out of every run until its sessions are closed.
Agents run as you, so one could read them on purpose."
