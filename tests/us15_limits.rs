//! 015 (0.3.2, independent review of 0.3.1): the limits of Auto hold whatever the command's
//! form. A control file (the runtime's settings, Fallow's own, git's record) written anywhere
//! in a command is refused, never held: not behind a here-document piped to a shell, not
//! behind a here-document tag with quotes inside, not beside a push onto the target, not with a
//! substitution, not as the lead's edit. "Always allow" allows what was approved, no more.

use std::path::PathBuf;

use fallow::agent::{PermissionRequest, RequestKind};
use fallow::project::PastLimit;
use fallow::studio::judge;
use fallow::studio::mode::{self, AssistantCx, ChatMode, Decision, LeadCx, Limit, Target};

fn request(kind: RequestKind, what: &str) -> PermissionRequest {
    let run = kind == RequestKind::Run;
    PermissionRequest {
        kind,
        command: run.then(|| what.to_string()),
        path: (!run).then(|| what.to_string()),
        url: None,
        query: None,
        tool: if run { "Bash" } else { "Write" }.into(),
    }
}

struct Fixture {
    root: PathBuf,
    wt: PathBuf,
    folder: PathBuf,
    protect: Vec<String>,
    control: Vec<String>,
    target: Target,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("fallow-limits-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wt = root.join("copy");
        let folder = root.join("folder");
        std::fs::create_dir_all(wt.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(wt.join(".fallow")).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let (wt, folder) = (wt.canonicalize().unwrap(), folder.canonicalize().unwrap());
        let control = vec![
            ".claude/settings.json".to_string(),
            ".claude/settings.local.json".to_string(),
        ];
        let mut protect: Vec<String> = fallow::agent::rights::CONTROL_GLOBS
            .iter()
            .map(|g| g.to_string())
            .collect();
        protect.extend(control.clone());
        protect.extend([".git".to_string(), ".git/**".to_string()]);
        Fixture {
            root,
            wt,
            folder,
            protect,
            control,
            target: Target {
                name: "main".into(),
                protected: true,
                also: Vec::new(),
            },
        }
    }

    fn assistant(&self, mode: ChatMode, kind: RequestKind, what: &str) -> Decision {
        let ctx = judge::Ctx {
            wt: &self.wt,
            branch: "fallow/solo/x",
            folder: &self.folder,
            protect: &self.protect,
        };
        mode::assistant(
            mode,
            &request(kind, what),
            &AssistantCx {
                judge: &ctx,
                control_files: &self.control,
                gates: &[],
                approved: &[],
                target: Some(&self.target),
            },
            |_| None,
        )
    }

    fn lead(&self, mode: ChatMode, kind: RequestKind, what: &str) -> Decision {
        mode::lead(
            mode,
            &request(kind, what),
            &LeadCx {
                workdir: &self.wt,
                control_files: &self.control,
                approved: &[],
                target: Some(&self.target),
                folder: &self.folder,
            },
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn refused(d: &Decision) -> bool {
    matches!(
        d,
        Decision::Refuse {
            limit: Limit::Control,
            ..
        }
    )
}

fn held_at(d: &Decision) -> Option<Limit> {
    match d {
        Decision::Hold { limit, .. } => Some(*limit),
        _ => None,
    }
}

/// Review 1 (critical): a here-document piped onward is run, not read as data; its body's
/// control writes and pushes are seen.
#[test]
fn a_here_document_piped_to_a_shell_hides_nothing() {
    let fx = Fixture::new("pipe");
    for body in [
        "echo x > .claude/settings.json",
        "echo x > .git/hooks/pre-commit",
        "echo x > .fallow/squad.toml",
    ] {
        let cmd = format!("cat <<'EOF' | sh\n{body}\nEOF");
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, &cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
        let d = fx.lead(ChatMode::Auto, RequestKind::Run, &cmd);
        assert!(refused(&d), "lead: {cmd}: {d:?}");
    }
    let cmd = "cat <<'EOF' | sh\ngit push origin main\nEOF";
    for d in [
        fx.assistant(ChatMode::Auto, RequestKind::Run, cmd),
        fx.lead(ChatMode::Auto, RequestKind::Run, cmd),
    ] {
        assert_eq!(held_at(&d), Some(Limit::Target), "{cmd}: {d:?}");
    }
    // Piped, the body is not stripped: the command is not read as written.
    assert!(judge::split("cat <<'EOF' | sh\nls\nEOF").is_err());
    // A pure write's body is still data (r13), and `||` is not a pipe.
    assert_eq!(
        fx.assistant(
            ChatMode::Auto,
            RequestKind::Run,
            "cat > notes.md <<'EOF'\nrm -rf /\nEOF\nwc -l notes.md"
        ),
        Decision::Allow
    );
    assert!(judge::split("cat > a <<'EOF' || true\nx\nEOF").is_ok());
}

/// Review 1, and the review of 0.3.2 (2): a here-document body that is run (fed to a shell or
/// an interpreter, piped onward, through a process substitution, `source`d) is read as
/// commands: its control writes are refused, its pushes held.
#[test]
fn a_here_document_that_is_run_is_read_as_commands() {
    let fx = Fixture::new("run");
    for open in [
        "cat <<'EOF' | sh",
        "cat <<'EOF' |& bash",
        "bash <<'EOF'",
        "python3 <<'EOF'",
        "{ cat <<'EOF'; } | sh",
        "tee x.sh <<'EOF' | sh",
        "source /dev/stdin <<'EOF'",
        "cat <<'EOF' > >(sh)",
        "cat > >(sh) <<'EOF'",
    ] {
        for body in [
            "echo x > .claude/settings.json",
            "echo x > .git/hooks/pre-commit",
            "echo x > .fallow/squad.toml",
        ] {
            let cmd = format!("{open}\n{body}\nEOF");
            let d = fx.assistant(ChatMode::Auto, RequestKind::Run, &cmd);
            assert!(refused(&d), "{cmd}: {d:?}");
        }
        let cmd = format!("{open}\ngit push origin main\nEOF");
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, &cmd);
        assert_eq!(held_at(&d), Some(Limit::Target), "{cmd}: {d:?}");
    }
    for cmd in [
        "sh <<< 'echo x > .claude/settings.json'",
        "sh <<< 'echo x > .git/hooks/pre-commit'",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
    }
    let d = fx.assistant(
        ChatMode::Auto,
        RequestKind::Run,
        "sh <<< 'git push origin main'",
    );
    assert_eq!(held_at(&d), Some(Limit::Target), "{d:?}");
}

/// Review of 0.3.2 (2): a here-document body that is only data (a file written, a commit
/// message) is not scanned: naming a control file or `git push` in it is no write and no push.
/// They run unasked in Auto, as on 0.3.1.
#[test]
fn a_here_document_that_is_data_is_not_scanned() {
    let fx = Fixture::new("data");
    for cmd in [
        "cat > docs/setup.md <<'EOF'\nNever edit .claude/settings.json by hand.\nEOF",
        "git commit -F - <<'EOF'\nfix: refuse writes to .git/hooks\nEOF",
        "git commit -F- <<'EOF'\nfix: refuse writes to .fallow/squad.toml\nEOF",
        "cat > README.md <<'EOF'\nRun git push origin main to publish.\nEOF",
    ] {
        assert_eq!(
            fx.assistant(ChatMode::Auto, RequestKind::Run, cmd),
            Decision::Allow,
            "{cmd}"
        );
        let d = fx.assistant(ChatMode::Manual, RequestKind::Run, cmd);
        assert!(
            !refused(&d) && held_at(&d).is_none(),
            "manual: {cmd}: {d:?}"
        );
    }
    assert!(!judge::pushes_onto(
        "cat > README.md <<'EOF'\nRun git push origin main to publish.\nEOF",
        "main"
    ));
}

/// Review 2 (critical): the tag is the whole shell word, its quotes removed: `E"O"F` is
/// `EOF`, so the lines after it run.
#[test]
fn a_here_document_tag_with_quotes_inside_is_read_as_bash_reads_it() {
    let fx = Fixture::new("tag");
    let cmd = "cat <<E\"O\"F\nEOF\ngit push origin main\nE";
    let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
    assert_eq!(held_at(&d), Some(Limit::Target), "{cmd}: {d:?}");
    let d = fx.lead(ChatMode::Auto, RequestKind::Run, cmd);
    assert_eq!(held_at(&d), Some(Limit::Target), "lead: {cmd}: {d:?}");
    for cmd in [
        "cat <<E\"O\"F\nEOF\necho x > .claude/settings.json\nE",
        "cat <<E'O'F\nEOF\necho x > .git/hooks/pre-commit\nE",
        "cat <<E\\OF\nEOF\necho x > .fallow/squad.toml\nE",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
    }
    // Read as bash reads it: the body ends at `EOF`, the rest are commands.
    let parts: Vec<String> = judge::split("cat <<E\"O\"F\nEOF\nls\nE")
        .unwrap()
        .into_iter()
        .map(|p| p.text)
        .collect();
    assert_eq!(parts, ["cat  </dev/null", "ls", "E"]);
}

/// Review 3: a control write anywhere in the command is refused, even beside a push onto the
/// target, which could otherwise be allowed always at the target limit.
#[test]
fn a_control_write_beside_a_push_is_refused_not_held() {
    let fx = Fixture::new("order");
    for cmd in [
        "git push origin main && echo x > .claude/settings.json",
        "git push origin main && echo x > .fallow/squad.toml",
        "git push origin main && echo x > .git/hooks/pre-commit",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
        let d = fx.lead(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "lead: {cmd}: {d:?}");
    }
    // The runtime's settings, in Manual too; never a hold at the target.
    let cmd = "git push origin main && echo x > .claude/settings.json";
    assert!(refused(&fx.assistant(
        ChatMode::Manual,
        RequestKind::Run,
        cmd
    )));
    let d = fx.assistant(
        ChatMode::Manual,
        RequestKind::Run,
        "git push origin main && echo x > .fallow/squad.toml",
    );
    assert!(held_at(&d).is_none(), "{d:?}");
}

/// Review 4: a control file written with a substitution in the command is refused, not held.
#[test]
fn a_control_write_with_a_substitution_is_refused() {
    let fx = Fixture::new("subst");
    for cmd in [
        "echo \"$(date)\" > .git/hooks/pre-commit",
        "echo \"$(date)\" > .fallow/squad.toml",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
        let d = fx.lead(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "lead: {cmd}: {d:?}");
    }
}

/// Review 5: the lead's edit of git's own record is refused, as its commands are.
#[test]
fn the_lead_s_edit_of_git_s_record_is_refused() {
    let fx = Fixture::new("lead-git");
    let wt = fx.wt.display().to_string().replace('\\', "/");
    for path in [
        ".git/hooks/pre-commit".to_string(),
        ".git/config".to_string(),
        ".GIT/config".to_string(),
        "specs/../.git/hooks/pre-commit".to_string(),
        format!("{wt}/.git/hooks/pre-commit"),
        format!("{wt}/.git/config"),
    ] {
        for mode in [ChatMode::Auto, ChatMode::Manual] {
            let d = fx.lead(mode, RequestKind::Edit, &path);
            assert!(refused(&d), "{path}: {d:?}");
        }
    }
    // A spec document still goes; `.gitignore` and `.github` are not git's record.
    assert_eq!(
        fx.lead(ChatMode::Auto, RequestKind::Edit, "specs/001-x/spec.md"),
        Decision::Allow
    );
    for path in [".gitignore", ".github/workflows/ci.yml"] {
        let d = fx.lead(ChatMode::Auto, RequestKind::Edit, path);
        assert_eq!(held_at(&d), Some(Limit::Edit), "{path}: {d:?}");
    }
}

fn entry(limit: &str, kind: &str, prefix: &str, starts: Option<bool>) -> PastLimit {
    let mut v = serde_json::json!({ "limit": limit, "kind": kind, "prefix": prefix });
    if let Some(s) = starts {
        v["starts"] = s.into();
    }
    serde_json::from_value(v).unwrap()
}

/// Review 6a: an edit entry compares paths, never a raw string prefix: a folder entry does not
/// reach out of its folder through `..`.
#[test]
fn always_allow_of_a_folder_stays_in_the_folder() {
    let e = entry("edit", "edit", "/Users/me/proj/docs/", None);
    assert!(e.allows("edit", "edit", "/Users/me/proj/docs/a.md"));
    assert!(e.allows("edit", "edit", "/Users/me/proj/docs/sub/b.md"));
    for path in [
        "/Users/me/proj/docs/../.git/hooks/pre-commit",
        "/Users/me/proj/docs/sub/../../.claude/settings.json",
        "/Users/me/proj/docs/./../x",
        "/Users/me/proj/docsx/a.md",
        "/Users/me/proj/a.md",
    ] {
        assert!(!e.allows("edit", "edit", path), "{path}");
    }
    let e = entry("edit", "edit", "/Users/me/proj/a.md", None);
    assert!(e.allows("edit", "edit", "/Users/me/proj/a.md"));
    assert!(!e.allows("edit", "edit", "/Users/me/proj/a.md.bak"));
    assert!(!e.allows("edit", "edit", "/Users/me/proj/x/../a.md"));
}

/// Review 6b: a command allowed always as a whole matches only itself; entries stored by 0.3.1
/// (no `starts`) are read that way. Only one the engineer shortened matches what starts with it.
#[test]
fn always_allow_of_a_command_is_exact_unless_shortened() {
    let e = entry("target", "run", "git push origin main", None);
    assert!(e.allows("target", "run", "git push origin main"));
    for cmd in [
        "git push origin main --force",
        "git push origin main --delete",
        "git push origin main +other:release",
    ] {
        assert!(!e.allows("target", "run", cmd), "{cmd}");
    }
    let e = entry("edit", "run", "rm ../folder/build/a.o", Some(false));
    assert!(e.allows("edit", "run", "rm ../folder/build/a.o"));
    assert!(!e.allows("edit", "run", "rm ../folder/build/a.o -rf ../folder/src"));
    // Shortened by the engineer: the commands starting with it, still no chaining.
    let e = entry("target", "run", "git push origin", Some(true));
    assert!(e.allows("target", "run", "git push origin main"));
    assert!(!e.allows("target", "run", "git push origin main; rm -rf x"));
    // Stored as 0.3.1 stored it, and written back the same way.
    let old: PastLimit =
        serde_json::from_str(r#"{"limit":"target","kind":"run","prefix":"git push origin main"}"#)
            .unwrap();
    assert!(!old.allows("target", "run", "git push origin main --force"));
    assert!(
        !serde_json::to_string(&old).unwrap().contains("starts"),
        "an exact entry is stored as before"
    );
}

/// The temp folder and the copy still work as before (no false refusal from the new checks).
#[test]
fn ordinary_work_still_runs() {
    let fx = Fixture::new("ordinary");
    for cmd in [
        "cat > notes.md <<'EOF'\nhello\nEOF",
        "git status && echo x > /tmp/fallow-limits-x.txt",
        "echo \"$(date)\" > build.log",
    ] {
        assert_eq!(
            fx.assistant(ChatMode::Auto, RequestKind::Run, cmd),
            Decision::Allow,
            "{cmd}"
        );
    }
}

/// Review of 0.3.2 (1): the assistant's edit of a control file is refused however it is
/// spelled: case (macOS's default filesystem folds it), and `\\` (Windows reads it as `/`).
#[test]
fn the_assistant_s_edit_of_a_control_file_is_refused_however_spelled() {
    let fx = Fixture::new("case");
    let wt = fx.wt.display().to_string().replace('\\', "/");
    for path in [
        ".Claude/settings.json".to_string(),
        ".CLAUDE/Settings.json".to_string(),
        ".fallow/SQUAD.toml".to_string(),
        ".git\\hooks\\pre-commit".to_string(),
        ".claude\\settings.local.json".to_string(),
        format!("{wt}/.Claude/settings.json"),
        format!("{wt}/.GIT/config"),
    ] {
        for mode in [ChatMode::Auto, ChatMode::Manual] {
            let d = fx.assistant(mode, RequestKind::Edit, &path);
            assert!(refused(&d), "{path}: {d:?}");
        }
    }
    // Manual too, never asked or held: refused.
    for path in [".Claude/settings.json", ".CLAUDE/Settings.json"] {
        let d = fx.assistant(ChatMode::Manual, RequestKind::Edit, path);
        assert!(refused(&d), "manual: {path}: {d:?}");
    }
    // An ordinary file in the copy still goes.
    assert_eq!(
        fx.assistant(ChatMode::Auto, RequestKind::Edit, "src/settings.json"),
        Decision::Allow
    );
}

/// Review of 0.3.2 (4): an Always-allow folder entry never reaches git's record or a control
/// file inside that folder, however spelled; the assistant's edit there is refused anyway.
#[test]
fn always_allow_of_a_folder_never_reaches_a_control_file() {
    let e = entry("edit", "edit", "/Users/me/proj/", None);
    assert!(e.allows("edit", "edit", "/Users/me/proj/src/a.py"));
    for path in [
        "/Users/me/proj/.git/config",
        "/Users/me/proj/.git/hooks/pre-commit",
        "/Users/me/proj/.GIT/hooks/pre-commit",
        "/Users/me/proj/.git\\hooks\\pre-commit",
        "/Users/me/proj/.claude/settings.json",
        "/Users/me/proj/.Claude/Settings.local.json",
        "/Users/me/proj/.fallow/squad.toml",
        "/Users/me/proj/sub/.fallow/agents/lead.md",
    ] {
        assert!(!e.allows("edit", "edit", path), "{path}");
    }
    let e = entry("edit", "edit", "/Users/me/proj/.git/config", None);
    assert!(!e.allows("edit", "edit", "/Users/me/proj/.git/config"));
    let fx = Fixture::new("folder-git");
    let f = fx.folder.display().to_string().replace('\\', "/");
    for path in [
        format!("{f}/.git/config"),
        format!("{f}/.git/hooks/pre-commit"),
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Edit, &path);
        assert!(refused(&d), "{path}: {d:?}");
    }
}

/// Review of 0.3.2, round 3 (1): `git commit -F -` reads its here-document as data only when
/// written plainly; git can run the body otherwise (an alias, an editor), so it is read as
/// commands.
#[test]
fn a_commit_message_from_stdin_is_data_only_when_written_plainly() {
    let fx = Fixture::new("commit");
    for open in [
        "git -c 'alias.x=!sh -s' x commit -F - <<'EOF'",
        "GIT_EDITOR=sh git commit --allow-empty -e -F - <<'EOF'",
        "git commit -m x -F - <<'EOF'",
    ] {
        for body in [
            "echo x > .claude/settings.json",
            "echo x > .git/hooks/pre-commit",
        ] {
            let cmd = format!("{open}\n{body}\nEOF");
            let d = fx.assistant(ChatMode::Auto, RequestKind::Run, &cmd);
            assert!(refused(&d), "{cmd}: {d:?}");
        }
        let cmd = format!("{open}\ngit push origin main\nEOF");
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, &cmd);
        assert!(held_at(&d).is_some(), "{cmd}: {d:?}");
        assert_ne!(d, Decision::Allow, "{cmd}");
    }
    let cmd = "git -c 'alias.x=!sh -s' x commit -F - <<'EOF'\necho PWNED > pwned.txt\nEOF";
    assert!(judge::split(cmd).is_err(), "not read as data: {cmd}");
    // Written plainly, it is still data.
    for cmd in [
        "git commit -F - <<'EOF'\nfix: refuse writes to .git/hooks\nEOF",
        "git -C sub commit -q --allow-empty -F - <<'EOF'\nfix: .claude/settings.json\nEOF",
    ] {
        assert_eq!(
            fx.assistant(ChatMode::Auto, RequestKind::Run, cmd),
            Decision::Allow,
            "{cmd}"
        );
    }
}

/// Review of 0.3.2, round 3 (2): a `#` starting a word starts a comment; a `<<` in it opens no
/// here-document, so the lines after it are commands.
#[test]
fn a_here_document_in_a_comment_is_no_here_document() {
    let fx = Fixture::new("comment");
    let cmd = "cat x.txt # <<'EOF'\necho x > .claude/settings.json\nEOF";
    let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
    assert!(refused(&d), "{cmd}: {d:?}");
    let cmd = "cat x # c <<'EOF'\ngit push origin main\nEOF";
    let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
    assert_eq!(held_at(&d), Some(Limit::Target), "{cmd}: {d:?}");
    // A `#` inside a word or quotes is no comment.
    assert_eq!(
        fx.assistant(
            ChatMode::Auto,
            RequestKind::Run,
            "cat > a#b.md <<'EOF'\nsee .claude/settings.json\nEOF"
        ),
        Decision::Allow
    );
}

/// Review of 0.3.2, round 3 (3): a git invocation that may run another program waits in Auto
/// and is asked in Manual; plain settings and `-C` still run.
#[test]
fn git_that_may_run_another_program_waits() {
    let fx = Fixture::new("git-c");
    for cmd in [
        "git -c 'alias.x=!git push origin main' x",
        "git -c core.sshCommand=sh status",
        "git --config-env=core.hooksPath=X status",
        "GIT_EDITOR=sh git commit --allow-empty",
        "GIT_SSH_COMMAND=sh git fetch",
        "env GIT_EXEC_PATH=/tmp/x git status",
        "sh -c \"git -c 'alias.x=!sh' x\"",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(held_at(&d).is_some(), "{cmd}: {d:?}");
        let d = fx.lead(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(held_at(&d).is_some(), "lead: {cmd}: {d:?}");
        let d = fx.assistant(ChatMode::Manual, RequestKind::Run, cmd);
        assert!(matches!(d, Decision::Ask(_)), "manual: {cmd}: {d:?}");
    }
    // Never a way around a control-file refusal.
    let cmd = "git -c 'alias.x=!echo x > .claude/settings.json' x";
    assert!(
        refused(&fx.assistant(ChatMode::Auto, RequestKind::Run, cmd)),
        "{cmd}"
    );
    for cmd in [
        "git -C sub status",
        "git -c user.name=me -c user.email=me@example.com commit --allow-empty -m x",
        "git -c color.ui=never log -1",
        "git -c core.quotepath=off status",
    ] {
        assert_eq!(
            fx.assistant(ChatMode::Auto, RequestKind::Run, cmd),
            Decision::Allow,
            "{cmd}"
        );
    }
}
