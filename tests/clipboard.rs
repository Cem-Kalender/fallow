//! T063: the clipboard (FR-008).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fallow::app::state::{AppState, Overlay};
use fallow::clipboard::{Clipboard, osc52};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn with_file(text: &str) -> (AppState, std::path::PathBuf) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("fallow-clip-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("a.txt"), text).unwrap();
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.clipboard = Clipboard::detached();
    s.open_file(&d.join("a.txt"));
    (s, d)
}

#[test]
fn osc52_encoding() {
    assert_eq!(osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(osc52("é\n"), "\x1b]52;c;w6kK\x07");
}

#[test]
fn copy_emits_osc52() {
    let (mut s, d) = with_file("hello world\n");
    for _ in 0..5 {
        s.handle_key(shift(KeyCode::Right));
    }
    s.handle_key(ctrl('c'));
    assert_eq!(s.clipboard.captured(), osc52("hello").as_bytes());
    assert_eq!(s.clipboard.copies, 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn copy_with_no_selection_is_a_no_op_that_shows_the_quit_hint() {
    let (mut s, d) = with_file("abc\n");
    s.handle_key(ctrl('c'));
    assert!(s.clipboard.captured().is_empty(), "nothing copied");
    assert_eq!(s.message.as_ref().unwrap().0, "Quit: Ctrl-Q");
    assert!(!s.quit);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn both_sides_failing_says_so() {
    let (mut s, d) = with_file("abc\n");
    s.clipboard = Clipboard::broken();
    s.handle_key(ctrl('a'));
    s.handle_key(ctrl('c'));
    assert_eq!(
        s.message.as_ref().unwrap().0,
        "Copy failed: clipboard unavailable"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_bracketed_paste_is_inserted_literally_with_no_auto_indent() {
    let (mut s, d) = with_file("    fn x() {\n");
    s.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let pasted = "\n  a?\n\tb\nCtrl-S";
    assert!(s.paste(pasted));
    assert_eq!(
        s.editor.active().unwrap().text().to_string(),
        format!("    fn x() {{{pasted}\n"),
        "no indentation added, '?' and key names are text"
    );
    assert!(matches!(s.overlay, Overlay::None), "no key was interpreted");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_paste_delivered_as_keys_is_not_auto_indented() {
    // Windows consoles deliver a terminal paste as key events in one burst.
    let (mut s, d) = with_file("    fn x() {\n");
    s.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    s.burst = true;
    for c in "\n  a\nb".chars() {
        let code = if c == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(c)
        };
        s.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    s.burst = false;
    assert_eq!(
        s.editor.active().unwrap().text().to_string(),
        "    fn x() {\n  a\nb\n"
    );
    // A single Enter typed by hand keeps the indentation of its line.
    s.editor.active_mut().unwrap().set_cursor(1, 3, false);
    s.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        s.editor.active().unwrap().text().to_string(),
        "    fn x() {\n  a\n  \nb\n"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cut_removes_the_selection() {
    let (mut s, d) = with_file("one two\n");
    for _ in 0..4 {
        s.handle_key(shift(KeyCode::Right));
    }
    s.handle_key(ctrl('x'));
    assert_eq!(s.editor.active().unwrap().text().to_string(), "two\n");
    assert_eq!(s.clipboard.captured(), osc52("one ").as_bytes());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn selection_and_clipboard_work_in_the_find_box() {
    let (mut s, d) = with_file("needle\n");
    s.handle_key(ctrl('f'));
    for c in "abc".chars() {
        s.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    s.handle_key(ctrl('a'));
    s.handle_key(ctrl('x'));
    assert_eq!(s.find.as_ref().unwrap().input.text(), "");
    assert_eq!(s.clipboard.captured(), osc52("abc").as_bytes());
    s.paste("pasted");
    assert_eq!(s.find.as_ref().unwrap().input.text(), "pasted");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_platform_clipboard_is_not_opened_at_startup() {
    let s = AppState::new(None, Vec::new());
    assert!(!s.clipboard.platform_touched());
}
