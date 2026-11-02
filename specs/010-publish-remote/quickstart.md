# Quickstart: checking publishing that respects the remote (010)

Runnable checks that prove the feature end to end. All automated checks run without network: the
remote is a local bare repository that git reaches through `url.<bare>.insteadOf
https://github.com/acme/shop.git` (research R-1), and `gh` is replaced by `fallow-fake-host`
(contracts/host-adapter.md).

Build and test as for the rest of Fallow (debug builds, `CARGO_TARGET_DIR=target/wip`, `-j 2`,
`--test-threads=2`).

## Q1 The project's remote (US1, FR-001 to FR-004)

`cargo test --test us10_remote`:

- no remote → `remote.state = none`;
- a GitHub remote, target protected by a rule → `known`, `protected = yes`; by the classic flag only
  → `yes`; neither → `no`;
- not signed in, or the fake failing → `unknown` with the reason; the fake's log shows no token
  request;
- `git@gitlab.com:acme/shop.git` → `unsupported`, named `gitlab.com`;
- two remotes, the target tracking `upstream` → `upstream`.

## Q2 A finished loop becomes a pull request (US2, FR-005 to FR-012)

`cargo test --test us10_publish`:

- protected target: `publish.offer = [pull_request]`, the merge request is refused; publishing
  pushes the loop's branch at exactly the verified commit to the bare remote, the target branch
  there does not move, the fake received one `pr create` with the base, head, title and a body that
  holds every criterion's verdict; the loop is Publishing, with the link;
- unprotected target: both offered; the local merge pushes nothing (the bare remote unchanged);
- a branch of the same name with another commit on the remote: published as `…-2`, the other not
  touched;
- the fake failing at `pr create`: `PublishFailed { step: pull_request }`, the pushed branch named,
  the loop still Ready, a second try opens one pull request, not two;
- a destination equal to the target: refused before git runs.

## Q3 Publishing when verified (FR-019 to FR-021)

In `us10_publish`: a loop locked with level Free and no `when_verified` gets `pull_request`, ends
verified and is published without a request, with a "pull request opened" notice; one that ends
not verified is not published; with `protected = unknown` it waits and says why.

## Q4 Following the pull request (US3, FR-013 to FR-016)

In `us10_publish`: the fake turns the pull request merged → within one poll the loop is Done with
`Merged { how: "pull request" }`, the feature merged, the local target branch untouched; closed →
back to Ready with "pull request closed"; check states changed in the fake → recorded once each.

## Q5 Restart while publishing (R-8)

In `us10_publish`: stop the engine between the push and the pull request; start it again: the
fake shows no pull request → the loop is Ready with "publishing was interrupted"; with a pull
request found → it is recorded and followed, never a second one.

## Q6 The CI gate (R-10)

`cargo test --test no_egress_no_push`: pushing and the hosting tool only in `src/publish/`; no
force, `+` refspec, merge, review or approval anywhere; plus the behavioural check of Q2 that the
target branch on the remote never moves.

## Q7 On a real repository (by hand, once)

With a GitHub repository of your own whose `main` requires pull requests: open it in Fallow (the
project shows `github · you/repo · main · protected`), run a small feature to Ready, publish, see
the pull request on GitHub with the summary, merge it there, and see the loop turn merged within
two minutes.
