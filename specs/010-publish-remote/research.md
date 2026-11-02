# Research: Publishing that respects the remote (010)

Decisions taken for the plan, each with why and what else was weighed. The code facts come from a
survey of the repository on 2026-11-02 (file references as of commit bc77490).

## R-1 Finding the remote, with git only

- **Decision**: The remote is the one the target branch tracks (`git rev-parse --abbrev-ref
  <target>@{upstream}` → `origin/main`); without an upstream, `origin` if it exists; otherwise
  "remotes but no target there". The hosting service is recognized from the remote's configured
  address as written (`git config --get remote.<name>.url`), never from the address after git's
  `insteadOf` rewriting: `https://github.com/<owner>/<repo>(.git)` and
  `git@github.com:<owner>/<repo>(.git)` (and `ssh://git@github.com/...`) are GitHub; anything else
  is "a service Fallow cannot publish to yet", named by its host.
- **Why**: the tracked branch is what the engineer pushes to; reading the configured address keeps
  tests honest: a test remote can be written as `https://github.com/acme/shop.git` while git's
  `url.<local bare repo>.insteadOf` sends every push to a local bare repository (checked on
  2026-11-02: `remote get-url` shows the local path, `config --get` the GitHub address, and the push
  lands in the bare repository). No network in tests.
- **Alternatives**: `git remote get-url` (gives the rewritten address, so tests could not look like
  GitHub); asking the hosting tool for the repository (network on every project open).

## R-2 The hosting adapter and GitHub through `gh`

- **Decision**: A `Host` adapter per service (`src/publish/host/`), with one implementation in this
  version, GitHub, which runs the engineer's own `gh` (constitution V: the engineer's own remote
  tool, with their sign-in). The adapter answers four things: is the engineer signed in; is the
  target branch protected; open a pull request; read a pull request's state and checks. The rules
  of FR-001 to FR-021 live outside the adapter (IX by analogy, FR-017). The program is `gh` from
  the PATH, or the one the settings name (`[hosting.github] command = "..."`), which is how tests
  put a fake in its place.
- **Why**: `gh` is the tool GitHub users already have and are signed in to (it is on this machine,
  2.97.0, signed in); Fallow never sees a token (it never runs `gh auth token` or `--show-token`;
  `gh auth status`'s exit code is enough). No HTTP client crate: deny.toml bans them, and Fallow's
  own process opens no connection (V).
- **Alternatives**: GitHub's REST API from Fallow (needs a token in Fallow, an HTTP client, and
  outbound connections from Fallow's process: against V); `git` alone (cannot open pull requests
  or read protection).

## R-3 What "protected" means, readable without admin rights

- **Decision**: Protected = changes reach the branch only through a pull request. Read in this
  order with `gh api`: the branch's rules (`repos/{o}/{r}/rules/branches/{b}`, readable with read
  access) — a `pull_request` rule means protected; then the classic branch protection flag
  (`repos/{o}/{r}/branches/{b}` → `protected: true`), which also counts as protected (its details
  need admin rights, so a protected branch whose rules cannot be read is treated as requiring a pull
  request, the safe side). Neither → not protected. A failed call (not signed in, no access, offline,
  404 for a branch that exists only locally) → "unknown" with the reason.
- **Why**: most engineers are not admins of their repositories; the classic protection details
  need admin, the rules endpoint and the branch's `protected` flag do not. Treating "protected,
  details unknown" as "pull request required" can only make Fallow more careful (FR-006 offers the
  pull request only), never push where it must not.
- **Alternatives**: only the classic protection details (fails for non-admins); trying a dry-run
  push (`git push --dry-run` does not see server-side rules).

## R-4 Pushing: exactly the verified commit, never the target, never forced

- **Decision**: `git push <remote> <verified commit>:refs/heads/<loop branch>`, nothing else: no
  `--force`, no `+` refspec, no other ref. Before it, `git ls-remote <remote>
  refs/heads/<loop branch>`: absent → push; present at the same commit → nothing to push; present
  at another commit → a new name, `<loop branch>-2`, `-3`, … (FR-012). The push is refused in code
  when the destination equals the target branch (a check of its own, tested), whatever caused it.
- **Why**: II and FR-009; a non-forced push of a new ref cannot rewrite anything on the remote.
- **Alternatives**: pushing the local branch by name (its tip may have moved past the verified
  commit, for example a later unlock); `--force-with-lease` (never needed for a new ref).

## R-5 The pull request's text

- **Decision**: Title: the feature's title. Body: Markdown built by Fallow from the loop's record,
  the same facts as "What was verified" (008): the feature and its spec path; each acceptance
  criterion with its evidence and verdict; gates against the baseline; decisions and who took them
  (lead or engineer); repairs; steps with agents and models; cost and time; a closing line "Verified
  by Fallow; the merge is the engineer's." Written to a file in the run's folder and given to
  `gh pr create --body-file`.
- **Why**: FR-010; the record already holds all of it, so the text is derived, not written by an
  agent (principle I: no agent's claim in it unverified).
- **Alternatives**: an agent writing the description (unverified prose).

## R-6 Following the pull request

- **Decision**: A loop with an open pull request is in the existing stage **Publishing**
  (`events.rs` already allows Ready → Publishing → Done). The engine's 1-second tick asks the
  adapter for the pull request's state at most every 60 s per publishing loop, on a thread, and at
  once when the loop page is read (no more than every 10 s). Merged → `Merged { how: "pull request"
  }` and Done (FR-014); closed → back to Ready with the closed pull request shown (FR-015: publish
  again or stop); checks are recorded only when they change (FR-013, FR-018).
- **Why**: SC-005 (within 2 minutes) with one `gh` call a minute per open pull request; the
  existing tick and `Msg::Done` pattern (as the local merge uses) need no new machinery.
- **Alternatives**: webhooks (need a public address: against V); polling faster (cost for nothing).

## R-7 "When verified"

- **Decision**: A per-loop choice `when_verified` = `wait` | `pull_request`, carried exactly like
  the autonomy level: the lock form (preset from the level: Free → `pull_request`), the lock request,
  `LoopCreated` (`#[serde(default)]` = `wait`, so old logs read), `LoopState`, `LockRec`. When
  `finish()` makes a loop Ready **with a verified outcome**, and the setting is `pull_request`, the
  next drive starts publishing as the engineer's action would, recorded as Fallow's own steering
  ("published when verified, as chosen at the lock"). Anything else waits (FR-020).
- **Why**: the level only governs questions; publishing is its own decision (agreed 2026-11-02).
- **Alternatives**: a project-wide setting only (the engineer wanted it per lock, preset by Free).

## R-8 Restart while publishing

- **Decision**: As the local merge does after a restart (machine.rs reconciles an interrupted
  merge), a run in Publishing without a recorded pull request checks the remote: the loop branch
  there and a pull request from it (`gh pr list --head <branch>`). Found → recorded and followed;
  not found → back to Ready with "publishing was interrupted; try again" (VII).
- **Why**: never a second pull request (FR-011), never a silent half-publication (FR-012).

## R-9 Notifications

- **Decision**: Two new notice kinds, "pull request opened" (with its link) and "merged on the
  remote", raised once per pull request, keyed like the others in `notified.json`; respecting the
  project's notifications switch. A failed automatic publication is a "needs you" item, as Ready is.
- **Why**: FR-020 asks to notify on automatic publishing; merged closes the loop for the engineer.

## R-10 The CI gate "no push"

- **Decision**: `tests/no_egress_no_push.rs` scans for `"push"` and `gh` anywhere in `src/`. It
  becomes what the constitution states (II, V; CI list "no code path pushes to a target branch,
  force-pushes or approves a pull request"): pushing and running the hosting tool are allowed only
  in `src/publish/`; nowhere may `--force`, `--force-with-lease`, a `+` refspec, `pr merge`, `pr
  review` or `--approve` appear; a behavioural test pushes through the real code to a local bare
  remote and checks that the target branch there never moves, and that a destination equal to the
  target is refused.
- **Why**: the scan predates publishing (007 kept it out of scope); the constitution allowed it all
  along. No amendment needed.
- **Alternatives**: keeping the blanket ban with an exemption list of lines (brittle).

## R-11 Timeouts and prompts for external programs

- **Decision**: A small runner in `src/publish/` that runs git or the hosting tool with a time limit
  (60 s for push and pull request creation, 20 s for reads), killing the process on expiry, and
  reports "timed out" as the failed step. Network work only ever runs on a thread, never on the
  engine thread. No prompt may ever wait (only for the commands Fallow runs; the engineer's own
  terminal is untouched):
  - `GIT_TERMINAL_PROMPT=0` and `GH_PROMPT_DISABLED=1`;
  - `GCM_INTERACTIVE=never`, so Git Credential Manager fails instead of opening a sign-in window;
  - for SSH addresses, `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` only when the engineer has set
    neither `GIT_SSH_COMMAND` nor `core.sshCommand` (theirs is used as it is otherwise), so a key
    with a passphrase and no agent fails instead of asking.
  A failure for want of a sign-in says "sign in to <service> (git or gh), then try again".
- **Why**: `verify::git::git()` has no time limit; a hanging `gh` (no network, a prompt) must not
  stall the engine (VII, VIII). `GH_PROMPT_DISABLED=1` and `GIT_TERMINAL_PROMPT=0` keep both from
  ever waiting for input.

## R-12 Whose credentials: by construction, and the account shown

- **Decision**: Fallow handles no credential. It runs `git` and `gh` as programs under the
  engineer's own account (it never runs elevated, VI), inheriting their environment; each uses its
  own sign-in: git its credential helper (on Windows usually Git Credential Manager), the SSH key
  or agent for SSH addresses, or `gh` when set up as git's helper; `gh` its stored login. Fallow
  passes no password or token, reads none, and does not look for any in git's configuration
  (`user.name`/`user.email` are authorship, not sign-in). The publish confirmation names the account
  `gh` is signed in as (`gh auth status` prints the login: a username, not a secret), since the pull
  request is opened as that account; the account git pushes with cannot be seen without touching
  credentials, and is not shown.
- **Why**: V ("with the engineer's credentials", "never reads secret values"); the engineer asked
  how this is ensured (2026-11-02): it is, because Fallow never chooses or holds a credential.
- **Alternatives**: reading `credential.helper` or tokens to check them (reads credential setup, and
  proves nothing a failed push would not show).
