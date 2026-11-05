//! 012 US8 (T023; quickstart Q9 4-5; FR-040, SC-012): with git missing or too old, the start
//! page's check names the problem, every project request is refused, and "Check again" passes
//! once git is back; with no git identity, the first commit is refused, saying how to set one.
//! One test in its own file: it changes this process's environment.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::studio::Req;

#[test]
fn a_missing_or_old_git_is_named_and_no_identity_stops_the_first_commit() {
    let root = scratch("us12-git");
    let shop = repo_named(&root, "shop");
    let fake = if cfg!(windows) {
        let p = root.join("old-git.cmd");
        std::fs::write(&p, "@echo git version 2.30.1\r\n").unwrap();
        p
    } else {
        let p = root.join("old-git");
        std::fs::write(&p, "#!/bin/sh\necho git version 2.30.1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    };
    // Git's global settings: no identity at all.
    let cfg = root.join("empty-gitconfig");
    std::fs::write(&cfg, "").unwrap();
    // SAFETY: the only test of this process.
    unsafe {
        std::env::set_var("FALLOW_GIT_CHECK", &fake);
        std::env::set_var("GIT_CONFIG_GLOBAL", &cfg);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    }
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, "").unwrap();
    let e = start_studio(&root, &scenario, None);
    let tx = &e.tx;

    // Too old: named, with the minimum and how to install; no project opens or is added.
    let g = ask(tx, Request::Git { check: false }).unwrap();
    assert_eq!(g["ok"], false, "{g}");
    assert_eq!(g["found"], "2.30");
    assert_eq!(g["min"], "2.38");
    assert!(!g["hint"].as_str().unwrap().is_empty());
    let err = sreq(tx, Req::AddProject { path: shop.clone() }).unwrap_err();
    assert!(err.contains("needs git 2.38"), "{err}");

    // Missing altogether: the same, never a folder without a repository.
    unsafe { std::env::set_var("FALLOW_GIT_CHECK", root.join("no-such-git")) };
    let g = ask(tx, Request::Git { check: true }).unwrap();
    assert_eq!(g["ok"], false, "{g}");
    assert!(g["found"].is_null(), "{g}");

    // Back: "Check again" passes, and the project is added and opens.
    unsafe { std::env::remove_var("FALLOW_GIT_CHECK") };
    let g = ask(tx, Request::Git { check: true }).unwrap();
    assert_eq!(g["ok"], true, "{g}");
    let added = sreq(tx, Req::AddProject { path: shop.clone() }).unwrap();
    let project = added["project"].as_str().unwrap().to_string();
    sreq(tx, Req::Project { id: project.clone() }).unwrap();
    // While git is gone again, every project request answers so.
    unsafe { std::env::set_var("FALLOW_GIT_CHECK", root.join("no-such-git")) };
    ask(tx, Request::Git { check: true }).unwrap();
    let err = sreq(tx, Req::Project { id: project.clone() }).unwrap_err();
    assert!(err.contains("needs git"), "{err}");
    unsafe { std::env::remove_var("FALLOW_GIT_CHECK") };
    ask(tx, Request::Git { check: true }).unwrap();

    // No identity: the first commit is refused before anything changes, saying how to set it.
    let plain = root.join("notes");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(plain.join("a.md"), "a\n").unwrap();
    let added = sreq(tx, Req::AddProject { path: plain.clone() }).unwrap();
    let notes = added["project"].as_str().unwrap().to_string();
    let scan = sreq(tx, Req::GitScan { id: notes.clone() }).unwrap();
    assert_eq!(scan["identity"], false, "{scan}");
    let err = sreq(tx, Req::GitInit { id: notes, gitignore: None }).unwrap_err();
    assert!(err.contains("git config --global user.name"), "{err}");
    assert!(!plain.join(".git").exists(), "nothing changed");
}
