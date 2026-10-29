# Contract: settings and the squad file

The more specific value wins: Fallow's settings, then the project's squad file, then the run's
options (FR-037, FR-038).

## Fallow's settings (`%APPDATA%\fallow\settings.toml`)

```toml
[defaults]
autonomy = "medium"          # high | medium | low
limit_usd = 10
limit_min = 120
step_min = 30                # time limit per agent step
repair_cap = 2
max_parallel = 2             # builder steps at once
memory_budget_mb = 0         # 0: three quarters of free memory at start (005)

[roles.planner]
agent = "claude"             # an agent profile id
model = ""                   # empty: the agent's default model
[roles.builder]
agent = "claude"
[roles.reviewer]
agent = "claude"
model = ""                   # e.g. "opus"; the page warns while it equals the builder's (FR-037)

[agents.claude]              # built in; shown here for overriding
command = "claude"
# unrestricted = false       # true passes --dangerously-skip-permissions; per run only, see below
```

## The project's squad file (`.fallow/squad.toml`)

005's file, with these changes:
- `[[role]]` tables, and `[merge] requires`, are no longer read. Roles are fixed: planner, builder,
  reviewer. The merge needs the gates, a complete approved review, and the engineer.
- **Kept:** `[[gate]]` (name, command, per-OS override, `timeout_min`, `baseline`), `repair_cap`,
  `max_parallel_builders`, `test_patterns`.
- **New:**

```toml
autonomy = "medium"
limit_usd = 10
limit_min = 120

[roles.reviewer]
model = "opus"

[allow]                      # added to the role allowlists (research R-4), as permission rules
builder = ["Bash(npm run lint:*)", "Bash(cargo fmt:*)"]
reviewer = ["Bash(npm run e2e:*)"]
```

## Per run

Only from `fallow run` options or the start form:
- autonomy, limits and roles;
- `unrestricted` (constitution VI), which needs a confirmation that names the risk.

An unrestricted run is marked as such on its page and in its record.
