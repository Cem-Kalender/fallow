# Contract: MCP tools during a step

Served to each agent step through `fallow mcp-bridge --step <token>` (research R-5).
- **The token** names the run, the step and the role. The service accepts it only from that step's
  process tree, and only while the step is running.
- **Messaging:** there are no tools to message another agent or the engineer (FR-012).

| Tool | Roles | Arguments | Returns |
|---|---|---|---|
| `plan_read` | all | `{doc?}` | The plan (the manifest, or a listed doc) at the plan commit in force |
| `criteria` | all | `{part?}` | The criteria in scope for this step as `part/id`, text, must_fail |
| `contract_read` | all | `{path}` | A contract at the plan commit in force |
| `progress` | builder, reviewer | `{text}` | `{}`. A line for the page's activity feed ("tests written") |
| `ask_decision` | builder | `{question, proposed_diff?}` | Waits up to 3 minutes. Then either `{"state":"decided","answer":…,"commit":…|null}` (merge the commit, then continue) or `{"state":"waiting","question":<id>}` (end the step with `status: blocked` and `question: <id>`; the step is resumed with the answer) |

**Rules:**
- `ask_decision` is the only way a builder raises a decision. One question at a time per step.
- A question raised while the part is reopened or paused waits like any other.
- The tools' results never contain hidden-test files or other runs' data (FR-029).
