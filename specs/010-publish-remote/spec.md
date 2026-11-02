# Feature Specification: Publishing that respects the remote

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/010-publish-remote`)

**Created**: 2026-11-02

**Status**: Draft

**Input**: User description: "Publishing that respects the remote (010), asked for by the engineer on 2026-11-02 before the solo sessions (011). Fallow recognizes whether the project's repository has a remote, which branch is the target there and whether it is protected, and shows it on the project. The end of a loop depends on it: without a remote, Merge as today; with a remote, Fallow pushes the loop's verified branch and opens a pull request whose description is the loop's summary, shown on the loop page with its link and state; with a protected target, that is the only way. The merge of the pull request is the engineer's, on the remote; Fallow notices when it is merged or closed. Fallow never pushes to the target branch, force-pushes, bypasses protection or approves its own pull request, and acts only with the engineer's own git and remote tools and credentials. Not tied to one hosting service in its rules; the first version may support GitHub only, saying so plainly. Out of scope: solo sessions (011), dark mode (012), the standalone app (013), approving or merging pull requests for the engineer, publishing anything but a finished loop." (The full description is kept in the conversation of 2026-11-02.)

## Context

When a loop is finished and verified, its page offers **Merge**. Today that is always a local merge: Fallow merges exactly the verified commit into the branch the feature was started from (the engineer's `master` or `main`, whatever it is called), in the engineer's repository, and never pushes. Fallow knows nothing of remotes.

That is right for a project without a remote. It is wrong for most team projects: when the project lives on a hosting service (GitHub, GitLab, …) whose main branch is **protected**, changes reach it only through a pull request. The engineer then has a local merge that cannot be pushed, has to undo it, and make the pull request by hand, without the loop's evidence.

The constitution already promises the right behaviour (II): with a protected remote branch, Fallow only opens a pull request, and never pushes to the target branch, force-pushes, bypasses protection or approves its own pull request. 006 specified it (US6, FR-024 to FR-027); 007 deferred it. This feature delivers it, starting with recognizing the remote.

**What does not change**: what a loop verifies and how (gates, checkers, the verified commit), the local Merge for a project without a remote, and that nothing reaches the target branch without the engineer's decision.

## Clarifications

### Session 2026-11-02

- Q: In the first version, should Fallow also wait for the remote's checks, turn a failing required check into a repair round, and bring a branch that fell behind up to date (006 FR-025, FR-026), or only push, open the pull request and show its state and checks? → A: Only publish, and show the pull request's state and the remote's checks; the engineer decides what a failing check leads to. Waiting, repairing and updating come later.
- Q: Can a loop publish itself when it ends verified, for example when the engineer chose the autonomy level Free? → A: Yes, as a setting of its own at the lock, "When verified": "Wait for me" or "Open a pull request". Choosing Free presets "Open a pull request"; the engineer can change it. It applies only to a loop that ends verified, in a project with a supported remote; anything else waits for the engineer.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Fallow shows the project's remote (Priority: P1)

The engineer opens a project and sees at a glance whether it has a remote: where it is (for example `github.com/acme/shop`), which branch is the target there, and whether that branch is protected. A project without a remote says so. When the remote is on a service Fallow cannot publish to yet, the project says that too.

**Why this priority**: every other part depends on it, and it alone already tells the engineer what Merge will do.

**Independent Test**: open three projects, one without a remote, one with a remote whose target branch is protected, one whose target branch is not; check what each shows, and that it changes after a remote is added or removed and the project is opened again.

**Acceptance Scenarios**:

1. **Given** a project whose repository has no remote, **When** the engineer opens it, **Then** the project shows "no remote" next to its branch, and nothing about publishing.
2. **Given** a project whose target branch tracks a branch on a supported hosting service, **When** the engineer opens it, **Then** the project shows the remote (service and repository), the target branch there, and whether it is protected (pull requests required).
3. **Given** a remote that cannot be reached, or whose protection Fallow cannot read with the engineer's tools (not signed in, no permission), **When** the engineer opens the project, **Then** the project shows the remote with "protection unknown" and why, never a guess.
4. **Given** a remote on a service Fallow does not support yet, **When** the engineer opens the project, **Then** the project shows the remote and says plainly that Fallow can only merge locally there.

---

### User Story 2 - A finished loop becomes a pull request (Priority: P1)

A loop is ready to merge in a project with a remote. On the loop page, the main action is **Push and open a pull request**. Fallow pushes the loop's verified branch, exactly the commit it gated, and opens a pull request into the target branch, described with the loop's summary: what the feature is, every criterion with its evidence, the gates, the decisions taken and by whom, the repairs and the cost. The loop page then shows the pull request: its link, its state, and the checks the remote reports on it.

**Why this priority**: it is the feature: a verified loop reaches a protected branch the way the team requires, with its evidence attached.

**Independent Test**: with a remote whose target branch is protected, finish a loop and publish it; check that the branch on the remote is at exactly the verified commit, that the pull request targets the right branch with the summary as its description, and that the target branch on the remote did not move.

**Acceptance Scenarios**:

1. **Given** a loop ready to merge in a project whose target branch is protected, **When** the engineer opens the loop page, **Then** "Push and open a pull request" is the only way to publish, and the page says why (the branch is protected).
2. **Given** that action confirmed, **When** Fallow publishes, **Then** the loop's branch on the remote is at exactly the verified commit, a pull request into the target branch exists with the loop's summary as its description, and the loop page shows its link and state ("open").
3. **Given** a loop ready to merge where the remote's target branch is not protected, **When** the engineer opens the loop page, **Then** the engineer can choose between "Push and open a pull request" (offered first) and a local Merge, which merges locally as today and pushes nothing.
4. **Given** a loop that already has an open pull request, **When** the engineer opens the loop page, **Then** it shows that pull request and offers no second one.
5. **Given** the push or the pull request fails (rejected, no permission, not signed in, offline), **When** Fallow publishes, **Then** the loop page says what failed in plain words and what the engineer can do, nothing is half-published silently, and the engineer can try again.
6. **Given** a loop locked with "When verified: Open a pull request" (preset when the engineer chose Free), **When** it ends verified in a project with a supported remote, **Then** Fallow publishes it without waiting, exactly as the action would, and notifies the engineer with the pull request's link.
7. **Given** the same setting, **When** the loop ends "done, not verified", **Then** nothing is published, and the loop waits for the engineer.

---

### User Story 3 - Fallow follows the pull request to its end (Priority: P2)

After the pull request is opened, the engineer reviews and merges it on the remote, as the team does. Fallow notices and shows it: the loop ends as merged, with where and when. A pull request closed without merging is shown as such, and the loop can be published again or stopped.

**Why this priority**: without it, the loop stays "ready" forever and the dashboard is wrong; but the pull request is useful even before this.

**Independent Test**: merge a published loop's pull request on the remote; check that the loop shows as merged within the stated time, and that a closed pull request is shown as closed with the choice to publish again or stop.

**Acceptance Scenarios**:

1. **Given** a published loop, **When** its pull request is merged on the remote, **Then** within 2 minutes (or at once when the engineer opens the loop) the loop shows "merged on the remote", with the time, and the feature counts as merged.
2. **Given** a published loop, **When** its pull request is closed without merging, **Then** the loop shows "pull request closed", and offers to publish again or stop the loop.
3. **Given** the remote reports checks on the pull request, **When** the engineer looks at the loop page, **Then** each check is shown with its state (waiting, passed, failed) and a link, read from the remote, not from an agent.

---

### Edge Cases

- **Several remotes** (for example `origin` and `upstream`): the remote is the one the target branch tracks; when it tracks none, `origin` if there is one; otherwise the project says it has remotes but no target there, and publishing is local only, saying why.
- **The target branch exists only locally** (never pushed): the project says so; publishing offers a pull request only once the target exists on the remote, and says so.
- **The loop's branch name already exists on the remote with other commits**: Fallow never overwrites it (no force); it publishes under a new name and says so.
- **The target branch moved on the remote since the loop locked**: the pull request is opened all the same, and the remote's own "behind" or "conflict" state is shown on the loop page; Fallow does not rewrite the loop's branch, and does not bring it up to date in this version.
- **A check fails on the pull request**: it is shown as failed with its link; Fallow starts nothing on its own. The engineer decides (for example a repair through a new loop or a solo session).
- **Automatic publishing in a loop that ends not verified** ("done, not verified"), or in a project without a supported remote, or whose protection became unknown: nothing is published on its own; the loop waits for the engineer as today, and says why.
- **Automatic publishing fails** (not signed in, offline): the loop waits for the engineer with the failure shown, as if the engineer had pressed the button; it does not try again on its own.
- **The engineer is not signed in to the remote's tool, or lacks the right to push**: nothing is pushed; the page names the tool and the step that failed and what to do (for example "sign in with your hosting service's command-line tool, then try again"), never a credential.
- **The hosting service is unreachable** when the project opens: the last known state is shown with its time and "could not check now".
- **A pull request merged with a method that rewrites commits** (squash, rebase): it still counts as the loop's merge; Fallow goes by the pull request's state on the remote, not by finding the commit on the target.
- **The engineer merges the loop locally and pushes it themselves** (unprotected target): that is the engineer's own git; the loop ends as merged locally, as today.

## Requirements *(mandatory)*

### Functional Requirements

**Recognizing the remote**

- **FR-001**: For each project, Fallow MUST determine whether its repository has a remote for the target branch (the branch a feature is started from), which remote it is (hosting service and repository), the target branch's name there, and whether that branch is protected so that changes reach it only through a pull request.
- **FR-002**: The project MUST show this next to its branch: "no remote"; the remote with its target branch and "protected" or "not protected"; "protection unknown" with the reason; or "Fallow cannot publish to this service yet". It MUST never present a guess as a fact.
- **FR-003**: Fallow MUST check again when the project is opened and when a loop of it reaches "ready to merge", and MUST show the time of the last check when it could not check now.
- **FR-004**: Fallow MUST learn all of this only through git and the engineer's own command-line tool for the hosting service, run with the engineer's own sign-in. It MUST NOT read, store or show credentials or tokens.

**Publishing a finished loop**

- **FR-005**: Without a remote for the target branch, or on a service Fallow cannot publish to, the loop's publishing MUST stay as today: the engineer's local Merge of the verified commit, which pushes nothing.
- **FR-006**: With a remote whose target branch is protected, the loop page MUST offer "Push and open a pull request" as the only way to publish, and MUST say why.
- **FR-007**: With a remote whose target branch is not protected, the loop page MUST offer "Push and open a pull request" first, and the local Merge as an alternative; the local Merge MUST push nothing. With the protection unknown, the loop page MUST offer both and say that the protection is unknown.
- **FR-008**: Publishing MUST be confirmed by the engineer, naming the remote, the branch to push, the target branch and the commit.
- **FR-009**: Publishing MUST push the loop's own branch at exactly the verified commit, and MUST open a pull request from it into the target branch. Fallow MUST NOT push to the target branch, force-push, bypass protection, approve or merge its own pull request.
- **FR-010**: The pull request's title MUST be the feature's name, and its description MUST be the loop's summary: the feature, every acceptance criterion with its evidence and verdict, the gates and their results against the baseline, the decisions with who took them, the repairs, the steps' agents and models, and the cost. It MUST state that it was verified by Fallow and that the merge is the engineer's.
- **FR-011**: A loop MUST have at most one open pull request; publishing again while one is open MUST show it instead.
- **FR-012**: When pushing or opening the pull request fails, the loop page MUST say which step failed and why in plain words, MUST leave nothing pushed that it does not report, and MUST let the engineer try again. A branch name taken on the remote by other commits MUST NOT be overwritten.

**Publishing when verified**

- **FR-019**: At the lock, beside the autonomy level, the engineer MUST be able to choose what happens when the loop ends verified: "Wait for me" (as today) or "Open a pull request". Choosing the autonomy level Free MUST preset "Open a pull request", Strict and Normal "Wait for me"; the engineer can change it before locking. The loop page MUST show the choice.
- **FR-020**: With "Open a pull request", a loop that ends verified ("ready to merge") in a project whose remote is supported and reachable MUST be published without waiting, exactly as FR-009 to FR-012 describe, and the engineer MUST be notified with the pull request's link. A loop that ends not verified, or a project without a supported remote, or one whose protection cannot be read, MUST wait for the engineer, and the loop page MUST say why it did not publish. A failed automatic publication MUST wait for the engineer, showing the failure.
- **FR-021**: This choice covers only pushing the loop's branch and opening its pull request; it never covers a merge, local or on the remote.

**Following the pull request**

- **FR-013**: The loop page MUST show the pull request's link and state (open, merged, closed) and the checks the remote reports on it with their states, read from the remote. A failing check MUST be shown, not acted on: the engineer decides what follows.
- **FR-014**: When the pull request is merged on the remote, the loop MUST end as merged, recording where and when, within 2 minutes or as soon as the engineer opens the loop; the dashboard and the project MUST show the feature as merged.
- **FR-015**: When the pull request is closed without merging, the loop MUST show it, and MUST offer to publish again or to stop the loop.
- **FR-016**: Fallow MUST NOT pull or change the engineer's local target branch after a remote merge; it MUST say that the merge is on the remote.

**Not tied to one hosting service**

- **FR-017**: What is particular to one hosting service (how protection is read, how a pull request is opened and followed) MUST live in that service's adapter; the rules above MUST hold for every adapter. The first version MUST support GitHub, and MUST say plainly for any other service that Fallow merges locally there.

**Activity and record**

- **FR-018**: Every publishing step (pushed, pull request opened, check states, merged, closed, failed) MUST appear in the loop's activity as Fallow's own lines, and be kept in the loop's record.

### Key Entities

- **Remote of a project**: the hosting service and repository the target branch tracks; the target branch's name there; whether it is protected (yes, no, unknown with a reason); when it was last checked.
- **Publication of a loop**: the pushed branch and commit, the pull request (link, number, state), the checks the remote reports, whether it was published by the engineer or on its own when verified, and how it ended (merged on the remote with time, closed, or failed with the step and reason).
- **"When verified" choice**: per loop, set at the lock: wait for the engineer, or open a pull request; preset from the autonomy level.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In 100% of the test projects (no remote, protected target, unprotected target, unsupported service, remote unreachable), the project shows the remote state correctly, and never "protected" or "not protected" as a guess.
- **SC-002**: Across all tests, zero pushes to a target branch, force-pushes, self-approvals or merges by Fallow.
- **SC-003**: For a protected target, a finished loop is published as a pull request with the loop's summary in at most 2 actions of the engineer (the action and its confirmation), within 30 seconds on a normal connection.
- **SC-004**: The pushed branch on the remote is at exactly the verified commit in 100% of publications.
- **SC-005**: A pull request merged on the remote shows as merged on the loop within 2 minutes in 95% of cases, and at once when the loop page is opened.
- **SC-006**: Every failed publication names the failed step and a next step for the engineer; none leaves something on the remote that the page does not show.
- **SC-007**: With "Open a pull request", 100% of loops that end verified in a project with a reachable supported remote are published without an action of the engineer, and 0% of loops that end not verified are published.

## Assumptions

- The engineer already uses the hosting service's own command-line tool (for GitHub: `gh`), signed in with their account; Fallow runs it as the engineer (constitution V), and never handles credentials. Without the tool or the sign-in, the project shows the remote with "protection unknown" and how to fix it.
- "Protected" means the target branch requires a pull request (as the hosting service reports it). Other rules (required reviews, required checks, up to date) are shown on the pull request as the remote reports them.
- The loop's branch on the remote has the same name as the local one (`fallow/<feature>/build` or its current name); a taken name gets a suffix.
- The first version supports GitHub (github.com); GitHub Enterprise and other services come through further adapters.
- A project's target branch is the branch the feature was started from, as today; this feature does not change how it is chosen.
- Solo sessions (011) publish by asking their assistant, under constitution 6.3.0 II; they reuse this feature's recognition of the remote for what they show, not its publishing.
