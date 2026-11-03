# Feature Specification: Merge conflicts between loops

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/011-merge-conflicts`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "Merge conflicts between loops (011, asked for by the engineer on 2026-11-03, before solo sessions, now 012). Today, when a finished loop's verified commit cannot be merged into its target branch because another loop (or the engineer) changed the same lines since, Fallow aborts the merge cleanly and the loop page says 'The merge did not go through' with the conflicting files and the options 'merge again' or 'stop'; 'merge again' fails the same way, so the engineer must resolve it by hand outside Fallow, unverified. For a pull request (010), bringing a branch up to date was left out. What the engineer wants: 1) A conflict, or a target that moved, is resolved like any change, verified: Fallow brings the loop's branch up to date with the target in the loop's own working copy; without a conflict the gates and the checker run again on the result; with a conflict, the work goes back to a builder as a repair step, then gates and checker as usual; only the newly verified commit is offered to merge or publish. Whether Fallow asks first follows the autonomy level and the loop's caps and budget. The same applies to a published loop whose pull request fell behind or conflicts (the loop's own branch updated, never forced). 2) Prevention: when the engineer locks a feature, or when a running loop starts changing files, Fallow warns when another unfinished loop of the project changes the same files, and offers to run them one after the other. Out of scope: solo sessions (012), dark mode (013), the standalone app (014); resolving conflicts in the engineer's own folder; merging pull requests for the engineer; a failing remote check turned into a repair (006 FR-025)."

## Context

A loop ends with one verified commit: built on the feature's branch from where the feature was locked, its gates run by Fallow against that starting point, its user stories checked by an independent checker. Fallow then merges exactly that commit into the target branch (the branch the feature was started from), or publishes it as a pull request (010).

When two loops of a project run at the same time, or the engineer commits on the target meanwhile, the target moves on under a loop. If the same lines changed, the merge conflicts. Today Fallow aborts it cleanly, leaves the target untouched, and names the conflicting files; but it offers nothing that can succeed: "merge again" conflicts again, since the verified commit never changes. The engineer has to resolve it by hand, outside Fallow, and the result is not verified, which is exactly what constitution I forbids Fallow to offer.

Even without a conflict, a verified commit merged onto a target that moved is a combination nobody checked: the gates ran without the other work underneath.

This feature treats both like any other change: Fallow brings the loop's branch up to date in the loop's own working copy, a builder resolves what conflicts, and the gates and the checker run again; only that newly verified commit is merged or published. And it warns before it comes to that, when two unfinished loops change the same files.

**What does not change**: how a loop builds and verifies, that only Fallow merges and only on the engineer's word (or the autonomy level the engineer chose), that the target branch and the engineer's folder are never used to resolve anything, and the remote rules of 010 (never a push to the target, never forced).

## Clarifications

### Session 2026-11-03

- Q: When the target moved without a conflict, must the loop always be verified again before it is merged? → A: Yes, always. To keep that from becoming an endless chase, merges into the same target go through a queue (a loop being verified again for its merge is never overtaken by another loop), and after a set number of updates in a row that were overtaken anyway (by commits from outside Fallow), the loop asks the engineer: "Try again" or "Merge anyway", the latter shown as not verified on top of the target as it is now. The number is set in the lock form, default 3.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A conflicting loop is brought up to date and verified again (Priority: P1)

Two loops of a project changed the same lines. The first is merged. The second is ready, but its merge would conflict. Instead of "The merge did not go through", its loop page says the target moved and conflicts with this loop in named files, and offers **Bring up to date**. Fallow then merges the target into the loop's own branch, in the loop's own working copy; the conflicting files go to a builder as a repair step that names them and what each side changed ("resolve these conflicts: this feature's criteria and the work already on the target must both keep working"). Then the gates run against the new starting point and the checker checks the user stories again. The loop is ready again with a new verified commit, which merges cleanly.

**Why this priority**: today a conflict is a dead end inside Fallow; this is the core of the feature.

**Independent Test**: on the fake runtime, run two loops that change the same lines; merge the first; check that the second's merge is not attempted blindly, that "Bring up to date" leads to a repair step naming the conflicting files, gates and checker run again, the new verified commit merges without conflict, and the target never held a conflict or an unverified commit.

**Acceptance Scenarios**:

1. **Given** a ready loop whose target moved with changes that conflict with it, **When** the engineer opens the loop page, **Then** it says so before any merge is tried, names the conflicting files and the commits on the target that caused it, and offers "Bring up to date" and "Stop"; "Merge" is not offered.
2. **Given** "Bring up to date", **When** Fallow brings the branch up to date, **Then** it does so in the loop's own working copy, never in the engineer's folder or on the target, and the conflicting files go to a builder as a repair step that names them.
3. **Given** the builder resolved the conflicts, **When** its step ends, **Then** Fallow checks that no conflict markers remain and that the files are committed, runs the gates against the new starting point (the target as it is now), and has the checker check every user story again, in the pipeline's review rounds; the loop is ready only when all of that passed.
4. **Given** a loop brought up to date and verified again, **When** the engineer merges, **Then** exactly the new verified commit is merged, without conflict.
5. **Given** the builder cannot resolve the conflict within the loop's repair cap, **When** the cap is reached (the pipeline's repair cap per task, counted from zero for this update), **Then** the loop waits for the engineer with what is left in conflict, as any loop does at its cap; nothing is merged.

---

### User Story 2 - A target that moved without conflict is verified again too (Priority: P1)

The target moved since the loop started, but nothing conflicts. The verified commit would merge, but the combination was never checked: the other work was not underneath when the gates ran. Fallow brings the branch up to date (no builder needed), runs the gates and the checker again, and offers the merge only then. Merges into the same target take turns, so another loop cannot overtake it meanwhile; only commits from outside Fallow can, and after a set number of updates in a row overtaken that way, the engineer decides.

**Why this priority**: an unchecked combination merged as "verified" is the quieter version of the same problem.

**Independent Test**: on the fake runtime, merge one loop, then make a second, non-conflicting loop ready; check what is offered, that the gates and the checker run on the combination before the merge, and that the merged commit is the newly verified one.

**Acceptance Scenarios**:

1. **Given** a ready loop whose target moved without a conflict, **When** the engineer opens the loop page, **Then** it says the target moved since the loop was verified (by how many commits, from which features or "outside Fallow"), and offers "Bring up to date" (by the autonomy level, User Story 3), not "Merge".
2. **Given** two ready loops with the same target, **When** the first is being brought up to date for its merge, **Then** the second waits in line, its page saying "in line to merge after <feature>", and its turn comes when the first is merged, stopped or leaves the line.
3. **Given** a loop whose updates were overtaken by commits from outside Fallow as many times in a row as its lock allows (3 by default), **When** the next would start, **Then** the loop asks the engineer: "Try again" (another update), "Merge anyway" (only when that merge would not conflict: the commit as last verified, merged with the label "not verified on top of <target> as it is now", naming the commits it was not verified with), or "Stop" (except as in FR-016b). For a publication, the choice is the same with "Publish anyway" for a first publication, and "Leave the pull request as it is" for an open one.
4. **Given** the loop brought up to date, **When** the gates and the checker pass on the combination, **Then** the loop is ready with the new verified commit.
5. **Given** the gates or the checker fail on the combination, **When** that happens, **Then** it is a repair like any failing gate or criterion: a builder gets the failure, within the repair cap.

---

### User Story 3 - The autonomy level decides who starts it (Priority: P2)

A loop locked with Free brings itself up to date and verifies again on its own, within its caps and budget, and the engineer finds it ready again (or waiting, with why). With Normal or Strict, Fallow asks first: the loop page and "Needs you" show "Bring up to date" with what it will do. When the loop's budget is used up, or its updates were overtaken as many times in a row as its lock allows, Fallow asks whatever the level.

**Why this priority**: it decides whether this is a click or nothing at all, but the mechanism comes first.

**Independent Test**: run the conflict of User Story 1 with a loop locked Free and with one locked Normal; check that the Free loop brings itself up to date without an action and the Normal one waits under "Needs you"; exhaust the budget of a Free loop and check that it asks.

**Acceptance Scenarios**:

1. **Given** a loop locked Free whose target moved, **When** it becomes ready, **Then** it brings itself up to date and verifies again without the engineer, records that it did so and why, and the engineer can see each step.
2. **Given** a loop locked Normal or Strict whose target moved, **When** it becomes ready, **Then** it waits for the engineer, shown under "Needs you" with "Bring up to date".
3. **Given** a loop whose budget is used up, or whose updates were overtaken as many times in a row as its lock allows, **When** it would start another update, **Then** it asks the engineer whatever its level, naming the limit.

---

### User Story 4 - A pull request that fell behind or conflicts (Priority: P2)

A loop was published as a pull request (010). Meanwhile the target moved on the remote, and the pull request now conflicts, or the target's rules require it to be up to date. The loop page says so and offers "Bring up to date". Fallow brings the loop's branch up to date with the target as the remote has it, in the loop's own working copy; a conflict goes to a builder; the gates and the checker run again; and Fallow pushes the new verified commit to the loop's own branch on the remote, moving it forward (never forced), so the pull request shows the new commit. The merge stays the engineer's, on the remote.

**Why this priority**: the same problem on a published loop; 010 left it open.

**Independent Test**: with the fake hosting tool, publish a loop, move the target on the remote so the pull request conflicts; check that bringing it up to date resolves, verifies again, and moves the loop's branch forward without forcing, that the pull request stays the same one, and that the target on the remote never moved.

**Acceptance Scenarios**:

1. **Given** an open pull request of a loop that conflicts with its target, or whose target requires branches to be up to date, **When** Fallow reads its state, **Then** the loop page says so and offers "Bring up to date"; by the autonomy level as in User Story 3.
2. **Given** "Bring up to date" on a published loop, **When** the new commit is verified, **Then** Fallow pushes it to the loop's own branch as a move forward from the last commit Fallow pushed there, never forced, and the same pull request shows it.
3. **Given** the loop's branch on the remote holds a commit Fallow did not push (someone pushed to it), **When** Fallow would push, **Then** it pushes nothing and says so; the engineer decides.
4. **Given** a pull request that is only behind, with a target that does not require it to be up to date, **When** the engineer opens the loop page, **Then** it shows that it is behind and offers "Bring up to date" without urging it.

---

### User Story 5 - A warning before two loops collide (Priority: P2)

When the engineer locks a feature, Fallow compares the files its tasks name with the files that other unfinished loops of the project have changed so far or name in their tasks. When they overlap, the lock form says which loops and files, and offers "Run after <feature>": the new loop then waits, and starts by itself when the other loop is merged or stopped. While loops run, when a step's commit changes files that another unfinished loop has also changed, Fallow says so on both loop pages and offers to let one wait for the other.

**Why this priority**: cheaper than any repair, but a warning, not a guarantee; resolving (User Stories 1 and 2) is what makes it safe.

**Independent Test**: lock a feature whose tasks name a file that a running loop has changed; check the warning names the loop and the file, that "Run after" makes the new loop wait and start when the other is merged, and that locking anyway works; during a run, make two loops change the same file and check both loop pages say so.

**Acceptance Scenarios**:

1. **Given** an unfinished loop that changed or names `shop/receipt.py`, **When** the engineer locks a feature whose tasks name the same file, **Then** the lock form names the loop and the file, and offers "Run after <feature>" and "Run now anyway".
2. **Given** "Run after", **When** the other loop is merged or stopped, **Then** the waiting loop starts by itself, from the target as it is then; until then it is shown as waiting for that loop.
3. **Given** two running loops, **When** a step's commit of one changes a file the other has changed, **Then** both loop pages say which files overlap and with which loop, and offer "Wait for <feature>" (this loop pauses after its current step and resumes when the other is merged or stopped).
4. **Given** no overlap, **When** the engineer locks, **Then** nothing is said.

### Edge Cases

- **The same file changed but different lines**: no conflict; User Story 2 applies (the target moved).
- **The target moved by the engineer's own commits**, not a loop: the same; the loop page names them as "outside Fallow".
- **A change on the target to this feature's locked documents** (its spec, plan or tasks), conflicting or not: it is not merged in silently and not handed to a builder (builders never edit the locked documents, and the checker must judge the criteria the engineer locked); the loop waits for the engineer, naming the documents, with "Lock again" or "Stop" (FR-005).
- **A file deleted on one side and changed on the other**, or a binary file in conflict: the builder gets it like any conflict, with what each side did; if it cannot be resolved within the cap, the loop waits for the engineer.
- **The target moves again while the loop is being brought up to date**: the loop is verified against the target as it was when the update started; at the merge, User Story 1 or 2 applies again. Other loops cannot cause this (the merge line, FR-009a); commits from outside Fallow can, and they count as overtaken updates (FR-007), up to the limit set at the lock.
- **The engineer's folder has the target checked out with uncommitted changes**: nothing in this feature touches it; the final merge behaves as today.
- **A waiting loop ("Run after") whose other loop is deleted or its project removed**: it starts by itself, saying why.
- **Fallow restarts while a loop is being brought up to date or waits for another**: it resumes where it was, without repeating a completed step (constitution VII); a waiting loop keeps waiting.
- **The target branch is deleted or renamed** (locally, or on the remote for a published loop): nothing is merged or pushed; the loop page says the target is gone, and the engineer decides (stop, except as in FR-016b, or lock again on another branch).
- **The pull request is merged or closed on the remote while an update runs**: the update's running steps are stopped, what they left uncommitted is dropped, nothing is pushed, and this is recorded; the loop follows the pull request's end as in 010.
- **A loop stopped during an update** (one not published): it is stopped like any loop; its branch keeps what was committed, and nothing is merged or pushed. A published loop is never stopped or paused while its pull request is open (FR-016b).
- **Parallel parts within one loop** already integrate their own work (stage "integrating"); this feature concerns the target moving under the loop as a whole.
- **Files named in tasks are only a forecast**: the warning at the lock says it compares with the files the tasks name and the files changed so far, not with what the loop will change.

## Requirements *(mandatory)*

### Functional Requirements

**Knowing that the target moved**

- **FR-001**: Before offering a merge or a publication, Fallow MUST check whether the target branch moved since the commit the loop was verified on (it moved when it holds commits that the verified commit does not contain), and whether merging would conflict, without changing the target or the engineer's folder.
- **FR-002**: When the target moved, the loop page MUST say so: by how many commits, from which features (a loop's merge) or "outside Fallow", and, when it would conflict, which files.
- **FR-003**: A merge that would conflict MUST NOT be offered; "Bring up to date" and "Stop" (except as in FR-016b) MUST be. The same check MUST be made again when the merge runs; when the target moved in between, the merge MUST NOT be tried and FR-002 and FR-003 apply.
- **FR-003a**: While an update runs, the loop MUST NOT be ready: its page shows it as being brought up to date, step by step, and the old verified commit MUST NOT be offered to merge or publish, nor published on its own (010's "When verified"). A loop that was ready goes back to building for the update's task and returns to ready when it is verified; a published loop stays publishing, its pull request still followed (a merge or close on the remote is seen during the update), and returns to it with the new commit.

**Bringing a loop up to date**

- **FR-004**: Bringing a loop up to date MUST merge the target, as it is then, into the loop's own branch in the loop's own working copy; it MUST NOT touch the target branch, the engineer's folder, or another loop's branch.
- **FR-005**: Conflicting files MUST go to a builder as a repair step that names each file and what each side changed, with the instruction that the feature's criteria and the work already on the target must both keep working. A change on the target to the feature's own locked documents, conflicting or not, MUST NOT be merged in by the update nor go to a builder; the loop waits for the engineer, naming the documents, with "Lock again" (the feature's documents taken from the target into its spec session, locked again by the engineer as any re-lock, after which the loop goes on from the new lock) and "Stop" (except as in FR-016b).
- **FR-006**: After the update, Fallow MUST verify the result as any result: no conflict markers left, the changes committed, the gates run by Fallow against the new starting point (the target as merged in), and every user story checked again by the checker (the pipeline's review rounds). Only then MUST the loop be ready, with the new verified commit.
- **FR-007**: An update MUST count as a new task of the loop: its repairs (the conflict, and any failure after it) count against the pipeline's repair cap per task, starting from zero for that update; the review rounds after it count against the pipeline's review rounds, starting from zero; and everything counts against the loop's budget. Reaching a cap or the budget during an update MUST stop it there and make the loop wait for the engineer, as at any cap. The lock form MUST set how many updates in a row may be overtaken before the engineer decides (default 3, at least 1): an update is overtaken when the target moved again before its result could be merged or published. When that number is reached, the loop MUST ask whatever the level (User Story 2, scenario 5). The count starts again when the loop is merged or published, or the engineer chooses "Try again".
- **FR-008**: Each update MUST be recorded in the loop's history: when, what the target was, what conflicted, who resolved it, and the outcome; the engineer can see every step as for any step.
- **FR-009**: A target that moved without a conflict MUST be handled like a conflict without files: the loop is brought up to date and verified again before any merge or publication. "Merge anyway" MUST be offered only when the number of overtaken updates is reached (FR-007) and the merge would not conflict (FR-003), and MUST label the merge as not verified on top of the target as it is then, in the loop's history, its page and its notification, never in the words or colors of a verified merge. For a publication at that limit, Fallow MUST offer "Publish anyway" for a first publication (the pull request's text then says which target commits the result was not verified with) and "Leave the pull request as it is" for an open one (it stays as published; the engineer decides on the remote); never a local merge.
- **FR-009a**: Local merges into the same target MUST take turns: while a loop is being brought up to date for its merge, or is merging, other loops' merges into that target MUST wait in line, in the order they became ready, each shown as "in line to merge after <feature>"; the line holds only while a loop is being brought up to date for its merge or is merging: a loop that waits (for the engineer, at a cap, at its budget, for a lock again, at the limit of overtaken updates, at Normal or Strict before its update or before the engineer's merge, or for another loop) MUST leave the line, so the loops behind it go on; when it goes on, it takes its turn again at the end of the line, and the check before the merge (FR-003) applies. Commits from outside Fallow cannot be held; the count of FR-007 covers them.

**Who starts it**

- **FR-010**: With a loop locked Free, Fallow MUST bring it up to date by itself within the loop's caps and budget. With Normal or Strict, it MUST ask first, under "Needs you", naming what it will do (both levels alike here: the update is a new task, and those levels leave starting new work to the engineer). When the loop's remaining budget is used up, or its overtaken updates reached the limit set at the lock (FR-007), it MUST ask whatever the level.

**Published loops**

- **FR-011**: For a loop with an open pull request, Fallow MUST show when the pull request conflicts with its target or is behind it, and whether the target requires it to be up to date, from what the hosting service reports.
- **FR-011a**: Before Fallow pushes a loop's commit for a pull request (010's first publication, or after an update), it MUST check the target as the remote has it then, as FR-001 does locally; when it moved since the commit was verified, the push MUST NOT happen and FR-002 and FR-003 apply, and the update then merges the target as the remote has it (as FR-012 does), so that the check passes after it.
- **FR-012**: Bringing a published loop up to date MUST use the target as the remote has it, verify as in FR-006, and push the new verified commit to the loop's own branch, so the same pull request shows it. This amends 010 FR-012 for this one case: the loop's branch on the remote MAY be moved forward only when it is exactly at the last commit Fallow pushed to it and the new commit descends from that one; never forced. In every other case (someone else pushed to it, it was deleted), Fallow MUST push nothing, MUST NOT fall back to another branch name or a second pull request, and MUST say so. Nothing is ever pushed to the target (constitution II).

**Warnings**

- **FR-013**: When the engineer locks a feature, Fallow MUST compare the files its tasks name with the files each unfinished loop of the project has changed so far or names in its tasks, and when they overlap, name the loops and files in the lock form, with "Run after <feature>" and "Run now anyway".
- **FR-014**: A loop locked to run after another MUST wait, shown as waiting for that loop, and start by itself when the other loop is merged, stopped or deleted. When the target moved since the lock and the loop has built nothing yet, its starting point becomes the target as it is then: the target is merged into its branch without gates or checker (there is nothing built to check), and a change there to its own locked documents is handled as in FR-005.
- **FR-015**: While loops run, when a step's commit of one loop changes files another unfinished loop has changed, Fallow MUST say so on both loop pages, naming the files and the loop, and offer "Wait for <feature>" (except on a loop whose pull request is open, FR-016b): the loop pauses after its current step and resumes when the other is merged, stopped or deleted.
- **FR-016**: Warnings MUST NOT block: the engineer can always run anyway. A wait that would make two loops wait for each other (directly or through others) MUST NOT be offered; the page says which loop already waits for this one.
- **FR-016a**: An unfinished loop, for the warnings, is one in any stage but done or stopped: planning, building, integrating, gating, reviewing, ready, paused, or publishing, including one whose pull request is open.
- **FR-016b**: At any time, a loop whose pull request is open MUST NOT be stopped or paused, and "Stop" and "Wait for" MUST NOT be offered for it; its page says to close the pull request on the remote first (what Fallow already enforces since 010). This holds wherever this spec offers "Stop" or "Wait for" (FR-003, FR-005, FR-015).

**Lasting and runtimes**

- **FR-017**: An update in progress, a loop waiting for another, the merge line's order and each loop's count of overtaken updates MUST survive Fallow restarting, without repeating a completed step (constitution VII).
- **FR-018**: All of this MUST be decided from what Fallow observes (git, its own records, the gates, the checker's verdict), never from how one runtime behaves, and MUST be testable on the fake runtime (constitution IX).

### Key Entities

- **Update of a loop**: bringing a loop's branch up to date with its target: the target commit merged in, the files in conflict (if any), the repair step that resolved them, the new verified commit, the outcome.
- **Overlap**: two unfinished loops of a project touching the same files: the loops, the files, when it was seen (at the lock or during a run), and what the engineer chose (run now, run after, wait).
- **Waiting for a loop**: a loop that does not run until another loop is merged, stopped or deleted.
- **Merge line**: per target branch, the loops waiting their turn to merge, in order.
- **Overtaken updates**: the count of a loop's updates in a row whose result the target outran, and the limit set at the lock (default 3).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In 100% of tests, no local merge leaves a conflict on the target, and every commit Fallow merges or publishes had its gates and checker run on top of the target as it was when the merge ran or the commit was published, except a merge or publication the engineer chose with "Merge anyway" or "Publish anyway", which is labelled so (a pull request merged later on the remote, after its target moved again, is the hosting service's to check).
- **SC-002**: With two loops changing the same lines, the second reaches ready again with a newly verified commit and merges without conflict, with at most one action of the engineer at Normal ("Bring up to date") and none at Free, besides the merge itself.
- **SC-003**: The target branch and the engineer's folder are unchanged by any update, in 100% of tests.
- **SC-004**: A published loop brought up to date keeps the same pull request, and its branch on the remote only moves forward, in 100% of tests.
- **SC-005**: Every overlap between the files named by a locked feature's tasks and the files an unfinished loop has changed so far or names in its tasks is reported at the lock, in 100% of tests.
- **SC-006**: A loop waiting for another starts within 10 seconds of the other being merged or stopped.
- **SC-007**: With two loops ready for the same target, neither has to be verified again because of the other more than once, in 100% of tests (the line holds the second).

## Assumptions

- **The files a loop will change** are forecast from the paths its tasks name (Spec Kit's tasks name a file per task) and the files its branch has changed so far; the warning says so and can miss files a task does not name.
- **"Run after" and "Wait for"** order loops; they do not lock files.
- **The checker checks every user story again** after an update: the other work can break any of them. The gates are run in full.
- **A pull request is updated by moving the loop's branch forward with a merge of the target**, which needs no force; rewriting the branch onto the target (a rebase) would need a force and is not done.
- **The target "requiring branches to be up to date"** is read with the engineer's own tools, as 010 reads protection; when it cannot be read, Fallow shows "behind" without urging.

## Dependencies

- 007 (loops, gates, the checker, repairs, the repair cap and budget, the local merge): reused; its merge-conflict outcome ("merge again" or "stop") is replaced.
- 010 (publishing, the remote, the engineer's own tools): the pull request's state; its FR-012 (a taken branch name is never overwritten) is amended for one case, moving the loop's own branch forward from the last commit Fallow pushed to it (FR-012 here).
- 006 FR-026 (bringing a branch that fell behind up to date), deferred by 010: delivered here. 006 FR-025 (a failing remote check as a repair) stays out.
- Constitution 6.5.0: I (only a verified commit is offered), II (the engineer decides; never a push to the target, never forced), III (git), VII (runs survive), IX (runtime-agnostic).
