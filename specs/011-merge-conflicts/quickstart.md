# Quickstart: checking merge conflicts between loops

All but Q7 run without network or a model: the fake runtime, a local bare remote behind a
`github.com` address and `fallow-fake-host` in place of `gh` (as 010). Build and test as usual:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/wip \
  cargo test -j 2 --test us11_update --test us11_line --test us11_publish_update --test us11_overlap -- --test-threads=2
```

## Q1 A conflict is resolved and verified again (US1)

Two features in one project change the same line of `shop/receipt.py`. Merge the first. The second's
loop page shows `target.moved`, `conflicts: ["shop/receipt.py"]`, `offer: ["bring_up_to_date",
"stop"]`, no `merge`. Bring it up to date: a repair step on `update-1` names the file; after it, the
gates run against the first feature's merge, both stories are checked again, the loop is ready with a
new verified commit, and it merges without conflict. The target never held a conflict.

## Q2 A target that moved without conflict (US2)

The same with different files: no builder step, but gates and checker run again before `merge` is
offered.

## Q3 Levels (US3)

Q1 with the second loop locked Free: no action needed until the merge. With Normal: "Needs you"
shows `update`. With a budget too small: it asks.

## Q4 The merge line and the limit (US2 scenarios 4-5)

Two Free loops ready for the same target: the second waits "in line to merge after" the first and is
verified again once. Then, with `update_limit = 1`, move the target from outside Fallow (a commit in
the test) while a loop holds its turn: the loop asks with `verify_again` and `merge_unchecked`;
merging that way records `unchecked` and labels the loop page "not checked with: 1 commit".

## Q5 A pull request that conflicts (US4)

Publish a loop; push a conflicting commit to the bare remote's target; set the fake host's
`mergeable: CONFLICTING`. Bringing it up to date fetches the remote target, resolves, verifies again,
and pushes forward: the bare remote's branch moves from the published commit to the new one, the
fake host's pull request number is unchanged, nothing was forced, the target did not move. Push a
foreign commit to the loop's branch first and the push is refused, without a `-2` branch.

## Q6 Warnings (US5)

Lock a feature whose tasks name a file a running loop changed: the overlap read names it; "Run
after" makes the new loop wait and start when the other is merged. During a run, make two loops
change the same file: both pages show the overlap with "Wait for".

## Q7 By hand (optional)

On the engineer's machine with two real features: the same flows on the Studio page, checking the
wording of the buttons and notifications.
