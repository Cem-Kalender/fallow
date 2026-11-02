# Contract: the hosting adapter (010)

What Fallow needs from a hosting service, in one place (FR-017; IX by analogy). The rules that use
it (when to offer a pull request, when to publish on its own, what is never done) are Fallow's,
outside the adapter, and hold for every adapter.

```rust
pub trait Host: Send + Sync {
    /// "github". Shown with the remote.
    fn service(&self) -> &str;
    /// Whether the engineer is signed in to the service's tool; Err with what to do when not.
    fn signed_in(&self) -> Result<(), String>;
    /// Whether changes reach `branch` of `repo` only through a pull request.
    fn protected(&self, repo: &Repo, branch: &str) -> Result<bool, String>;
    /// Opens a pull request from `head` into `base`; returns its number and address.
    fn open(&self, repo: &Repo, head: &str, base: &str, title: &str, body: &Path)
        -> Result<PullRequest, String>;
    /// The pull request's state and checks.
    fn state(&self, repo: &Repo, number: u64) -> Result<PrState, String>;
    /// An open or merged pull request from `head`, if any (after a restart, R-8).
    fn find(&self, repo: &Repo, head: &str) -> Result<Option<PullRequest>, String>;
}
```

Every method runs the engineer's own tool with a time limit (R-11), without prompts, and returns
an error in plain words naming the step. No method merges, approves, reviews, closes or edits a
pull request, or changes a branch: there is no such method to call.

## GitHub (`gh`)

| Method | Command (`gh`, with `GH_PROMPT_DISABLED=1`) |
|---|---|
| `signed_in` | `gh auth status --hostname github.com` (exit code only; never `--show-token`, never `gh auth token`) |
| `protected` | `gh api repos/{o}/{r}/rules/branches/{b}` (a rule of type `pull_request` → yes); else `gh api repos/{o}/{r}/branches/{b}` (`protected: true` → yes); 404 for the branch → Err "the branch is not on the remote yet" |
| `open` | `gh pr create --repo {o}/{r} --base {base} --head {head} --title {title} --body-file {body}`; the address printed; the number from it |
| `state` | `gh pr view {n} --repo {o}/{r} --json state,mergedAt,url,statusCheckRollup` |
| `find` | `gh pr list --repo {o}/{r} --head {head} --state all --json number,url,state --limit 1` |

`statusCheckRollup` entries become checks: `SUCCESS`/`NEUTRAL` → passed, `FAILURE`/`ERROR`/
`TIMED_OUT`/`CANCELLED`/`ACTION_REQUIRED` → failed, `SKIPPED` → skipped, anything pending → waiting.

## The fake (tests)

`fallow-fake-host` (a test binary beside `fallow-fake-agent`) answers the `gh` command lines above
from a JSON state file named by `FALLOW_FAKE_HOST` (protection per branch, signed in or not, pull
requests with their states and checks, failures to inject), and records every command line it was
given, so tests check what Fallow asked and that it never asked for a merge, an approval or a
token. Tests point `[hosting.github] command` at it.
