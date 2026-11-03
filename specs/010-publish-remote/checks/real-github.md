# T032: one publication on a real GitHub repository (quickstart Q7)

2026-11-03, asked for by the engineer. A throwaway repository of the engineer's,
`Cem-Kalender/fallow-t032-check`, made public for the check only (GitHub Free protects no branch of a
private repository), with `main` protected: pull requests required, admins included, no approvals
needed. Fallow ran with the engineer's own `git` (credentials by gh's git credential helper) and
`gh` (signed in as Cem-Kalender); the loop itself ran on the fake runtime. Run by
`tests/real_github_010.rs` (ignored; `FALLOW_REAL_REPO=<owner>/<name> cargo test --test
real_github_010 -- --ignored --nocapture`). The repository was deleted afterwards.

| Check | Result |
|---|---|
| The remote as the project shows it | `github · Cem-Kalender/fallow-t032-check · main · protected`; offer: the pull request only ("changes reach it only through a pull request"); account Cem-Kalender |
| SC-003: from the confirmed "Push and open a pull request" to the pull request open | **7.9 s** (under 30 s) |
| SC-004: the pushed branch | `fallow/001-calc/build` at exactly the verified commit `9dfb399` (the pull request's head commit on GitHub) |
| The pull request | #1, base `main`, title "Calc", the loop's summary as its text (criteria with evidence, gates, the loop, "Verified by Fallow. The merge is the engineer's.") |
| SC-002: the target | `main` on GitHub unchanged until the engineer's merge; no force, no approval, no merge by Fallow |
| SC-005: merged on GitHub (by the engineer's `gh pr merge`) to the loop ending merged, without opening its page | **56.7 s** (under 2 minutes; the background read is every 60 s) |

First attempt: stopped by the check's own assertion (it expected `protected: true`; the view says
`"yes"`), before anything was published; corrected in the check, not in Fallow.
