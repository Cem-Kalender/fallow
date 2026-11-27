# Data model: Agent files are complete and say what they need (017)

No new files on disk and no new stored state. What changes is what an agent file must hold, what a runtime declares, and what the library reports.

## Agent file (`<name>.md`)

Unchanged places: built in, `~/.fallow/agents/`, a project's `.fallow/agents/`; the last valid one wins by name.

| Field | Required | Valid when | Changed by 017 |
|---|---|---|---|
| `name` | yes | `[a-z0-9-]+`, equal to the file name | an empty value is missing |
| `description` | yes | one line, not empty | the message says what belongs there |
| `runtime` | yes | one of `defs::RUNTIMES` (`claude`, `fake`) | an empty value is missing |
| `model` | **yes** (was optional) | fits the runtime's `ModelNaming` (R-1) | new rule; aliases refused |
| `kind` | yes | `maker`, `checker`, `lead`, `assistant` | unchanged |
| `may_run` | **yes** (was optional) | a list; `[]` for `lead` and `assistant`; items pass 007's command check | new rule |
| `returns` | yes | the kind's result shape | unchanged |
| `limits` | no | `{minutes, turns, usd}`, positive | unchanged |
| body | yes | not empty | the message says what belongs there |

A field is **missing** when its line is absent (reported at the closing `---` line) or its value is empty after removing a comment (reported at its own line).

**States**: a file is *valid* (in force unless a later place has a valid file of that name) or *unusable* (listed with its problems; never used; never rewritten by Fallow). An unusable file becomes valid only when the engineer saves a valid text over it (FR-011, FR-013) or edits it outside Fallow.

## ModelNaming (per runtime, declared by its adapter)

| Field | `claude` | `fake` |
|---|---|---|
| `prefix` | `claude-` | `fake-` |
| `example` | `claude-sonnet-5-5` | `fake-1` |
| `lookup` | Anthropic's models overview | the fake runtime's scenario |
| `lookup_url` | `https://platform.claude.com/docs/en/models/overview` | none |

`fits(name)`: starts with `prefix`; at most 64 characters; the rest non-empty, only `a-z 0-9 . -`.
`hint()`: one sentence built from the four fields, used in messages, the template and `GET /api/runtimes`.

## Library (in memory, `defs::Library`)

| Field | Meaning |
|---|---|
| `agents` | the valid definitions in force, by name (unchanged) |
| `pipelines` | unchanged |
| `problems` | every problem as `file:line: message` text, agents and pipelines (unchanged) |
| `updates` | unchanged |
| `unusable` **(new)** | `Unusable { name, file, source, text, problems: [Problem], in_force: Option<Source> }` for each agent file that failed, in load order |

`why_not(name)`: the reason an agent name cannot be used: no file of that name, or the first problem of its unusable file (R-3).

## Shipped agents

| Name | Kind | Model (was) | Model (017) | `may_run` (017) |
|---|---|---|---|---|
| lead | lead | opus | claude-opus-5-5 | `[]` (added) |
| assistant | assistant | opus | claude-opus-5-5 | `[]` (added) |
| builder | maker | sonnet | claude-sonnet-5-5 | unchanged |
| reviewer | checker | sonnet | claude-sonnet-5-5 | unchanged |
| test-writer | maker | sonnet | claude-sonnet-5-5 | unchanged |
| security-reviewer | checker | opus | claude-opus-5-5 | unchanged |
| docs-writer | maker | sonnet | claude-sonnet-5-5 | unchanged |

Changing the shipped text changes its hash in `~/.fallow/.shipped.json`: unedited copies are replaced, edited ones get `<name>.md.new` (007 FR-025, unchanged code).

## Run record (`measure::record`)

`AgentRow.model`: kept when `model_id(runtime, name)` accepts it: it fits the runtime's naming, or one of the forms accepted before 017 (old records). Otherwise `None`, as today.
