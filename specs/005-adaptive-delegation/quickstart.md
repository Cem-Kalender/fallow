# Quickstart: Adaptive Delegation

Manual validation scenarios. Automated coverage: `cargo test` (manifest, delegation checks, signals,
plan-commit and ownership checks, verdicts, run records with the marker test, memory budget) and
`cargo xtask squad` (scripted agents, three OSes).

**Setup**: a test project with Spec Kit, a squad file using the built-in roles, and a task set of two
tasks (one small, one with two independent components), each with two hidden test commands.

## Q1 One loop by default (US1)

1. Start a burst; ask the lead for a small change with no manifest entry.
2. Expect: the lead builds it itself; a `session_start` for a builder is refused with the reason;
   the gate and a reviewer still run; the merge needs `work:done` and the review.

## Q2 A prepared split (US1, US3)

1. Have the lead record `cart` as two parts with owned files, criteria and a contract, and commit.
2. Expect: two builders may start; remove the contract and try again: refused, naming it.
3. Have the API builder raise a question; the lead answers by committing a contract change.
4. Expect: the page builder's done report without that commit is refused, naming it; a change to the
   API part's criteria only does not affect the page builder.
5. Have a builder edit a file the other part owns: the lead is told the file and its owner.

## Q3 Verification (US4)

1. Have the reviewer approve without a check per criterion: the verdict is `incomplete` and does not
   count.
2. Approve with every criterion and a must-fail case: it counts.
3. Have the reviewer change a file: its verdict stops counting, the engineer is warned.

## Q4 Replace the lead (US5)

1. Midway through Q2, close the lead; name another session with `L`.
2. Expect: from `whoami` and `squad_status` alone it lists the manifest commit, parts, open signals and
   what the merge needs, and finishes without the engineer repeating anything.
3. While nothing needs a decision, the lead receives no updates.

## Q5 Compare (US2)

1. Register the task set (the hidden-tests notice appears); start a series of both tasks × plain,
   solo and squad × 3, recorded.
2. Expect: runs go one at a time in shuffled order, each in its own copy (the project's own branches
   are unchanged); each ends idle or at its limit and asks once to close its sessions; hidden tests
   run in the lead's worktree (the plain session's in plain mode) after that and the copy is removed; the Runs tab groups runs
   with pass rate, median time, interventions, cost (known for Claude Code) and spread.
3. Open `<cache>/fallow/runs/`: records hold numbers, fingerprints and fixed values only.
4. Restart Fallow: the Runs tab still shows them.

## Q6 Memory (US6)

1. Set the budget to fit two sessions; ask the lead for three.
2. Expect: the third waits with the shortfall; it starts when a session ends.

## Q7 Switch a component off (US2)

1. Start a squad run with the reviewer switched off.
2. Expect: no reviewer starts, the record names `reviewer` as off, and the merge needs the engineer's
   explicit approval.

## T050 setup and results

**Setup (Windows, prepared 2026-10-29)**

- Project: `C:\Repos\FallowSandbox`, reset to `782a472` (minishop: a small Python shop with tests,
  Spec Kit, and the 005 squad file: gate `python -m unittest` with a baseline, merge on
  `gate:tests`, `work:done`, `review:approved`, at most 2 parallel builders). Reset before each
  demo: remove its worktrees and `fallow/*` branches, `git reset --hard 782a472`, `git clean -fdx`.
- Task set: `C:\Repos\FallowTasks`, outside every project. `search` (small, one component) and
  `cart` (two components meeting at the `Cart` interface), both starting at `782a472`, each with two
  hidden test commands (with `python3` overrides for Linux). The hidden tests fail on the seed and
  pass on a reference solution.
- Fallow: the debug build of `27633bd`.
- Linux VPS: the same setup, cloned there; the task set's `project` then names the clone.

**Results** (fill in: PASS, FAIL with what happened, or not run)

| Scenario | Windows | Linux VPS |
|---|---|---|
| Q1 One loop by default | PASS 2026-10-29: builder refused (no manifest); lead built and reported it; baseline and final gate; reviewer #2 approved; merged by `m` (`530a604`) | |
| Q2 A prepared split | | |
| Q3 Verification | | |
| Q4 Replace the lead | | |
| Q5 Compare | | |
| Q6 Memory | | |
| Q7 Switch a component off | | |
