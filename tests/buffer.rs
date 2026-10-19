//! T059: buffers (FR-003, spec edge case "large or binary file").

use std::path::PathBuf;

use fallow::editor::buffer::{Buffer, OpenError, UNDO_CAP_BYTES};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-buffer-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(b: &Buffer) -> String {
    b.text().to_string()
}

#[test]
fn edits_undo_and_redo() {
    let mut b = Buffer::from_text("fn main() {}\n");
    b.set_cursor(0, 12, false);
    for c in " // hi".chars() {
        b.type_char(c);
    }
    assert_eq!(text(&b), "fn main() {} // hi\n");
    assert!(b.dirty());
    b.newline();
    b.insert("x");
    assert_eq!(text(&b), "fn main() {} // hi\nx\n");
    assert!(b.undo());
    assert!(b.undo());
    assert_eq!(text(&b), "fn main() {} // hi\n");
    // Typing is grouped into a few undo steps, not one per character.
    let mut steps = 0;
    while b.undo() {
        steps += 1;
    }
    assert_eq!(text(&b), "fn main() {}\n");
    assert!(steps <= 3, "{steps} undo steps for one typed comment");
    assert!(!b.dirty(), "back at the loaded version");
    while b.redo() {}
    assert_eq!(text(&b), "fn main() {} // hi\nx\n");
    b.set_cursor(0, 0, false);
    b.delete_forward();
    b.end(false);
    b.backspace();
    assert_eq!(text(&b), "n main() {} // h\nx\n");
}

#[test]
fn undo_is_an_edit_log_capped_at_10_mb_per_buffer_the_oldest_entries_are_dropped_first() {
    let mut b = Buffer::from_text("");
    let chunk = "x".repeat(1024 * 1024);
    for i in 0..15 {
        b.doc_end(false);
        b.insert(&chunk);
        assert!(
            b.undo_log().bytes() <= UNDO_CAP_BYTES,
            "after {i}: {} bytes",
            b.undo_log().bytes()
        );
    }
    let kept = b.undo_log().undo_len();
    assert!((8..15).contains(&kept), "kept {kept} of 15 changes");
    // The newest changes are the ones kept: undoing all of them leaves the oldest text.
    while b.undo() {}
    assert_eq!(b.text().len_chars(), (15 - kept) * chunk.len());
    assert!(
        b.dirty(),
        "the dropped changes can't be undone, so this isn't the loaded text"
    );

    let mut small = Buffer::from_text("");
    small.set_undo_cap(1000);
    for _ in 0..50 {
        small.insert("0123456789");
        small.left(false);
    }
    assert!(small.undo_log().bytes() <= 1000);
}

#[test]
fn files_over_the_threshold_open_read_only() {
    let d = temp("ro");
    let big = d.join("big.txt");
    std::fs::write(&big, "a".repeat(10 * 1024 * 1024 + 1)).unwrap();
    let small = d.join("small.txt");
    std::fs::write(&small, "abc").unwrap();
    let ten_mb = 10 * 1024 * 1024;
    let mut b = Buffer::open(&big, ten_mb).unwrap();
    assert!(b.read_only);
    b.type_char('z');
    b.insert("zz");
    assert_eq!(
        b.text().len_chars(),
        10 * 1024 * 1024 + 1,
        "read-only ignores edits"
    );
    assert!(b.save().is_err());
    assert!(!Buffer::open(&small, ten_mb).unwrap().read_only);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn binary_files_are_not_opened_as_text() {
    let d = temp("bin");
    let p = d.join("x.bin");
    std::fs::write(&p, b"abc\0def").unwrap();
    assert!(matches!(Buffer::open(&p, 1 << 30), Err(OpenError::Binary)));
    let late = d.join("late.bin");
    let mut bytes = vec![b'a'; 9000];
    bytes.push(0);
    std::fs::write(&late, &bytes).unwrap();
    assert!(
        Buffer::open(&late, 1 << 30).is_ok(),
        "only the first 8 KiB are sniffed"
    );
    let latin1 = d.join("l1.txt");
    std::fs::write(&latin1, b"caf\xe9").unwrap();
    assert!(matches!(
        Buffer::open(&latin1, 1 << 30),
        Err(OpenError::NotUtf8)
    ));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn save_writes_the_file_and_clears_dirty() {
    let d = temp("save");
    let p = d.join("a.py");
    std::fs::write(&p, "print(1)\r\n").unwrap();
    let mut b = Buffer::open(&p, 1 << 30).unwrap();
    assert_eq!(b.language.map(|l| l.name()), Some("Python"));
    b.end(false);
    b.newline();
    b.insert("print(2)");
    assert!(b.dirty());
    b.save().unwrap();
    assert!(!b.dirty());
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "print(1)\r\nprint(2)\r\n",
        "Enter uses the file's CRLF line endings"
    );
    b.undo();
    assert!(b.dirty(), "undo past the save point");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn crlf_is_one_step_for_the_cursor() {
    let mut b = Buffer::from_text("ab\r\ncd");
    b.set_cursor(1, 0, false);
    b.left(false);
    assert_eq!(b.line_col(), (0, 2));
    b.right(false);
    assert_eq!(b.line_col(), (1, 0));
    b.backspace();
    assert_eq!(text(&b), "abcd");
}

#[test]
fn indent_and_outdent() {
    let mut b = Buffer::from_text("a\nb\nc\n");
    b.select_all();
    b.indent(4);
    assert_eq!(text(&b), "    a\n    b\n    c\n");
    b.outdent(4);
    assert_eq!(text(&b), "a\nb\nc\n");
    b.set_cursor(1, 1, false);
    b.indent(4);
    assert_eq!(
        text(&b),
        "a\nb   \nc\n",
        "Tab without a selection goes to the next stop"
    );
    let mut t = Buffer::from_text("\tx\n");
    t.set_cursor(0, 2, false);
    t.indent(4);
    assert_eq!(text(&t), "\tx\t\n", "a file indented with tabs gets tabs");
    t.outdent(4);
    assert_eq!(text(&t), "x\t\n");
}

#[test]
fn selection_and_vertical_movement() {
    let mut b = Buffer::from_text("hello\nhi\nworld\n");
    b.set_cursor(0, 4, false);
    b.vertical(1, true, 4);
    assert_eq!(b.line_col(), (1, 2), "clamped to the short line");
    b.vertical(1, true, 4);
    assert_eq!(b.line_col(), (2, 4), "the column is remembered");
    assert_eq!(b.selected_text().as_deref(), Some("o\nhi\nworl"));
    assert_eq!(b.cut().as_deref(), Some("o\nhi\nworl"));
    assert_eq!(text(&b), "helld\n");
    b.select_all();
    assert_eq!(b.selected_text().as_deref(), Some("helld\n"));
}

#[test]
fn goto_validates_and_moves() {
    let mut b = Buffer::from_text("a\nb\nc");
    assert!(b.goto(4, None).is_err());
    assert!(b.goto(0, None).is_err());
    assert_eq!(b.line_col(), (0, 0), "invalid input moves nothing");
    b.goto(3, Some(1)).unwrap();
    assert_eq!(b.line_col(), (2, 0));
    b.goto(1, Some(99)).unwrap();
    assert_eq!(
        b.line_col(),
        (0, 1),
        "columns past the end clamp to the line end"
    );
}
