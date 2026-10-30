# Contract: Fallow's MCP tools

These tools are served to agents through `fallow mcp-bridge`, on 006's named pipe, with a token per step that is checked against the step's Job Object (006 R-5). There is no tool to message another agent or the engineer (constitution III, FR-019).

| Tool | Who gets it | Arguments | Returns |
|---|---|---|---|
| `ask` | every agent in a loop | `{question, options?: [string]}` | Up to 3 minutes for the lead's call, then one of: `{state: "answered", answer, by: "lead", commit}`; `{state: "decided", answer, commit}` (Free level, the lead decided a small open question; the engineer can overrule it later); `{state: "waiting", question: <id>}` (end the step with `status: blocked` and `question: <id>`; it is resumed with the answer) |
| `progress` | every agent in a loop | `{text}` | `{}`. A line for the feed |
| `approve` | the lead, in chats in Ask mode | Claude Code's permission prompt input: `{tool_name, input}` | `{behavior: "allow" \| "deny", message?}` after the engineer answers on the page, or `deny` after 10 minutes (research S-7) |

**Rules:**
- One open `ask` per step. A second one while the first waits is refused.
- An `ask` while the loop is paused waits like any other question.
- A step's token is accepted only while the step runs, and only from its process tree.
- Answers are commits to `specs/NNN-name/spec.md`, in its `## Clarifications` section under `### Loop <date>`, written by Fallow as `- Q (Tnnn, <agent>): … → A (<lead|engineer>): …`.
