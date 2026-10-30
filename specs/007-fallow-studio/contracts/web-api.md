# Contract: the page's API

The page and the service talk over loopback HTTP, as in 006 (research R-6):
- **Authentication**: a secret per launch, in a cookie set by the launch URL.
- **Request checks**: the Host and Origin headers.
- **Content Security Policy**: `default-src 'self'`.

All bodies are JSON. Errors are `{"error": "<message for the engineer>"}` with 4xx. Every action that changes something is recorded as a steering entry (constitution II).

## Static

| Path | What |
|---|---|
| `/` | The single page (S-1), which routes by hash |
| `/app.js`, `/app.css` | The page's code and styles |
| `/fonts/<file>.woff2` | Inter and JetBrains Mono (S-3) |

## Live updates

| Method and path | Returns |
|---|---|
| `GET /api/events?scope=<global\|loop:<id>\|conv:<id>>&after=<seq>` | As soon as the scope has changed since `after`, or `{seq, timeout: true}` after 25 s (S-4). `global`: `{seq, events[]}`, the events after `after`. `loop:<id>` and `conv:<id>`: `{seq}`, a mark that grows with every event and activity line; the page then reads what it shows. `reset: true` when `after` is from before a restart |

The global events are `project`, `feature`, `loop_progress`, `needs_you` and `needs_you_cleared`.

## Dashboard and projects

| Method and path | Body | Returns |
|---|---|---|
| `GET /api/dashboard` | | `{projects[], needs_you[], loops[], features[], agents_running, cost_today}` |
| `POST /api/projects` | `{path}` | The project. Installs Spec Kit (S-15) and returns what was installed or found |
| `DELETE /api/projects/<p>` | `{confirm: true}` | Removes the project from Fallow; its folder is not touched |
| `PATCH /api/projects/<p>` | `{pipeline?, autonomy?, budget_per_task?, notifications?, path?}` | The project |
| `GET /api/projects/<p>/commands` | | The approved commands |
| `PUT /api/projects/<p>/commands` | `{commands[]}` | Checked as in the data model |
| `GET /api/data` | | Sizes per project: loops, conversations and worktrees, with totals (FR-003b) |
| `DELETE /api/data/<kind>/<id>` | `{confirm: true}` | `kind` is `loop` or `conv`. Deletes that data after the page's confirmation naming it. A running loop cannot be deleted |
| `POST /api/projects/<p>/git-init` | `{confirm: true}` | `git init` and an initial commit (edge cases) |

## Conversations (the project conversation and spec sessions)

| Method and path | Body | Returns |
|---|---|---|
| `GET /api/conversations/<c>` | | `{kind, mode, turns[], busy, pending_approval?}` |
| `POST /api/conversations/<c>/say` | `{text}` | `{turn}`. The reply streams through the `conv:<c>` events |
| `POST /api/conversations/<c>/mode` | `{mode}` | The new mode (FR-007a) |
| `POST /api/conversations/<c>/approve` | `{id, answer: once\|always\|deny, prefix?}` | Answers the lead's pending command (S-7) |
| `POST /api/projects/<p>/features` | `{title, from_turn?}` | A new feature, and its spec session forked from the project conversation (FR-034) |

## Spec sessions and documents

| Method and path | Body | Returns |
|---|---|---|
| `GET /api/features/<f>` | | `{stage, step, documents[], markers[], lock_check, locks[], pipeline, autonomy, budget_suggestion}` |
| `GET /api/features/<f>/doc?path=<p>` | | `{text, version, last_edit_by}` |
| `PUT /api/features/<f>/doc?path=<p>` | `{text, version}` | Refused with 409 while a lead turn runs, or if the version moved; the page keeps the draft (research S-21). Only the spec paths are allowed |
| `POST /api/features/<f>/step` | `{to}` | Moves the stepper (FR-006); the lead's next turn runs that step |
| `POST /api/features/<f>/lock` | `{pipeline, autonomy, budget}` | 422 with the lock check's problems, or `{lock, loop}` |
| `POST /api/features/<f>/unlock` | `{confirm: true}` | Pauses the loop after its current step, and reopens the session (FR-009) |
| `GET /api/features/<f>/relock-preview` | | `{affected: ["US2", ...], why[]}`, shown before the new lock (S-17) |

## Loops

| Method and path | Body | Returns |
|---|---|---|
| `GET /api/loops/<l>` | | `{state, pipeline, stage, phases[], agents[], checks, questions[], budget, used, repairs, steering[]}` |
| `GET /api/loops/<l>/activity?agent=&kind=&after=` | | Activity events (FR-018) |
| `GET /api/loops/<l>/tasks/<t>` | | The task's steps, diff, commands, commit and gate result (SC-004) |
| `POST /api/loops/<l>/pause` and `/resume` | | |
| `POST /api/loops/<l>/stop` | `{confirm: true}` | |
| `POST /api/loops/<l>/note` | `{text, agent?}` | A note for the next step |
| `POST /api/loops/<l>/limits` | `{usd?, minutes?, once?}` | 006's semantics |
| `POST /api/loops/<l>/retry` | | 006's semantics |
| `POST /api/loops/<l>/questions/<q>/answer` | `{answer}` | Commits the answer, and resumes the step |
| `POST /api/loops/<l>/questions/<q>/overrule` | `{answer}` | Commits the reversal, and reopens (S-10) |
| `POST /api/loops/<l>/merge` | `{confirm: true}` | `ready` or `done_unverified` only. Exactly the gated commit (FR-014) |

## Agents and pipelines

| Method and path | Body | Returns |
|---|---|---|
| `GET /api/agents?project=<p>` | | The definitions with their source, and parse errors with line numbers |
| `PUT /api/agents/<name>?scope=user\|project:<p>` | `{text}` | Checked (contracts/agent-definition.md); 422 with problems |
| `DELETE /api/agents/<name>?scope=...` | `{confirm: true}` | A user's or project's own definition only; defaults are reset, not deleted |
| `GET /api/pipelines?project=<p>` | | Pipelines with `verifying` and what is missing |
| `PUT /api/pipelines/<name>?scope=...` | `{toml}` | Checked (contracts/pipeline.md) |
| `DELETE /api/pipelines/<name>?scope=...` | `{confirm: true}` | |
| `GET /api/runtimes` | | `[{name, installed, version, models}]` |
