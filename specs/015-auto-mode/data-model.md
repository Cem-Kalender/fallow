# Data model: Manual and Auto mode (015)

## Chat mode (in memory, per spec session and per solo session)

| Field | Type | Notes |
|---|---|---|
| `mode` | `manual` \| `auto` | `ChatMode`. Replaces the assistant's `allow_until_switch` and, as the lead chat's mode, `agent::rights::Mode`. Set by the switch, Shift+Tab, or "Switch to Auto". At load, it comes from `[defaults] chat_mode`. |
| `mode_note` | `Option<String>` | Shown once after a restart, when the mode differs from the one before (research R-6). Replaces `mode_reset`. |
| `grants` | list of `{ kind, what }` | One-shot, from "Allow this once"; used up by the first matching request; lost at a restart. |
| `waiting` (lead) | list of Fallow's messages | Sent when the lead's turn ends (R-5). The assistant already has one (012). |

**On disk**: `<conversation dir>/mode` holds the mode the chat was last in. It is read only to word the restart note.

## Settings

`settings.toml`:

```toml
[defaults]
chat_mode = "manual"   # or "auto": the mode of new chats (lead and assistant) and of every chat after a restart
```

Any other value is a settings error, named the way the other `[defaults]` keys are named.

## Agent rights (`src/agent/rights.rs`)

- `Mode` and `Mode::next` go, together with `Rights::any_command`.
- `lead(approved)` always has the prompt tool.
- `lead_in_loop(approved)` stays approved-only, with no prompt tool, as today.
- `assistant(..)` is unchanged.

## Conversation events (appended to `events.jsonl`)

| Event | Fields | When |
|---|---|---|
| `refused` | `id` (`r1`, `r2`, … per chat), `kind` (`edit`, `run`, `tool`), `what` (path, command, URL or tool name), `reason` (plain words), `limit` (`edit`, `target`, `tool`, `control`), `facts` (optional: 012's folder facts) | Fallow refused a request (research R-4), in either chat and either mode. |
| `refusal_allowed` | `id` | The engineer chose "Allow this once". |
| `refusal_used` | `id` | The agent's request used the grant. |
| `refusal_dismissed` | `id` | The engineer chose "Dismiss". |
| `chat_mode` | `mode` | The engineer changed the mode, for the chat's one-line notes. It is never used to restore the mode. |
| `mode_set` (old) | `mode` | The lead's old event. Read and ignored. |

The existing `approval_answered` with `auto: true` keeps meaning "allowed without the engineer". Its `why` is `"switched to Auto"` (research R-8).

## Refusal states

```text
refused ──Allow this once──▶ allowed (grant waiting) ──matching request──▶ used
   │                                   │
   └──Dismiss──▶ dismissed             └──restart──▶ expired (the card says so)
```

Only `refused` is a "Needs you" item (R-9). A `control` refusal offers only "Dismiss".

## Views: changed keys

In the conversation view of both chats:
- `mode`: `"manual"` or `"auto"`. For the lead it was `read_only`, `approved`, `ask` or `run_anything`; for the assistant, `ask` or `until_switch`.
- `mode_note`: a string or null. It replaces `mode_reset`.
- `default_mode`: `"manual"` or `"auto"`.
- `refusals`: per turn, `[{ id, kind, what, reason, limit, facts?, state }]`.
