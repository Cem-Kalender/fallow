# Research: Adaptive Delegation

Decisions for the plan of [spec.md](./spec.md). Each entry: decision, rationale, alternatives.
Constitution v5.0.0.

## R-1 The work manifest

**Decision**: one TOML file, `.fallow/work.toml`, next to the squad file. The lead writes and commits
it; Fallow only reads it, at the commit it needs (`git show <commit>:.fallow/work.toml`). It holds
the pieces of work (`[[work]]`: id, `build` = `lead` | `builder` | `features` | `parts`, reason,
criteria), parts (`[[part]]`: id, work, `owns` globs, criteria), contracts (`[[contract]]`: path and
the parts on each side), reviewer focuses (`[[review]]`), shared files any part may change
(`shared` globs, for lock files and the like) and `docs` pointers to the prose (the Spec Kit spec,
plan and tasks). Contract: [contracts/work-manifest.md](./contracts/work-manifest.md).

**Rationale**: Fallow must check ownership, criteria, decisions and contracts mechanically (spec
FR-020); prose in markdown cannot be checked reliably. TOML matches the squad file the engineer
already knows, and `toml`/`serde` are already dependencies. Reading at a commit (not from a
worktree) makes every check reproducible and independent of uncommitted edits.

**Alternatives**: parsing Spec Kit's `tasks.md` (free-form, breaks with template changes);
frontmatter in `plan.md` (mixes machine and human text, and Spec Kit regenerates the file); a JSON
file (the 004 squad file is TOML; Anthropic used JSON for a feature list agents must not rewrite,
which here is covered by FR-024's lead-only rule instead).

## R-2 One loop by default, enforced at session start

**Decision**:
- The lead builds every piece of work recorded as `lead`, or with no record (FR-001). Its briefing
  says so, and it reports the piece done with evidence through `task_report` on a task Fallow
  creates for it from the manifest.
- `session_start` for a builder takes `work` (and `part` for parallel parts). Fallow refuses it when
  the manifest at the lead's latest commit:
  - records the work as `lead`, or as `builder` and a builder already has it (FR-003);
  - records `parts` and a part's `owns` is empty, owned globs of two parts overlap (checked against
    the files of the starting commit plus literal globs), a boundary has no committed contract file,
    or a part owns no file outside the test patterns or only files under `docs` (FR-004, FR-006).
- The squad file may set `max_parallel_builders` (FR-010).

**Rationale**: enforcing at the start is the one point Fallow controls without trusting agents; the
reason given in the refusal tells the lead what a split needs, which is the lesson in
context-centric decomposition.

**Alternatives**: a separate "plan approval" step with the engineer (adds a human round trip per
burst; the engineer sees decisions in the Squad tab and can stop the burst); letting the lead start
builders freely and judging afterwards (a wrong split already cost its tokens).

## R-3 A level that may run commands but not edit

**Decision**: a fourth level, `check`, for reviewers and investigators (FR-009, FR-030):
- Claude Code: `--permission-mode dontAsk`, `--allowedTools Bash PowerShell`, `--disallowedTools Edit
  Write NotebookEdit AskUserQuestion`. Reading is allowed by default. With all of Bash allowed, 004's
  git `--output`/`--ext-diff` denials have no effect at this level; that is accepted, since the level
  may run anything except Claude Code's edit tools.
- Codex: `--sandbox workspace-write --ask-for-approval never`. Codex's read-only sandbox (004's
  `read-only` mapping) blocks the writes builds and tests need, so a reviewer could not run tests.
- Gemini and others: no mapping until verified; as in 004, an agent without a mapping cannot take the
  role (Verify item).
- The guarantee is the outcome check: 004's worktree check (tracked changes and commits; ignored
  build output does not count) voids the session's verdicts. Writes outside the worktree are not
  seen; the engineer is told when a squad uses the level (FR-030).

**Rationale**: a reviewer that cannot run tests cannot check anything as a black box. No agent CLI
offers "run commands but never write", so the honest design is the outcome check plus the notice.

**Alternatives**: keep reviewers read-only and let Fallow's gates do all running (the review then
cannot test criteria); run reviewer commands through a Fallow tool (Fallow would run arbitrary agent
commands, against FR-040).

## R-4 Signals instead of free messages between builders

**Decision**: three new MCP tools.
- `signal_raise {kind: "question" | "change", work, part?, text, diff?}` by builders, to the lead
  only. `change` needs a diff of the plan, manifest or a contract.
- `signal_answer {signal, commit?, text}` by the lead. A `commit` is required when the answer
  changes a decision; Fallow checks the commit is on the lead's branch and touches the plan, the
  manifest or a contract file (FR-022). Without one the answer may only say there is no change.
- `signal_list` for the lead and the raiser.

Signals are memory-only (Principle VII), shown in the Squad tab, and count for the record. `deliver`
from a builder may target only the lead (FR-021); 004's consult tool is replaced by `signal_raise`.
A signal unanswered for 15 minutes (squad setting) is told to the engineer once.

**Rationale**: structured, few kinds, and the decision itself lands in the repository; Fallow can
count them per part (the coupling measure the other design proposed).

**Alternatives**: plain deliveries with a convention (not checkable); acknowledgement messages for
plan revisions (rejected in the spec: git already orders commits).

## R-5 The plan-commit check

**Decision**: each builder starts from the lead's branch at the latest manifest commit, recorded as
its base. At a done report, the git worker lists commits on the lead's branch after the base that
changed `.fallow/work.toml` or a contract file of the part. For a manifest commit it parses the file
at that commit and its parent and keeps it only if the part's criteria or contracts changed. Each
kept commit must be an ancestor of the reported commit (`git merge-base --is-ancestor`); otherwise
the report is refused with the commit to take in (FR-023).

**Rationale**: only changes that concern the part matter (spec review); everything is read-only git
on the existing worker thread.

**Alternatives**: require the latest manifest commit for every report (builders merge irrelevant
changes); revision numbers in the manifest (duplicate git's own ordering).

## R-6 Ownership check

**Decision**: at each done report and at each gate run, `git diff --name-only <base> <commit>` for the
builder's branch, matched with `globset` against the part's `owns` and the manifest's `shared`. Files
outside both are reported to the lead with their owner part (FR-025). Commits by a non-lead session
that change the manifest, plan docs or a contract are reported to the lead and the engineer, and the
report does not count until the lead accepts it with `signal_answer` (FR-024).

**Rationale**: detectable after the fact with read-only git; `globset` is already a dependency.

**Alternatives**: blocking edits as they happen (needs per-file hooks in every agent CLI; not
portable).

## R-7 Verdicts that answer the criteria

**Decision**: `review_verdict` gains `checks: [{criterion, ran, expect: "pass" | "fail", result:
"pass" | "fail"}]`. A verdict counts only if every criterion of the reviewed work has a check and at
least one check has `expect: "fail"` (a case that must fail) (FR-029). The texts stay in memory;
only counts reach a run record. A reviewer's focus comes from `[[review]]` in the manifest (FR-031).

**Rationale**: makes "looks fine" impossible to submit while being honest that Fallow checks
completeness, not execution.

## R-8 Decision-only updates to the lead

**Decision**: 004's quiet-time status (FR-037) and periodic waiting notices to the lead are removed.
The lead's update queue receives only: a signal, a failed gate, a verdict, an exited session, a
session at its permission prompt, a ready merge, an ownership or plan-change report, and a signal
left unanswered. Grouping (10 s) stays. Everyone-waiting notices to the engineer stay.

**Rationale**: every line costs the lead tokens; Anthropic's research found coordination dominating
role-split systems.

## R-9 Replacing the lead and restarting Fallow

**Decision**: the lead's briefing and `squad_status` add the manifest commit, each piece of work with
its builder, base, branch, latest reported commit and evidence, open signals, and merge readiness
(FR-033). `L` in the Squad tab (004) names another lead (FR-034). After a Fallow restart, the burst is
not resumed silently: when the engineer starts a burst on a project whose `.fallow/work.toml` has
open work, Fallow reads the manifest at the main branch and the builders' branches (002's worktree
names), shows each piece's state and tells the engineer that permissions, tasks and signals were lost
(FR-036).

**Rationale**: Principle VII keeps tasks and signals in memory; what survives is what agents
committed, which is exactly the repository-carried state the spec asks for.

## R-10 Cost as the agent reports it

**Decision**: only in recorded runs, Fallow gives Claude Code sessions a status line through the
session's settings on its command line: `--settings` with a `statusLine` command that runs `fallow
usage-report`. Claude Code runs it after each response with its session JSON on standard input;
`usage-report` prints one short marker line, `fallow-usage cost=<usd> model=<id> version=<v>`, which
Claude Code shows at the bottom of its screen. Fallow reads the latest marker from the session's
screen in memory, as it reads permission prompts (FR-035 of 004).
- No connection, token, file or transcript is involved, so plain sessions need no link to Fallow.
  (Implementation note: the first design sent the report over the session's local channel, but a
  second hello for a session replaces its MCP connection, which would have cut the lead off.)
- The model is kept only when it looks like a provider's model id and the version only when it
  starts with a digit, so a custom alias or text never reaches a record (the marker test).
- The command path is written with forward slashes and quoted, since Claude Code runs status line
  commands through Git Bash on Windows (Verify item).
- `--settings` is added only when the profile passes none; otherwise cost is unknown for that
  session. It replaces the engineer's own status line in run sessions only.
- Cumulative tokens: the status line's `context_window` totals describe the current context, not the
  session's use, so tokens are recorded as unknown (FR-015); cost is Claude Code's own estimate.
- An agent could print a fake marker; the figure is the agent's report either way.
- Codex and Gemini: unknown until a documented report exists (Verify item).

**Rationale**: the agent's own report, read where Fallow already reads session output (in memory,
Principle VII), with no new dependency and no change to the channel.

**Alternatives**: the local channel (replaces the session's MCP connection); reading transcripts
(forbidden); OpenTelemetry export (needs a network listener); estimates (FR-015 forbids them).

## R-11 The task set, run copies and hidden tests

**Decision**:
- A task set is a directory the engineer chooses, outside any project, with `taskset.toml` ([contracts/task-set.md](./contracts/task-set.md)):
  per task a name, the project path, a starting commit, a description file, a time limit, setup files
  (copied into each run's copy, for example `.claude/settings.local.json`), hidden files with target
  paths, and hidden test commands (each a pass by exit code 0).
- **Run copy** (FR-011a): `git init` in a new directory under Fallow's cache, `git fetch --no-tags
  <project> <start>` and `git checkout` of that commit on a branch `main`, then the setup files. The
  copy holds only history up to the starting commit: no other run's branches. Run worktrees (002) are
  created from this copy, never from the engineer's repository.
- **Hidden tests** (FR-013): after the engineer confirms closing the run's sessions, Fallow copies the
  hidden files into the lead's worktree (the plain session's in plain mode), runs each command there as a gate session with the
  task's time limit, records the counts and removes the copy.

**Rationale**: isolation removes cross-run contamination (a later run reading an earlier solution
with `git log --all`) and keeps the engineer's repository untouched. Testing the worktree as left,
after the sessions are gone, treats committed and uncommitted work alike in every mode and keeps the
hidden tests away from running agents. Commands as tests keep Fallow independent of test frameworks.

**Alternatives**: temporary worktrees next to the run's own (004's `create_detached` puts them beside
the run's worktrees, where running sessions could reach them); testing only the final commit
(penalises a mode whose agent did not commit).

## R-12 Fingerprints without a new dependency

**Decision**: `git hash-object --stdin` of canonical text with line endings normalised to LF:
- task: the description (the time limit and idle setting are recorded as durations, and runs are
  grouped only when they match);
- hidden tests: the sorted target paths with the blob hash of each file (normalised) and the commands.

Git is already required by 002 to 004.

**Rationale**: one-way, stable across machines and checkouts, no new crate.

## R-13 Runs: modes, start, end, interventions

**Decision**:
- **Start**: in the run's copy (R-11), Fallow starts the first session in a fresh worktree:
  - plain: the profile with the `edit-run` level's flags (never asking the engineer, no
    `AskUserQuestion`) and the usage-only link, without MCP tools;
  - solo or squad: a burst with that session as lead, the squad file from the starting commit, the
    components switched off as chosen (R-17), and an allowance and memory budget that need no
    approval.
  Fallow types the fixed description followed by one closing line, the same in every mode: "When you
  are finished, commit your work."
- **End** (FR-013): when no session of the run has been working for `run_idle_end_min` (default 3)
  and nothing is in progress (no gate session running, no start pending or waiting for memory, no
  hand-over being typed), when the time limit passes, or when the engineer ends it. Then Fallow asks
  once, naming the run's sessions, to close them (Principle IV: one confirmation names every target);
  hidden tests run after they are closed.
- **Interventions** (FR-014): each line the engineer sends into a run session, and each approval or
  decision during the run. Closing the sessions at the end is not one.
- **Series** (FR-018): one run at a time, order shuffled with a generator seeded from the clock;
  repeats default 3; the series waits at each run's end for the close confirmation.

**Rationale**: the same permission level, inputs, end rule and tested state in every mode remove the
confounds the review found (prompts in plain mode only, commit-dependent testing, early ends while a
gate runs).

**Alternatives**: a `run_done` tool (plain sessions would need Fallow tools); closing sessions without
asking (a destructive action, Principle IV).

## R-14 Memory budget

**Decision**: a setting `orchestration.memory_budget_mb` (default: 75% of total memory minus Fallow's
own). Before starting any session (agent or gate), Fallow sums the memory footprint the 002 sampler
reports for every tracked session tree (gate sessions are sessions) and adds the new session's
expected size: the largest seen for that profile, or `orchestration.session_size_mb` (default 700)
until one was seen. A start that does not fit waits, telling the lead and the engineer the shortfall,
and proceeds when a sample shows room (FR-037, FR-038). Build spikes inside a running session are not
predicted (SC-008 says so).

**Rationale**: the sampler already measures footprint per session; measured sizes adapt to the
agent. The first guess is a Verify item.

## R-15 Run records on disk

**Decision**: `<cache>/fallow/runs/v1/<run-id>.json` (the format version is the directory, not a
field), written once when a recorded run's hidden tests finish, with only the fields of Principle VII
([contracts/run-record.md](./contracts/run-record.md)). Parts and roles are numbers; outcomes are
enums; the model and version are the ones the agent reported (R-10), never a profile's configured
text. On Unix the file is created with mode 0600; on Windows it inherits the user profile's
permissions. The Runs tab lists, compares and deletes records and shows their count and size.

A CI test runs a recorded run whose task name and description, part, role and focus names, messages,
signals, reviews, test names, paths, session output and a custom model alias contain marker text, and
checks that the record holds none of it.

**Not recorded** (not allowed by VII, and not needed): series seed and position (order is recoverable
from start times), the end reason (an engineer's end counts as an intervention; a time-limit end shows
as a duration at the limit), uncommitted files (the worktree is tested as left), a reason for hidden
tests not running (they count as failed).

## R-16 The built-in squad, merge conditions and integration

**Decision**:
- The built-in squad: lead (edit and run), builder (edit and run, max 3, only for recorded
  decisions), reviewer (`check`, max 2). Its merge needs `work:done` and `review:approved`.
- `work:done` (new): every piece of work in the manifest at the lead's latest commit is reported done
  with evidence; with no manifest, the burst's single task, which Fallow creates for the lead from
  the burst's description, is.
- `review:approved` (new): an approval that counts (R-7) for every focus in the manifest's
  `[[review]]`, or, with no focuses, from every counting reviewer (004's rule). `signal:` conditions
  stay for custom squads.
- Integration (FR-028): the lead merges builders' branches into its own branch, as in 004; the final
  gates run on the lead's commit, and that commit is the integrated result.

**Rationale**: with the lead building, 004's `done:builder` would wait for builders that never exist;
per-focus approval is what FR-031 asks.

## R-17 What switching a component off does

**Decision** (FR-017): a run's burst carries the set of components switched off, and each check reads
it. Effects as in the spec's FR-017 table. When the criteria precondition is off and work has no
criteria, the verdict check uses the task description as the one criterion. With `lead_updates`
off, Fallow types nothing into the lead after the run's description; signals, gates and verdicts remain visible through
`signal_list` and `squad_status`.

## R-18 Restarts and builder branches

**Decision**: a builder started for a work piece and part gets the branch `fallow/<work>-<part>` (or
`fallow/<work>` for one builder), instead of 002's `fallow/<profile>-n`, so the repository names what
each branch is for. After a Fallow restart, starting a burst on a project with open manifest work
shows each piece's branch and latest commit and says that permissions, tasks and signals were lost
(FR-036). A recorded run in progress at exit is discarded and its copy removed at the next start
(FR-019a).

## R-19 Non-lead changes to the manifest

**Decision** (FR-024): when a done report or gate finds a non-lead session's commit changing the
manifest, plan docs or a contract, Fallow raises a signal from itself to the lead (kind `change`,
naming the commit). The work counts once the lead answers that signal with `accept` (or with a
commit of its own that supersedes it).

## Verify items

Results (2026-10-29, Windows 11, T049):

- **Claude Code `--settings` with `statusLine`**: Claude Code 2.1.312's `--settings
  <file-or-json>` takes a JSON string. The command form Fallow writes (`"C:/…/fallow.exe"
  usage-report`, forward slashes, quoted) runs under Git Bash and prints the marker from status
  line JSON. Still open: the marker inside a live session's status line (quickstart Q5, T050).
- **Codex and Gemini flags for `check`**: not installed on this machine; open. Codex's
  `--sandbox workspace-write --ask-for-approval never` follows its documented flags; Gemini has no
  mapping, so it cannot take a `check` role until a squad gives `[role.flags]`.
- **Footprint of a Claude Code session on Linux**: needs the Linux VPS; open. The 700 MB default
  (`orchestration.session_size_mb`) is replaced by the largest footprint Fallow measures per
  profile as soon as one session ran, so the guess matters only for the first start.
