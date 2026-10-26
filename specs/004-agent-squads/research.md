# Research: Agent Squads and Coordination Mode

Phase 0 for [plan.md](./plan.md). Facts checked on 2026-10-26 against the agents' official docs and
sources; items marked **verify** are confirmed during implementation. 003's research still applies;
numbering here is local to 004.

## R-1: A burst is a 003 permission plus a frozen squad

**Decision**: a burst is a record in the hub: the 003 standing permission (source and target
`Project(p)`, deliver + create tasks + start up to the allowance, ≤ 8 h), the lead, the squad as
parsed at start, the start commit and the target branch. Ending it revokes that permission
(`revoke`), so every 003 path (approvals, announcements, the gate test SC-003 of 003) is unchanged.
A pending `--coordinate` burst is only a value in the app until the first session of the project
starts; no hub, channel or git call before that (constitution II).

**Alternatives**: a separate "burst mode" bypassing approvals: rejected, it would duplicate 003's
gate and its tests.

## R-2: Permission levels per agent CLI

| Level | Claude Code | Codex CLI | Gemini CLI |
|---|---|---|---|
| read-only | `--permission-mode dontAsk --disallowedTools Edit Write NotebookEdit` | `--sandbox read-only --ask-for-approval never` | `--approval-mode plan` |
| edit | `--permission-mode acceptEdits` | `--sandbox workspace-write --ask-for-approval on-request` (commands run sandboxed without asking; Codex has no "ask before every command" any more) | `--approval-mode auto_edit` |
| edit-run | `--permission-mode acceptEdits --allowedTools Bash` | `--sandbox workspace-write --ask-for-approval never` | no mapping (only `yolo`); a squad must give flags |
| model | `--model <name>` | `--model <name>` | `--model <name>` |
| bypass flags (warn, FR-014) | `--dangerously-skip-permissions`, `--permission-mode bypassPermissions` | `--dangerously-bypass-approvals-and-sandbox`, `--yolo` | `--yolo`, `--approval-mode yolo` |

Notes:
- **Claude Code `plan` is not read-only in interactive sessions** where bypass is available ("doesn't
  enforce plan mode's blocks"), so read-only uses `dontAsk` (anything that would prompt is denied)
  plus deny rules, which win over allow rules in every mode. `acceptEdits` also auto-approves
  `mkdir`, `touch`, `rm`, `mv`, `cp` and `sed` inside the working directory.
- Claude Code's `--allowedTools` and `--disallowedTools` take several values; Fallow merges them with
  003's `--allowedTools mcp__fallow__*` into one occurrence each, appended at the end (003 R-5).
- **Codex**: `untrusted` is retired, `on-failure` deprecated, `--full-auto` deprecated (**verify** on
  the installed version: Fallow reads `codex --version` when the start menu opens and refuses roles
  whose flags its version does not know).
- **Gemini**: "replaced by Antigravity CLI" for some tiers since June 2026; the profile stays, and an
  `edit-run` role on Gemini needs explicit flags (FR-008: no silent guessing).
- Sources: code.claude.com/docs/en/{cli-reference,permissions,permission-modes};
  learn.chatgpt.com/docs/{developer-commands,sandboxing}; geminicli.com/docs/cli/{cli-reference,plan-mode}.

**Verification**: a CI test asserts the exact argument vector per agent and level; the manual run
(quickstart Q4) checks a Claude Code reviewer cannot edit.

## R-3: Delivering the briefing

**Facts**: Claude Code loads MCP server instructions at session start (limit 4,096 characters);
Gemini CLI adds them to the context ("instructions provided by the tool server…"); Codex attaches
them to the tool namespace description.

**Decision**: the per-role briefing goes into the `instructions` of the `initialize` result, which
the connection knows after the hello (003 R-3): the role is looked up for that session. It is kept
under 3,500 characters; the full text (with the Spec Kit pipeline and the squad's gates) is in
`whoami.briefing`, which the short instructions tell the agent to read first. A board post by Fallow
carries the parts common to all roles. A session that connected before the burst started gets a
delivery (approved by the burst) asking it to call `whoami`.

## R-4: Git, read-only, during a burst

**Decision**: Fallow runs the user's `git` (as 002 R-11) for:
- burst start: `rev-parse HEAD`, `symbolic-ref --short HEAD`, `remote` (has a remote → draft PR);
- read-only roles: `status --porcelain` and `rev-list <start_commit>..HEAD` in the session's worktree,
  when it signals and every 2 minutes (FR-013);
- evidence: `merge-base --is-ancestor <commit> <session branch>` and `diff --name-only
  <start_commit>..<commit>` (FR-023);
- local merge only: `rev-list` of the target branch every 2 minutes (FR-028a), and on the engineer's
  confirmed press, `merge --no-ff <gated commit>` in the project folder, which must be on the target
  branch with a clean working tree (else Fallow refuses with the reason).

Each call is short, on a worker thread, results back on the event channel; nothing is written
except the engineer's merge. **Constitution**: these run only during a burst the engineer started
(opt-in, II) and only read, except the confirmed merge (V).

## R-5: Naming failing checks (baseline classification)

**Decision**: a gate's `failures` setting:
- `auto`: Fallow recognizes the summary lines of common runners in the gate session's output (in
  memory): `cargo test` (`test <name> ... FAILED`), pytest (`FAILED <id>`), Jest/Vitest
  (`✕`/`FAIL <file>`), Go (`--- FAIL: <name>`), .NET (`Failed <name>`); otherwise exit-code only;
- `exit-code`: no names; classification is pass / fail;
- a regex with one capture group.

Classification: final passes → `Pass`; final fails, baseline failed, final failing ⊆ baseline failing
(and both non-empty) → `BaselineOnly`; any failing name not in the baseline → `Regression`; no names
or no baseline → `Fail`.

## R-6: Gate sessions

**Decision**: a gate is a 002 session running the gate's command through the shell profile's
executable (`$SHELL -c "<command>"` on Linux and macOS, `pwsh -NoProfile -Command "<command>"` or
Windows PowerShell on Windows), no orchestration,
role `gate`, in the right worktree (baseline: a temporary worktree at `start_commit`, removed after;
final: the lead's worktree). Its exit status (002 `SessionExited`) is the only input for
`gate-<name>-passed/-failed`, signalled by `Party::Fallow`, a new sender no agent can be. Gates count
against `sessions.max_sessions` but not against the burst's agent allowance.

## R-7: Deadlock notice

**Decision**: every tick, the burst checks whether every running agent session of the burst is in a
wait (event wait, blocked task, or idle with no task) and no gate runs; if that holds for
`wait_notice_min`, one notice lists who waits for what (FR-026), repeated at most every 30 minutes.

## R-8: Spec Kit

**Facts**: requires Python 3.11+ and uv (or pipx). One shot:
`uvx --from git+https://github.com/github/spec-kit.git specify init --here --force --non-interactive --integration claude`
(**verify** the uvx form; documented install: `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git`,
then `specify init --here --force --non-interactive --integration claude`).

**Decision**: detection is "the project has `.specify/`". The offer delivers this command to the lead
as a task (approved by the engineer's "yes"), saying to check for `uv` first and to ask the engineer
if it is missing. Fallow downloads and writes nothing itself (FR-007).

## R-9: Verifying squads

- `cargo test`: squad parsing and validation (`file:line`), level → flags per agent (exact vectors),
  tool filtering and refusals, restricted events, role preconditions, evidence checks against a real
  temporary repository, classification (R-5 table), repair caps, deadlock detection, merge conditions,
  local merge refusal cases.
- `cargo xtask squad` (new, CI on three OSes): Fallow in the PTY harness; lead, two builders and a
  reviewer as `mcp-agent` scripts; a gate whose command passes or fails on demand; the run must end
  with "ready to merge" and no agent handed a merge (SC-009), and must show SC-002/003/004 refusals.
- Memory: `bench-memory --loaded` with a burst of four sessions and a gate (SC-008).
