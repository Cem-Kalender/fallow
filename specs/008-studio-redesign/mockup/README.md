# 008's reference mockup

Static screens, for the redesign's look (FR-017). They are made from the mocks the engineer reviewed on 2026-10-31 and 2026-11-01, use 007's tokens, fonts and logo (`mockup.css` copies them from `src/service/web/app.css`), and show the cart feature of the T086 runs. Open the `.html` files in a browser; the `.png` files are their screenshots at 1440 px and 390 px.

| Screen | Shows |
|---|---|
| `loop.html` | The loop page during a US2 repair: the steering in the header (Note, Limits, Pause, Stop) with the latest steering line; the pipeline strip with the review alongside; the "Verified by Fallow" row (US1 ✓ 5/5, US2 ✗ 1 failed, US3 0/4); US2's evidence panel open with a failed criterion (what was run, what came out, what was expected); the body with the task tree, three agent cards (reviewer and security reviewer side by side), the feed, and a side column holding only gates, questions and commits |
| `pipelines.html` | The Pipelines tab: the pipeline list (verifying / not verifying, the project's own); the graph of Standard with a security reviewer alongside the reviewer (groups per phase and per checkpoint, branches that join, red repair back-edges, + on edges); under it the selected step's settings, its agent in brief with "Edit this agent in Agents →", and what the pipeline guarantees; "Save for: this project only" |
| `agents.html` | The Agents tab: agents grouped Lead / Makers / Checkers with their source and use; the reviewer's editor with "Used in" links, runtime, model, kind, returns and may-run as fields, the instructions as text, "Edit as text", "Save for" and "Reset to the default" |

**Status**: approved by the engineer afterwards (decision of 2026-11-01: built overnight against this mockup, approved in the morning). At 390 px nothing scrolls sideways on any of the three; the graph keeps its proportions and scrolls inside its box when narrower than 700 px.

**Not shown**: "What was verified" (a dialog listing every story's evidence, next to Merge once the loop is ready); the + form; the keyboard focus states.
