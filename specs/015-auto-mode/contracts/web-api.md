# Contract: the page's requests for 015

The window's requests go through `service::answer` (014). Engine requests are `studio::Req`.

## Changed

### `POST /api/sessions/<s>/mode` (the assistant) and `POST /api/conversations/<c>/mode` (the lead)

Body: `{ "mode": "manual" | "auto" }`, for both. This was `"ask" | "until_switch"` for the assistant, and `read_only`, `approved`, `ask` or `run_anything` for the lead. The old values are refused with `no mode "<value>": manual or auto`.

- Sets the chat's mode, appends `chat_mode`, and adds the chat note (spec FR-006).
- Switching to `auto` answers a waiting request under Auto (research R-8).
- Answer: `{}`, or `{ "error": … }` for a chat that is closed or gone.

`Req::SessionMode { session, mode }` and the lead's mode request keep their shape and take the new values.

### `POST /api/approvals/<id>` (the existing approve route)

The `answer` field gains the value `"auto"`: "Switch to Auto" on a Manual card. It allows this request once and sets the chat to Auto. It is accepted for every request kind, because Auto allows reads too (R-2).

## New

### `POST /api/conversations/<c>/refusals/<id>` (both chats: a solo session is a conversation)

Body: `{ "answer": "allow_once" | "dismiss" }`.

- `allow_once`:
  - records `refusal_allowed`;
  - adds the grant;
  - tells the agent (research R-5): queued if a turn runs, sent now if the chat is idle;
  - answer: `{ "told": "queued" | "sent" }`.
- `dismiss`: records `refusal_dismissed`; answer: `{}`.
- `allow_once` is refused for a `control` refusal ("this file is never editable").
- Refused when the refusal is not in the `refused` state ("already answered"), or when the chat is closed.

`Req::Refusal { conv, id, answer }`.

## Conversation view

As data-model.md "Views": `mode`, `mode_note`, `default_mode`, and `refusals` per turn, in both chats.

## Dashboard (`GET /api/dashboard`)

`needs_you` items gain `kind: "refused"`:

```json
{ "kind": "refused", "key": "<s>:refused:<id>", "project": "<project id>",
  "conv": "<c>", "text": "<chat name> — Fallow refused: <what, ≤ 80 chars>", "at": "<time>" }
```

Clicking it opens the chat at the refusal.

## What the runtime receives

- **Allowed in Auto**: the runtime's ordinary allow reply (`agent::registry::permission_reply(runtime, true, "", raw)`), the same as for an approved command.
- **Refused**: the runtime's deny reply with the message `Fallow refused this: <reason>. The engineer can allow exactly this once from the chat; if they do, you will be told.`
- **Allowed once**: the matching request's ordinary allow reply. Before it, the assistant received a message from Fallow: `The engineer allowed, once, exactly: <what>. Run exactly that again if it is still needed; anything else is decided as before.`
