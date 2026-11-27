# Contract: a runtime's model naming (017)

Amends [007's runtime contract](../../007-fallow-studio/contracts/runtime.md) and [012's](../../012-solo-session/contracts/runtime.md).

## What every runtime declares

Every runtime adapter declares a `ModelNaming` constant: how its provider's full model names look and where the engineer looks them up. Fallow keeps no list of models, and neither declares nor fetches one (constitution V, IX).

| Field | Meaning |
|---|---|
| `prefix` | what every full name of this runtime's provider starts with |
| `example` | one full name, for the hint |
| `lookup` | where the names are listed, in words |
| `lookup_url` | that place's address, or none |
| `families` | the provider's family words, for run records only (FR-022); not serialized, never a list to choose from |

`agent::registry::model_naming(runtime)` returns it for a runtime Fallow knows, without settings or a profile.

**A name fits** when it starts with `prefix`, has at most 64 characters, and the rest is non-empty and only lowercase letters, digits, `.` and `-`. Fallow does not check whether the provider has a model of that name.

**A run record keeps a name** (`recordable()`, FR-022) only when it is `<prefix><family>-<version>`: `family` one of `families` (none: the version follows `prefix`), the version only digits separated by `.` or `-`, at most 64 characters.

**The hint** (`hint()`): "`<prefix>` followed by lowercase letters, digits, `.` and `-`, for example `<example>`; look the name up in <lookup> (<lookup_url>)".

## The runtimes in this version

| Runtime | prefix | example | lookup |
|---|---|---|---|
| `claude` | `claude-` | `claude-sonnet-5-5` | Anthropic's models overview, `https://platform.claude.com/docs/en/models/overview`; families opus, sonnet, haiku, fable |
| `fake` | `fake-` | `fake-1` | the fake runtime's scenario (no address); no families |

## The model at the call

- `Call.model` is the definition's `model`, as written. A runtime MUST NOT replace it or fall back to a model of its own; a call without a model is refused before the program starts (failure: `the agent has no model: its definition must name one`).
- `claude` passes it as `--model <name>`.
- A name the provider does not accept fails the call with the runtime's own message, reported as any failed call ("the agent ended with an error: …").

## `GET /api/runtimes`

`models` is removed. Each runtime gains `model_naming`:

```json
{ "name": "claude", "installed": true, "version": "…", "capabilities": { … },
  "model_naming": { "prefix": "claude-", "example": "claude-sonnet-5-5",
                    "lookup": "Anthropic's models overview",
                    "lookup_url": "https://platform.claude.com/docs/en/models/overview",
                    "hint": "claude- followed by lowercase letters, digits, . and -, for example claude-sonnet-5-5; look the name up in Anthropic's models overview (https://platform.claude.com/docs/en/models/overview)" } }
```
