# Contract: runtime interface changes (012)

These changes are amendments to 007's [runtime contract](../../007-fallow-studio/contracts/runtime.md).
Every runtime implements them (constitution IX, FR-010a).

## Capabilities

```rust
pub struct Capabilities { pub images: bool, pub web_off: bool, pub control_files: Vec<String> }
pub trait Runtime: Send + Sync {
    // ... as 007
    fn capabilities(&self) -> Capabilities;
}
```

| Capability | Meaning | Without it |
|---|---|---|
| `images` | the runtime delivers `Call.attachments` to the model with the instruction | Fallow refuses attachments before sending, saying so (FR-010) |
| `web_off` | the runtime can run a call with web tools disabled (`Call.web = false`) | the session's web switch is disabled, and the session says it cannot switch off (FR-009) |
| `control_files` | the files in a working copy through which the runtime reads permission rules or settings of its own (Claude: `.claude/settings.json`, `.claude/settings.local.json`) | none: Fallow protects nothing more |

`GET /api/runtimes` gains `capabilities` for each runtime.

## Which settings apply

A runtime MUST NOT apply settings from the working copy: a repository or an agent must not be able
to grant a command. For unattended kinds (the lead, makers, checkers), it MUST apply no settings
of its own, so their rights are exactly Fallow's. For the assistant, the engineer's user-level
settings may apply, but every command and edit MUST still reach the permission prompt tool. A
runtime that cannot guarantee this refuses the call, naming why, as for any right it cannot
enforce.

Claude:
- for the lead, makers and checkers: `--setting-sources` with an empty list, and `--model` always
  given;
- for the assistant: `--setting-sources user` and `--settings <json>`, with whole-tool
  `permissions.ask` rules (`Bash`, `PowerShell`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`) and
  `disableAllHooks: true`.

Every Claude call also passes `--tools` with a fixed list: `Read`, `Glob`, `Grep`, `Bash`,
`PowerShell`, `Task` and Fallow's MCP tools for every kind; `Edit`, `Write` and `NotebookEdit` for
makers, the lead and the assistant; `WebSearch` and `WebFetch` for the assistant with web access on.
Every other tool is off. The instruction goes on stdin.

Fallow never reads the user's settings file. T001 checked this behaviour on the installed `claude`
(`checks/claude-check.md`). The control files are added to every call's `protect`, and the project's instruction
files are handed to every kind as plain text.

## Call

| Field | Meaning |
|---|---|
| attachments | `[{path, media_type}]`: png, jpeg, gif or webp, up to 4 of 5 MiB each, stored by Fallow. Empty for every kind but the assistant |
| web | `true` only for the assistant with web access on. A runtime with `web_off` must disable its web tools when it is `false`; one without `web_off` is never given `false` for an assistant, and for other kinds `false` changes nothing (as before 012) |

## Permission requests

A runtime's permission prompt reaches Fallow as `Request { kind: run | edit | web | other,
command?, path?, url?, query?, tool }` (`url` for a fetch, `query` for a search). The adapter maps its own tools to these kinds (Claude: `Bash` and
`PowerShell` → `run`; `Edit`, `Write`, `MultiEdit` and `NotebookEdit` → `edit`, from `file_path` or
`notebook_path`; `WebFetch` and `WebSearch` → `web`). Fallow judges only this shape and denies
`other` (IX).

## Events

`tool_call.kind` gains `web`: a request to the web made as a tool call. `summary` is the URL or the
query. Requests made by commands the agent runs stay `run` calls.

## The result schema for the assistant

`schemas::assistant_reply()`:

```json
{ "reply": "string",
  "proposed_feature": { "title": "string", "brief": "string" },
  "proposed_pipeline": { "...": "as 009" } }
```

Only `reply` is required. A suggestion of a feature is read only from `proposed_feature` (FR-030).

## The runtimes in this version

| Name | images | web_off | How |
|---|---|---|---|
| `claude` | yes, once the check task confirms it | yes | Images go as a stream-json user message on stdin (`--input-format stream-json`), as base64 image blocks. Web off is `WebFetch` and `WebSearch` in `--disallowedTools`. `WebFetch` and `WebSearch` tool uses map to `web` |
| `fake` | from its profile (`images`, `web_off`) | from its profile | A scenario step can set `expect_images = n` (the step fails on any other count) and `web = ["<url>", …]` (emitted as `web` tool calls). Its `output` can hold `proposed_feature` |
