# Quickstart: validating the Fallow Studio redesign

Scenarios that show 008 works end to end:
- **Q1–Q4** run on the page against loops of the fake runtime, or real agents in the sandbox.
- **Q5** is automated.
- **The look** is checked against `mockup/`.

## Prerequisites

As for 007 (its quickstart):
- a debug build;
- the sandbox `C:\Repos\FallowSandbox`;
- `claude` signed in, for real runs.

## Q1: What was verified (US1)

1. Open a finished loop whose stories were all checked.
   - **Expected:**
     - one "Verified by Fallow" row, for example `US1 ✓ 5/5`;
     - no criterion text on the page until a story is opened.
2. Open a story.
   - **Expected:** each criterion with its status, what the reviewer ran and what came out.
3. Run a loop where a criterion fails.
   - **Expected:**
     - the pill turns to `✗ 1 failed`;
     - the story's panel opens by itself, with what was expected;
     - the feed shows the failure.
4. Open a loop on the Solo pipeline.
   - **Expected:** every story shows "not checked", and its panel says that no checker verifies these criteria.
5. At ready, choose "What was verified".
   - **Expected:** the evidence of every story.

## Q2: Steering from the header (US2)

1. On a running loop, leave a note and raise the cost limit from the header.
   - **Expected:**
     - the latest steering line shows them;
     - the next step receives the note;
     - no steering card in the side column.
2. At ready.
   - **Expected:** no Pause.

## Q3: Pipelines and Agents (US3)

1. Open Agents, click `builder`, then `reviewer`.
   - **Expected:** the editor shows the reviewer's definition.
2. Change the reviewer's model to `opus`, save for "this project only".
   - **Expected:** `.fallow/agents/reviewer.md` in the project differs from the default only in its `model:` line.
3. Open Pipelines and select the Review step.
   - **Expected:** the reviewer in brief, with "Edit this agent in Agents →".
4. Change an agent so that a pipeline in use stops verifying.
   - **Expected:** the save is refused with the reason.

## Q4: The graph (US4)

1. Open Standard.
   - **Expected:**
     - Spec session and Lock;
     - the builder and the gates in "For each phase";
     - the reviewer in "At each story's checkpoint";
     - Merge;
     - the red repair back-edges.
2. Add `security-reviewer` alongside the reviewer through the + after the gates.
   - **Expected:**
     - the two branch and join again;
     - "Edit as TOML" shows `parallel_with`;
     - the pipeline is still verifying.
3. Remove the reviewer and the security reviewer.
   - **Expected:** "not verifying", with what is missing.
4. Use only the keyboard: Tab to a node, Enter, Delete.
   - **Expected:** the same as with the mouse.

## Q5: Parallel checkers (US5, `cargo test`)

- **Overlap:** two checkers marked `parallel_with`, each taking 3 s, overlap in time, and the checkpoint waits for both.
- **One repair:** both fail, so one repair carries both sets of findings; then both run again.
- **Slot cap:** with `max_parallel = 1` they run one after the other, with the same outcome.
- **Restart:** Fallow is stopped while both run; after the start, each resumes in its own session.

## The look (US6)

Open each changed screen next to its mockup in `mockup/`, at 1440 px and at 390 px:
- the loop page;
- Pipelines;
- Agents.

Check that the regions, their order, the colours and the fonts match. At 390 px nothing scrolls sideways. The engineer approves.
