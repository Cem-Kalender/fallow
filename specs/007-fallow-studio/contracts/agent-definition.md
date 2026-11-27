# Contract: agent definitions

A definition is a markdown file named `<name>.md`. Fallow looks for it in three places, and the last one found wins:

1. built into Fallow (the defaults);
2. `~/.fallow/agents/`, where the defaults are copied on first start and the engineer's own agents live;
3. `.fallow/agents/` in a project.

## Format

```markdown
---
name: security-reviewer
description: Reads changed code for injection, secrets and auth gaps, and proves each finding by running it
runtime: claude
model: opus
kind: checker            # maker | checker | lead
may_run: [gates, plan.commands, "npm audit"]
returns: verdict         # work (maker) | verdict (checker) | reply (lead)
limits: {minutes: 20, turns: 60, usd: 1.5}
---

You check the changes of one user story for security problems...
```

**The frontmatter** is a strict subset of YAML (research S-11):
- one `key: value` per line;
- strings either bare or in double quotes;
- lists as `[a, b, "c d"]`;
- `limits` as `{k: v, ...}`;
- `#` comments.

Unknown keys, nesting, multi-line values and anchors are refused, with the file and line.

| Key | Required | Rules |
|---|---|---|
| name | yes | `[a-z0-9-]+`, equal to the file name |
| description | yes | One line |
| runtime | yes | A known runtime (`GET /api/runtimes`) |
| model | no | Passed as is; empty means the runtime's default |
| kind | yes | `maker`, `checker` or `lead` |
| may_run | no | Command prefixes that pass the approved-command check, and the words `gates` and `plan.commands` |
| returns | yes | `work` for a maker, `verdict` for a checker, `reply` for the lead |
| limits | no | Per step. Defaults: 30 minutes, the runtime's turn default, and the loop's remaining budget |

> Replaced by 017 (specs/017-agent-file-fields/contracts/agent-definition.md): `model` and `may_run` are required; `model` is the model's full name in the form the runtime declares, and there is no runtime default ("empty means the runtime's default" no longer holds); `kind` is `maker`, `checker`, `lead` or `assistant`.

**The body** is required and not empty. It is added to the agent's system prompt, after Fallow's own rules for its kind.

## What Fallow adds, whatever the definition says (FR-026)

- **Its own worktree**, on the loop's build branch for makers, or on the commit under review for checkers.
- **Rights by kind**:
  - a maker may edit within its worktree, but not the locked documents;
  - a checker may not edit, and a checker whose worktree changes stops counting (006);
  - the lead's rights follow the chat mode (research S-7).
- **Commands**: only `may_run`, expanded with the project's gates and the locked plan's commands, one per call, with no chaining. Commands the runtime classifies as read-only (`git log`, `ls`) also run: they change nothing (research S-7).
- **The result** follows the schema for `returns` (contracts/step-results.md).
- **The `ask` and `progress` tools** through Fallow's MCP bridge. There is no tool to reach another agent.
- **Limits** on time, turns and cost.

## The defaults

| Name | Kind | Runtime and model | Role |
|---|---|---|---|
| lead | lead | claude, opus | The project conversation, spec sessions, answering questions, proposing pipelines |
| builder | maker | claude, sonnet | Works through a phase's tasks; writes the phase's handover |
| reviewer | checker | claude, sonnet | Checks every acceptance scenario of a story by running something |
| test-writer | maker | claude, sonnet | Writes a story's failing acceptance tests before its tasks (Test first) |
| security-reviewer | checker | claude, opus | Security checks of a story's changes, each proven by running something |
| docs-writer | maker | claude, sonnet | Updates the README and quickstart once, at the end |

> Replaced by 017 (specs/017-agent-file-fields/contracts/agent-definition.md#the-defaults): the defaults name full model names (`claude-opus-5-5`, `claude-sonnet-5-5`) and their `may_run`.

On first start, each default is copied to `~/.fallow/agents/`. The engineer's changes are never overwritten (FR-025). When an update of Fallow changes a default:
- a copy the engineer never edited is replaced by the new default;
- an edited copy is kept, and the new default is written beside it as `<name>.md.new`, which the designer mentions;
- a default the engineer deleted is not put back.
