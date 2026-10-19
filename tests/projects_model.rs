//! 002 T009: the in-memory project list (data-model.md "ProjectList").

use std::path::PathBuf;

use fallow::projects::{MAX_PROJECTS, ProjectList};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-projects-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::path::absolute(&d).unwrap()
}

#[test]
fn add_validates_and_dedups() {
    let d = temp("add");
    std::fs::create_dir_all(d.join("api")).unwrap();
    std::fs::write(d.join("file.txt"), "").unwrap();
    let mut l = ProjectList::default();
    let i = l.add(&d.join("api")).unwrap();
    assert_eq!(l.projects[i].name, "api");
    assert_eq!(l.selected, Some(0));
    let e = l.add(&d.join("file.txt")).unwrap_err();
    assert!(e.contains("not a folder"), "{e}");
    assert!(l.add(&d.join("nope")).is_err());
    // The same folder again (with a different case where paths are case-insensitive).
    let again = if cfg!(any(windows, target_os = "macos")) {
        d.join("API")
    } else {
        d.join("api")
    };
    assert_eq!(l.add(&again).unwrap(), 0);
    assert_eq!(l.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn remove_rename_and_move_never_touch_folders() {
    let d = temp("ops");
    for n in ["a", "b", "c"] {
        std::fs::create_dir_all(d.join(n)).unwrap();
    }
    let mut l = ProjectList::default();
    for n in ["a", "b", "c"] {
        l.add(&d.join(n)).unwrap();
    }
    assert_eq!(l.selected, Some(2));
    assert!(l.move_by(2, -1));
    assert_eq!(l.projects[1].name, "c");
    assert_eq!(l.selected, Some(1), "the selection follows the moved entry");
    assert!(!l.move_by(0, -1));
    assert!(l.rename(0, "  API  ").is_ok());
    assert_eq!(l.projects[0].name, "API");
    assert!(l.rename(0, "").is_err(), "1..=64 chars");
    assert!(l.rename(0, &"x".repeat(65)).is_err(), "1..=64 chars");
    let removed = l.remove(1).unwrap();
    assert!(removed.path.is_dir(), "the folder is still there");
    assert_eq!(l.len(), 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn missing_folders_are_kept_and_marked() {
    let d = temp("missing");
    std::fs::create_dir_all(d.join("gone")).unwrap();
    let mut l = ProjectList::default();
    l.add(&d.join("gone")).unwrap();
    std::fs::remove_dir_all(d.join("gone")).unwrap();
    l.refresh_presence();
    assert!(!l.projects[0].present);
    l.push(d.join("never"), Some("named".into()), false);
    assert!(!l.projects[1].present);
    assert_eq!(l.projects[1].name, "named");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_list_is_limited() {
    let d = temp("limit");
    let mut l = ProjectList::default();
    for i in 0..MAX_PROJECTS {
        l.push(d.join(format!("p{i}")), None, false);
    }
    let e = l.add(&d).unwrap_err();
    assert!(e.contains("200"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}
