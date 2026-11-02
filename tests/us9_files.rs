//! 009 US5: the project's files, read-only. Versions (the folder, a spec session's worktree, a
//! loop's, a branch), marks against the feature's base, who changed a file in which task, the
//! limits (binary, large, secrets, paths outside), "Open in my editor", `@` references, and
//! proof that nothing is written.

mod common;

use common::*;
use fallow::run::engine::Msg;
use fallow::studio::Req;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

fn files(tx: &Sender<Msg>, project: &str, version: &str) -> Result<Value, String> {
    sreq(
        tx,
        Req::Files {
            project: project.into(),
            version: version.into(),
        },
    )
}

fn file(
    tx: &Sender<Msg>,
    project: &str,
    version: &str,
    path: &str,
    changes: bool,
) -> Result<Value, String> {
    sreq(
        tx,
        Req::File {
            project: project.into(),
            version: version.into(),
            path: path.into(),
            changes,
        },
    )
}

fn entry<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"] == path)
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Every file's path, size and time under `root`, and git's view of it: what "nothing changed"
/// means for the read-only check (FR-035, SC-009).
fn snapshot(root: &Path) -> (String, String, Vec<(String, u64, std::time::SystemTime)>) {
    let mut v = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                if e.file_name() != ".git" {
                    stack.push(p);
                }
            } else {
                let m = e.metadata().unwrap();
                v.push((p.display().to_string(), m.len(), m.modified().unwrap()));
            }
        }
    }
    v.sort();
    (
        git(root, &["status", "--porcelain"]),
        git(root, &["rev-parse", "HEAD"]),
        v,
    )
}

fn setup(
    name: &str,
) -> (
    PathBuf,
    PathBuf,
    fallow::service::ToolEngine,
    String,
    PathBuf,
) {
    let root = scratch(name);
    let repo = repo(&root);
    put(&repo, "gone.txt", b"bye\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "more"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let project = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        &e.tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let conv = f["conversation"].as_str().unwrap().to_string();
    let wd = PathBuf::from(
        sreq(&e.tx, Req::Conversation { id: conv }).unwrap()["workdir"]
            .as_str()
            .unwrap(),
    );
    (root, repo, e, project, wd)
}

#[test]
fn versions_marks_limits_and_nothing_written() {
    let (_root, repo, e, project, wd) = setup("files-spec");
    let tx = &e.tx;
    // On the feature's branch: a spec commit by the lead, then work not committed yet.
    put(&wd, "specs/001-cart/spec.md", b"# Cart\n");
    git(&wd, &["add", "."]);
    git(&wd, &["commit", "-q", "-m", "spec: specify (turn 1)"]);
    put(&wd, "shop/cart.py", b"def total():\n    return 0\n");
    put(&wd, "README.md", b"calc\nand a cart\n");
    std::fs::remove_file(wd.join("gone.txt")).unwrap();
    put(&wd, ".gitignore", b"build/\n");
    put(&wd, "build/out.txt", b"ignored\n");
    put(&wd, ".env", b"TOKEN=do-not-read\n");
    put(&wd, ".env.example", b"TOKEN=\n");
    put(&wd, "deploy/production.env", b"TOKEN=do-not-read\n");
    put(&wd, "logo.bin", &[0u8, 1, 2, 3, 0, 5]);
    put(&wd, "big.txt", "x\n".repeat(300 * 1024).as_bytes());
    // The engineer's folder has a secret of its own.
    put(&repo, ".env", b"TOKEN=folder-secret\n");

    let before = (snapshot(&repo), snapshot(&wd));

    let vs = sreq(
        tx,
        Req::FileVersions {
            project: project.clone(),
        },
    )
    .unwrap();
    let ids: Vec<&str> = vs["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["folder", "feature:001-cart"], "{vs}");
    assert_eq!(vs["versions"][1]["kind"], "spec");

    // The folder: no marks, nothing ignored or of git's own.
    let folder = files(tx, &project, "folder").unwrap();
    assert!(
        entry(&folder, "README.md").unwrap()["mark"].is_null(),
        "{folder}"
    );
    assert!(
        !folder["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"].as_str().unwrap().starts_with(".git/"))
    );
    assert_eq!(entry(&folder, ".env").unwrap()["secret"], true);

    // The feature: marks against its base, its folder tagged, ignored files left out.
    let feat = files(tx, &project, "feature:001-cart").unwrap();
    assert_eq!(
        entry(&feat, "shop/cart.py").unwrap()["mark"],
        "added",
        "{feat}"
    );
    assert_eq!(entry(&feat, "README.md").unwrap()["mark"], "changed");
    assert_eq!(entry(&feat, "gone.txt").unwrap()["mark"], "deleted");
    assert_eq!(
        entry(&feat, "specs/001-cart/spec.md").unwrap()["spec"],
        true
    );
    assert!(entry(&feat, "build/out.txt").is_none(), "ignored: {feat}");
    assert_eq!(entry(&feat, ".env").unwrap()["secret"], true);
    assert_eq!(entry(&feat, ".env.example").unwrap()["secret"], false);
    assert!(feat["changed"].as_u64().unwrap() >= 4);
    // The fingerprint: the same until something changes.
    let fp = feat["fingerprint"].clone();
    assert_eq!(
        files(tx, &project, "feature:001-cart").unwrap()["fingerprint"],
        fp
    );

    // Files.
    let readme = file(tx, &project, "feature:001-cart", "README.md", true).unwrap();
    assert_eq!(readme["kind"], "text");
    assert_eq!(readme["added"], json!([[2, 2]]), "{readme}");
    assert!(readme["changes"].as_str().unwrap().contains("+and a cart"));
    let cart = file(tx, &project, "feature:001-cart", "shop/cart.py", false).unwrap();
    assert_eq!(cart["added"], json!([[1, 2]]));
    let spec = file(
        tx,
        &project,
        "feature:001-cart",
        "specs/001-cart/spec.md",
        false,
    )
    .unwrap();
    assert_eq!(spec["last"]["who"], "lead", "{spec}");
    let gone = file(tx, &project, "feature:001-cart", "gone.txt", false).unwrap();
    assert_eq!(gone["kind"], "deleted");
    assert_eq!(gone["text"], "bye\n");
    let bin = file(tx, &project, "feature:001-cart", "logo.bin", false).unwrap();
    assert_eq!(bin["kind"], "binary");
    assert_eq!(bin["size"], 6);
    assert!(bin["text"].is_null());
    let big = file(tx, &project, "feature:001-cart", "big.txt", false).unwrap();
    assert_eq!(big["cut"], true);
    assert!(big["text"].as_str().unwrap().len() <= 512 * 1024);
    for secret in [".env", "deploy/production.env"] {
        let s = file(tx, &project, "feature:001-cart", secret, true).unwrap();
        assert_eq!(s["kind"], "secret", "{s}");
        assert!(s["text"].is_null() && s["changes"].is_null(), "{s}");
        assert!(!s.to_string().contains("do-not-read"));
    }
    // Paths outside the project.
    for bad in ["../x", "/etc/hosts", "C:x", "a\\b", "shop/../../x"] {
        let err = file(tx, &project, "feature:001-cart", bad, false).unwrap_err();
        assert!(err.contains("outside the project"), "{bad}: {err}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc", wd.join("etc-link")).unwrap();
        let err = file(tx, &project, "feature:001-cart", "etc-link/hosts", false).unwrap_err();
        assert!(err.contains("outside the project"), "{err}");
        std::fs::remove_file(wd.join("etc-link")).unwrap();
    }

    // Other spellings of a secret's name, and git's own folder, are refused (FR-036, FR-036a).
    for other in [".env ", ".env.", "ENV~1", ".git/config", ".GIT/config"] {
        let err = file(tx, &project, "feature:001-cart", other, false).unwrap_err();
        assert!(err.contains("outside the project"), "{other}: {err}");
    }

    // Nothing was written by any of it (FR-035, SC-009).
    assert_eq!((snapshot(&repo), snapshot(&wd)), before);

    // A junction to a folder outside the project is not followed.
    #[cfg(windows)]
    {
        let outside = _root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("x.txt"),
            "outside
",
        )
        .unwrap();
        let ok = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(wd.join("jn"))
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "mklink /J");
        let err = file(tx, &project, "feature:001-cart", "jn/x.txt", false).unwrap_err();
        assert!(err.contains("outside the project"), "{err}");
        std::fs::remove_dir(wd.join("jn")).unwrap();
    }

    // FR-034 (CI): a file written while the view is shown changes the next answer.
    put(&wd, "shop/cart.py", b"def total():\n    return 1\n");
    assert_ne!(
        files(tx, &project, "feature:001-cart").unwrap()["fingerprint"],
        fp
    );

    // The spec session's worktree goes: the branch is read instead.
    git(
        &repo,
        &["worktree", "remove", "--force", wd.to_str().unwrap()],
    );
    let vs = sreq(
        tx,
        Req::FileVersions {
            project: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(vs["versions"][1]["kind"], "branch", "{vs}");
    let br = files(tx, &project, "feature:001-cart").unwrap();
    assert!(entry(&br, "specs/001-cart/spec.md").is_some(), "{br}");
    assert!(
        entry(&br, "shop/cart.py").is_none(),
        "only what was committed: {br}"
    );
    let spec = file(
        tx,
        &project,
        "feature:001-cart",
        "specs/001-cart/spec.md",
        false,
    )
    .unwrap();
    assert_eq!(spec["text"], "# Cart\n");
    // And the branch too: the version is gone.
    git(&repo, &["branch", "-D", "001-cart"]);
    let vs = sreq(
        tx,
        Req::FileVersions {
            project: project.clone(),
        },
    )
    .unwrap();
    assert_eq!(vs["versions"].as_array().unwrap().len(), 1, "{vs}");
    let err = files(tx, &project, "feature:001-cart").unwrap_err();
    assert!(err.contains("This version is gone"), "{err}");
}

#[test]
fn a_loops_files_say_who_changed_them_in_which_task() {
    let root = scratch("files-loop");
    let (e, repo, id) = locked_calc(&root, &calc_loop_ok(), None);
    let tx = &e.tx;
    wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let project = sreq(tx, Req::Project { id: "repo".into() })
        .map(|_| "repo".to_string())
        .unwrap();
    let vs = sreq(
        tx,
        Req::FileVersions {
            project: project.clone(),
        },
    )
    .unwrap();
    let v = &vs["versions"][1];
    assert_eq!(v["id"], "feature:001-calc", "{vs}");
    assert_eq!(v["kind"], "loop");
    let list = files(tx, &project, "feature:001-calc").unwrap();
    assert_eq!(
        entry(&list, "calc/__init__.py").unwrap()["mark"],
        "added",
        "{list}"
    );
    // The drawer's list says who changed each file in which task (FR-033).
    assert_eq!(
        entry(&list, "calc/__init__.py").unwrap()["who"],
        "builder",
        "{list}"
    );
    assert_eq!(
        entry(&list, "calc/__init__.py").unwrap()["task"],
        "T004",
        "{list}"
    );
    let f = file(tx, &project, "feature:001-calc", "calc/__init__.py", false).unwrap();
    assert_eq!(f["last"]["who"], "builder", "{f}");
    assert_eq!(f["last"]["task"], "T004", "{f}");
    assert!(f["last"]["commit"].is_string());
    let t = file(tx, &project, "feature:001-calc", "tests/test_add.py", false).unwrap();
    assert_eq!(t["last"]["task"], "T003", "{t}");
    let _ = repo;
}

#[test]
fn open_in_my_editor_asks_once_and_never_through_a_shell() {
    let (root, _repo, e, project, _wd) = setup("files-editor");
    let tx = &e.tx;
    let open = |editor: Option<Value>, command: Option<&str>| {
        sreq(
            tx,
            Req::OpenFile {
                project: project.clone(),
                version: "folder".into(),
                path: "README.md".into(),
                editor: editor.map(|v| serde_json::from_value(v).unwrap()),
                command: command.map(str::to_string),
            },
        )
    };
    // Nothing saved: the editors found, and the path to copy.
    let first = open(None, None).unwrap();
    assert!(first["choose"].is_array(), "{first}");
    assert!(
        first["path"].as_str().unwrap().ends_with("README.md"),
        "{first}"
    );

    let (program, args, log) = fake_editor(&root);
    let mut with_flag = args.clone();
    with_flag.push("--flag".into());
    let out = open(Some(json!({ "program": program, "args": with_flag })), None).unwrap();
    assert_eq!(out["opened"], true, "{out}");
    let wait_log = |n: usize| {
        let t0 = std::time::Instant::now();
        loop {
            let lines: Vec<String> = std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect();
            if lines.len() >= n || t0.elapsed().as_secs() > 10 {
                return lines;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let lines = wait_log(2);
    assert_eq!(lines[0], "--flag", "{lines:?}");
    assert!(
        lines[1].ends_with("README.md"),
        "the path is the last argument: {lines:?}"
    );
    // Saved: the next open does not ask.
    assert_eq!(open(None, None).unwrap()["opened"], true);
    assert_eq!(wait_log(4).len(), 4);
    assert!(root.join("data/prefs.json").is_file());
    // Cleared: it asks again.
    sreq(
        tx,
        Req::SetEditor {
            editor: None,
            command: None,
            clear: true,
        },
    )
    .unwrap();
    assert!(open(None, None).unwrap()["choose"].is_array());
    // A program that is not there is refused when saved.
    let err = sreq(
        tx,
        Req::SetEditor {
            editor: None,
            command: Some("no-such-editor-anywhere -r".into()),
            clear: false,
        },
    )
    .unwrap_err();
    assert!(err.contains("no program"), "{err}");
    // A typed command is split, never run through a shell: `&` reaches the editor as text.
    let typed = format!("\"{program}\" --record \"{}\" & del x", log.display());
    let out = open(None, Some(&typed)).unwrap();
    assert_eq!(out["opened"], true, "{out}");
    let lines = wait_log(8);
    assert_eq!(&lines[4..7], ["&", "del", "x"], "{lines:?}");
}

#[cfg(windows)]
#[test]
fn a_cmd_launcher_refuses_what_cmd_would_read() {
    let (root, repo, e, project, _wd) = setup("files-cmd");
    let launcher = root.join("ed.cmd");
    std::fs::write(&launcher, "@echo %* >> \"%~dp0cmd.log\"\r\n").unwrap();
    put(&repo, "a&b.txt", b"x\n");
    let out = sreq(
        &e.tx,
        Req::OpenFile {
            project: project.clone(),
            version: "folder".into(),
            path: "a&b.txt".into(),
            editor: Some(
                serde_json::from_value(
                    json!({ "program": launcher.display().to_string(), "args": [] }),
                )
                .unwrap(),
            ),
            command: None,
        },
    );
    let err = out.unwrap_err();
    assert!(err.contains("would read"), "{err}");
}

#[test]
fn at_references_reach_the_lead_and_secrets_stay_names() {
    let root = scratch("files-refs");
    let repo = repo(&root);
    let scenario = root.join("scenario.toml");
    std::fs::write(
        &scenario,
        r##"
[[step]]
kind = "lead_turn"
when = "`shop/cart.py` (001-cart on its branch):\n```\ndef total():\n    return 0\n```"
output = { reply = "About the cart.", step = "none" }

[[step]]
kind = "lead_turn"
when = "`.env` (the project folder): Fallow does not read files that may hold secrets."
output = { reply = "Not reading it.", step = "none" }
"##,
    )
    .unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;
    let project = sreq(tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "spec kit"]);
    let f = sreq(
        tx,
        Req::NewFeature {
            project: project.clone(),
            title: "Cart".into(),
            lead: None,
        },
    )
    .unwrap();
    let wd = PathBuf::from(
        sreq(
            tx,
            Req::Conversation {
                id: f["conversation"].as_str().unwrap().into(),
            },
        )
        .unwrap()["workdir"]
            .as_str()
            .unwrap(),
    );
    put(&wd, "shop/cart.py", b"def total():\n    return 0\n");
    put(&repo, ".env", b"TOKEN=never\n");
    let pc = sreq(tx, Req::ProjectConversation { project }).unwrap()["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let say = |text: &str| {
        sreq(
            tx,
            Req::Say {
                conv: pc.clone(),
                text: text.into(),
            },
        )
    };
    say("@shop/cart.py (on 001-cart) why zero?").unwrap();
    let v = turn_ended(tx, &pc, 1);
    assert_eq!(v["turns"][0]["reply"], "About the cart.", "{v}");
    say("What is in @.env?").unwrap();
    let v = turn_ended(tx, &pc, 2);
    assert_eq!(v["turns"][1]["reply"], "Not reading it.", "{v}");
}

/// SC-008 (CI): the tree of a project of 5,000 files is listed within 2 seconds.
#[test]
fn five_thousand_files_are_listed_within_two_seconds() {
    let root = scratch("files-many");
    let repo = repo(&root);
    for d in 0..50 {
        for f in 0..100 {
            put(&repo, &format!("src/m{d:02}/f{f:03}.py"), b"x = 1\n");
        }
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "many"]);
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let project = sreq(&e.tx, Req::AddProject { path: repo.clone() }).unwrap()["project"]
        .as_str()
        .unwrap()
        .to_string();
    files(&e.tx, &project, "folder").unwrap();
    let mut took = std::time::Duration::MAX;
    for _ in 0..3 {
        let t0 = std::time::Instant::now();
        let v = files(&e.tx, &project, "folder").unwrap();
        took = took.min(t0.elapsed());
        assert!(v["entries"].as_array().unwrap().len() >= 5000);
    }
    assert!(
        took < std::time::Duration::from_secs(2),
        "5,000 files took {took:?}"
    );
}
