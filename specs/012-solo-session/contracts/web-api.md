# Contract: web API changes (012)

These changes go on top of the web API contracts of 007, 009 and 011. The general rules stay the
same:
- every reply is `{ok:true, value}` or `{ok:false, error}` (409);
- the per-launch secret is required;
- non-GET requests need the same origin.

## Sessions

| Request | Body / query | Answer |
|---|---|---|
| `GET /api/projects/<p>/sessions` | | `{open: [SessionItem], closed: [SessionItem], earlier: [{conversation, title, at}], records_bytes}`. Open sessions come newest first, with the open one on top; the page ticks it (FR-002, FR-025) |
| `POST /api/projects/<p>/sessions` | `{agent?, text, attachments?}` | `{session, turn}`: the first message of a draft creates the session, its working copy and branch, then sends the message (R-2). 409 if git is not ok, the project needs git or a move, or the assistant is not available |
| `GET /api/sessions/<s>` | | `Session` (below). It is long-polled on scope `conv:<s>` |
| `POST /api/sessions/<s>/say` | `{text, attachments?: [upload id]}` | `{turn: n}` or `{queued: id}` while a turn works or no slot is free. 409 in four cases: a pending commit ("Fallow could not commit the last turn: …"); closed or gone; an image refused (R-8); an empty message without attachments |
| `POST /api/sessions/<s>/attachments` | raw bytes, `Content-Type`, `?name=` | `{id, sha, media_type, size}`; 409 if the type is not allowed by its magic bytes or the file is over 5 MiB |
| `GET /api/sessions/<s>/attachments/<sha>` | | the image, with its media type (for the conversation) |
| `POST /api/sessions/<s>/queue/<id>` | `{text}`, `{remove: true}` or `{send: true}` | `{}` (R-9) |
| `POST /api/sessions/<s>/cancel` | | as for conversations: stop the turn. What it changed is committed as cut short |
| `POST /api/sessions/<s>/mode` | `{mode: "ask" \| "until_switch"}` | `{}`. `ask` is `Mode::Ask`; `until_switch` is Ask plus the judge, in memory only (R-6) |
| `POST /api/sessions/<s>/approve` | `{id, answer: once \| always \| deny}` | `{}`. `always` is refused (409) unless the command's prefix is `prefix_safe` (R-6): a prefix that could cover a command asked per command is never added |
| `POST /api/sessions/<s>/rename` | `{name}` | `{}` |
| `POST /api/sessions/<s>/web` | `{on}` | `{}`; 409 "This assistant's runtime cannot switch web access off" (FR-009) |
| `GET /api/sessions/<s>/undo` | `turn` | `{turns: [n], shared: [[n, m]], edits: [{commit, files}], elsewhere: [GitChange], base_moves: bool}`: what the confirmation names (R-5) |
| `POST /api/sessions/<s>/undo` | `{turn, confirmed: true}` | `{head}`. 409 while a turn works, a commit is pending without a choice, or `confirmed` is missing when the undo goes back more than one turn, past the engineer's edits, or over a turn that merged or pushed elsewhere |
| `POST /api/sessions/<s>/pending` | `{choice: "fold_next" \| "discard", confirmed?}` | `{}` (R-4). `discard` needs `confirmed` and answers with the files lost |
| `POST /api/sessions/<s>/gates` | | `{running: true}` (R-12). The result arrives in the session view. 409 "Wait until the turn ends" while a turn works |
| `GET /api/sessions/<s>/changes` | | `{base: {mark, gates}, files: [{path, mark, turns: [n], by: [assistant \| you]}]}` (FR-019) |
| `POST /api/sessions/<s>/close` | `{choice?: keep \| discard, confirmed?}` | Without `choice`, when the session is merged and clean, holds no ignored file outside dependency and build folders and has no undone turns: `{closed: "merged"}`; otherwise, without changing anything, `{stop_first: n}` or `{ask: {commits, files, edits, ignored: {files, counts}, undone: [{turn, commit}]}}`. A merged and clean session whose only question is its undone turns is closed with `{confirmed: true}` and no `choice`: `{closed: "merged"}`, its undone commits dropped. With `choice`: `{closed: how, branch?, worktree?}` (R-14) |
| `POST /api/sessions/<s>/remove-copy` | `{confirmed: true}` | `{files_lost}`, for a closed session kept with its copy |
| `DELETE /api/sessions/<s>` | `{confirmed: true}` | `{}`: deletes a closed session's record (FR-025) |
| `POST /api/sessions/<s>/feature` | `{title, brief, lead?}` | `{feature, conversation}` (R-15). 409 for an empty title or brief, a title already used ("a feature named … exists already (…)"), a project folder on no branch, or a lead that is not available |
| `GET /api/sessions/<s>/feature-draft` | `suggestion?` | `{title, brief, lead, leads}`: the last suggestion, prefilled for the dialog |
| `POST /api/conversations/<c>/feature` | `{proposal, title, brief, lead?}` | as above, from an earlier project conversation's unstarted proposal (FR-037) |

**Session** (the view):
- the session itself: `{id, name, agent: {name, runtime, model, problem?}, branch, start: {branch?,
  commit, dirty}, worktree, web: {on, can_switch}, mode, images, gone, closed?}`;
- the conversation: `turns: [Turn]`, `queue: [{id, text, attachments}]`, `live`, `activity`,
  `pending_approval?`;
- the working copy: `pending_commit?: {n, error}`, `base`, `start_moved?: {commits}`,
  `changes_count`, `instructions: [{path, cut, skipped?}]`, `editor_during_turn`, `lfs`;
- checking: `gates: {label: not_run | passed | failed | stale | no_gates, commit?, results?,
  running}`;
- around it: `observed: [GitChange]`, `suggestions`, `features: [{feature, title}]`, `slot_wait?`.

A **Turn** is 007's, plus `{commit?, cut, undone, commit_failed?, folded_into?, attachments,
engineer_edits?}`.

An **approval** is 008's, plus `{reasons: [string], merge?: {snapshots, assistant_commits:
"unknown", squashes, folder_branch, folder_dirty, control_changes}, note: "Fallow judges the command as written; it
cannot see what a script does inside"}`.

## The project page

| Request | Change |
|---|---|
| `GET /api/projects/<p>` | + `session_last?`, `assistant: {name, problem?}`, `git: {kind: git \| needs_git \| needs_move, top?}`. `conversation` is gone from the view: the earlier project conversation is listed by `…/sessions` as `earlier` |
| `PATCH /api/projects/<p>` | + `{assistant}`, checked as `lead` is (009) |
| `POST /api/projects/<p>/conversation` | removed: answers 409 "Project conversations are retired; start a session" (FR-035) |
| `POST /api/projects/<p>/features` | as before, but the spec session starts without forking from a project conversation |
| `POST /api/conversations/<c>/say` | 409 "This conversation is read-only" for an earlier project conversation |
| `GET /api/projects/<p>/git-changes` | `after?` | `{changes: [GitChange]}`, long-polled on the new scope `project:<p>` (R-10) |
| `GET /api/projects/<p>/files/versions` | + `{id: "solo:<s>", kind: "solo", label: <session name>, base: base.mark}` for each open session |
| `GET /api/projects/<p>/file` | `version=solo:<s>`: `last.who` is `assistant` (turn n) or `you` (R-3) |
| `POST /api/files/open` | `version=solo:<s>` opens the file in the session's working copy, and sets the session's `editor_during_turn` while a turn works |

## Git

| Request | Answer |
|---|---|
| `GET /api/git` | `GitCheck {found?, ok, min, hint}` (R-18) |
| `POST /api/git/check` | the same, read again |
| `GET /api/projects/<p>/git-init` | `{needs: init \| first_commit, files, bytes, heavy: [folder], secrets: [path], gitignore: text, identity: bool, inside?: <top folder>}` (read only) |
| `POST /api/projects/<p>/git-init` | `{gitignore?: text \| null}`: `{commit}`. 409 when git has no identity ("Set your name and e-mail in git: git config --global user.name …"), the folder is inside another repository, or git is not ok |
| `GET /api/projects/<p>/move` | | `{top, features: [{feature, from, to}]}` (read only, R-18) |
| `POST /api/projects/<p>/move` | `{confirmed: true}` | `{path: top}`: the project moved to its repository's top folder, with its features' branches and worktrees |
| `POST /api/projects` | as before. A folder inside a bigger repository answers 409 with `{inside: <top folder>}` so the page can offer it |

Every project request answers 409 "Fallow needs git 2.38 or later" while `GitCheck.ok` is false.

## Unchanged

Loops, their steering, publishing (010), updates and lines (011), agents, pipelines and the spec
session's own requests, apart from FR-029's layout, which is the page's.
