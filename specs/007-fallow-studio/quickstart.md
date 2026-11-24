# Quickstart: validating Fallow Studio

These are scenarios that show the feature works end to end. Q1–Q5 use real agents in the sandbox. Q6–Q8 are automated with the fake runtime.

## Prerequisites

- A debug build: `cargo build` (C: is nearly full: no release build, no debug info, as in 006).
- `claude` 2.1.x signed in, plus `git` and Python 3.
- The sandbox `C:\Repos\FallowSandbox`, reset to its seed (`782a472` on `master`), with no `.specify\` in it.
- No other Fallow running: `target\debug\fallow.exe stop`.

## Q1: Add a project, then talk about it (US1, US5)

1. Run `target\debug\fallow.exe`. The dashboard opens in the browser and looks like the mockup's screen 1.
2. Add `C:\Repos\FallowSandbox`.
   - **Expected**: the card shows `git` and `Spec Kit 1.0.12`. `.specify\` and `.claude\skills\speckit-*` now exist in the sandbox, and no existing file changed (`git status` shows only new files).
3. Open the project and ask the lead "how are prices handled?".
   - **Expected**: the reply streams in, the files it read are listed, and the mode chip shows `Ask · Shift+Tab`.
4. Press Shift+Tab three times.
   - **Expected**: the chip cycles Run anything, Read only, Approved commands, and back to Ask.

## Q2: A spec session to a lock (US1)

1. In the project conversation, ask the lead to propose a cart feature, then start its spec session.
   - **Expected**: the spec session opens (screen 2). The lead's first `spec.md` reflects what was discussed, and the stepper shows Specify done.
2. Answer the clarify questions by clicking choices. Edit one line of `spec.md` yourself, then send a message.
   - **Expected**: the lead's reply mentions your edit. The markers disappear as they are answered.
3. Before tasks and analyze are done, look at "Lock & start the loop".
   - **Expected**: it is disabled, and says what is missing.
4. Let the lead finish plan, tasks and analyze, then lock with the suggested budget.
   - **Expected**: the feature branch has the lock commit with the documents, and the loop page opens.

## Q3: The loop to one merge (US2, US3)

1. Watch the loop (screen 3) without doing anything.
   - **Expected**:
     - the task tree ticks tasks one by one;
     - each accepted task has a commit and a gate result;
     - a handover appears at the end of each phase, and the next phase's builder card shows a new session;
     - the reviewer runs at each story's checkpoint and answers every `USn/k`.
2. Open any finished task.
   - **Expected**: within a minute, you can name the agent, the diff, the commands and their results, the commit and the gates (SC-004).
3. Merge when it is ready.
   - **Expected**: one merge commit on `master` holds the whole feature, and `tasks.md` on `master` shows every task ticked.

## Q4: A question through the lead (US4)

1. Lock a feature whose spec leaves one behaviour open on purpose, and answers another in its Clarifications.
   - **Expected**:
     - the question the spec answers is answered by the lead ("lead answered"), committed, and the builder carries on;
     - the open question reaches you: in the inbox, as a desktop notification, and on the loop page with options. The loop waits for your answer.
2. Answer it.
   - **Expected**: the builder resumes in the same session with the committed answer.
3. Overrule the lead's answer.
   - **Expected**: the tasks built on it reopen, and the story's review runs again.

## Q5: Restart, unlock, unverified (US2)

1. Run `fallow.exe stop` during a task, then `fallow.exe` again.
   - **Expected**: the same task resumes in its session, and nothing completed runs again.
2. Unlock, change one user story, and look at the relock preview.
   - **Expected**: only that story is listed as affected. After the new lock, only it is rebuilt and reviewed.
3. Lock a feature with the Solo pipeline.
   - **Expected**: it ends as "Done, not verified by Fallow: no review at checkpoints".

## Q6–Q8: Automated (`cargo test`)

- **Q6, the loop with the fake runtime**:
  - tasks, phases, handovers and checkpoints;
  - false claims: a missing handover, edited locked documents, an incomplete verdict, a pipeline without gates;
  - resume at every stage;
  - an unlock with the affected stories.
- **Q7, `ask`**: answered, decided (Free), relayed, reserved items always relayed (Strict and Normal), and overruled.
- **Q8, formats and the page's API**:
  - the agent definition and pipeline parsers, with errors at their lines;
  - the `tasks.md` and `spec.md` readers;
  - the API's authentication and checks (006);
  - the chat modes turning into the right permission rules.

## The look (SC-007, SC-008)

Open each screen next to its mockup file (`mockup/*.dc.html` on the canvas) at 1440 px wide, and check that the fonts, colors, logo and layout are the same. Then check each screen at 390 px for horizontal scrolling. The engineer approves.
