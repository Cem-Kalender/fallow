# Contract: the agents' web API (017)

Amends [007's web API](../../007-fallow-studio/contracts/web-api.md) and replaces [008's agents API](../../008-studio-redesign/contracts/agents-api.md). Errors are 409 with the message, as every refusal in 007's web API.

## `GET /api/agents?project=<p>`

As 007/008, plus `unusable`:

```json
{
  "agents": [ { "name": "builder", "kind": "maker", "runtime": "claude", "model": "claude-sonnet-5-5",
                "may_run": ["gates", "plan.commands"], "source": "default", "file": "…", "text": "…",
                "update": null, "used_by": ["standard"], "shipped": true, … } ],
  "problems": [ "…/reviewer.md:5: model \"sonnet\" is not a full model name for claude: …" ],
  "unusable": [ { "name": "reviewer", "file": "/Users/…/.fallow/agents/reviewer.md", "source": "user",
                  "text": "---\nname: reviewer\n…",
                  "problems": [ { "line": 5, "message": "model \"sonnet\" is not a full model name for claude: …" } ],
                  "in_force": "default" } ]
}
```

`in_force`: the source of the definition of that name in force (`default`, `user`, `project`), or `null` when none is.

## `GET /api/agents/template?name=<n>`

`{ "text": "<the template>" }` ([agent-definition.md](agent-definition.md#the-template-for-a-new-agent)). Refused when `<n>` is not `[a-z0-9-]+`.

## `PUT /api/agents/<name>?scope=user|project:<p>`

Only `{ "text": "<the whole file>" }`. Checked as Fallow reads it (agent-definition.md), and against the pipelines in use (as 007); refused with every problem, one per line, `file:line: message`. On success `{ "saved": "<path>" }`. Saving over an unusable file at that scope replaces it.

A body with `fields` (008's form save) is refused: `an agent is saved as its whole text`.

## `GET /api/runtimes`

`models` is replaced by `model_naming` ([runtime-model-naming.md](runtime-model-naming.md#get-apiruntimes)).

## Unchanged

`DELETE /api/agents/<name>` (Delete, Reset to the default), `PATCH /api/projects/<p>` `{lead}` ("Use for this project").
