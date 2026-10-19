//! T042: `projects.toml` (002 contracts/config.md; FR-014, FR-015, FR-017, FR-026).

use std::path::{Path, PathBuf};

use fallow::app::state::AppState;
use fallow::projects::ProjectList;
use fallow::projects::store::{self, PROJECTS_FILE};

/// A fresh temporary directory for one test.
fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-store-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Folders to list as projects, inside `root`.
fn folders(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| {
            let p = root.join(n);
            std::fs::create_dir_all(&p).unwrap();
            p
        })
        .collect()
}

#[test]
fn round_trip_keeps_order_selection_and_names() {
    let root = temp("roundtrip");
    let dirs = folders(&root, &["api", "web", "docs"]);
    let mut list = ProjectList::default();
    for d in &dirs {
        list.add(d).unwrap();
    }
    list.rename(1, "Front \"end\" \\ ok").unwrap();
    list.move_by(2, -1);
    list.move_by(1, -1);
    list.selected = Some(1);
    let cfg = root.join("config");
    store::write(&cfg, &list).unwrap();

    let loaded = store::read(&cfg);
    assert!(loaded.notices.is_empty(), "{:?}", loaded.notices);
    assert!(loaded.writable);
    let paths: Vec<&PathBuf> = loaded.entries.iter().map(|(p, _)| p).collect();
    let expected: Vec<&PathBuf> = list.projects.iter().map(|p| &p.path).collect();
    assert_eq!(paths, expected, "order");
    assert_eq!(loaded.selected, Some(1));
    let names: Vec<Option<&str>> = loaded.entries.iter().map(|(_, n)| n.as_deref()).collect();
    assert_eq!(names, vec![None, None, Some("Front \"end\" \\ ok")]);

    let mut again = ProjectList::default();
    assert!(!again.restore(loaded.entries, loaded.selected));
    let names: Vec<&str> = again.projects.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["docs", "api", "Front \"end\" \\ ok"]);
    assert_eq!(again.selected, Some(1));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn writes_atomically_through_a_temporary_file() {
    let root = temp("atomic");
    let dirs = folders(&root, &["a"]);
    let mut list = ProjectList::default();
    list.add(&dirs[0]).unwrap();
    let cfg = root.join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join(PROJECTS_FILE), "# old\n").unwrap();
    store::write(&cfg, &list).unwrap();
    let names: Vec<String> = std::fs::read_dir(&cfg)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![PROJECTS_FILE.to_string()],
        "no temporary file is left"
    );
    let text = std::fs::read_to_string(cfg.join(PROJECTS_FILE)).unwrap();
    assert!(text.starts_with("# Written by Fallow."), "{text}");
    assert!(!text.contains("# old"));

    // The write goes to a temporary file first: a directory squatting on that name makes the
    // write fail and the old file stays whole.
    let tmp = cfg.join(format!(".{PROJECTS_FILE}.{}.tmp", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    list.add(&root).unwrap();
    assert!(store::write(&cfg, &list).is_err());
    assert_eq!(
        std::fs::read_to_string(cfg.join(PROJECTS_FILE)).unwrap(),
        text,
        "the old file is untouched"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_write_keeps_the_list_and_reports_a_notice() {
    let root = temp("failed");
    let dirs = folders(&root, &["a", "b"]);
    // The config "directory" is a file, so nothing can be written there.
    let cfg = root.join("config");
    std::fs::write(&cfg, "not a directory").unwrap();
    let mut s = AppState::new(None, Vec::new());
    s.store.dir = Some(cfg.clone());
    s.store.writable = true;
    s.projects.add(&dirs[0]).unwrap();
    s.projects.add(&dirs[1]).unwrap();
    s.save_projects();
    assert_eq!(s.projects.len(), 2, "the in-memory list is kept");
    assert!(
        s.notices
            .iter()
            .any(|n| n.contains(PROJECTS_FILE) && n.contains("cannot save")),
        "{:?}",
        s.notices
    );
    s.save_projects();
    assert_eq!(
        s.notices
            .iter()
            .filter(|n| n.contains("cannot save"))
            .count(),
        1,
        "reported once"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn invalid_entries_are_skipped_with_file_and_line() {
    let abs = if cfg!(windows) {
        "C:\\\\work\\\\api"
    } else {
        "/work/api"
    };
    let text = format!(
        "selected = 7\n\n[[project]]\nname = \"no path\"\n\n[[project]]\npath = \"relative/dir\"\n\n[[project]]\npath = \"{abs}\"\nname = \"\"\n\n[[project]]\npath = \"{abs}\"\n"
    );
    let loaded = store::parse("projects.toml", &text);
    assert_eq!(loaded.entries.len(), 1, "{:?}", loaded.entries);
    assert_eq!(
        loaded.entries[0].1, None,
        "an invalid name falls back to the folder"
    );
    assert_eq!(loaded.selected, None);
    let notices: Vec<String> = loaded.notices.iter().map(ToString::to_string).collect();
    for expected in [
        "projects.toml:3: project needs a path",
        "projects.toml:6: relative/dir: path must be absolute",
        "projects.toml:9: name must be a string",
        "projects.toml:13:",
        "projects.toml:1: selected must be the index",
    ] {
        assert!(
            notices.iter().any(|n| n.starts_with(expected)),
            "missing {expected:?} in {notices:#?}"
        );
    }
    assert!(loaded.writable);

    // A syntax error: nothing loads and the list is not saved over the file.
    let broken = store::parse("projects.toml", "[[project]\npath = 1\n");
    assert!(!broken.writable);
    assert!(broken.entries.is_empty());
    assert!(
        broken.notices[0]
            .to_string()
            .starts_with("projects.toml:1:")
    );
}

#[test]
fn a_command_line_folder_is_added_selected_and_written() {
    let root = temp("cli");
    let dirs = folders(&root, &["one", "two", "three"]);
    let cfg = root.join("config");
    let mut list = ProjectList::default();
    list.add(&dirs[0]).unwrap();
    list.add(&dirs[1]).unwrap();
    list.selected = Some(0);
    store::write(&cfg, &list).unwrap();

    // A new folder: appended, selected and written.
    let mut s = AppState::new(Some(dirs[2].clone()), Vec::new());
    s.store.dir = Some(cfg.clone());
    s.load_projects();
    let names: Vec<&str> = s
        .projects
        .projects
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, vec!["one", "two", "three"]);
    assert_eq!(s.projects.selected, Some(2));
    let loaded = store::read(&cfg);
    assert_eq!(loaded.entries.len(), 3);
    assert_eq!(loaded.selected, Some(2));

    // A folder already listed keeps its place and is selected.
    let mut s = AppState::new(Some(dirs[0].clone()), Vec::new());
    s.store.dir = Some(cfg.clone());
    s.load_projects();
    assert_eq!(s.projects.len(), 3);
    assert_eq!(s.projects.selected, Some(0));
    assert_eq!(store::read(&cfg).selected, Some(0));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_file_holds_only_paths_names_and_the_selection() {
    let root = temp("contents");
    let dirs = folders(&root, &["api", "web"]);
    let mut list = ProjectList::default();
    list.add(&dirs[0]).unwrap();
    list.add(&dirs[1]).unwrap();
    list.rename(0, "API").unwrap();
    let text = store::render(&list);
    let keys: Vec<&str> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            if l == "[[project]]" {
                l
            } else {
                l.split(" = ").next().unwrap()
            }
        })
        .collect();
    for k in &keys {
        assert!(
            ["selected", "[[project]]", "path", "name"].contains(k),
            "unexpected {k:?} in\n{text}"
        );
    }
    assert_eq!(keys.iter().filter(|k| **k == "path").count(), 2);
    let _ = std::fs::remove_dir_all(&root);
}
