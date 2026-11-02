# Quickstart: validating chat commands and lead choice

Scenarios that show 009 works end to end. Q1–Q5 run on the page against the fake runtime (the demo service on the `vc` fixture, as in 008) or with real agents in `C:\Repos\FallowSandbox`. Q6 is the automated suite.

## Prerequisites

As for 008:
- a debug build;
- the sandbox;
- `claude` signed in, for the real-agent checks.

For Q2, a second lead: a project override `.fallow/agents/careful-lead.md`, a copy of `lead` with other instructions.

## Q1: The `/` menu (US1)

1. In a spec session, type `/`.
   - **Expected:** four groups, each entry with a line of description. The project's commands show where they came from.
2. Type `pl`, then Enter, then type `keep it small`, then Enter.
   - **Expected:**
     - the chat shows `/plan keep it small`;
     - the lead runs the plan step and `plan.md` appears;
     - with a real lead, the step is the same as when asked in words (SC-003).
3. Type `/`, then pick "Lock the spec" before the spec is ready.
   - **Expected:** it is greyed out with "Not ready to lock: …" and cannot be picked.
4. In the project conversation, send `/specify A receipts page`.
   - **Expected:** a new feature "A receipts page" opens its spec session, where the lead runs specify.
5. Add `.claude/commands/review-copy.md` to the sandbox, then send `/review-copy the cart page`.
   - **Expected:** the lead follows that file's instructions with "the cart page" as the input.
6. Send `/nothing here`.
   - **Expected:** it is sent as plain text.
7. Use only the keyboard throughout.
   - **Expected:** a screen reader announces the highlighted entry and the count.

## Q2: Choosing the lead (US2)

1. On the Agents tab, make `careful-lead` this project's lead. Start a feature.
   - **Expected:** the chat head says "Lead: careful-lead", and careful-lead answers.
2. Start another feature and pick `lead` before the first message.
   - **Expected:** `lead` answers there.
3. Try to change the lead after the first message.
   - **Expected:** not offered, with "A session keeps its lead".
4. Change careful-lead's model, then send in its session.
   - **Expected:** the notice says the next message starts a new session, and it does.
5. Lock the careful-lead feature and run its loop until a builder asks a question the lead answers.
   - **Expected:** the feed shows "careful-lead answered".
6. Delete careful-lead.
   - **Expected:** its session says the lead is gone; the project falls back to `lead`.

## Q3: The working indicator (US3)

1. Send a message.
   - **Expected:** the dots move until the answer arrives, and do not restart while live text streams in.
2. Turn on the system's reduced-motion setting, then send again.
   - **Expected:** the dots are still.

## Q4: Unsaved changes (US4)

1. Change an agent's instructions, then click Pipelines.
   - **Expected:** "Leave your changes?" appears.
2. Choose to stay.
   - **Expected:** the agent and the change are still there.
3. Repeat step 1 with the Dashboard, a project link, Back, a reload and closing the window.
   - **Expected:** each asks (the last two through the browser's prompt).
4. Edit a spec document and click Loops.
   - **Expected:** it asks.

## Q5: The project's files (US5)

1. With a loop running, open the project page and choose Files, then the feature's version.
   - **Expected:**
     - changed files are marked;
     - "Only changed" narrows the tree;
     - `.env` shows a lock and does not open.
2. Open a changed file.
   - **Expected:**
     - the added lines are green;
     - the header says "by builder in T009";
     - the builder's next change appears within 5 s without losing the place.
3. Choose "Ask the lead about it" and send.
   - **Expected:** the project conversation's lead answers about that file on that branch.
4. Choose "Open in my editor" for the first time.
   - **Expected:** Fallow proposes the editor it found; after confirming, the file opens in it, and next time it opens directly.
5. On the loop page, choose Files and open a file.
   - **Expected:** the drawer widens over the cards and the feed; "Back to the loop" restores them.
6. Compare with `mockup/files.html`.
   - **Expected:** it matches at 1440 px and 390 px.

## Q6: Automated

`cargo test --test-threads=2`:
- `tests/us9_leads.rs`
- `tests/us9_commands.rs`
- `tests/us9_files.rs`
- every earlier test

The page checks (the menu by keyboard, the indicator under reduced motion, the leave prompt, and the file views against the mockup) run with headless Chrome, as 008's visual check does.
