# T001: the check on the installed `claude` (2026-11-05)

`claude --version`: 2.1.318 (Claude Code), with the engineer's normal login and `--model haiku`.
The probes ran in a scratch repository, not tracked by git. Fallow did not read the engineer's
settings file. The total cost was about $0.04.

The probes ran from a shell script rather than as the ignored Rust test T001 names. The script and
the outputs are summarized below.

## Settings (research R-6)

| # | Point | Outcome |
|---|---|---|
| 1 | An empty `--setting-sources` list is accepted; the login and `--model` still work | **Confirmed**: answered "ok" with `--setting-sources ""` |
| 2 | `--setting-sources user` is accepted | **Confirmed** |
| 3 | An allow rule in the working copy's `.claude/settings.json` does not apply | **Confirmed in this setup**: with `user` only, a project rule allowing `Bash(touch:*)` did not apply. But `claude` also refused it with `project` loaded: "Ignoring 1 permissions.allow entry from .claude/settings.json: this workspace has not been trusted". So the probe cannot show the difference in a folder the engineer has trusted. Marking that folder trusted would change the engineer's own Claude configuration, so the probe did not do it. |
| 4 | Under `dontAsk` with an empty source list, a user-level allow rule does not apply | **Not confirmed by the probe**: it cannot know which commands the engineer's list allows without reading the file. The flag's own help says it is the list of sources to load (user, project, local). **Accepted on Claude's documented behaviour** (the engineer, 2026-11-05): with no source listed, no user settings are loaded. |
| 5 | A whole-tool `permissions.ask` rule given through `--settings` beats an allow rule and reaches the permission prompt | **Confirmed**: beats `--allowedTools Bash(touch:*)`, and beats `--allowedTools Write` for a file in an `--add-dir` folder. With `--permission-prompts none`, both were denied as "needs approval", so they went to the prompt path. Beating a *user* allow rule is the same precedence, but is not shown directly (see 4). |
| 5b | No other tool that can write or run things is open to a user allow rule | **Not as the plan assumed**: see "Tools" below |
| 6 | `disableAllHooks: true` through `--settings` stops a hook | **Confirmed**: a project `PreToolUse` hook answering "allow" ran, and let `touch` run, even in the untrusted folder. With `disableAllHooks` it did not run, and the command was denied |
| 7 | Is the project's `CLAUDE.md` still loaded? | **Not loaded** with an empty source list or with `user`. Fallow must hand it as text (T007), as planned |

## Tools and MCP

- **The tools loaded** with `--setting-sources user --strict-mcp-config` (27):
  - shell and files: `Bash`, `PowerShell`, `Edit`, `Write`, `NotebookEdit`, `Read`, `Glob`, `Grep`;
  - web: `WebSearch` (no `WebFetch` on this install);
  - others with effects outside the working copy: `Task`, `CronCreate`, `CronDelete`, `RemoteTrigger`, `ScheduleWakeup`, `Workflow`, `EnterWorktree`, `ExitWorktree`, `SendMessage`, `PushNotification`, `DesignSync`, `Skill`, `Monitor`, `TaskStop`, `ToolSearch`, `ListAgents`, `ReportFindings`.

  `MultiEdit` is not on this install.
- **MCP servers**: without `--strict-mcp-config`, the claude.ai connectors (Claude Docs, Google Drive) load even with an empty source list. Fallow's adapter already passes `--strict-mcp-config`, and with it no MCP server loads.
- **Built-in plugins** load whatever the sources: `cc-plugin-agents-md`, `cc-plugin-telemetry`, `cc-plugin-privacy` and `cc-plugin-plugin-authoring`.

The ask rules in the plan cover only the shell and file tools. A user allow rule for one of the
other tools above (scheduling, remote triggers, worktrees, messages) would let the assistant use
it without Fallow seeing it.

## Runtime contract (R-8)

| Point | Outcome |
|---|---|
| An image sent as a stream-json user message with a base64 png | **Confirmed**: a solid red 16×16 png was answered "Red" |
| `WebFetch` and `WebSearch` in `--disallowedTools` | **Confirmed**: the model had no web tool ("CANNOT") |

## Also noticed

- `--allowedTools` and `--disallowedTools` take a list, so a prompt placed after them on the
  command line is swallowed. The adapter must send the instruction on stdin or after `--`.
- `claude` has a `--restricted` mode. It drops the command-running tools and `WebFetch` unless
  `--tools` names them, ignores user, project and local settings files, keeps file tools inside the
  working directories, and lets only a person or the permission handler approve writes to settings,
  git and tool-configuration files. It is not used by the plan; it may be worth considering for the
  unattended kinds.
