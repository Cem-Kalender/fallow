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

/// Review 1: the raw text is read too, so a body taken out can never hide a control file or
/// a push, whatever the stripping makes of it.
#[test]
fn a_stripped_here_document_s_raw_text_still_counts() {
    let fx = Fixture::new("raw");
    for cmd in [
        "cat > x.sh <<'EOF'\necho x > .claude/settings.json\nEOF\nsh x.sh",
        "cat > x.sh <<'EOF'\necho x > .git/hooks/pre-commit\nEOF\nsh x.sh",
    ] {
        let d = fx.assistant(ChatMode::Auto, RequestKind::Run, cmd);
        assert!(refused(&d), "{cmd}: {d:?}");
    }
    assert!(judge::pushes_onto(
        "cat > x.sh <<'EOF'\ngit push origin main\nEOF\nsh x.sh",
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
