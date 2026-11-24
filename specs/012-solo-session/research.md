# Research: Solo sessions

Decisions for [plan.md](./plan.md), taken from the code as it is after 011:
- the conversations: `src/conversation/events.rs`, `src/studio/{mod.rs,conv.rs,files.rs,commands.rs}`;
- the runtime: `src/agent/{mod.rs,rights.rs,claude.rs,prompts.rs,schemas.rs}` and `src/bin/fallow-fake-agent.rs`;
- the engine: `src/run/engine.rs`, the slots, and 011's target watch in `src/run/update.rs`;
- git, projects and gates: `src/verify/{git.rs,gates.rs}`, `src/project/mod.rs`.

## R-1 A session is a third kind of conversation

**Decision**: a solo session is a conversation of a new kind, `Kind::Solo`. It keeps its log in
`conversations/<id>/events.jsonl` and reuses the turn machinery: `say`, the turn thread and
`Note::Turn`/`TurnDone`, the long-poll on `conv:<id>`, the `/` menu, the working line, the
activity, stopping a turn, and approvals over the MCP bridge. What only a session has lives in a new
module, `src/studio/solo.rs`:
- the working copy and the branch;
- the per-turn commit;
- undo and the base;
- the waiting messages;
- close;
- the command judge (R-6).

`Studio::say` branches on the kind where the lead's rules differ: the agent, its rights, its
prompt, the result schema, and the commit at the end of the turn.

**Rationale**:
- 007 and 009 already give a session what FR-006 asks (the chat, the `/` menu, the working line,
  live activity, stop).
- A second chat engine would duplicate all of that and drift from it.
- The lead's rules that do not apply to a session are all decided in a few places in `say` and
  `turn_done`, so a kind check there is enough.

**Alternatives considered**:
- Sessions as loops (`Run`): rejected. A loop is driven by the engine from a locked plan and has
  stages and verification that a session does not have.
- A separate crate or service: rejected, because it would add a process and a protocol (VIII).

## R-2 The working copy, the branch, and their names

**Decision**:
- **A session is made when its first message is sent**, not when the page opens. Opening a
  project with no open session, or picking "+ New session", shows a *draft*: an empty composer with
  the assistant picker, which holds no conversation, branch or working copy (FR-001; review 18).
  The first send creates the session and then sends the message. SC-001's "ready for its first
  message" is the draft, which appears at once; the 5 s are measured from the first send to the
  turn's start.
- **Intent first** (VII; review 11): `SessionCreating{branch, start_commit}` is written before any
  git command. On load, a `SessionCreating` without `SessionCreated` is reconciled from git: the
  branch and worktree are completed when present, or removed, and the draft's message stays in the
  composer.
- **Working copy**: `worktrees/<project>/_solo/<id>`, made with `git worktree add -b <branch> <dir>
  <start>`.
- **No hooks in any Fallow git command on a session** (review 6): every git command Fallow runs on
  a session's refs or working copy (`worktree add`, `branch -m`, `update-ref`, `reset`, `branch -D`,
  `worktree remove`, the snapshot) runs as `git -c core.hooksPath=<data>/nohooks …`. Otherwise
  `worktree add` would fire `post-checkout` and `reference-transaction`.
- **No download by a filter** (second review B2): Git LFS is a filter, not a hook, so the empty
  `core.hooksPath` does not stop it, and its smudge filter can download from the remote (FR-018,
  V). Every checkout Fallow makes for a session (`worktree add`, `reset --hard`, the gates'
  worktree) runs with `GIT_LFS_SKIP_SMUDGE=1`, so LFS files stay pointers. When `.gitattributes`
  names `filter=lfs`, the session says so (`lfs: true`), and the assistant can run `git lfs pull`
  when the engineer asks.
- **Reads in the engineer's folder take no lock** (review 7): every read Fallow makes there
  (`status`, `rev-parse`, `branch`, `for-each-ref`, `ls-files -u`) runs as
  `git --no-optional-locks …`, so Fallow never holds the folder's `index.lock`.
- **Starting point (`<start>`)**: the project folder's `HEAD` when the session starts.
- **Starting branch**: `git branch --show-current` in the project folder. On a detached `HEAD`
  there is none, and the session says it started from that commit.
- **Uncommitted changes**: `git status --porcelain` in the project folder. When it is not empty,
  `SessionCreated.dirty_at_start` is true and the session says those changes are not in it
  (FR-012).
- **Branch name**: `fallow/solo/<slug>`. The slug comes from the session's first message (up to 5
  words, lowercased, `[a-z0-9-]`) and is decided when that first message is sent. Until then the
  branch is `fallow/solo/<id>`, and it is renamed with `git branch -m` while nothing has been
  committed on it.
- **A name that is taken**: `-2`, `-3`… are tried (`git::rev_exists`) until one is free, and the
  name used is shown (FR-015). An existing branch is never overwritten.
- **The engineer's folder**: never touched. No command runs in it, except reads (`status`,
  `rev-parse`, `branch`) and what the assistant runs there on the engineer's word (FR-016).

SC-001 (5 s on 10,000 files): `git worktree add` checks out the files once. On this machine it
takes under a second for a repository of that size, and the benchmark covers it (R-17).

**Alternatives considered**:
- A clone: rejected. It is slower, and branches would not be shared, so a merge would need a push.
- `git stash` in the project folder: rejected, because it touches the engineer's folder.

## R-3 The commit at the end of a turn: Fallow's snapshot, no hooks, not signed

**Decision**: Fallow makes its commits on a session's branch with plumbing, so no hook can run.
`git::snapshot(wt, message, author)` does this:

0. `git symbolic-ref HEAD` must be `refs/heads/<branch>`, the session's own. When it is not (a
   command moved it), the snapshot is refused and shown like a failed commit, "the working copy is
   not on the session's branch (it is on …)", until the engineer asks the assistant to switch back
   or undoes (third review M2).
1. `git -c core.hooksPath=<data>/nohooks add -A`, in the session's working copy. This follows the
   project's ignore rules. `<data>/nohooks` is an empty directory of Fallow's own.
2. `git write-tree`.
3. When the tree equals `HEAD^{tree}`, stop: there is no commit (FR-013, a turn that changed
   nothing).
4. `git -c core.hooksPath=<data>/nohooks -c commit.gpgSign=false commit-tree <tree> -p HEAD
   --no-gpg-sign -m <message>`.
5. `git -c core.hooksPath=<data>/nohooks update-ref refs/heads/<branch> <commit> <old>`.

**Why the empty `core.hooksPath`**:
- `--no-verify` alone still runs `prepare-commit-msg`, `post-commit` and `post-rewrite`.
- `update-ref` runs `reference-transaction`.
- Pointing `core.hooksPath` at an empty directory covers hooks from every source (`.git/hooks`, a
  configured path, a manager such as husky).
- `commit-tree` honours `commit.gpgSign` (review 16). The `-c commit.gpgSign=false` and
  `--no-gpg-sign` are what keep the snapshots unsigned.

**The author and committer** are the engineer's git identity, as configured: `git config
user.name` and `user.email`, both checked (`commit_all` today checks only `user.email`; the session's snapshot checks both), not `git var`, which can
return an auto-detected identity (review 16). When either is missing, Fallow sets `GIT_AUTHOR_NAME`,
`GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME` and `GIT_COMMITTER_EMAIL` to its own author for that
command only, as `commit_all` falls back to it.

**Messages**:
- A turn: `Fallow solo <id>, turn <n>: <first line of the message, up to 60 chars>`.
- A turn cut short: the same, with ` (cut short)` at the end.
- Two turns folded into one commit (R-4): `…, turns <n>-<m>: …`.
- The engineer's edits between turns: `Fallow solo <id>: the engineer's edits before turn <n>`.

`files::who_of` reads these subjects. A turn's commit is shown as the assistant's, and the edits
commit as "you".

**The assistant's own commits during a turn** stay (edge case). The turn's commit sits on top of
whatever the assistant committed, and `TurnFinished.commit` is the branch's head after the turn.
`TurnFinished.commits` lists every commit the turn added (`rev-list <before>..<after>`), so undo
and "merged" can count them.

**When the commit fails**: `TurnFinished.commit_failed` holds the error, and also
`changed[{path, blob}]`: every changed path with the hash of its content at the end of the turn
(`git hash-object` of each file, or "deleted"). The turn shows the error, and the changes stay in
the working copy. Before the next turn, Fallow tries again (R-4), and both the next turn and undo
wait.

**Alternatives considered**:
- `git commit --no-verify`: rejected, because it still runs some hooks.
- `commit_all`, which runs `git commit`: rejected for the same reason. It stays as it is for the
  lead and loops.

## R-4 Before each turn, an undo and a close: settling the working copy

**Decision**: `solo::settle(session)` runs before a turn starts, before an undo, and before a close.

1. **A failed turn commit still pending**: Fallow tries the commit again.
   - It first compares the working copy with `TurnFinished.changed` (review 14). Paths whose content
     is still what the turn left are committed as the turn's (`TurnCommitted{n, commit}`). Paths
     that differ since, or that are new, are the engineer's: they go in a second snapshot right
     after, `EngineerEdits{before, commit, files}`. That way no commit mixes them, and FR-013
     ("any commit that holds the engineer's between-turn edits names them") holds.
   - When it succeeds, `TurnCommitted{n, commit}` is written.
   - When it fails, the action waits. The session offers two ways on:
     - "Fix it in the next turn": `FoldNext{n}`. The next turn's prompt says the previous turn's
       commit failed and why, and that turn's commit holds both turns, marked `turns n-m`.
     - "Undo these changes": discards the working copy's uncommitted changes, after a confirmation
       naming the files.
2. **Otherwise**, when the working copy differs from `HEAD`, the engineer edited it between turns.
   Fallow snapshots the changes as the engineer's (`EngineerEdits{before, commit, files}`).
   When that commit fails, the action waits and the session says why (FR-013).

Edits the engineer makes while a turn runs cannot be told apart from the assistant's (edge case).
When the engineer opens a file with "Open in my editor" during a turn, the session's
`editor_during_turn` notice says so.

## R-5 Undo, the base, and undone commits that stay recoverable

**Decision**: undoing turn `k` resets the session's branch and working copy to the commit before
turn `k`.

**How it is done** (every git command without hooks, R-2):
1. `settle` first (R-4).
2. `UndoStarted{from: k, head_before, head_after}` is written first (VII; review 11).
3. Before the reset, a ref keeps the commits about to be undone:
   `refs/fallow/solo/<id>/undone/<k>` points at the old head.
4. Then `git reset --hard <before>` runs in the session's working copy.
5. `TurnsUndone{from: k, to: last, head_before, head_after}` is written.

On load, an `UndoStarted` without its `TurnsUndone` is finished from git: the ref is made when it is
missing, the reset is done when the branch is still at `head_before`, and then `TurnsUndone` is
written.

**Afterwards**:
- The turns are folded as undone and stay visible, marked so.
- The next turn's prompt starts with "The engineer undid turns k–m: …" (FR-014).
- The `undone/*` refs are deleted at close, after the close names the undone turns and their
  commits and asks once (R-14; analyze D2). Commits holding the engineer's edits count as unmerged
  work (R-14).

**The confirmation**, `GET …/undo?turn=k`, names:
- the turns to be undone, with the turns that share one commit undone together;
- the engineer's edit commits among them;
- every merge or push the undone turns made elsewhere (from R-10's observed changes whose source is
  one of those turns), saying that only the session's branch is reset.

An undo needs `confirmed` when it goes back more than the last turn, past the engineer's edits, or
over a turn that merged or pushed elsewhere (review 18). The last case applies even to the last
turn alone, so the session always says so before undoing (FR-014).

**The base** has two parts:
- `base.mark`: the commit on the session's branch that Changes diff against;
- `base.gates`: the commit the gates' baseline is run on.

Both start as the session's starting commit. They move only by a recorded event, `BaseMoved{turn,
mark, gates, why: up_to_date | squash}`. The base is the last `BaseMoved` of a turn that is not
undone, else the start (review 10). It is decided when the event is written, so it does not change
later as the starting branch `S` moves.
- **Bringing the branch up to date** is decided at the end of each turn, from the commits the turn
  added (`rev-list --parents <before>..<after>`, with `S` read at that moment):
  - *A merge*: a merge commit `M` whose parents include a commit `P` that is reachable from `S` and
    not from the session's head before the turn. Then `mark` is `M` and `gates` is `P`.
  - *A rebase or a fast-forward onto `S`*: the turn's new history has a merge-base with `S`, `B`,
    that is newer than the current `base.gates`, and every one of the session's own changes was
    replayed on top. Then `mark` is the new commit at `B` (the last commit of the turn that is
    reachable from `S`; with a rebase, `B` itself) and `gates` is `B`.
  - The session's own earlier commits that are already on `S`, and the start commit, never count
    as `P`: they are reachable from the head before the turn.
  - A plain merge of the session *into* `S` moves nothing.
- **A squash Fallow observed** (R-11) into `S`, up to turn `j`, writes `BaseMoved{turn: j, mark:
  turn j's commit, gates: the squash commit, why: squash}`. It belongs to the turn whose command
  made the squash, so undoing that turn moves the base back.

**Undoing the turn that moved the base** moves the base back to the previous `BaseMoved` that is not
undone. The squashed work stays on the starting branch. The undone turns no longer count as merged
(FR-014).

**The starting branch moved**: the view's `start_moved {commits}` holds the number of commits on `S`
past `base.gates` (`rev-list --count base.gates..S`), read by the ref watcher (R-10). It shows
"main moved by 3 commits since this session's base; ask the assistant to bring the branch up to
date" (edge case; review 18).

## R-6 The assistant's rights and commands: Ask, and a judge for "until I switch back"

**Decision**: `defs::agent::Kind::Assistant` (`kind: assistant` in a definition), with a shipped
default `assets/agents/assistant.md`. `rights::assistant(mode, approved, web)`:
- **Edits**: `Edits::Anywhere` inside the session's working copy, which is the call's `cwd`, with
  `protect` = `CONTROL_GLOBS` (`.fallow/squad.toml`, `.fallow/agents/**`, `.fallow/pipelines/**`,
  as for the lead), plus the runtime's own control files (`Capabilities.control_files`, R-8; for
  Claude, `.claude/settings.json` and `.claude/settings.local.json`): the assistant cannot change the
  gates it is held to, nor grant itself commands (review 5; third review H1). Fallow passes no `--add-dir`.
  An edit outside the `cwd` is not permitted by the runtime's own working-directory rule. One that
  still reaches the prompt tool is denied by `tool_call` (below) (review 17).
- **Where the rules are read from** (review 5): the gates and `[instructions] extra` are read from
  the project folder's committed `HEAD` (`git show HEAD:<spec_root>/.fallow/squad.toml`), never from
  the session's branch or working copy. The approved commands are `Project.approved_commands` in
  `projects.json`, which only the engineer's approvals change.
- **Commands that run without asking** (for the assistant, the runtime allow-list never
  short-cuts anything: the whole-tool ask rules below send every command to `tool_call`, which
  applies this rule): the gates, and the project's approved commands, *except*
  any approved prefix that could cover a command the judge asks per command (review 1). For the
  assistant, `rights::assistant` puts in the runtime's allow-list only the gate commands and the
  approved prefixes for which `judge::prefix_safe` holds. A gate is matched as a whole command,
  exactly as declared in the project's committed squad file, never as a prefix: it then runs
  without asking whatever it starts with (FR-008), since the engineer declared it in their own
  committed configuration (analyze F4). `cd` is never in the assistant's
  allow-list (second review B5), unlike the lead's, so a `cd` reaches the judge. A prefix is not safe when it starts with
  `git`, or with a shell or interpreter that can run other commands (`sh`, `bash`, `pwsh`, `cmd`,
  `node -e`, `python -c`, `npx`, …), or when it holds any form the judge parses as destructive or
  "cannot tell". `prefix_safe` and the judge's "cannot tell" share one list, `judge::OPAQUE`
  (second review B4). Those prefixes go through the prompt tool, so the judge sees each command. There,
  a command matching such an approved prefix is allowed by itself only when the judge allows it.
- **Which settings a runtime reads** (third to fifth reviews; the engineer's decisions of
  2026-11-05): a runtime may merge settings of its own with the call's. Claude Code does this from
  `.claude/settings.json` and `settings.local.json` in the working copy, and from the user's
  `~/.claude/settings.json`. A command that their allow rules cover never reaches Fallow, and an
  agent could grant itself commands by writing one of those files. The engineer keeps a list of
  allowed commands in their user settings and logs in to Claude normally (no API key, proxy,
  Bedrock or Vertex). They decided:
  - **The working copy's settings never apply**, for any kind: neither a repository nor an agent
    can grant a command.
  - **Unattended kinds (the lead, makers, checkers) read no settings at all**: `--setting-sources`
    with an empty list. Their rights are exactly Fallow's (VI). User allow rules would otherwise
    widen them under `dontAsk`, where ask rules cannot help (fifth review H2). Fallow passes the
    model explicitly (`--model`, from the definition, or the runtime profile's default when the
    definition names none), since the user's default model no longer applies. The normal Claude
    login is not a setting, so it keeps working.
  - **The assistant reads the user's settings, but every command and edit still reaches Fallow**
    (fifth review H1): `--setting-sources user`, plus Fallow's own `--settings <json>`, which takes
    precedence over the user's:
    - whole-tool `permissions.ask` rules (`Bash`, `PowerShell`, `Edit`, `Write`, `MultiEdit`, `NotebookEdit`).
      An ask rule beats an allow rule from any source, Fallow's own `--allowedTools` included, so
      every call reaches the permission prompt tool. There, `tool_call` allows by itself the gates,
      the `prefix_safe` approved commands, edits inside the working copy (outside `protect`) and,
      under "until I switch back", what the judge allows. It asks the engineer for the rest;
    - `disableAllHooks: true`, so a user hook cannot answer "allow" for a tool.

    **How `tool_call` decides what runs without asking** (sixth review H1, M1). A Bash or
    PowerShell command runs without asking only when the judge splits it (on `&&`, `||`, `;`, `|`,
    newlines), finds none of its "cannot tell" forms (`$(`, backticks, variables in a name or path,
    `~`, `&`, subshells, redirections), and every part is one of these:
    - a gate command, equal to a declared gate as a whole (not a prefix);
    - a `prefix_safe` approved prefix, matched against the live `Project.approved_commands` (an
      "Always" given during the turn counts at once; sixth review L2);

    There is no separate list of reading commands (seventh review H1, H2). FR-008 and VI let only
    the gates and the approved commands run without asking in the base mode, so `ls` or
    `git status` is asked there too. Under "until I switch back" the judge allows them. Before
    this, Claude's own read-only allowance let them through; the whole-tool ask rule ends that, as
    FR-008 asks. The session's mode chip says so: "Ask for commands: every command but the gates
    and your approved ones is asked".

    So `npm test && rm -rf ../x`, `npm test > ../f` and `npm test; git push --force` are asked.
    Under "until I switch back", anything the judge allows also runs. Everything else is asked.

    **Edits**: `tool_call` judges the `edit` request's path, which the adapter took from the
    runtime's own input fields (for Claude, `file_path` or `notebook_path`; see "Permission requests
    in the contract's terms" below). A request of kind `other`, or one whose shape the adapter
    does not know, is denied, with the reason (sixth review L1; analyze D1).

    The user's allow rules therefore shortcut nothing in a session. To be asked less, the engineer
    uses "Always in this project" or "until I switch back" (R-6). Their list keeps working in their
    own use of Claude outside Fallow.
  - Fallow never reads the user's settings file (it may hold keys, V).
  - **A fixed tool list for every kind** (T001's check; the engineer's decision of 2026-11-05): the
    installed `claude` has tools beyond files and commands (scheduling, remote triggers, worktrees,
    messages, notifications, design sync, skills, monitors). A user allow rule for one of them
    would bypass Fallow. Every Fallow call passes `--tools` with a fixed list, and every other tool
    is off:
    - all kinds: `Read`, `Glob`, `Grep`, `Bash`, `PowerShell`, `Task`, and Fallow's MCP tools;
    - makers, the lead and the assistant also: `Edit`, `Write`, `NotebookEdit` (the lead's edits
      stay limited to spec documents);
    - the assistant also, while its web access is on: `WebSearch`, `WebFetch`.

    Checkers get no edit tools. The adapter puts the instruction on stdin, since `--tools`,
    `--allowedTools` and `--disallowedTools` take lists that would swallow it.
  - **Project instructions**: with the project source gone, `claude` may no longer load the
    project's `CLAUDE.md` by itself. Fallow hands the project's instruction files (R-7) as plain
    text to every kind, as IX asks anyway. When `claude` loads them too, the text is only repeated.
  - **The check task** (as T001), on the installed `claude`, confirms seven things:
    - an empty `--setting-sources` is accepted, and login and `--model` still work;
    - `--setting-sources user` is accepted;
    - a working copy's `settings.json` allow rule does not apply;
    - a user allow rule does not apply under `dontAsk` with an empty source list;
    - a `--settings` whole-tool ask rule beats a user allow rule, an `--allowedTools` allow and an
      `additionalDirectories` path, and reaches the permission prompt tool;
    - `disableAllHooks` from `--settings` stops a user hook;
    - whether `CLAUDE.md` is still loaded;
    - with `--setting-sources user` and `--strict-mcp-config`, whether any plugin or user MCP tool
      loads. If one does, the assistant gets the empty source list too (sixth review M2).
      `tool_call` denies any tool it does not know either way.

    A user-level allow rule being overridden is confirmed only by this check, since the tests run on
    the fake runtime (sixth review L3).

    If one of these does not hold, the implementation stops there and the engineer decides
    (contracts/runtime.md, "cannot guarantee": the call is refused rather than run with fewer
    restrictions).
  - **Tests**:
    - a `.claude/settings.json` allowing `Bash(git:*)` and `Bash(rm:*)` in a session's working
      copy, with `git push --force` and `rm -rf <folder>` still asked;
    - the adapter's arguments for each kind, checked on the fake runtime;
    - a lead call and a builder step still get the project's instructions.
  - **005**: its task sets' `setup/settings.local.json` (005 contracts/task-set.md), not built yet,
    no longer reaches run agents. Their permissions come from Fallow's rights. 005's contract is
    amended with a note (fourth review M1).
  - A command that writes a control file (a redirection, `cp`, `Set-Content`) is asked per command.
- **Every other command**, and every tool the allow-list does not cover, goes through `prompt_tool`
  (`Mode::Ask`), Fallow's `tool_call` and `approve`, as for the lead in Ask mode today.
- **Permission requests in the contract's terms** (analyze D1, IX): the adapter, not Fallow, knows
  a runtime's tool names and their inputs. It turns each permission request into
  `Request { kind: run | edit | web | other, command?, path?, url?, query?, tool }`:
  - `run`: a shell command, with its text;
  - `edit`: a file write, with its path;
  - `web`: a fetch or search, with its URL or query;
  - `other`: anything else, which Fallow denies.

  For Claude, `Bash`/`PowerShell` map to `run`, `Edit`/`Write`/`MultiEdit` (`file_path`) and
  `NotebookEdit` (`notebook_path`) to `edit`, and `WebFetch`/`WebSearch` to `web`. `tool_call`
  judges only that shape. The names below are Claude's, mapped in its adapter.
- **`tool_call` gains an assistant branch** (today it denies everything but Bash and PowerShell
  for the lead) (review 2):
  - `WebFetch` and `WebSearch` are allowed when the session's web access is on. With web on, the
    Claude adapter also puts them in `--allowedTools`, so they never wait for the engineer.
  - An edit or write whose path is outside the working copy, or matches `protect`, is denied with
    the reason.
  - Bash and PowerShell commands go to the judge (below).
  - Any other tool is denied, as today.
- **"Always in this project"** in a session is offered only when the prefix is `prefix_safe`, so
  approving `git push origin x` never lets `git push --force` through later (review 1).
- **Modes** (review 8): the session's base mode is `Mode::Ask` (labelled "Ask for commands"), and
  the session offers one more, **"until I switch back"**. That one is *not* the runtime's
  run-anything:
  - The runtime stays in Ask mode, so every command that is not in the allow-list still reaches
    Fallow's `tool_call`.
  - There, the judge (`src/studio/judge.rs`) answers "allow" by itself, unless the command is one
    that must be asked per command (FR-008).
  - The other modes (`ReadOnly`, `Approved` as dontAsk, `RunAnything`) are not offered in a
    session.
  - Its flag lives only in memory (`Conv.allow_until_switch`) and is never written to the log, so
    a restart reads as `Ask` (FR-008). The session says so after a restart when it was on.
  - Turning it on shows the notice that Fallow cannot see what scripts do inside.
- **Per command, always**: `judge::assess(command, ctx)` returns `Allow | Ask(why[])`. It parses the
  command as written: it splits on `&&`, `||`, `;`, `|` and newlines, and handles one level of
  quoting. `ctx` holds the session's working copy, its own branch and the project folder. It asks
  when any part:
  - runs `git push` with `-f`, `--force`, `--force-with-lease`, `--force-if-includes`, `--mirror`,
    `--prune`, `--delete`/`-d`, a `+` refspec or a `:branch` refspec;
  - runs `git branch -d/-D/-m/-M/-f`, `git update-ref` (any form), `git checkout -B`,
    `git switch -C`, `git reset` (any mode), `git rebase`, `git filter-branch`/`filter-repo`,
    `git commit --amend` or `git push … <src>:<dst>`, when it names a branch other than the
    session's own (a `reset` or `rebase` of the session's own branch in its own working copy is
    allowed);
  - discards uncommitted changes: `git checkout -- …`, `git checkout .`, `git restore` without
    `--staged`, `git reset --hard`, `git clean`, `git stash drop/clear`, `git switch -f`,
    `git checkout -f`;
  - names a path outside the working copy. Every path is resolved first, symlinks followed, and
    `--opt=value` and `-oVALUE` count as paths. PowerShell provider drives (`env:`, `HKCU:`,
    `HKLM:`, `Variable:`, `Cert:`) and UNC paths (`\\host\…`) count as outside, so `Get-Content
    env:…` is asked (V; seventh review H2). Edit and Write paths are resolved the same way before
    the working-copy and `protect` checks (seventh review M);
  - changes the engineer's folder or another worktree: `-C <dir>`, `--git-dir`, `--work-tree`,
    `cd`/`pushd`/`Set-Location` to a directory outside the session's working copy, or a path
    argument that resolves outside it;
  - names a path matching `protect` (the control files and `.fallow/`'s squad file, agents and
    pipelines) as an argument of any command, so `cp x .fallow/squad.toml` is asked (ninth
    review M1);
  - cannot be told: anything with `$(`, backticks, `$VAR` or `%VAR%` (and `$env:`) in a command
    name or path, `~`, a background `&`, a subshell `( … )`, a redirection (`>`, `>>`, `2>`) to a
    path outside the working copy or matching `protect`, `eval`, `Invoke-Expression`, `iex`, `sh -c`, `bash -c`,
    `pwsh -c`, `cmd /c` or `xargs`, or an alias Fallow does not know (review 17);
    `judge::OPAQUE` also holds the inline interpreters and launchers: `node -e/-p`, `python -c`,
    `perl -e`, `ruby -e`, `pwsh`/`powershell` with `-Command`, `-c` or `-EncodedCommand`,
    `Start-Process`, `find` with `-exec`, `-execdir`, `-ok`, `-okdir`, `-delete`, `-fprint`,
    `-fprint0`, `-fprintf` or `-fls`, `env <cmd>`, `npx`, `rg --pre`/`--pre-glob`, and any global
    git option that can run a program or read another config: `-c`, `--config-env`, `--exec-path`
    (second review B4; seventh review H2);
  - writes to git's configuration: any `git config` that sets, adds, unsets or edits a value. The
    configuration is shared with the engineer's folder, so it counts as changing it (seventh
    review H2);
  - writes a file through an option: `--output`/`--output=<f>` (git `diff`, `log`, `show`,
    `format-patch`), `-o <f>`, or `tee`. Like a redirection, these are allowed when the file
    resolves inside the working copy and outside `protect`, and asked otherwise (eighth review L);
  - sets the environment for a command or the shell: a `NAME=value` before a command, `export`,
    `set`, `setx`, `declare -x`, `env NAME=value`. `GIT_DIR=../proj/.git git merge x` would
    otherwise target the engineer's folder (eighth review H);
  - shows the environment's values: bare `env`, `printenv`, `export -p`, `set` without arguments,
    bare `declare`/`typeset`, `declare -p`, `compgen -e`/`-v`, and `Get-ChildItem env:`. Any
    PowerShell static call (`[Type]::…`, such as `[Environment]::GetEnvironmentVariables()`) is
    "cannot tell", and provider drives match without regard to case (`env:`, `Env:`) (ninth
    review M2). Their output would land in the activity (V; eighth review M);
  - moves the session's working copy's `HEAD` anywhere but the session's own branch:
    `git checkout`/`git switch` to another branch, a commit (detached), `-b`, `-c`, `-B`, `-C` or
    `--detach`, `git symbolic-ref HEAD`, and `git worktree` in any form (third review M2). Its
    later commits would land elsewhere, and Fallow's snapshot moves only the session's branch
    (second review B5);

  Each reason names what the command affects ("deletes branch main", "changes your project folder
  C:\…"). Every request also carries the fixed text "Fallow judges the command as written; it cannot
  see what a script does inside".
- **The merge request** (FR-016): a command that runs `git merge` or `git -C <folder> merge`
  carries the merge facts:
  - the number of Fallow's snapshot commits the merge would bring, whose hooks did not run:
    `rev-list <target>..<session head>`, counting those whose subject is one of R-3's (review 17).
    After a squash, a plain merge carries the earlier commits again, and this count includes them.
    It also says "the assistant's own commits: whether hooks ran on them, Fallow does not know";
  - whether the command squashes them (`--squash` together with a commit in the same command line);
  - any change to `.fallow/` that the merge would bring (`git diff --name-only <target>...<session
    head> -- .fallow`), named, since it would change the gates and pipelines the project is held to
    (second review B10);
  - the folder's checked-out branch and uncommitted changes (`git --no-optional-locks -C <folder>
    status --porcelain`).

  The approval card shows them. An approval is per command line, so "squash and commit together" is
  one approval (SC-010).
- **The folder's state in every request that changes it** (analyze C2, FR-016): any command the
  judge flags as changing the engineer's folder (a `-C <folder>` command, a `cd` into it, a path in
  it), not only a merge, carries the folder's checked-out branch and its uncommitted changes.
- **Afterwards**, for a command that changed the engineer's folder (R-10's snapshot shows the
  folder's branch moved, or its status changed), `FolderState{state: changed | conflicted |
  staged_only | unchanged}` is written. `conflicted` means `git -C <folder> ls-files -u` is not
  empty. `staged_only` means `MERGE_MSG` or `SQUASH_MSG` is present and `HEAD` did not move.
- **No runtime option** that skips permission checks is ever used (VI, FR-008).

**Rationale**:
- Running the runtime in Ask mode and allowing commands in Fallow keeps one place where commands
  are judged, whatever the runtime (IX).
- The runtime's own allow-all would hide the commands that must still be asked.

**Alternatives considered**:
- A command allow-list per runtime: rejected (IX).
- Asking for every git command: rejected. SC-010 asks for one approval.

## R-7 The prompt, the instruction files, and the result schema

**Decision**:
- **`prompts::assistant_system`**: plain text. It says:
  - the agent works only in the current directory, which is the session's working copy and its
    own branch;
  - Fallow commits each turn;
  - to bring work into another branch, it uses one squashed commit, as one command line
    (`git -C <folder> merge --squash <branch> && git -C <folder> commit -m …`), unless the engineer
    asks for a plain merge;
  - it never force-pushes unless asked;
  - when a discussion has settled enough to become a feature, it suggests one in its result's
    `proposed_feature`, and it never creates the feature or its files itself;
  - a pipeline it designs goes in `proposed_pipeline`.

  Nothing names a runtime's built-in commands or files (IX).
- **`prompts::assistant_turn`**: a turn's instruction. It holds:
  - the engineer's message;
  - the engineer's `@` references (as 009);
  - the instruction files (below), on the first turn and again when one of them changed;
  - "turns k–m were undone" (R-5);
  - "the previous turn's commit failed: …" (R-4);
  - "the engineer edited these files since your last turn" (R-4);
  - for `/feature`: "Suggest a feature from the discussion so far: give a title and a brief in
    `proposed_feature`".
- **For every kind** (R-6): the same files are handed to the lead, makers and checkers as plain
  text, in `prompts::lead_turn` and the loop's step prompts (`run/feature.rs`):
  - for a maker or checker step, read from that step's own worktree when the step starts, with the
    `[instructions] extra` list read from the project folder's `HEAD`, as for a session (sixth
    review L4);
  - for the lead, read on its first turn and again at a turn after one of them changed, by the
    same per-turn sha as for a session.

  `[solo] instructions` is named `[instructions] extra` in `.fallow/squad.toml`, since it applies to
  every kind.
- **Instruction files**: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.github/copilot-instructions.md`,
  `.cursorrules` and `.cursor/rules/*.mdc` at the working copy's root, plus any paths listed in
  `.fallow/squad.toml` `[instructions] extra = [...]`.
  - Each is read up to 64 KiB, and a longer file is cut and named as such.
  - They are handed under "The project's instructions for coding agents, from <path>:".
  - Their fingerprint (64-bit FNV-1a, enough to see a change) is kept per turn, in `TurnStarted.instructions[{path, sha}]`, so a change is seen
    at the next turn. The session shows the list.
  - Each path is resolved (symlinks followed) before it is read. A path that resolves outside the
    working copy is skipped and named, and so is one matched by `files::is_secret` after resolving
    (V; review 19). The files' contents are read from the working copy (FR-007); the list of extra
    paths is read from the project's `HEAD` (R-6).
- **The result schema**, `schemas::assistant_reply()`: `{reply: string, proposed_feature?: {title,
  brief}, proposed_pipeline?: <as 009>}`. It is plain JSON in the runtime contract's `schema`, the
  "structured part" FR-030 asks for. A suggestion is never read from the free text.

## R-8 The runtime contract gains capabilities, images and a web switch

**Decision**: `Runtime` gains `fn capabilities(&self) -> Capabilities`, with `Capabilities { images:
bool, web_off: bool, control_files: Vec<String> }`, which every runtime declares (FR-010a). `Call` gains:
- `attachments: Vec<Attachment{path, media_type}>`: only png, jpeg, gif and webp, up to 5 MiB each
  and 4 per message;
- `web: bool`.

`specs/007-fallow-studio/contracts/runtime.md` is amended.
- **Claude**:
  - `images: true`. A call with attachments is sent with `--input-format stream-json`, as one user
    message with a text block and base64 image blocks on stdin. A first task checks this on the
    installed `claude`, as T001 did for 007. If the check fails, `images` is false and attaching is
    refused (FR-010).
  - `web_off: true`. With `web: false`, `WebFetch` and `WebSearch` go in `--disallowedTools`. With
    `web: true`, they go in `--allowedTools` (R-6).
  - `tool_call` maps `WebFetch` and `WebSearch` to a new `ToolKind::Web`, with the URL or the query
    as the summary.
- **Fake**:
  - `images` and `web_off` come from its profile (`[agents.fake] images = true`, `web_off = true`),
    so both paths can be tested.
  - The scenario step can say `expect_images = n`: the step fails when it receives a different
    number.
  - It can also say `web = ["https://…"]`, which emits `tool_call` events of kind `web`.
- **Every other kind** gets `web: false`. When the runtime cannot switch web off, nothing changes
  for the other kinds, as today (FR-009: "no other agent kind gains web access through this
  feature").
- **Storage of attachments**: the file is checked by its magic bytes, then stored at
  `conversations/<id>/attachments/<sha256>.<ext>`. Its name, type and size go in the message's
  event.
- **Activity**: a `web` tool call shows as "Fetched <url>" or "Searched <query>". Requests made by
  commands show only as those commands.

## R-9 Waiting messages

**Decision**:
- **While a turn is at work**, `say` on a session does not refuse: it appends `MessageQueued{id,
  text, attachments}`. The queue can be changed with `QueueEdited{id, text}` and `QueueRemoved{id}`.
- **When the turn ends** (after `turn_done` and its commit), the engine sends the oldest waiting
  message as the next turn and writes `QueueSent{id, n}`. It never merges several messages into
  one.
- **Waiting for a slot**: a message whose turn cannot start yet stays waiting, with the reason
  "waiting for a free agent slot" (FR-027).
- **After a restart**: the queue survives, but nothing is sent by itself (FR-026). Each waiting
  message has a "Send now" button.
- **When the turn ended in a failure**, or by an approval refused, the queue waits for the
  engineer as well.

## R-10 Watching the project's branches and remotes (FR-017)

**Decision**: a ref watcher per open project, `src/studio/refwatch.rs`.

**The snapshot**: `git --no-optional-locks for-each-ref --format='%(refname) %(objectname)' refs/heads
refs/remotes`, together with the project folder's `HEAD` and `git --no-optional-locks status
--porcelain=v1 -uno` (R-2: no lock is taken in the engineer's folder).
- It runs on a thread every 2 s for every project, while Fallow runs (second review B7; SC-004).
  A `for-each-ref` costs a few milliseconds, and the thread does nothing while nothing changed.
- It is taken again right after each `tool_result` of a session's `run` call.
- It is local and costs a few milliseconds.

**The diff** against the last snapshot gives one `GitChange{ref, before, after, how, source, at}`
per changed ref, where `how` is one of:
- `created`;
- `deleted`;
- `commits(n)`: `before` is an ancestor of `after`;
- `rewound`: `after` is an ancestor of `before`;
- `rewritten`: neither.

Remote refs are "as git last saw them": Fallow never fetches.

**Snapshots around a command** (review 3): every command that can move a ref is a git command,
and the assistant's git commands never sit in the runtime's allow-list (R-6), so each one reaches
Fallow's `tool_call`. The opening snapshot is taken there, at the moment `tool_call` answers
"allow" (by the judge, or after the engineer's approval), not when the request arrives, so nothing
the engineer did while the card waited falls in the call's window (second review B6; third review
M1). The closing one is taken when its `tool_result` arrives.
The periodic watch runs in between as usual. A change seen by the periodic watch while a call is
open is held until the call's closing snapshot, and is then attributed as below.

**The source** of a change, decided per ref:
- **A session**: only when all of these hold.
  - The change falls between the opening and closing snapshot of one of the session's `run` calls.
  - That call's parsed command targets *that ref* (review 3):
    - a `push` names it through its refspec (or the session's branch when no refspec is given), as
      `refs/remotes/<remote>/<b>`;
    - a `-C <folder>` `merge`, `commit`, `pull`, `cherry-pick`, `reset` or `rebase` moves the
      branch checked out in that folder, as the opening snapshot read it;
    - a `branch`, `update-ref`, `checkout -B` or `switch -C` names the branch.
  - No other session's open call targets the same ref.
- **A loop or a spec session, by its records** (review 4; second review B1): an owner is named
  only when its own log holds the change's new commit, and otherwise the change is "outside Fallow":
  - for a loop: its step commits (`task.commit`, the gate and verdict commits), the working and
    merge commits of its updates (011), `Merged.commit`, `Published.commit` and `PushedForward.to`,
    and the creation and deletion of its own branches, which its log records;
  - for a spec session: the commits of its turns (`TurnFinished.commit`), its locks, and the brief;
  - for a session: its `TurnFinished`, `TurnCommitted` and `EngineerEdits` commits, and its
    branch's creation, renaming and deletion.

  Every change shows on the project (FR-017). Changes whose source is Fallow's own work on its own
  branches (a loop's or a spec session's) are shown in a quiet group, "Fallow's own branches (12
  changes)", folded by default. Everything else shows in the main list: the engineer's branches, a
  target, the remote-tracking refs, every push, and any change to a Fallow branch that its owner's
  log does not hold (named "outside Fallow").
- **Otherwise**, "outside Fallow".

Fallow never guesses (FR-017). A test in `us12_git.rs` runs a loop and a spec session next to
a session and checks that no change of theirs shows as "outside Fallow" or as the session's.

**What is not shown**: the moves of a session's own branch by Fallow's snapshots, and the
assistant's commits during its own turn (those are the turn's).

**Gone** (review 11): when the watch sees a session's own branch deleted, or its working copy
missing, outside Fallow's own close, the session is marked `gone` at once, not only at the next
load (R-19).

**Storage and display**:
- The changes go in `<data>/projects/<project>/git.jsonl`, capped at the last 1,000. The last
  snapshot goes in `git-seen.json`, so a change made while Fallow was stopped shows after a
  restart, as "outside Fallow".
- A change a session made also goes in that session's log, as `GitObserved{ref, before, after,
  how, commits[], turn}`. `commits` lists every first-parent commit in `before..after`, so a
  fast-forward or a merge of several commits is matched commit by commit (review 15).
- The page reads them through the project's long-poll scope (`project:<p>`, new). SC-004's 5 s is
  met by the 2 s period, and by the snapshot after a session's commands.

**With 011**: `update::watch`'s `MovedCommit.from` becomes typed (review 15), as `From { Feature
(name) | Session (id, name) | Outside }`. Today's string reads as `Feature`; the loop view's JSON
gains `from_kind`. A commit is from a session when it is in one of that session's
`GitObserved.commits`. A loop whose target moved because of a session's merge
then says "main moved: 1 commit from solo session <name>". 011's rules (who starts an update, the
merge line, overtaken updates) stay as they are: a session's merge is one of "the target's new
commits" like any other.

## R-11 Squashes Fallow observed, and "merged"

**Decision**: a squash is recognized only when both hold:
- a session's `run` call is the named source of a `commits(1)` change on a local branch `B`
  (R-10);
- the new commit's changes (`git diff --name-status <c>^ <c>` and the tree) equal the session's
  changes from its base up to some turn `j` (`git diff <base.mark> <turn j commit>`).

The comparison is of the trees: for every path the session changed, the blob in `<c>` equals the
blob at turn `j`, and no other path differs between `<c>^` and `<c>`.

Then `Squashed{branch: B, commit: c, upto_turn: j}` is written in the session. When `B` is the
starting branch, the base moves (R-5).

**"Merged" at close** (Assumptions, SC-007): true when either holds:
- every commit on the session's branch since its start that is not undone is reachable from
  another local branch or a `refs/remotes/*` ref (`git for-each-ref --contains <head>
  refs/heads refs/remotes`, excluding the session's own branch);
- or the last `Squashed` covers the session's last turn with changes, and the engineer's edit
  commits are among the squashed ones.

A squash merge on a remote (a pull request) is not recognized, so Fallow asks first.

## R-12 Gates on a session's branch

**Decision**: `solo::run_gates(session)`, run from the Gates panel.
1. While a turn works, it is refused (409 "Wait until the turn ends"), so the assistant's
   half-done edits are never committed as the engineer's (review 9). Otherwise `settle` runs first
   (R-4), so the gates run on a commit.
2. It adds a fresh detached worktree, `worktrees/<project>/_solo/<id>-gates-<ms>`, at the session's
   head, without hooks (R-2). It does not use the session's working copy.
3. It runs `verify::gates::run` on a thread, for each gate declared in the project folder's
   committed `.fallow/squad.toml` (R-6), never the session's copy of it (review 5).
4. It classifies the results with `gates::classify` against a baseline run at `base.gates`. The
   baseline is run the same way and cached per commit in the session's log.
5. It writes `GatesRun{commit, base, results[]}` and removes the worktree.

**What undo and Changes cover** (analyze C1, I): the Changes panel and every undo confirmation say
"Undo and Changes cover only this session's working copy: what a command wrote elsewhere is not
seen or undone".

**What the session shows** (FR-022):
- With no gates run on its head: "Not verified by Fallow: the gates did not run on <short>", or
  "the project declares no gates".
- After a pass: "Gates passed on <short> — no independent check", in neutral colors, never the
  verified loop's green badge or its words.
- After a later change: "Not verified by Fallow" again.

The assistant's own test runs stay in the activity as commands (FR-021).

A gate run takes no agent slot, as a loop's gate runs take none today: it is not an agent (R-13).

## R-13 Slots: a session's turn counts as an agent

**Decision**:
- A session turn takes a slot, `machine::slot()`'s counterpart in the studio
  (`Ctx.slots.take()`), before its runtime starts. `Slots` has no release: `refresh_slots`
  recounts what is busy on every tick. A session whose turn ended stops being counted at the next
  recount (review 20).
- Lead turns keep their rule from 007: they always start.
- When no slot is free, the message waits (R-9) with "waiting for a free agent slot", and is sent
  when one frees. The engine's 1 s tick checks the waiting sessions.
- `refresh_slots` already counts busy conversations; now a busy session also holds one of the
  `max_parallel` slots (FR-027).

## R-14 Closing

**Decision**: `POST /api/sessions/<id>/close` first answers with what closing will do, read without
changing anything:
- **A turn at work**: `{stop_first: turn}`.
- **Uncommitted changes, or work that is not merged** (R-11):
  `{ask: {commits: [...], files: [...], edits: [...]}}`.
- **Merged and clean**, with no ignored file outside the dependency and build folders of R-18:
  closes at once. With such files (a `.env`), the close asks first and names them (second review
  B3).

With `{choice: keep | discard}`:
- **keep**:
  - `settle` commits what is uncommitted; then the worktree is removed and the branch kept (named
    in the closed record);
  - when that commit fails, the worktree is kept too, and the closed session offers
    "Remove working copy" (confirmed, naming the uncommitted files).
- **discard**: the worktree is removed and the branch deleted with `git branch -D`. This is the
  engineer's confirmed choice, naming what is lost (II).

**Ignored files** (review 18): removing a working copy also deletes the files the project's ignore
rules exclude (`git status --ignored --porcelain`). The close dialog lists them, apart from those
under the dependency and build folders of R-18, which it gives as a count ("node_modules: 1,204
files"). With "keep", a list that is not empty is part of the question, so a `.env` the engineer put
there is never lost unnamed.

**Undone commits at close** (analyze D2, II): when the session holds `undone/*` refs, the close
names them (the undone turns and their commits) and asks once, even for a merged and clean
session; nothing is removed before the answer. When that is the only question, the engineer
confirms (`{confirmed: true}`, no choice), `CloseStarted{choice: merged}` is written, and the
session closes as merged with its undone commits dropped.

**Intent first** (VII; review 11): `CloseStarted{choice}` is written before anything is removed. On
load, a `CloseStarted` without `SessionClosed` is finished from git, and steps already done are
skipped.

**In every case**:
- Every git command runs without hooks (R-2).
- The `refs/fallow/solo/<id>/undone/*` refs are deleted, and the `-gates-*` worktrees are pruned.
- `SessionClosed{how: merged | kept | kept_with_copy | discarded, branch?}` is written.
- The session moves to "Closed sessions…".
- "Delete record" removes `conversations/<id>/`, attachments included, after a confirmation, and
  the project view shows the size of all session records (FR-025).
- A runtime's own `/exit` is sent as text and means nothing to Fallow (FR-023).

## R-15 From a discussion to a feature

**Decision**:
- **A suggestion**: a turn whose result has `proposed_feature{title, brief}` writes
  `FeatureSuggested{n, title, brief}`. `/feature` is a Fallow command in the session's `/` menu,
  not a Spec Kit step: it runs a turn with R-7's request.
- **The confirmation**: a dialog in the middle of the screen (FR-032) with the title, the brief
  (both editable) and the lead picker, preset to the project's default. An empty title or brief is
  refused, and so is a title already used (`new_feature`'s same-name rule, FR-033).
- **On confirm**: `POST /api/sessions/<id>/feature {title, brief, lead}` calls
  `Studio::new_feature(project, title, lead, origin: Some(session))`, changed as follows:
  - it no longer forks from a project conversation. The spec session starts a new agent session
    (`forked_from: None`), for "+ New feature" too (FR-035);
  - in the feature's worktree, Fallow writes `<feature dir>/brief.md` (`# <title>`, the brief, and
    "From solo session <name>, <date>") and commits it with `commit_all` as
    `spec: brief from solo session <name>`. This is the same kind of commit as the lead's `spec:`
    commits on the feature branch: `commit_all` passes `--no-verify` and leaves signing to the
    engineer's git configuration, as it does today (review 13);
  - it then sends the spec session's first turn as `/speckit-specify` with the brief as
    `$ARGUMENTS`, through the existing command path (009's `CommandUse`);
  - when the project's constitution is still the template, `new_feature` sets the step to
    `constitution`, as today (review 13). The first turn is then `/speckit-constitution`, and the
    spec session says that specify runs on the brief next. When that step finishes, Fallow sends
    `/speckit-specify` with the brief by itself. That is the one case where SC-011's "one
    confirmation" does not cover the constitution step's own questions. SC-011 is stated for a
    project whose constitution is set (spec, amended with this plan), and quickstart Q8 tests both;
  - when Spec Kit is missing, the feature stays created and the spec session says why (edge case).
- **Links**: `FeatureStarted{feature, title}` in the session, and `ConversationCreated.origin:
  {session}` in the spec session (FR-034).
- **Intent first** (VII; review 11): `FeatureStarting{title, brief, lead}` is written in the session
  before `new_feature` runs. On load, a `FeatureStarting` without `FeatureStarted` is resolved from
  `projects.json`:
  - when the feature exists, its brief is committed if missing, the first turn is sent if none was,
    and then `FeatureStarted` is written;
  - otherwise the confirmation opens again with the same text.
- **A second suggestion** of the same discussion shows the feature already started from it next to
  "Make this a feature".

**Earlier project conversations** (FR-035, FR-037, FR-038):
- `Studio::project_conversation` is no longer called by "+ New feature" or by the page.
- Existing project conversations are listed under "Earlier conversations with the lead…" and open
  read-only (`say` refuses with "This conversation is read-only").
- Their unstarted proposals keep "Make this a feature", which opens the same confirmation with the
  proposal's summary as the brief.

**As built** (Phase 8 and its reviews):
- The brief and the spec session's first turn are a marker, `brief_pending.json`, in the spec
  session's folder. It is written before they run and removed once the first turn has started,
  so a restart finishes them. A failure keeps the marker, set aside until the next start.
- A marker in a spec session that already has turns is dropped, never sent over work done. The
  `brief_next` marker (specify after the constitution step) runs only at the specify step.
- A restart after the feature exists but before `feature_started` finds the feature by its origin
  (`solo:<id>`) and title, writes `feature_started`, and writes the marker when the first turn
  was never reached.
- Known limit: a handover from an earlier project conversation writes no intent of its own. A
  restart in the short gap between making the feature and writing the marker leaves a spec
  session without its brief; the engineer specifies there by hand.
- Spec Kit cannot be missing from a spec session: Fallow puts its skills in the lead's copy
  before every turn.

## R-16 The page

**Decision**:
- **`project.js`**: the middle column becomes the session view.
  - `#chat-col` shows the open session, with the session dropdown at its top (FR-002).
  - The rail shows Changes and Gates for a session.
  - The Files tab's version becomes `solo:<id>` while a session is open (a new `files::Version`
    kind `solo`, with base `base.mark`, and `who` read from R-3's subjects).
  - A file opens in the middle, with the chat beside it on the right (FR-020, FR-029): a two-pane
    layout, `#file-view` and a narrow `#chat-col`.
- **The new code** lives in `screens/solo.js`: the dropdown, the header, the waiting messages,
  undo, close and the feature dialog. `app.js`'s `messages`, `composer` and `chatAct` are reused,
  and their approval card gains the merge facts and the "asked per command" reasons.
- **"Ask about it"** (files.js) and the pipeline designer's "Ask the lead" go to the session open
  last, or a new one (FR-036).
- **A proposed pipeline** (FR-036; review 18): a turn whose result has `proposed_pipeline` shows
  the same card 009 shows in a lead conversation (`app.js` `messages`), with "Open in the designer".
  That opens `pipelines.js` with the proposal loaded, unsaved, as 009 does.
- **The spec page** (spec.js) gets the same file-beside-chat layout (FR-029).
- **The start page** (dashboard.js) shows the git check (R-18).

The mockup `mockup/solo.html` is the reference for the layout. Page checks are done with headless
Chrome over CDP, as in 009 to 011.

## R-17 Measurements (SC-001, SC-004, SC-009)

**Decision**: new benches in `xtask bench`, which run in CI:
- `solo_start`: from a draft's first send to the turn's start within 5 s (R-2), on a generated
  repository of 10,000 files;
- `solo_four`: 4 scripted fake sessions working together. Fallow's resident memory stays under
  200 MB, and the activity is on the page within 2 s (p95), read through the long-poll;
- `refwatch_latency`: a branch moved by a test shows within 5 s.

## R-18 Every project is a git repository (FR-039 to FR-041)

**Decision**:

**The git check**: `git::check()` runs `git --version` at start and when a project is added.
- The minimum is **2.38**. 011's update already needs `merge-tree --write-tree`, and session
  worktrees need nothing newer.
- The result is `GitCheck{found: Option<version>, ok}`. It is shown on the start page with the
  installation hint for the system:
  - Windows: `winget install --id Git.Git -e`, or git-scm.com;
  - macOS: `xcode-select --install`, or Homebrew;
  - Linux: the distribution's package manager.
- "Check again" is `POST /api/git/check`. While git is not ok, `project_view` and every project
  request answer 409 "Fallow needs git <min>".

**Adding a folder**, classified by `project::kind_of`:
- **A repository at its top**: added as today.
- **Inside a bigger repository** (`git rev-parse --show-toplevel` is a parent): refused, and the
  repository's top folder is offered instead (FR-039).
- **No repository, or no commit**: added as `Kind::NeedsGit`. Its page shows only the "Make it a
  git repository" confirmation.

**The confirmation**, `GET /api/projects/<p>/git-init` (read only), shows:
- the number and total size of the files that would be committed;
- the folders among them that are dependencies or build output (`node_modules`, `.venv`, `venv`,
  `target`, `dist`, `build`, `__pycache__`, `.gradle`, `bin`/`obj` next to a `.csproj`);
- the files that are likely secrets (`files::is_secret`: `.env*`, `*.pem`, `id_rsa*`, …);
- a proposed `.gitignore` excluding them. The engineer can edit it, or decline it.

**The click**, `POST …/git-init {gitignore?: text}`:
1. `git init` (when needed), plus the ignore file when it was accepted.
2. A check of the identity (`git config user.name` and `user.email`, as R-3). When either is
   missing, it stops before any commit, saying how to set it (`git config --global user.name/user.email`). It does not set it.
3. `git add -A` and `git commit -m "Initial commit (Fallow)"`, with the engineer's git as it is set
   up.
4. The existing `git_init`'s work of moving features to branches and worktrees.

**The plain-folder project from before**: it reads as `NeedsGit` and gets the same question
(FR-039).

**A project added at a subfolder of a bigger repository** (review 12): today such a project is
`Kind::Folder`, with its features in the subfolder's own `specs/` and no feature branches. It now
reads as `NeedsMove {top}`. Its page shows only "Use the repository's top folder", naming `top`,
the features that move and where their documents will be, and nothing changes before the click.
The click:
1. sets `Project.path = top`;
2. records each feature's spec folder against the new path: `Feature.dir = <sub>/<dir>`;
3. creates a branch and a worktree for each feature from `top`'s `HEAD`, as `git_init` does for a
   folder that becomes a repository. When the feature's documents are not committed in `top`,
   that becomes a first `spec:` commit on the feature's branch, holding them;
4. leaves `.specify/` and `.fallow/` (squad file, agents, pipelines) at the subfolder, where they
   were. Spec Kit's scripts are run with the subfolder as their working directory, and Fallow reads
   the project's `.fallow/` at `<spec_root>/.fallow/`, so the gates and pipelines stay (second
   review B8). The confirmation names both folders as staying where they are.

`Feature.dir` is already a path relative to the project, so the code that joins it with the
working copy works as it is. The code that assumes `specs/` at the project's root is changed to use
the project's *spec root*, the new `Project.spec_root` (empty, or `<sub>`):
- `rights::SPEC_GLOBS` and `SPEC_DIRS`, which become per project (`<spec_root>/specs/**`,
  `<spec_root>/.specify/memory/**`);
- `Project::next_dir` and `spec_dirs`;
- `update.rs`'s `doc_dir` (011);
- `studio::files`' `feature_dir`;
- `lock_prepare`, and the `.specify/...` paths in `say` and `new_feature`;
- `config::squad::SQUAD_FILE`, `defs`'s project library (`.fallow/agents`, `.fallow/pipelines`)
  and `CONTROL_GLOBS`, which are read under `<spec_root>`.

`us1_projects.rs` gains a test that moves a subfolder project, with a feature, and then locks it.

**Retired (FR-041)**:
- `Kind::Folder`;
- the "plain folder" notices (project.js, spec.js, dashboard.js);
- the lead's plain-folder instructions;
- the branches of `undo`, `say`, `turn_done` and `lock_check` for `git_project == false`;
- `files::versions`' plain-folder case.

Tests that built plain-folder projects (`us1_projects.rs`) are changed to the `NeedsGit` flow.

## R-19 Restarts

**Decision**: `Studio::load` already closes turns that a crash cut off. For a session it also,
in this order (review 11):
- stops the turn's process if it is still alive, *before* anything is snapshotted, so a running
  agent cannot write after the snapshot. On Unix `conversations/<id>/turn-<n>.pid` holds the
  turn's process group (its leader is the root) and its leader's start time. Once the leader has
  ended, the members still in the group are the turn's own, because POSIX gives no new process
  or group that id while any member is left, so they are killed too. Load waits until none is
  left, at most five seconds. When Fallow is interrupted (Ctrl-C), terminated or hung up, it
  stops its agents' groups itself. On Windows nothing is left to kill (Phase 4 review): every turn runs in a job object
  made with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway, and its handle is never
  inherited, so Windows ends the whole tree when Fallow's process ends, crash included. The
  Toolhelp walk planned here is not needed. Known limits (Phase 4 review, kept): on Windows a
  process the agent starts in the milliseconds before it is put in the job, or every process
  when that fails, is outside the job (the agent is not started suspended; this is as old as
  the job objects). On Unix a process that leaves the group (`setsid`, a detached spawn) is not
  killed. Either can write after the cut turn's snapshot; its changes show as the engineer's
  edits at the next turn;
- then snapshots the working copy's changes as the cut turn's commit (`(cut short)`) and writes
  `TurnFinished{cut: true}`;
- finishes the intents left half-done: `SessionCreating` (R-2), `UndoStarted` (R-5),
  `CloseStarted` (R-14) and `FeatureStarting` (R-15), each reconciled from git;
- writes `ApprovalAnswered{answer: Deny, why: "Fallow restarted"}` for a pending approval;
- leaves waiting messages waiting (R-9) and the "until I switch back" flag off (R-6);
- marks a session whose working copy or branch is gone as `gone`, so it can only be closed (edge
  case).

Nothing is resent by itself (FR-026, SC-008).

## R-21 The assistant's reads (FR-043)

**Decision**: `Read`, `Grep` and `Glob` join the whole-tool ask rules the assistant runs with
(R-6), so each one reaches Fallow's permission prompt; the adapter maps them to the contract's
`read` kind (`path`: the file or folder, `query`: the file pattern).
- `judge::read_problem`: a path in the working copy that is not a likely secret goes unasked;
  one in the engineer's folder, outside the copy, or a likely secret (`files::is_secret`) is
  asked, with its reason.
- `judge::search_problem` (for `Grep`, which shows contents): also asked when the file pattern
  names a likely secret, or the folder searched holds one. The files are git's list of the copy
  (`git ls-files`: tracked and untracked, not ignored, as the search leaves ignored files out),
  read without the copy's lock. A search that names a folder git ignores reads it all the same,
  so that folder is looked through (up to 20,000 entries, then asked). A file pattern overrides
  the ignore rules in the search, so the ignored files it matches count too, an ignored folder
  that is itself a secret's (`.aws/`), and the files of an ignored folder it matches (`*` takes
  the search into `build/`; looked through as above).
- `judge::listing_problem` (for `Glob`, which shows names): its folder, or the folder its
  pattern starts from when the pattern names one (absolute, `..`, `~`).
- A tool's path is judged as a path: a `$` in a file name (`routes/$id.tsx`) is not a variable.
- A command naming a likely secret (`cat .env`, `less a.pem`, a redirect from it, a wildcard
  matching one in its folder, dot-names included as PowerShell matches them, `*/.env`,
  `git show HEAD:.env` or `:.env`) is asked, in either mode. A plain word without a file's form (`grep -rn token src`) is a search word, unless a
  file has that name; quoted text and words with wildcards in their folders are not followed.
- A read put to the engineer has no "Always": it is allowed once at a time. The adapter keeps
  no allow rule for `Read`, `Grep` or `Glob` beside their ask rules.

**Limits**:
- A command that reads files without naming them (`grep -r x .`) is judged like any command:
  asked in "Ask for commands", run under "until I switch back" unless the judge asks.
- A search of the whole copy is not asked about ignored files (a `.env` git ignores), as
  Claude Code's search follows git's ignore rules; T055 checks that by hand. The contents of
  submodules are not listed.
- Files Fallow's file view treats as likely secrets (`credentials*`, `secrets.*`) are asked at
  every read, ordinary source files with those names included.
- A broad pattern (`*`, `**`) matches every ignored folder, so in a project with a large
  ignored folder (`target/`, `node_modules/`) such a search is asked as too big to look through.
- A word `:name` naming a likely secret is asked even outside git (`echo :token`): a rare extra
  question.
- Only one request waits for the engineer at a time: a second read needing an answer meanwhile
  is refused with "ask again once it is answered".
Whether Claude Code sends `Read`/`Grep`/`Glob` to the prompt tool under these rules is checked
by hand in T055 (the fake runtime plays it as `exec` is played).

## R-20 Testing without a model

**Decision**:
- **The tests**: `tests/us12_*.rs`, on the fake runtime and its new `expect_images`, `web` and
  `proposed_feature` outputs. There is one test file per story plus `us12_with_loops.rs` (011),
  for quickstart Q1–Q10. Q11 is by hand.
- **Common helpers** in `tests/common/mod.rs`: `start_session`, `say_solo`, `wait_turn`,
  `approve_next`, and a project repository whose `pre-commit`, `prepare-commit-msg`, `post-commit`,
  `post-checkout` and `reference-transaction` hooks write a marker file, to prove R-2 and R-3.
- **Unit tests**:
  - `judge::assess`, with a table of commands and verdicts;
  - `snapshot`, with its no-op and its ignore rules;
  - the base fold (R-5);
  - the squash comparison (R-11);
  - the ref diff (R-10);
  - the instruction files;
  - the git-init scan (R-18).
- `no_egress_no_push.rs` still finds no push or fetch in Fallow's own code for sessions (FR-018).
