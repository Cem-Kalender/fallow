//! T061: search (FR-005): `.gitignore`, the 10,000 result cap, cancellation, file scope
//! next/previous, and project scope running only on submit.

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fallow::app::editing::Scope;
use fallow::app::state::{AppState, Pane};
use fallow::editor::buffer::Buffer;
use fallow::editor::search::{RESULT_CAP, SearchSession, SearchState};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-search-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn gitignore_is_respected() {
    let d = temp("ignore");
    std::fs::create_dir_all(d.join("build")).unwrap();
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join(".gitignore"), "build/\n*.log\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "let needle = 1;\nno\nNeedle again\n").unwrap();
    std::fs::write(d.join("build/out.rs"), "needle\n").unwrap();
    std::fs::write(d.join("x.log"), "needle\n").unwrap();
    std::fs::write(d.join("bin.dat"), b"needle\0binary").unwrap();
    let mut s = SearchSession::start(&d, "needle");
    assert_eq!(s.wait(Duration::from_secs(20)), SearchState::Done);
    let found: Vec<String> = s
        .results
        .iter()
        .map(|h| format!("{}:{}:{}", s.relative(&h.path).display(), h.line, h.col))
        .collect();
    let a = PathBuf::from("src").join("a.rs").display().to_string();
    assert_eq!(
        found,
        [format!("{a}:1:4"), format!("{a}:3:0")],
        "smart case, ignored files skipped"
    );
    let mut exact = SearchSession::start(&d, "Needle");
    exact.wait(Duration::from_secs(20));
    assert_eq!(
        exact.results.len(),
        1,
        "an uppercase letter makes it case-sensitive"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn results_are_capped_at_10000_with_state_capped() {
    let d = temp("cap");
    let line = "needle\n".repeat(6_000);
    std::fs::write(d.join("a.txt"), &line).unwrap();
    std::fs::write(d.join("b.txt"), &line).unwrap();
    std::fs::write(d.join("c.txt"), &line).unwrap();
    let mut s = SearchSession::start(&d, "needle");
    assert_eq!(s.wait(Duration::from_secs(30)), SearchState::Capped);
    assert_eq!(s.results.len(), RESULT_CAP);
    assert_eq!(RESULT_CAP, 10_000);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_search_can_be_cancelled() {
    let d = temp("cancel");
    for i in 0..300 {
        std::fs::write(
            d.join(format!("f{i}.txt")),
            "hay\n".repeat(200) + "needle\n",
        )
        .unwrap();
    }
    let mut s = SearchSession::start(&d, "needle");
    s.cancel();
    assert_eq!(s.state, SearchState::Cancelled);
    std::thread::sleep(Duration::from_millis(200));
    s.poll();
    assert_eq!(
        s.state,
        SearchState::Cancelled,
        "a cancelled search stays cancelled"
    );
    assert!(
        s.results.len() < 300,
        "it stopped early ({} results)",
        s.results.len()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn file_scope_next_and_previous() {
    let mut b = Buffer::from_text("foo\nbar foo\nbaz\nFoo\n");
    assert!(b.find("foo", true));
    assert_eq!(
        b.line_col(),
        (0, 3),
        "first match is selected, cursor at its end"
    );
    assert_eq!(b.selected_text().as_deref(), Some("foo"));
    assert!(b.find("foo", true));
    assert_eq!(b.line_col(), (1, 7));
    assert!(b.find("foo", true));
    assert_eq!(
        b.line_col(),
        (3, 3),
        "case-insensitive for a lowercase query"
    );
    assert!(b.find("foo", true));
    assert_eq!(b.line_col(), (0, 3), "wraps around");
    assert!(b.find("foo", false));
    assert_eq!(b.line_col(), (3, 3), "previous wraps backwards");
    assert!(b.find("foo", false));
    assert_eq!(b.line_col(), (1, 7));
    assert!(!b.find("nothing", true));
}

#[test]
fn project_scope_runs_only_on_submit() {
    let d = temp("submit");
    std::fs::write(d.join("a.rs"), "fn needle() {}\n").unwrap();
    std::fs::write(d.join("b.rs"), "needle();\n").unwrap();
    let mut s = AppState::new(Some(d.clone()), Vec::new());
    s.open_file(&d.join("a.rs"));
    assert_eq!(s.focus, Pane::Editor);
    s.handle_key(ctrl('f'));
    assert_eq!(
        s.find.as_ref().unwrap().scope,
        Scope::File,
        "Ctrl-F starts in this file"
    );
    s.handle_key(key(KeyCode::Tab));
    assert_eq!(s.find.as_ref().unwrap().scope, Scope::Project);
    for c in "needle".chars() {
        s.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(s.searches, 0, "typing never searches");
    assert!(s.find.as_ref().unwrap().session.is_none());
    s.handle_key(key(KeyCode::Enter));
    assert_eq!(s.searches, 1);
    let f = s.find.as_mut().unwrap();
    assert!(f.results_open);
    let sess = f.session.as_mut().unwrap();
    sess.wait(Duration::from_secs(20));
    assert_eq!(sess.results.len(), 2);
    let target = sess.results[1].path.to_path_buf();
    // ↓ selects the second result, Enter opens it at its line.
    s.handle_key(key(KeyCode::Down));
    s.handle_key(key(KeyCode::Enter));
    assert!(s.find.is_none());
    assert_eq!(
        s.searches, 1,
        "Enter on a result opens it; it does not search again"
    );
    let b = s.editor.active().unwrap();
    assert_eq!(b.path.as_deref(), Some(target.as_path()));
    assert_eq!(b.line_col().0, 0);
    // Esc closes the bar; the scope is shown in the bar while open (UI test in layout).
    s.handle_key(ctrl('f'));
    s.handle_key(key(KeyCode::Esc));
    assert!(s.find.is_none());
    let _ = std::fs::remove_dir_all(&d);
}
