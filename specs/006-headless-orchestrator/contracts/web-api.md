# Contract: the page and its API

Served on `127.0.0.1:<random port>` only (research R-6).

**Every request** must:
- carry the session cookie, obtained by opening the URL with `?k=<secret>` once;
- have a `Host` of `127.0.0.1:<port>` or `localhost:<port>`.

**State-changing requests** (`POST`) must also carry an `Origin` equal to the page's own. Otherwise
the response is 401 (no cookie) or 403 (host or origin).

**Every response** carries `Content-Security-Policy: default-src 'self'`, `X-Frame-Options: DENY`
and `Cache-Control: no-store`. The page loads nothing from any other origin.

## Pages

| Path | Shows |
|---|---|
| `/` | All runs across repositories: stage, what needs the engineer, cost, time. A form to start a run (repository path, task, options) |
| `/runs/<id>` | The run view, with the layout of the mockup "Fallow Run View": pipeline, agents, live activity, decisions, gates, criteria, the bar for what needs the engineer, the steering controls |
| `/runs/<id>/plan` | The planning conversation, and the plan with its checks and the Approve button |
| `/bench/<id>` | A measured series: runs grouped by task and mode, with pass rate, median time, cost, interventions and spread |

## Reading

| Request | Response |
|---|---|
| `GET /api/runs` | `[{id, repo, task, stage, needs_engineer, cost_usd, started, updated}]` |
| `GET /api/runs/<id>` | The folded run state (data-model.md): run, plan, parts, steps (without activity), decisions, gates, verdicts, publication, limits, and `seq` (the last event number) |
| `GET /api/runs/<id>/steps/<n>/activity?after=<i>` | Activity lines of one step |
| `GET /api/runs/<id>/wait?seq=<n>&activity=<m>` | Long polling (research R-6): `{seq, activity}` at once when the run has an event after `seq` or activity after `activity`, else with the next change, or after 25 s with `timeout: true` |

## Acting (steering, FR-017 to FR-020)

Each request is a `POST` with a JSON body. On success it returns `{"ok": true, "event": <seq>}`.
On failure it returns 409 with `{"error": "<why>"}` (for example "the plan has problems: …" or
"the repair cap is reached").

| Request | Body | Effect |
|---|---|---|
| `/api/runs` | `{repo, task, target?, autonomy?, limits?, roles?}` | Starts a run (as `fallow run`) |
| `/api/runs/<id>/plan/say` | `{text}` | One turn of the planning conversation (a `plan_turn` step) |
| `/api/runs/<id>/plan/approve` | `{}` | Only when the plan is approvable (data-model.md) |
| `/api/runs/<id>/answer` | `{decision, choice \| text}` | Answers a question that needs the engineer |
| `/api/runs/<id>/overrule` | `{decision, text}` | A decide step with the engineer's ruling; affected parts reopen |
| `/api/runs/<id>/criteria` | `{part, id?, text, must_fail}` | Adds or changes a criterion: a planner commit, then reopening and review as FR-018 |
| `/api/runs/<id>/note` | `{part \| role, text}` | Added to that agent's next step instruction |
| `/api/runs/<id>/pause` | `{part?}` | Pauses a part, or the run. The running step ends at its next tool boundary and is resumed later by session |
| `/api/runs/<id>/resume` | `{part?}` | Resumes |
| `/api/runs/<id>/stop` | `{reason?}` | Stops the run (confirmed on the page, constitution II). Branches are kept |
| `/api/runs/<id>/take-over` | `{part}` | Ends the part's running step. Returns the command `claude --resume <session>` and the worktree path |
| `/api/runs/<id>/hand-back` | `{part}` | Verifies the part's branch (R-7) as a step result |
| `/api/runs/<id>/autonomy` | `{level}` | From the next decision on |
| `/api/runs/<id>/limits` | `{usd?, minutes?, repair_cap?, once?}` | Raises the limits or the repair cap, or allows one more step (FR-038, FR-008). Values must be above zero |
| `/api/runs/<id>/retry` | `{}` | Tries again after a question Fallow raised itself (a transient error, or a review without a usable verdict); refused for other questions |
| `/api/runs/<id>/merge` | `{}` | In `ready`: local merge of exactly the verified commit, or `gh pr merge` |
| `/api/runs/<id>/request-changes` | `{part, text}` | A repair round with the engineer's comment, within the cap |
| `/api/runs/<id>/reject` | `{reason?}` | As stop, from `ready` or `publishing` |
