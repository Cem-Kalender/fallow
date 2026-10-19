//! T060: one case per FR-006 transition (data-model.md "Buffer"), plus the dialog and the
//! watcher wiring.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fallow::app::editing::Pending;
use fallow::app::state::{AppState, Overlay};
use fallow::editor::buffer::{Buffer, DiskEvent, DiskState};
use fallow::editor::watch::{FileWatcher, watch_set};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-disk-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// "`dirty = false` and the file changed: reload, show a notice, stay `InSync`".
#[test]
fn clean_buffer_reloads_and_stays_in_sync() {
    let d = temp("clean");
    let p = d.join("a.txt");
    std::fs::write(&p, "one\n").unwrap();
    let mut b = Buffer::open(&p, 1 << 30).unwrap();
    assert_eq!(b.check_disk(), DiskEvent::Unchanged, "no change yet");
    std::fs::write(&p, "two\n").unwrap();
    assert_eq!(b.check_disk(), DiskEvent::Reloaded);
    assert_eq!(b.text().to_string(), "two\n");
    assert_eq!(b.disk_state, DiskState::InSync);
    assert!(!b.dirty());
    // The notice: the app shows one when this happens.
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&p);
    std::fs::write(&p, "three\n").unwrap();
    assert!(s.apply_changes([&p]));
    assert!(s.message.as_ref().unwrap().0.contains("reloaded"));
    assert_eq!(s.editor.active().unwrap().text().to_string(), "three\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// "`dirty = true` and the file changed: go to `ChangedOnDisk` and prompt 'keep mine / load
/// disk'. Nothing is written until the user chooses".
#[test]
fn dirty_buffer_goes_to_changed_on_disk_and_prompts() {
    let d = temp("dirty");
    let p = d.join("a.txt");
    std::fs::write(&p, "base\n").unwrap();
    let mut b = Buffer::open(&p, 1 << 30).unwrap();
    b.insert("mine ");
    std::fs::write(&p, "agent\n").unwrap();
    assert_eq!(b.check_disk(), DiskEvent::Conflict);
    assert_eq!(b.disk_state, DiskState::ChangedOnDisk);
    assert_eq!(
        b.text().to_string(),
        "mine base\n",
        "the buffer is untouched"
    );
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "agent\n",
        "nothing written"
    );

    // Through the app: the dialog opens with Cancel ("Keep mine") focused.
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&p);
    s.editor.active_mut().unwrap().insert("mine ");
    std::fs::write(&p, "agent 2\n").unwrap();
    s.apply_changes([&p]);
    match &s.overlay {
        Overlay::Confirm { dialog, action } => {
            assert_eq!(action, &Pending::DiskConflict(p.clone()));
            assert_eq!(dialog.cancel_label, "Keep mine");
            assert_eq!(dialog.confirm_label, "Load disk");
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "agent 2\n");
    // Keep mine: the buffer stays, unsaved; still nothing written.
    s.handle_key(key(crossterm::event::KeyCode::Esc));
    let b = s.editor.active().unwrap();
    assert_eq!(b.text().to_string(), "mine agent\n");
    assert!(b.dirty());
    assert_eq!(b.disk_state, DiskState::InSync);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "agent 2\n");

    // Load disk replaces the buffer; Ctrl-Z brings the user's version back.
    std::fs::write(&p, "agent 3\n").unwrap();
    s.apply_changes([&p]);
    s.handle_key(key(crossterm::event::KeyCode::Char('y')));
    let b = s.editor.active_mut().unwrap();
    assert_eq!(b.text().to_string(), "agent 3\n");
    assert!(!b.dirty());
    b.undo();
    assert!(b.text().to_string().starts_with("mine "));
    let _ = std::fs::remove_dir_all(&d);
}

/// "The file was deleted or renamed: go to `DeletedOnDisk`, show a notice, keep the buffer,
/// and set `dirty = true`".
#[test]
fn deleted_or_renamed_file_keeps_the_buffer_and_marks_it_dirty() {
    let d = temp("deleted");
    let p = d.join("a.txt");
    std::fs::write(&p, "keep me\n").unwrap();
    let mut b = Buffer::open(&p, 1 << 30).unwrap();
    std::fs::remove_file(&p).unwrap();
    assert_eq!(b.check_disk(), DiskEvent::Deleted);
    assert_eq!(b.disk_state, DiskState::DeletedOnDisk);
    assert!(b.dirty());
    assert_eq!(b.text().to_string(), "keep me\n");
    b.save().unwrap();
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "keep me\n",
        "saving recreates it"
    );
    assert!(!b.dirty());

    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&p);
    std::fs::rename(&p, d.join("b.txt")).unwrap();
    s.apply_changes([&p]);
    assert!(s.message.as_ref().unwrap().0.contains("deleted or renamed"));
    let b = s.editor.active().unwrap();
    assert_eq!(b.disk_state, DiskState::DeletedOnDisk);
    assert!(b.dirty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn own_saves_are_not_reported_as_changes() {
    let d = temp("own");
    let p = d.join("a.txt");
    std::fs::write(&p, "x").unwrap();
    let mut b = Buffer::open(&p, 1 << 30).unwrap();
    b.insert("y");
    b.save().unwrap();
    assert_eq!(b.check_disk(), DiskEvent::Unchanged);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn watches_are_non_recursive_and_cover_only_what_is_open() {
    let d = temp("watch");
    std::fs::create_dir_all(d.join("sub/deeper")).unwrap();
    let f = d.join("sub/file.rs");
    std::fs::write(&f, "1").unwrap();
    let set = watch_set([f.as_path()], vec![d.clone()]);
    assert_eq!(
        set.into_iter().collect::<Vec<_>>(),
        vec![d.clone(), d.join("sub")],
        "the expanded root and the open file's folder, nothing below"
    );

    let mut w = FileWatcher::default();
    assert!(w.sync(watch_set([f.as_path()], Vec::new())));
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(&f, "2").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    while Instant::now() < until && !seen {
        seen = w.poll().paths.iter().any(|p| p.ends_with("file.rs"));
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(seen, "a write to an open file is reported");
    let _ = std::fs::remove_dir_all(&d);
}

fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}
