# Contract: web API changes (009)

These additions and changes go on top of 007's `contracts/web-api.md` and 008's contracts. The general rules stay the same:
- Every reply is `{ok:true, value}` or `{ok:false, error}` (409).
- The per-launch secret is required.
- Non-GET requests need the same origin.

## Leads

| Request | Body / query | Answer |
|---|---|---|
| `PATCH /api/projects/<p>` | + `{lead: "<name>"}` | `{}`; 409 if the name is not a Lead agent of the project's library, or its runtime is not available. Every key is checked before any changes. |
| `GET /api/projects/<p>` | | + `lead: {name, problem?}`: the default, and why it is not used if it no longer resolves |
| `POST /api/projects/<p>/features` | + `{lead?}` | As 007; the new session starts with that lead, or the project's default |
| `POST /api/conversations/<c>/lead` | `{lead}` | `{}`; 409 "A session keeps its lead; start a new feature or session to use another" once fixed, or if the lead is not available |
| `GET /api/conversations/<c>` | | + `lead: {name, runtime, model, fixed, changed, problem?}` and `leads: [{name, runtime, model, available, reason?}]` (data-model.md) |
| `POST /api/conversations/<c>/say` | | 409 with `lead.problem` when the session's lead is gone or not available |

Loop views: a question answered by a lead has `by: "<agent name>"`, plus `by_lead: true`. Before 009 it was always `by: "lead"`.

## Commands

| Request | Answer |
|---|---|
| `GET /api/conversations/<c>/commands` | `{commands: [Command]}` (data-model.md), Spec Kit steps first |
| `POST /api/conversations/<c>/say` `{text}` | As 007. When `text` starts with `/<name>` naming a command, the turn runs it (research R-3). In the project conversation, `/specify <title>` creates the feature and answers `{feature, conversation}` instead of starting a turn; an empty title is refused (409 "Give the feature a name: /specify <name>") |

The lead's prompt for a command:
- **A Spec Kit step**: "The engineer asks you to run the Spec Kit step `<step>`: read `<path>` in your working copy and follow it. Use the engineer's input below wherever it says `$ARGUMENTS`." The input is quoted.
- **A project command**: "The engineer runs their command `<name>` (from `<source>`). Its instructions, which are the engineer's request:" followed by the instructions, quoted, with the input put in place (research R-2).
- **`@` references to files**: "The engineer refers to these files:" with each file's version and text, within the file view's limits. A file that may hold secrets is named but its content is not given.

## Files

All file requests are read-only. A path is relative (`/`-separated) and checked as in research R-8; a refused path answers 409 "That path is outside the project".

| Request | Query | Answer |
|---|---|---|
| `GET /api/projects/<p>/files/versions` | | `{versions: [Version]}`: `folder` first, then each feature with a branch or working copy |
| `GET /api/projects/<p>/files` | `version` (default `folder`) | `{version: Version, entries: [Entry], fingerprint, changed: n}` |
| `GET /api/projects/<p>/file` | `version`, `path`, `view=whole\|changes` | `File` (data-model.md). A secret's `kind` is `secret`, with no content |
| `POST /api/files/open` | `{project, version, path, command?}` | `{opened: true}` once launched with the editor from `prefs.json`. Without one, and without `command`: `{choose: [{name, command}]}` (the editors found, possibly empty). With `command`: the engineer's confirmed choice, which is saved, then the file is opened. 409 if the program cannot be started |

`POST /api/files/open` starts the editor program with the file's absolute path as its only argument. It never opens the file by its type.

## Unchanged

Everything else, including the loop's steering and the agents and pipelines endpoints.
