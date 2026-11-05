# Quickstart: validating solo sessions (012)

Q1 to Q10 run without network or a model. They use:
- the fake runtime, with `images` and `web_off` set in its profile;
- a project repository that has hooks writing a marker file (`pre-commit`, `prepare-commit-msg`,
  `post-commit`, `post-checkout` and `reference-transaction`);
- a local bare remote, where a push is needed.

Q11 is by hand. Build and test as usual, one test file at a time when memory is short:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/wip \
  cargo test -j 2 --test us12_session -- --test-threads=2
```

## Q1 A turn, committed on the session's branch (US1)

1. On a project whose folder has an uncommitted change, pick "+ New session": an empty session is
   ready at once, with no working copy yet.
2. Send "fix the Sunday bug". The session gets its working copy and branch, and says the folder's
   change is not in it. The fake assistant edits `picker.js`.
3. The turn ends.
4. Expect:
   - `fallow/solo/fix-the-sunday-bug` has exactly one new commit, `Fallow solo <id>, turn 1: fix the
     Sunday bug`;
   - no hook marker was written, by the worktree's creation, the branch's naming or the commit;
   - the commit is not signed;
   - on a project with an LFS file, the session's copy holds its pointer, nothing is downloaded,
     and the session says so;
   - the project folder's files, branch and status are as before.
5. A second turn that changes nothing makes no commit.
6. Stop a third turn halfway. Its edits are committed, marked `(cut short)`.

## Q2 Two sessions on the same file (US2)

1. Run two sessions at once. Each edits line 3 of `a.txt` differently.
2. Each branch holds only its own change (SC-005).
3. The dropdown lists both, with a working mark while each works and the changed-file counts.
4. Rename one, and the dropdown shows the new name.
5. Restart Fallow during a turn:
   - the turn shows as cut short, and is not repeated;
   - its changes are committed;
   - a pending approval is cancelled (SC-008).

## Q3 Changes, files, gates, undo (US3)

1. Over three turns, add `b.txt` (turn 1), change `a.txt` (turn 2) and delete `c.txt` (turn 3).
2. Changes lists all three with their turns.
3. Files (version `solo:<id>`) marks them, and opens a file read-only beside the chat.
4. Edit `a.txt` by hand in the working copy, then send turn 4. It first commits the edit as the
   engineer's edits.
5. The gates:
   - before any run, "Not verified by Fallow";
   - run them: "Gates passed on <short> — no independent check";
   - after a later turn, "Not verified" again.
6. Undo turn 2:
   - the confirmation names turns 2–4 and the engineer's edits before turn 4;
   - the branch is at turn 1's commit;
   - the undone turns are marked, and `refs/fallow/solo/<id>/undone/2` exists;
   - the next turn's prompt names what was undone.
7. Make a turn's commit fail (an index lock file left in the worktree):
   - the next turn and undo wait;
   - the gates are refused while a turn works;
   - "Fix it in the next turn" makes one commit, `turns n-m`;
   - when the engineer edited a file after the failure, the retry puts that edit in its own
     commit, named as the engineer's.

## Q4 Squash-merge by asking, one approval (US4, SC-010)

1. Ask "merge this into main".
2. The fake assistant runs `git -C <folder> merge --squash fallow/solo/x && git -C <folder>
   commit -m "..."`.
3. One approval card shows:
   - the snapshot count;
   - "squashes: yes";
   - the folder's branch and its uncommitted changes.
4. After allowing:
   - `main` moved by 1 commit, with the source this session (in the session and on the project);
   - `squashed` is recorded, and the base moved.
5. Close: nothing is asked, and the working copy and branch are gone.

## Q5 What is asked per command, and what moves outside Fallow (US4)

0. In the base mode, with `npm test` approved, `npm test` runs without asking. `ls`, `npm test;
   git push --force` and `npm test > ../f` are asked.
1. With "until I switch back" on:
   - `npm test` and `git status` run without asking;
   - `git -c core.pager=x log`, `git config core.hooksPath x`, `git diff --output=.fallow/squad.toml`,
     `rg --pre x .`, `find . -fprint ../f`, `Get-Content env:PATH`, `printenv` and
     `GIT_DIR=../proj/.git git merge x` are each asked;
   - `git push --force`, `git branch -D main`, `git reset --hard` in the project folder,
     `git clean -fd` and `sh -c "…"` are each asked, with their reasons.
2. Move `main` from a terminal (`git commit` in the project folder). The project shows it, from
   "outside Fallow", within 5 s (SC-004).
3. Push to the bare remote from the session. The project shows `refs/remotes/origin/fallow/solo/x`
   created, with the session as its source.
4. Approve `git push origin x`. The approval card offers no "always", and a later
   `git push --force` is still asked.
5. Run a loop and a spec session at the same time. None of their branch changes shows as "outside
   Fallow" or as the session's.
6. Put a `.claude/settings.json` allowing `Bash(git:*)` and `Bash(rm:*)` in the session's working
   copy. `git push --force` and `rm -rf <folder>` are still asked. The fake runtime's arguments
   show `--setting-sources user` and Fallow's whole-tool ask rules for the assistant, and an empty
   `--setting-sources` with `--model` for a lead or builder. A lead call and a builder step still get the project's
   instruction files as text.
7. Restart Fallow. "Until I switch back" is off.

## Q6 Images, waiting messages, instruction files, web (US5)

1. Attach a png. The fake step with `expect_images = 1` passes. With `images = false` in the
   profile, the attachment is refused before sending.
2. Send two messages while a turn works:
   - they wait, and can be edited or removed;
   - they are sent as the next turns, in order.
3. Put "always answer in French" in `AGENTS.md`:
   - the first turn's instruction holds it, and the session lists `AGENTS.md`;
   - after the file changes, the next turn holds the new text.
4. A fake `web = [...]` step shows "Fetched <url>" in the activity.
5. Turn web access off:
   - the call carries `web: false`;
   - with `web_off = false` in the profile, the switch is refused, saying so.

## Q7 Closing (US6)

1. Close an unmerged session that has an uncommitted change:
   - Fallow asks;
   - "keep" commits the change and keeps the branch, named;
   - the working copy is removed.
2. Close another, and choose "discard". The confirmation names its commits and files, and then the
   branch is gone.
3. Typing `/exit` sends a message.
4. Closing during a turn asks to stop it first.
5. A merged and clean session with an undone turn asks once, naming the undone turn and its commit.
6. A closed session reads under "Closed sessions…", and its record can be deleted.

## Q8 From a discussion to a feature (US7, SC-011)

1. A fake turn outputs `proposed_feature`, and "Make this a feature" shows.
2. Confirm with an edited brief:
   - the feature exists, created from the engineer's branch;
   - `<dir>/brief.md` is committed with exactly the edited text;
   - its spec session runs `/speckit-specify` with the brief, and links back to the session;
   - the session links to the feature.
3. Cancelling creates nothing.
4. With the constitution still the template, the spec session runs the constitution step first,
   then specify on the brief by itself.
5. A taken title is refused.
6. `/feature` asks the assistant for a suggestion.
7. An earlier project conversation opens read-only. Its unstarted proposal's "Make this a feature"
   opens the same dialog.

## Q9 Every project is a git repository (US8, SC-012)

1. Add a folder without git that holds `node_modules/` and `.env`:
   - the confirmation names them, the file count and size, and a proposed `.gitignore`;
   - nothing in the folder changed before the click;
   - after it, there is one commit without them.
2. Add a subfolder of a repository. The top folder is offered instead. A subfolder project added
   before, with a feature, offers "Use the repository's top folder". After the click, its feature
   has a branch, its documents are where they were, and it can be locked.
3. A plain-folder project from before gets the same question when opened.
4. Start with `git` off the `PATH`, or a fake `git` that reports 2.30:
   - the start page names the problem, the minimum (2.38) and how to install git;
   - "Check again" finds git once it is back.
5. With no git identity, the first commit is refused, saying how to set one.

## Q10 With 011's loops

1. A ready loop targets `main`.
2. A session squash-merges into `main`:
   - the loop's page says main moved, by 1 commit, from solo session `<name>`;
   - 011's rules apply: Free updates by itself, and Normal asks.
3. The session's merge is never counted in 011's merge line.

## Q11 By hand (optional)

On the engineer's machine, with the real `claude`:
- a session from the start to a squash into the branch;
- pasting a screenshot;
- a web lookup;
- making a feature.

Also check the wording of the dropdown, the approval cards, the close dialog and the feature
dialog.
