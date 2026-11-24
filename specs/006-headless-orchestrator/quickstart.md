# Quickstart: Headless Orchestrator

Manual validation with real agents, on Windows.
- **Automated coverage:** `cargo test` with the fake agent (research R-15) covers every scenario
  below without network or cost.
- **Benchmarks:** `cargo xtask bench` runs the constitution VIII benchmarks.

## Setup

- **Fallow:** a debug build, `target\debug\fallow.exe`.
- **The agent:** Claude Code signed in. `gh` signed in, for V7 only.
- **The project:** the sandbox `C:\Repos\FallowSandbox`, reset to the minishop seed `782a472`.
  Its squad file declares the gate `python -m unittest`.
- **The task set:** `C:\Repos\FallowTasks` (the `search` and `cart` tasks with hidden tests, from
  T050).

## V1 A solo run to the merge (US1)

1. `cd C:\Repos\FallowSandbox` and run `fallow run "Make product search case-insensitive and add a
   search command"`. The run page opens at the planning conversation.
2. Answer the planner until it shows a plan with criteria, including a must-fail case, and
   **Approve** is enabled. Approve it.
3. **Expect, without touching anything:**
   - a build step; Fallow's verification (commit, files, gates with the baseline);
   - a review with an answer per `main/<id>`;
   - "Ready to merge".
4. Merge. **Expect:** the verified commit on `master`, and 2 interventions recorded (approve,
   merge).

## V2 False claims are refused (US1, constitution I)

Use the fake agent's `false-claims` script:
- **Expect** each of these to be rejected with its reason, followed by a repair, with no question
  to the engineer:
  - a commit that does not exist;
  - a commit with no changes in the owned files;
  - a failing gate;
  - an incomplete verdict.

## V3 Resume after a restart (US1, constitution VII)

1. During V1's build step, run `fallow stop`, then `fallow`.
2. **Expect:**
   - the run continues: the step is resumed under the same session;
   - no completed step runs again;
   - the history is complete.

## V4 A decision during a step, and reopening (US2)

1. Start the `cart` task at autonomy **High** with a two-part plan (`cart-core`, `receipt`) and a
   contract.
2. When the cart-core builder runs, its instruction includes the `clear()` question (fake script
   `ask-clear`; or a note to the builder asking it to raise the question).
3. **Expect:**
   - a decide step commits the contract change, and the builder continues with that commit;
   - the receipt part, if already done, is reopened, takes the commit and is verified again;
   - no question to the engineer.
4. Repeat at **Medium**. **Expect:** the question waits for the engineer with the planner's
   recommendation, and only cart-core waits.

## V5 The page (US3)

During V4:
- **Expect** each agent's reads, edits and commands to appear within 2 seconds;
- the decisions log shows who decided and the commit;
- the criteria list each `part/id` with evidence;
- closing and reopening the page shows the whole run.

## V6 Steering (US4)

In one `cart` run, do each of these once and check its effect and its line in the "You" history:
- **overrule** the `clear()` decision: both parts reopen;
- **add a criterion** to receipt after its review: the review checks it;
- **leave a note** for a builder: it appears in its next instruction;
- **pause, then resume** a part;
- **take over** a part, change a file, commit, and **hand back**: it is verified;
- **request changes** at the end: a repair round runs.

## V7 Publishing with a protected branch (US6)

On a test GitHub repository whose `main` requires one status check and one review:
1. **Expect:**
   - a pull request with the run summary;
   - the page shows "Required by main: <check>" and "Needs 1 approving review";
   - a deliberately failing check becomes a repair;
   - nothing is pushed to `main`.
2. Approve the pull request in GitHub, then **Merge** on the page. **Expect:** GitHub's merge,
   with the rules enforced.

## V8 Measurement (US7)

1. `fallow bench C:\Repos\FallowTasks --modes plain,solo --repeats 3`.
2. **Expect:**
   - runs one at a time, each in a fresh copy of the project;
   - hidden tests applied after each run's agents end;
   - the bench page grouping by task and mode, with pass rate, median time, cost and
     interventions.
3. Open `%LOCALAPPDATA%\fallow\records\`. **Expect:** numbers, identifiers and fingerprints only.
