# Contract: Fallow's MCP tools

These tools are served to agents through `fallow mcp-bridge`, on 006's named pipe, with a token per step that is checked against the step's Job Object (006 R-5). There is no tool to message another agent or the engineer (constitution III, FR-019).

| Tool | Who gets it | Arguments | Returns |
|---|---|---|---|
| `ask` | every agent in a loop | `{question, options?: [string]}` | Up to 3 minutes for the lead's call, then one of: `{state: "answered", answer, by: "lead", commit}`; `{state: "decided", answer, commit}` (Free level, the lead decided a small open question; the engineer can overrule it later); `{state: "waiting", question: <id>}` (end the step with `status: blocked` and `question: <id>`; it is resumed with the answer) |
| `progress` | every agent in a loop | `{text}` | `{}`. A line for the feed |
| `approve` | the lead, in chats in Ask mode | Claude Code's permission prompt input: `{tool_name, input, tool_use_id}` | A text content holding JSON: `{"behavior": "allow", "updatedInput": <input>}` after the engineer allows it, or `{"behavior": "deny", "message": "…"}`. It denies at once, without asking, any tool other than `Bash` and `PowerShell` and any edit outside the spec paths, and denies after 10 minutes without an answer (research S-7, verified by T001) |

**Rules:**
- One open `ask` per step. A second one while the first waits is refused.
- An `ask` while the loop is paused waits like any other question.
- A step's token is accepted only while the step runs, and only from its process tree.
- Answers are commits to `specs/NNN-name/spec.md`, in its `## Clarifications` section under `### Loop <date>`, written by Fallow as `- Q (Tnnn, <agent>): … → A (<lead|engineer>): …`; a decision of the lead as `→ A (lead, decided): …`; an overruling as `→ A (engineer, overruling the lead): …`.
- Fallow commits only the clarification: when the spec session's `spec.md` has changes that are not committed, the answer is not committed (the question goes to the engineer, or the engineer is told why). While the feature is unlocked, the lead's answers are relayed and the engineer answers after the new lock.
- The commits are made on the feature branch, in the spec session's worktree. The build branch takes them in (a merge) when the next step starts in the build worktree.
- A step whose question went to the engineer ends blocked on it, whatever it returns: a builder, a checker or a handover, and whether it ends `blocked` or carries on. The loop starts nothing new until the engineer answers; then that step runs again in its session with the committed answer (FR-021). This holds when the answer comes while the step still runs, after it ended, or after Fallow restarted. A step that ends while its question is still with the lead is judged once the lead has answered.
- After a restart, a question whose lead call was cut short goes to the engineer.
