# Data Model: Solo sessions (012)

What 012 stores. Every field added to an existing event is `#[serde(default)]`, so logs from before
012 load unchanged. Research decisions are referred to as R-n ([research.md](./research.md)).

## Solo session (a conversation of kind `solo`, `conversations/<id>/events.jsonl`)

### Events

| Event | Fields | Written when |
|---|---|---|
| `conversation_created` | + kind `solo`; `agent` (the assistant's name); + `origin?` {session} for a spec session started from a session (FR-034) | "+ New session"; a feature confirmed from a session |
| `session_creating` | branch, start_commit | the first message was sent, before any git command (R-2) |
| `session_created` | branch, start_branch? (none when detached), start_commit, worktree, dirty_at_start, web, lfs | the working copy and branch exist (R-2) |
| `session_renamed` | name | the engineer renames it; the first message names it, `by: first_message` |
| `branch_renamed` | from, to | the first message gives the branch its slug, while nothing is committed (R-2) |
| `turn_started` | + attachments[] {sha, name, media_type, size}, instructions[] {path, sha, cut}, slot_waited_ms | a turn's runtime starts |
| `turn_finished` | + commit (head after the turn), commits[] (all the turn added), commit_failed?, changed[] {path, blob} (when the commit failed), cut, folded_into? | the turn ends (R-3) |
| `base_moved` | turn, mark, gates, why: up_to_date \| squash | a turn brought the branch up to date, or a squash was recognized (R-5) |
| `turn_committed` | n, commit | a failed turn commit succeeded on retry (R-4) |
| `fold_next` | n | "Fix it in the next turn" (R-4) |
| `uncommitted_discarded` | n, files[] | "Undo these changes" after a failed commit, confirmed (R-4) |
| `engineer_edits` | before (turn n, or `undo`/`close`), commit, files[] | the engineer's between-turn edits snapshotted (R-4) |
| `undo_started` | from, head_before, head_after | before an undo's git commands (R-5) |
| `turns_undone` | from, to, head_before, head_after, keep_ref | an undo (R-5) |
| `message_queued` | id, text, attachments[] | a message sent while a turn works (R-9) |
| `queue_edited` | id, text | |
| `queue_removed` | id | |
| `queue_sent` | id, n | the waiting message became turn n |
| `web_set` | on | the engineer switched web access (FR-009) |
| `git_observed` | ref, before, after, how, n?, commits[] (first-parent, before..after), turn | a change this session's command made (R-10) |
| `folder_state` | state: changed \| conflicted \| staged_only \| unchanged, turn | after a command that touched the engineer's folder (R-6) |
| `squashed` | branch, commit, upto_turn | a squash Fallow recognized (R-11) |
| `gates_run` | commit, base, results[] {gate, outcome, class, output_ref} | the Gates panel's run (R-12) |
| `feature_suggested` | n, title, brief | a turn's `proposed_feature` (R-15) |
| `feature_starting` | title, brief, lead | the engineer confirmed, before the feature is made (R-15) |
| `feature_started` | feature, title, conversation | the feature exists and its brief is committed (R-15) |
| `close_started` | choice | before close removes anything (R-14) |
| `session_closed` | how: merged \| kept \| kept_with_copy \| discarded, branch?, worktree? | close (R-14) |
| `copy_removed` | files[] | "Remove working copy" of a closed session kept with its copy |

`mode_set` takes the session's modes `ask` and `until_switch`. `until_switch` is never written:
it is in memory only, so a restart reads as `ask` (R-6).

`approval_asked` gains `reasons[]` (why it is asked per command, R-6) and `merge?` {snapshots,
squashes, folder_branch, folder_dirty[], control_changes[]} (the `.fallow/` paths the merge would
change). `approval_answered` gains `auto: bool` (allowed by "until
I switch back") and `why?` ("Fallow restarted").

### State (folded)

| Field | Rule |
|---|---|
| name | the last `session_renamed`, else `New session` |
| agent, runtime, model | from `conversation_created` and the first `turn_started` |
| branch, worktree, start_branch, start_commit | `session_created`, `branch_renamed` |
| turns[] | each with n, said, attachments, reply, commit, commits, cut, undone, commit_failed, folded_into, engineer_edits_before |
| base | `{mark, gates}`: the last `base_moved` of a turn not undone, else the start commit for both (R-5) |
| start_moved | in memory, from the ref watch: commits on the starting branch past `base.gates` |
| pending_commit | the turn whose commit failed and is not yet committed, folded or discarded |
| queue[] | waiting messages in order |
| web | the last `web_set`, else `session_created.web` (true) |
| gates | the last `gates_run`; `verified_label` derived: `not_run` / `passed_on(commit)` / `failed` / `stale` (head ≠ its commit) / `no_gates` |
| suggestions[], features[] | `feature_suggested`, `feature_started` |
| observed[] | `git_observed` and `folder_state`, newest last |
| closed | `session_closed`, if any |
| gone | in memory: its working copy or branch missing (R-19) |
| allow_until_switch | in memory only (R-6), never folded from the log |

**Validation**:
- `say` refuses while `pending_commit`, until the retry succeeds or the engineer chose a way on.
  It refuses while `closed` or `gone`, and on an earlier lead conversation (read-only).
- `undo(k)` requires `k` to be a turn that is not undone. Turns sharing one commit are undone
  together.
- `close` refuses while a turn works (`stop_first`).
- `gates` refuses while a turn works (R-12).
- `undo(k)` needs `confirmed` when it goes back more than the last turn, past the engineer's
  edits, or over a turn that merged or pushed elsewhere.
- An attachment is refused when the runtime lacks `images`. It is also refused when it is not png,
  jpeg, gif or webp by its magic bytes, when it is over 5 MiB, or when it is the fifth on one
  message.

## Observed git change (project, `<data>/projects/<p>/git.jsonl`)

| Field | Rule |
|---|---|
| ref | `refs/heads/<b>` or `refs/remotes/<r>/<b>` |
| before, after | shas; none for created/deleted |
| how | `created \| deleted \| commits \| rewound \| rewritten`, with n for commits |
| source | `{session: id, turn}` \| `{loop: id}` \| `{spec: feature}` \| `outside` (R-10) |
| at | ms since the epoch |

`git-seen.json` holds the last snapshot `{refs: {ref: sha}, head, status_hash}`. `git.jsonl` is
capped at 1,000 entries, oldest dropped. An entry gains `quiet: bool`, for Fallow's own work on its
own branches (R-10).

## Feature brief (in the project, `<feature dir>/brief.md`, committed on the feature branch)

`# <title>`, a blank line, the brief as confirmed, a blank line, then `From solo session "<name>",
<YYYY-MM-DD>.` (or `From an earlier conversation with the lead, <date>.`). Its text is specify's
`$ARGUMENTS` (R-15). `Feature` in `projects.json` gains `origin?` {session | conversation}.

## Project (`projects.json`)

| Field | Change |
|---|---|
| kind | derived as today, `Folder` retired: `Git` \| `NeedsGit` \| `NeedsMove {top}` \| `Missing` (R-18) |
| spec_root | new, optional: the subfolder holding `specs/` and `.specify/` for a project moved to its repository's top folder (R-18); empty otherwise |
| assistant? | the project's default assistant (absent: `assistant`), set as `lead` is (009) |
| sessions[] | ids of the project's sessions, open and closed, newest first |
| last_session? | the session open last (FR-001) |
| conversation | kept for reading the earlier project conversation; never set again (FR-035) |

## Agent definition

`kind` gains `assistant`. The shipped default is `assets/agents/assistant.md` (`kind: assistant`,
`runtime: claude`). An Assistant is never a pipeline step: `defs::pipeline` refuses it, as it
refuses a lead.

## 011's moved commits

`update::MovedCommit.from` becomes `From { Feature(name) | Session{id, name} | Outside }`. A plain
string from before reads as `Feature`. The loop view gains `from_kind` (R-10).

## Runtime contract

`Capabilities {images, web_off, control_files[]}`; `Call.attachments[]`, `Call.web`; `ToolKind::Web` (R-8;
[contracts/runtime.md](./contracts/runtime.md)).

## Settings and squad

- `.fallow/squad.toml` gains `[instructions] extra = ["docs/AGENT_NOTES.md", …]`: further
  instruction files, for every kind (R-7).
- `settings.toml` is unchanged. Sessions count against `defaults.max_parallel` (R-13).

## Service-wide

`GitCheck {found: Option<String>, ok: bool, min: "2.38", hint}`, held in memory and refreshed by
"Check again" (R-18).
