# Research: Headless Orchestrator

All decisions below resolve the plan's Technical Context. Facts marked **verified** were checked on
2026-10-29 on Windows 11, with Claude Code 2.1.312 and gh 2.97.0.

## R-1 One service, a thin command line

**Decision**:
- **`fallow serve`** runs the service in the foreground. It is the only process that holds runs,
  starts agents and serves the page.
- **`fallow`** (no arguments) starts the service in the background if it is not running, then opens
  the page in the default browser.
- **`fallow run "<task>"`** in a repository starts a run: the service is started if needed, and the
  run's page opens.
- **`fallow status`** and **`fallow stop`** do what their names say.
- The command line talks to the service over the existing local channel: the Windows named pipe
  from 003, restricted to the current user. One service per user is enforced by the pipe's name.
- Autostart at login is not written by Fallow (constitution VI: Fallow writes only to its own
  directories). `fallow autostart` prints the shortcut command for the Startup folder, for the
  engineer to run.

**Rationale**: FR-035 asks for one service and runs started from the page or the command line. The
named pipe exists, is user-only, and opens no socket.

**Alternatives considered**: a Windows service (needs elevation for install, against VI); a
registry Run key written by Fallow (writes outside its directories).

## R-2 Run state as an append-only event log

**Decision**:
- **Where:** each run is a directory under `%LOCALAPPDATA%\fallow\runs\<run-id>\`, readable by the
  current user only (ACL set at creation, as 005's records).
- **`events.jsonl`:** an append-only log of typed events (contracts/storage.md), flushed and
  `fsync`ed before Fallow acts on an event. The run's state is the fold of its events. Nothing else
  is authoritative.
- **`steps/<n>.jsonl`:** each step's activity, the agent's streamed events filtered to the kinds the
  page shows.
- **`runs.json`:** an index of runs for the list page, rebuilt from the run directories if missing.

**Resume (constitution VII)**:
- On start, every run whose last stage is not done or stopped is loaded.
- A step with a `step_started` event but no `step_finished` event was interrupted:
  - its agent process is gone, since the Job Object from 002 ends with the service;
  - it is resumed with `--resume <session-id>` and the note "you were interrupted; check the state
    of your working copy and continue";
  - if the session cannot be resumed, the step is redone.
- Completed steps are never repeated, because their `step_finished` events are in the log.

**Rationale**: an event log makes crash safety and history the same thing. The page's activity
feed and the run record are both views of it. No database dependency.

**Alternatives considered**:
- SQLite (`rusqlite` bundled): +1.5 MB binary, a C build on Windows, more than needed for
  one-writer logs.
- A state snapshot file rewritten on each change: loses history, risks torn writes.

## R-3 Agent steps with `claude -p` (verified)

**Decision**: a step is one process with its working copy as the current directory:

```text
claude -p <instruction>
  --output-format stream-json --verbose
  --json-schema <the step kind's result schema>        (contracts/step-results.md)
  --permission-mode dontAsk --allowedTools <role allowlist>   (R-4)
  --mcp-config <step MCP file> --strict-mcp-config      (R-5)
  --session-id <new uuid> | --resume <session-id>
  --model <role model>                                  (FR-037)
  --max-budget-usd <what is left of the run's limit>    (FR-038)
  --append-system-prompt <the role's standing rules>
```

**Verified on 2026-10-29**:
- **The final event:** the last stream event is `{"type":"result", …}`, with:
  - `structured_output` (the object matching `--json-schema`);
  - `total_cost_usd`, `session_id`, `num_turns`, `is_error`;
  - `permission_denials` (the tool uses `dontAsk` refused).
- **The events before it:** `system/init` names the model, tools and MCP servers. `assistant`
  and `user` events carry tool uses and results, which become the activity feed.
- **Turn limit:** this version of `claude` has **no turn limit flag**. A step is bounded by
  `--max-budget-usd` and by a time limit that Fallow enforces by ending the process tree (002's Job
  Object).
- **The engineer's own hooks and settings** apply to headless calls (a user hook ran in the
  probe). That is accepted, and the page's setup check lists them.

**Result handling**:
- A missing or schema-invalid `structured_output`, `is_error: true`, or a time-out each make a
  failed step (spec edge cases).
- `permission_denials` that end the step become a `blocked` result naming the commands (FR-036).

**Rationale**: these flags give a typed result, the cost, resumable sessions, and per-step rights
enforced by the agent itself (constitution VI).

**Alternatives considered**: the Claude Agent SDK (TypeScript or Python). It is a second runtime
next to Rust and is not needed, since the CLI exposes what the SDK would.

## R-4 Role allowlists through permission rules

**Decision**: the rights of each role are Claude Code permission rules passed with
`--allowedTools`, under `--permission-mode dontAsk`, so anything not listed is refused without a
prompt.

| Role | Rules |
|---|---|
| builder | `Read`, `Grep`, `Glob`, `Edit`, `Write`, `Bash(git status:*)`, `Bash(git diff:*)`, `Bash(git log:*)`, `Bash(git add:*)`, `Bash(git commit:*)`, `Bash(git merge:*)`, `Bash(git show:*)`, each declared gate command as `Bash(<command>:*)`, read-only shell tools (`Bash(ls:*)`, `Bash(cat:*)`, `Bash(dir:*)`, `Bash(type:*)`, `Bash(findstr:*)`), plus the squad file's `[allow] builder` |
| reviewer | the builder's list without `Edit`, `Write`, `git add`, `git commit`, `git merge` |
| planner | `Read`, `Grep`, `Glob`, `Edit(.fallow/**)`, `Write(.fallow/**)`, `Edit(<plan docs>)`, `Write(<plan docs>)`, `Bash(git add:*)`, `Bash(git commit:*)`, `Bash(git status:*)`, `Bash(git diff:*)`, `Bash(git log:*)` |

- All roles also get `mcp__fallow__*` (R-5).
- `--dangerously-skip-permissions` and `bypassPermissions` are never passed unless the engineer
  turned them on for that run (constitution VI). The page then shows that the run is unrestricted.
- **Verify (a setup task):** the exact `Bash(...)` prefix matching for commands with arguments,
  on Windows shells. A fixture step runs an allowed and a refused command and checks
  `permission_denials`.

**Rationale**: FR-036 with enforcement by the agent itself, which Fallow then verifies (I).

## R-5 MCP tools during a step, through the existing bridge

**Decision**:
- **The bridge:** each step's MCP file starts `fallow mcp-bridge --step <token>`. That is 003's
  stdio-to-named-pipe bridge, reused, connecting the step to the service. The token identifies the
  run, step and role; the service accepts it only from that step's process tree (003's identity
  check).
- **Tools** (contracts/mcp-tools.md): `plan_read`, `criteria`, `contract_read`, `progress`,
  `ask_decision`.
- **`ask_decision` blocks** for up to **3 minutes** while a decide step runs.
  - Decided in time: it returns the decision commit, and the step continues in context.
  - Otherwise, because the engineer must answer: it returns `{"state":"waiting","question":<id>}`
    and tells the agent to end the step with `status: blocked` and that question id. When the answer
    comes, Fallow resumes the same session (`--resume`) with "take <commit>".

  This keeps the "same step, same context" behaviour of FR-010 without holding a tool call open for
  hours. MCP clients time out long calls.

**Rationale**: no network socket (constitution V), reuse of tested code, and resumable sessions
cover long waits.

## R-6 The page: loopback HTTP with a per-launch secret

**Decision**:
- **Server:** `tiny_http`, a small synchronous Rust HTTP server, no TLS, no async runtime, about
  250 KB.
- **Address:** bound to `127.0.0.1` on a random free port at start.
- **Authentication** (constitution V):
  - a 256-bit secret from the OS random source, per launch;
  - the first request carries it as `?k=`; it is exchanged for an `HttpOnly; SameSite=Strict`
    cookie and dropped from the URL;
  - every request without the cookie gets 401.
- **Request checks:** `Host` must be `127.0.0.1:<port>` or `localhost:<port>`, against DNS
  rebinding. State-changing requests must carry `Origin` equal to the page's origin.
- **The page:** one HTML file, one script and one stylesheet, embedded in the binary
  (`include_str!`). No framework, no build step, no external fonts or CDNs. Headers:
  `Content-Security-Policy: default-src 'self'` and `X-Frame-Options: DENY`.
- **Live updates:** Server-Sent Events, `GET /api/runs/<id>/events`, resuming from `Last-Event-ID`
  (the event sequence number). One thread per open stream, since there is a single user.
- **Notifications:** the page uses the browser's Notification API (localhost is a secure context)
  for "needs you". The service also prints the line on its console. Native Windows toasts are
  deferred.

**Rationale**: the smallest server that meets V and VIII. The UI is the mockup, made real.

**Alternatives considered**:
- `axum` with `tokio`: an async runtime and about 2 MB, for a single user.
- WebSockets: SSE is one-way, which is all the feed needs. Actions are plain POSTs.

## R-7 Verification Fallow does itself

**Decision** (constitution I):
- **Commit on branch:** `git merge-base --is-ancestor <commit> <step branch>`, and the commit is
  newer than the step's start.
- **Files:** `git diff --name-only <step base>..<commit>` against the part's `owns` and the plan's
  `shared` globs. Files outside fail a builder's result; for the planner, only plan files count.
- **Gates:**
  - the declared commands are run by Fallow as plain processes in a temporary worktree of the
    commit, with a time limit and output captured to the step's activity;
  - each run is classified against the baseline on the starting commit, using 004's classification,
    adapted from the PTY gate session to a process.
- **Verdicts:** the reviewer's `structured_output` lists answers per `part/id`. Completeness is
  005's check, with ids qualified by part: every criterion answered, with what ran and the result,
  and every must-fail criterion with a failing case.
- **Reviewer integrity:** the reviewer's worktree must be unchanged after the step (004).

## R-8 The plan reuses 005's manifest

**Decision**:
- **Format:** the plan is `.fallow/work.toml` in 005's format, with two changes:
  - criterion ids need only be unique **within a part** and are addressed as `part/id`;
  - `build` is `"builder"` (one builder, the default) or `"parts"`, since the lead no longer builds.

  Prose lives in the documents the manifest lists (`docs`). In Spec Kit projects these are its spec,
  plan and tasks, and the planner's briefing points to `/speckit-specify` and `/speckit-plan`. This
  resolves the item deferred in clarification.
- **Validation:** the existing parser and checks: owned files that don't overlap, code and its
  tests, a committed contract per boundary, and at least one must-fail criterion per part. A plan
  that fails them cannot be approved (FR-021). The page names the problems with file and line (the
  T050 fix).

## R-9 Decisions, reopening, and the branches of a run

**Decision**: the branches of run `r` are:

| Branch | Holds |
|---|---|
| `fallow/r<r>/plan` | the plan, and every decision commit |
| `fallow/r<r>/<part>` | each builder's work; in solo, one part named `main` |
| `fallow/r<r>` | the integration, merged by Fallow |

How a decision flows:
- **The decision:** a decide step commits on the plan branch.
- **Reopening:** Fallow computes the parts affected (criteria changed for that part, or a contract
  the part is `between`) and opens a **take** step for each, including parts already done. A take
  step resumes the part's session with "merge <commit> and check your work against it".
- **Refusing stale results:** a result whose commit does not contain every decision commit
  affecting its part is refused, naming the commit (005 R-5).
- **Overruling:** the engineer's overrule is a decide step whose instruction is the engineer's
  ruling, followed by the same reopening.

## R-10 Integration and conflicts

**Decision**:
- **Merging:** Fallow merges the parts' branches into the integration branch in its own worktree,
  in the plan's part order, with `--no-ff`.
- **Conflicts:** the merge is aborted, and a repair goes to the part being merged, with the files in
  conflict and the other part's commit to take.
- **Integration gate failures:** a failing test is attributed to the part that owns the test file.
  When no part owns it, or several do, the engineer is asked (FR-023).

## R-11 Publishing with the engineer's own tools (verified: gh 2.97.0 signed in)

**Decision**:
- **Remote and rules:**
  - the remote is `origin` if present;
  - the rules come from `gh api repos/{owner}/{repo}/rules/branches/{target}` (readable with read
    access), with `gh api …/branches/{target}/protection` when the engineer has admin rights;
  - required status checks are matched by context name.
- **Local merge:** without a remote, or with an unprotected target, it is 004's local merge of
  exactly the gated commit, after the engineer's click.
- **Pull request:**
  - with protection, Fallow runs `git push origin fallow/r<r>`, then
    `gh pr create --base <target> --head fallow/r<r> --body-file <run summary>`;
  - checks are polled every 30 s with `gh pr checks --json name,state,bucket`;
  - a failing required check becomes a repair (FR-026);
  - with nothing required and nothing reporting after 5 minutes, the page says "No CI found";
  - the engineer's Merge runs `gh pr merge --merge`, and GitHub enforces the rules.
- **Never:** `--force`, `--admin`, `gh pr review --approve`, or a push to the target branch. CI
  checks these call sites (constitution II).
- **Network:** these are the engineer's tools making the requests, with the engineer's credentials
  (constitution V). Fallow's own process makes none, which the egress test checks.

## R-12 Memory and concurrency (carried over)

**Decision**: 005's memory budget (R-14 and FR-037/038 there), applied to step processes:
- measured memory of running agent process trees, plus the expected size of starts not yet
  measured, against the budget;
- a step that does not fit waits, with the reason on the page;
- a default concurrency cap of 2 builder steps (the squad's `max_parallel_builders`).

## R-13 Measurement (carried over)

**Decision**:
- **Carried over from 005:** task sets, run copies, hidden tests after the run's agents end,
  fingerprints and run records, with the record field rules unchanged (constitution V).
- **Plain mode:** one `claude -p` call with the task description, the builder's allowlist, no MCP
  tools and no Fallow briefing, in the copy. Then the hidden tests.
- **Solo and squad:** this feature's runs with autonomy High, so that runs need no engineer
  between the plan and the merge. The plan step uses the task's description as the engineer's only
  turn, and approval is automatic for recorded runs (they are set up to need no interventions).

## R-14 What is removed, and the dependency budget

**Decision**:
- **Removed:** the terminal app's code (editor, terminal session hosting, process monitor and
  network attribution, TUI), and its crates: `crossterm`, `ratatui`, `ropey`, `tree-sitter` and
  its 17 grammars, `notify`, `grep-*`, `ignore`, `arboard`, `vt100`, `portable-pty`, `ipnet`,
  `compact_str`, `smallvec` and `regex` if no longer used.
- **Kept:** `serde`, `toml`, `serde_json`, `globset`, `windows-sys` (trimmed features), and from
  the code:
  - orchestration git helpers, gates (adapted), manifest, task sets, runs and records, the memory
    sampler and caps;
  - the named-pipe channel and `mcp-bridge`;
  - config paths and merging;
  - process Job Objects.
- **Added:** `tiny_http` only. UUIDs and secrets come from `BCryptGenRandom`, already in
  `windows-sys`.
- The old benchmarks (startup to frame, editor memory) are replaced by the constitution VIII ones
  (R-16).

## R-15 Testing without real agents

**Decision**:
- **A fake agent:** `fallow-fake-agent`, a test binary in `xtask`, that speaks the same stream-json
  and result format as `claude -p`, scripted per scenario: edits, commits, false claims, blocked
  results, `ask_decision` calls, crashes and slow steps.
- **Wiring:** an agent profile with `command = <fake agent>` makes every test and benchmark run
  without network or cost.
- **Real agents:** the quickstart covers them manually (V1 to V8).

## R-16 Benchmarks and constitution gates

**Decision**: the new CI benchmarks run on the Windows runner. The reference run is a pinned fixture
project and a fake-agent script with 4 concurrent steps.
- **Idle memory:** under 60 MB.
- **Running memory:** under 200 MB.
- **Page ready:** under 500 ms (p95 of 30 launches after 3 warm-ups).
- **Activity latency:** under 2 s (p95).

The gates are tests:
- false claims are refused;
- no call site pushes to the target branch, uses `--force` or `--admin`, or approves a pull
  request;
- the listener is loopback only, and requests without the secret, with a foreign `Origin` or with a
  foreign `Host` are refused;
- the page references no external URL;
- the egress test: Fallow's own process opens no outbound connection while a run publishes through
  a fake `gh`;
- a resume test at every stage;
- the run-record marker test.
