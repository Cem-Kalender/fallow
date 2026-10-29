# Contract: MCP tool changes (on top of 004)

| Tool | Who | Change |
|---|---|---|
| `session_start` | lead | Builders take `work` and, for parallel parts, `part`. Refused by the manifest's decision (R-2): work built by the lead, a second builder for `builder` work, unprepared parts (no `owns`, overlap, missing contract), a part owning only tests or docs, `max_parallel_builders` reached. A start that does not fit the memory budget is `waiting` with the shortfall (FR-038). Reviewers and investigators take a `focus` (from `[[review]]`, or a hypothesis text for investigators). |
| `signal_raise` | builder | NEW. `{kind: "question" \| "change", work, part?, text, diff?}`; `change` needs `diff`. Goes to the lead only. Returns the signal id. |
| `signal_answer` | lead | NEW. `{signal, text, commit?, accept?}`. `accept: true` answers a signal Fallow raised for a non-lead change to the manifest, docs or a contract: that work then counts (R-19). A `commit` must be on the lead's branch and change the plan docs, the manifest or a contract; required unless the answer states no change. The raiser receives the text and the commit to take in. |
| `signal_list` | lead, builder | NEW. Open and answered signals (the builder sees its own). |
| `task_report` | builder, lead | Done reports are also refused when the reported commit lacks a commit that changed the part's criteria or contracts since its base (R-5), naming it. Files outside `owns`/`shared` and non-lead changes to the manifest, docs or contracts are reported to the lead (R-6). The lead reports its own pieces of work on tasks Fallow creates from the manifest. |
| `review_verdict` | reviewer | Adds `checks: [{criterion, ran, expect: "pass" \| "fail", result: "pass" \| "fail"}]`. Without a check per criterion of the reviewed work and one with `expect: "fail"`, the verdict is recorded as `incomplete`, does not count, and the reply says what is missing (R-7). A reviewer started with a focus approves for that focus; `review:approved` needs one per focus (R-16). |
| `deliver` | builder | May target only the lead (FR-021). |
| `consult_lead` (004) | | Removed; use `signal_raise`. |
| `whoami` | all | The lead's adds the manifest commit, its problems when it has any, the manifest's format, and the default ("you build each piece yourself unless the manifest records otherwise"). A builder's adds its work, part, owned globs, criteria and base. |
| `squad_status` | all | Adds the manifest commit and its problems when it has any, each piece of work with its build decision and reason, builder, base, branch, latest reported commit and evidence, open signals, switched-off components, memory budget use and waiting starts (FR-033). |

## Briefings (changes)

- **Lead**: build each piece yourself unless it has clean boundaries; record every decision and its
  reason in `.fallow/work.toml` and commit it before starting a builder; write criteria (with at least
  one case that must fail) before building; answer signals by committing to the plan, manifest or a
  contract and point to the commit; use your own subagents for investigation.
- **Builder**: you own `owns` for your part and write its tests; build against the contract; ask the
  lead with `signal_raise` and carry on with unblocked work; take in the commits the lead points to;
  use your own subagents for investigation.
- **Reviewer** (`check` level): you may run commands but must not change files; check the integrated
  result against each criterion as a black box, including the cases that must fail; your focus is X.
- **Investigator** (`check` level): test your hypothesis; report evidence for or against it to the
  lead with `signal_raise` (question).

## Updates typed into the lead (FR-035)

Only: a signal; a failed gate; a verdict; an exited session; a session at its permission prompt; a
ready merge; an ownership or plan-change report; a signal unanswered past the limit (also to the
engineer). No quiet-time status.
