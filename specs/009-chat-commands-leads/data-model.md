# Data Model: Chat commands and lead choice (009)

009 stores three new things: the project's default lead, a session's lead (with the runtime and model it ran on), and the engineer's editor. The rest is derived when the page asks.

## Project (changed, `projects.json`)

| Field | Type | Rule |
|---|---|---|
| lead | string, optional | The project's default lead. Absent means `lead`. Set by `PATCH /api/projects/<p>` `{lead}`. It must name an agent of kind Lead in the project's library with an available runtime (FR-013, FR-020). Not checked again later: when it no longer resolves, the project view says so and new sessions use `lead` (edge case). |

`patch_project` checks every key of a patch before it changes any (R-4).

## Conversation (changed, `conversations/<c>/events.jsonl`)

**Events**:

| Event | Change |
|---|---|
| `conversation_created` | + `lead: string?` (`#[serde(default)]`): the lead at creation, from the project's default or the engineer's pick at "+ New feature" |
| `lead_set` (new) | `{lead}`: the engineer picked another lead. Valid only while no turn has started that was not withdrawn and has a session (FR-014, FR-015) |
| `turn_started` | + `runtime: string?`, `model: string?` (`#[serde(default)]`): what the lead's call ran on |

**State** (folded):

| Field | Type | Rule |
|---|---|---|
| lead | string? | The last `lead_set`, else `conversation_created.lead`; `None` reads as `lead` (FR-018) |
| lead_runtime, lead_model | string? | Taken from `turn_started` when it begins an agent session (the turn's `session` is new, not a resume), and kept for as long as that session lasts. Cleared with the session (a withdrawn first turn, R-4 of 008) |

**Derived in the view** (`GET /api/conversations/<c>`):

| Key | Rule |
|---|---|
| lead.name | `state.lead` or `lead` |
| lead.runtime, lead.model | From the definition as it is now |
| lead.fixed | `true` once a turn that was not withdrawn has a session: the lead cannot change (FR-015) |
| lead.changed | The definition's runtime or model differs from `lead_runtime`/`lead_model`: the next message starts a new agent session (FR-019a) |
| lead.problem | The lead is gone or unavailable (deleted, not kind Lead, runtime not available), with the reason; `say` refuses with it (FR-019) |
| leads[] | Every Lead agent of the project's library: `{name, runtime, model, available, reason}`, for the picker (FR-014, FR-020) |

## Lead for a loop's questions (changed, in memory)

`run::questions::Lead` + `agent: String`. It is filled by `Studio::lead_of` from the feature's spec session (R-5). Nothing new is stored in the loop's log; the answer's `by` names the agent.

## Command (derived, per conversation)

| Field | Type | Rule |
|---|---|---|
| name | string | `plan`, `review-copy`, `frontend:review` |
| group | `spec_kit \| project` | Fallow actions and agents are the page's own (R-9) |
| description | string | From the frontmatter, else the first line |
| hint | string? | `argument-hint` |
| source | string | The file, relative to the project, e.g. `.claude/commands/review-copy.md` |
| runtime | string? | Whose folder it came from (`claude`, `gemini`, `copilot`, `cursor`), for "where it came from" |
| available | bool | False when the file is too large (over 64 KiB), or the step's skill is missing in the working copy |
| reason | string? | Why not |

**Rules**:
- Spec Kit steps come first, then project commands in the order of R-2's table. The first of a name wins when a message is sent.
- `speckit-*` skills are never project commands.
- The project conversation lists only `specify` among the steps.

## File view (derived)

**Version**:

| Field | Type | Rule |
|---|---|---|
| id | `folder \| feature:<f>` | |
| label | string | "Project folder", or the feature's name |
| kind | `folder \| loop \| spec \| branch` | Which copy is read (R-8): the loop's build worktree, the spec session's worktree, or the branch via git |
| running | bool | The feature's loop is running |
| base | commit? | `merge-base` with the feature's base branch; none for `folder` |

**Entry** (one per file, flat):

| Field | Type | Rule |
|---|---|---|
| path | string | Relative, `/`-separated |
| mark | `changed \| added \| deleted`? | Against the base, uncommitted changes included |
| spec | bool | Under the feature's folder |
| secret | bool | Matches R-8's secrets list: never read |

The list carries a `fingerprint`: a hash of the paths and marks, for the 3-second refresh (FR-034).

**File**:

| Field | Type | Rule |
|---|---|---|
| path, version | | As asked |
| kind | `text \| binary \| secret \| deleted` | `secret` and `binary` carry no content |
| text | string? | Up to 512 KiB |
| cut | bool | The file is longer than what is shown |
| size | int | Bytes |
| added | `[[from, to]]` | Line ranges added against the base |
| changes | string? | The unified diff against the base (asked for with `view=changes`) |
| last | object? | `{who, task?, step?, commit?, when, live}`. `who` is the agent definition, `lead` or `you`. `live` means the change is not committed yet and a loop is at work |

## Preferences (new, `<data>/prefs.json`)

| Field | Type | Rule |
|---|---|---|
| editor.command | string | The program "Open in my editor" starts with the file's path. It is set only when the engineer confirms one (R-10), and can be changed the same way |
