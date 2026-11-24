# T040: real-agent run (2026-11-02)

Run against the engineer's own Fallow (the 009 build) on `C:\Repos\FallowSandbox`, with `claude`.
The budget agreed with the engineer was $10; the run cost **$1.94**.

The lead is a project-only copy of the shipped lead on Sonnet, `sonnet-lead`, chosen when the feature
was started (`POST /api/projects/fallowsandbox/features {lead}`): the feature `010-receipt-printout`,
conversation `c058dcc81`.

| Check | Sent | What happened | Cost |
|---|---|---|---|
| Q2.1 a chosen lead | (new feature with `sonnet-lead`) | the view showed `Lead: sonnet-lead · claude · sonnet`, not fixed; fixed after the first message | – |
| Q1.2 a Spec Kit step from the menu | `/specify A receipt printout …` | the lead read `.claude/skills/speckit-specify/SKILL.md`, wrote `spec.md` and its checklist, one story with three `(must fail)` refusals; `step: specify` | $0.29 |
| Q1.2 a step with input | `/plan keep it small: one module, no new dependencies` | `plan.md` with one module and no dependencies, as asked; the same agent session resumed | $0.38 |
| Q1.5 a project command | `/summarize-spec what is printed per line` (from `.claude/commands/summarize-spec.md`) | exactly three bullets, focused on the input, no file changed | $0.42 |
| Q5.3 a file for the lead | `@shop/cart.py Which function …` | answered from the file's text: `lines(cart)` and `total_cents(cart)` | $0.44 |

One turn ($0.41) was spent on a message the test harness itself mangled (Git Bash turned
`/summarize-spec` into a Windows path before it was sent); the lead noticed and still answered.
It was sent again correctly (the row above).

Not run with a real agent: a loop question answered by the session's lead (Q2.5). A builder asks
only when it decides to, and a full loop would cost most of the budget; `tests/us9_leads.rs`
covers it with the fake runtime (the question goes to the session's lead, `by_lead`, overrule,
and no fork across a changed model).

Left in the sandbox for the engineer to keep or delete: the feature `010-receipt-printout` (its
spec session, `spec.md`, `plan.md`) and the agent `.fallow/agents/sonnet-lead.md`. The temporary
command file was removed.
