# Contract: agent definitions (017)

This contract replaces the "Format" table and "The defaults" table of [007's agent-definition contract](../../007-fallow-studio/contracts/agent-definition.md). Its other parts (where definitions live, the frontmatter's subset of YAML, what Fallow adds whatever the definition says, how defaults are copied and updated) are unchanged.

## Format

```markdown
---
name: security-reviewer
description: Reads changed code for injection, secrets and auth gaps, and proves each finding by running it
runtime: claude
model: claude-opus-5-5
kind: checker
may_run: [gates, plan.commands, "npm audit"]
returns: verdict
limits: {minutes: 20, turns: 60, usd: 1.5}
---

You check the changes of one user story for security problems...
```

| Key | Required | Rules | What belongs there (the message when it is missing) |
|---|---|---|---|
| name | yes | `[a-z0-9-]+`, equal to the file name | `name is required: the file's name without .md, in a-z, 0-9 and -` |
| description | yes | one line, not empty | `description is required: one line saying what this agent does` |
| runtime | yes | a runtime Fallow knows | `runtime is required: the program that runs this agent, one of claude, fake` |
| model | yes | fits the runtime's model naming ([runtime-model-naming.md](runtime-model-naming.md)) | `model is required: the model's full name. <the runtime's hint>` (every runtime's hint when the runtime is unknown) |
| kind | yes | `maker`, `checker`, `lead` or `assistant` | `kind is required: maker (changes files), checker (only runs and verifies), lead or assistant` |
| may_run | yes | a list; `[]` for a lead or an assistant; each item passes the approved-command check, or is `gates` or `plan.commands` | `may_run is required: the commands this agent may run, as a list: [gates, plan.commands, "npm test"], or [] for none` (for a lead or an assistant: `may_run is required: [] (a lead's commands follow the chat mode)`) |
| returns | yes | `work` for a maker, `verdict` for a checker, `reply` for a lead, `assistant_reply` for an assistant | `returns is required: work for a maker, verdict for a checker, reply for a lead, assistant_reply for an assistant` (only the kind's value when the kind is known) |
| limits | no | per step; defaults: 30 minutes, the runtime's turn default, the loop's remaining budget | (optional) |

Other problems:

| Case | Message |
|---|---|
| model does not fit (`opus`, `Claude-Opus-5-5`, `claude-opus-5-5[1m]`) | `model "opus" is not a full model name for claude: <hint>` |
| model of another runtime's form | the same message, with the file's runtime's hint |
| lead or assistant with a non-empty `may_run` | `may_run: a lead's commands follow the chat mode (Manual or Auto), not this list: write []` (`an assistant's` for an assistant) |
| empty instructions | `the instructions after the frontmatter are empty: write what the agent does, below the closing ---` |

Each problem is `file:line: message`. A missing field's line is its own line when written without a value, else the frontmatter's closing `---` line.

**The body** is required and not empty. It is added to the agent's system prompt, after Fallow's own rules for its kind.

A file with any problem is not saved and, on disk, not used; the earlier valid definition of that name stays in force.

## The defaults

| Name | Kind | Runtime and model | may_run |
|---|---|---|---|
| lead | lead | claude, claude-opus-5-5 | `[]` |
| assistant | assistant | claude, claude-opus-5-5 | `[]` |
| builder | maker | claude, claude-sonnet-5-5 | `[gates, plan.commands]` |
| reviewer | checker | claude, claude-sonnet-5-5 | as shipped |
| test-writer | maker | claude, claude-sonnet-5-5 | as shipped |
| security-reviewer | checker | claude, claude-opus-5-5 | as shipped |
| docs-writer | maker | claude, claude-sonnet-5-5 | as shipped |

## The template for a new agent

`GET /api/agents/template?name=<n>` ([web-api.md](web-api.md)) returns the text a new agent starts from: every required key, `name` filled in, the others with no value, each followed by a `#` comment saying what to write; the runtime's comment names, and the model's comment gives the hint of, each runtime usable on this machine (one with a profile in the settings; `claude` when none is; spec Clarifications); a comment line for `limits` (optional); a comment line saying the instructions go below the closing `---`; an empty body. Saved untouched, it is refused with 6 missing fields and the empty instructions.
