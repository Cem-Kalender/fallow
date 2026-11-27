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

`{ "text": "<the whole file>", "base"?: "<the text the page loaded>", "replace"?: true }`. Checked as Fallow reads it (agent-definition.md), and against the pipelines in use (as 007); refused with every problem, one per line, `file:line: message`. On success `{ "saved": "<path>" }`.

A file already at that place is written over only when it holds `base` (or the new text already), or when `replace` is `true`, sent after the engineer confirmed (017 review 1, FR-017). A new agent sends no `base`. Otherwise refused: `<path> holds your own <name>.md[, which Fallow could not use], not the text this page loaded: open it to see it, or replace it on purpose`; the page asks the engineer and saves again with `replace`.

A body with `fields` (008's form save) is refused: `an agent is saved as its whole text`.

## `GET /api/runtimes`

`models` is replaced by `model_naming` ([runtime-model-naming.md](runtime-model-naming.md#get-apiruntimes)).

## Unchanged

`DELETE /api/agents/<name>` (Delete, Reset to the default), `PATCH /api/projects/<p>` `{lead}` ("Use for this project").
