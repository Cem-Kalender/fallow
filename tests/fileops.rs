//! T062: Files pane operations (FR-068, FR-034).

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fallow::app::state::{AppState, Overlay, Pane};
use fallow::editor::buffer::DiskState;
use fallow::editor::fileops;
use fallow::ui::confirm::{Dialog, Outcome};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-fileops-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::path::absolute(&d).unwrap()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(s: &mut AppState, text: &str) {
    for c in text.chars() {
        s.handle_key(key(KeyCode::Char(c)));
    }
}

fn select(s: &mut AppState, path: &Path) {
    let p = s.project.as_mut().unwrap();
    p.tree.reveal(path);
    p.select_path(path);
    assert_eq!(p.selected_row().unwrap().path, path);
    s.focus = Pane::Files;
}

#[test]
fn new_items_go_inside_the_selected_folder_or_next_to_the_selected_file() {
    let d = temp("new");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/main.rs"), "").unwrap();
    assert_eq!(fileops::target_dir(&d.join("src"), true), d.join("src"));
    assert_eq!(
        fileops::target_dir(&d.join("src/main.rs"), false),
        d.join("src")
    );

    let mut s = AppState::new(Some(d.clone()), Vec::new());
    select(&mut s, &d.join("src/main.rs"));
    s.handle_key(key(KeyCode::Char('a')));
    assert!(matches!(s.overlay, Overlay::Prompt { .. }));
    typed(&mut s, "lib.rs");
    s.handle_key(key(KeyCode::Enter));
    assert!(d.join("src/lib.rs").is_file(), "next to the selected file");
    assert_eq!(
        s.project.as_ref().unwrap().selected_row().unwrap().path,
        d.join("src/lib.rs")
    );

    select(&mut s, &d.join("src"));
    s.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
    typed(&mut s, "util");
    s.handle_key(key(KeyCode::Enter));
    assert!(d.join("src/util").is_dir(), "inside the selected folder");

    // Never overwrites; the prompt stays open with an error.
    select(&mut s, &d.join("src/lib.rs"));
    s.handle_key(key(KeyCode::Char('a')));
    typed(&mut s, "main.rs");
    s.handle_key(key(KeyCode::Enter));
    match &s.overlay {
        Overlay::Prompt { error, .. } => assert!(error.as_ref().unwrap().contains("exists")),
        o => panic!("{o:?}"),
    }
    assert!(fileops::validate_name("a/b").is_err());
    assert!(fileops::validate_name("..").is_err());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn rename_updates_the_open_buffers_path() {
    let d = temp("rename");
    std::fs::write(d.join("x.txt"), "hello").unwrap();
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&d.join("x.txt"));
    select(&mut s, &d.join("x.txt"));
    s.handle_key(key(KeyCode::Char('r')));
    // The prompt starts with the old name selected, so typing replaces it.
    typed(&mut s, "y.txt");
    s.handle_key(key(KeyCode::Enter));
    assert!(matches!(s.overlay, Overlay::None), "{:?}", s.overlay);
    assert!(!d.join("x.txt").exists());
    assert!(d.join("y.txt").is_file());
    let b = s.editor.active().unwrap();
    assert_eq!(b.path.as_deref(), Some(d.join("y.txt").as_path()));
    assert!(!b.dirty(), "the buffer simply follows the rename");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn delete_requires_a_confirmation_token() {
    let d = temp("token");
    let f = d.join("x.txt");
    std::fs::write(&f, "").unwrap();
    let other = d.join("other.txt");
    std::fs::write(&other, "").unwrap();
    // The only way to get a token is to confirm a dialog, and it is bound to its path.
    let mut dlg: Dialog = fileops::delete_dialog(&other);
    let Outcome::Confirmed(token) = dlg.key(key(KeyCode::Char('y'))) else {
        panic!()
    };
    assert!(
        fileops::delete(&f, token).is_err(),
        "a token for another path"
    );
    assert!(f.exists());
    let mut dlg = fileops::delete_dialog(&f);
    assert!(dlg.body.iter().any(|l| l.contains("no trash integration")));
    assert!(
        dlg.body
            .iter()
            .any(|l| l.contains(&f.display().to_string()))
    );
    let Outcome::Confirmed(token) = dlg.key(key(KeyCode::Char('y'))) else {
        panic!()
    };
    fileops::delete(&f, token).unwrap();
    assert!(!f.exists());

    // Folder dialogs count what is inside.
    std::fs::create_dir_all(d.join("dir/a/b")).unwrap();
    std::fs::write(d.join("dir/a/1.txt"), "").unwrap();
    std::fs::write(d.join("dir/2.txt"), "").unwrap();
    let dlg = fileops::delete_dialog(&d.join("dir"));
    assert!(
        dlg.body.iter().any(|l| l.contains("2 files and 2 folders")),
        "{:?}",
        dlg.body
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn delete_from_the_files_pane_confirms_and_makes_an_open_buffer_unsaved() {
    let d = temp("pane");
    let f = d.join("x.txt");
    std::fs::write(&f, "keep").unwrap();
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&f);
    select(&mut s, &f);
    s.handle_key(key(KeyCode::Char('d')));
    assert!(matches!(s.overlay, Overlay::Confirm { .. }));
    s.handle_key(key(KeyCode::Enter));
    assert!(f.exists(), "Enter on the default (Cancel) button cancels");
    s.handle_key(key(KeyCode::Delete));
    s.handle_key(key(KeyCode::Esc));
    assert!(f.exists(), "Esc cancels");
    s.handle_key(key(KeyCode::Char('d')));
    s.handle_key(key(KeyCode::Char('y')));
    assert!(!f.exists());
    let b = s.editor.active().unwrap();
    assert!(b.dirty());
    assert_eq!(b.disk_state, DiskState::DeletedOnDisk);
    assert_eq!(b.text().to_string(), "keep");
    let _ = std::fs::remove_dir_all(&d);
}
