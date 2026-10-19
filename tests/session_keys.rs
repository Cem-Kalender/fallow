//! 002 T012: keys go to the agent unchanged; only the leave key is captured (FR-003, R-3).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use fallow::session::keys::{LeaveKey, encode, paste};

fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, m)
}

const N: KeyModifiers = KeyModifiers::NONE;
const C: KeyModifiers = KeyModifiers::CONTROL;
const S: KeyModifiers = KeyModifiers::SHIFT;
const A: KeyModifiers = KeyModifiers::ALT;

#[test]
fn keys_become_the_bytes_a_terminal_sends() {
    let cases: &[(KeyEvent, &[u8])] = &[
        (k(KeyCode::Char('a'), N), b"a"),
        (k(KeyCode::Char('A'), S), b"A"),
        (k(KeyCode::Char('é'), N), "é".as_bytes()),
        (k(KeyCode::Enter, N), b"\r"),
        (k(KeyCode::Esc, N), b"\x1b"),
        (k(KeyCode::Tab, N), b"\t"),
        (k(KeyCode::BackTab, S), b"\x1b[Z"),
        (k(KeyCode::Backspace, N), b"\x7f"),
        (k(KeyCode::Up, N), b"\x1b[A"),
        (k(KeyCode::Left, N), b"\x1b[D"),
        (k(KeyCode::Home, N), b"\x1b[H"),
        (k(KeyCode::End, N), b"\x1b[F"),
        (k(KeyCode::PageUp, N), b"\x1b[5~"),
        (k(KeyCode::PageDown, N), b"\x1b[6~"),
        (k(KeyCode::Delete, N), b"\x1b[3~"),
        (k(KeyCode::F(1), N), b"\x1bOP"),
        (k(KeyCode::F(5), N), b"\x1b[15~"),
        (k(KeyCode::F(12), N), b"\x1b[24~"),
        (k(KeyCode::Char('a'), C), b"\x01"),
        (k(KeyCode::Char('c'), C), b"\x03"),
        (k(KeyCode::Char('p'), C), b"\x10"),
        (k(KeyCode::Char('q'), C), b"\x11"),
        (k(KeyCode::Char('z'), C), b"\x1a"),
        (k(KeyCode::Char('x'), A), b"\x1bx"),
        (k(KeyCode::Up, C), b"\x1b[1;5A"),
        (k(KeyCode::Right, S), b"\x1b[1;2C"),
        (k(KeyCode::F(5), C), b"\x1b[15;5~"),
        (k(KeyCode::F(1), S), b"\x1b[1;2P"),
    ];
    for (key, want) in cases {
        assert_eq!(
            encode(key, false).as_deref(),
            Some(*want),
            "{key:?} -> {:?}",
            encode(key, false).map(|b| String::from_utf8_lossy(&b).into_owned())
        );
    }
}

#[test]
fn application_cursor_mode_changes_arrows() {
    assert_eq!(
        encode(&k(KeyCode::Up, N), true).as_deref(),
        Some(&b"\x1bOA"[..])
    );
    assert_eq!(
        encode(&k(KeyCode::Home, N), true).as_deref(),
        Some(&b"\x1bOH"[..])
    );
    assert_eq!(
        encode(&k(KeyCode::Up, C), true).as_deref(),
        Some(&b"\x1b[1;5A"[..]),
        "modified arrows keep the CSI form"
    );
}

#[test]
fn releases_are_not_sent() {
    let mut r = k(KeyCode::Char('a'), N);
    r.kind = KeyEventKind::Release;
    assert_eq!(encode(&r, false), None);
}

#[test]
fn only_the_leave_key_is_captured_in_all_its_encodings() {
    let leave = LeaveKey::default();
    assert!(leave.matches(&k(KeyCode::Char(']'), C)));
    assert!(
        leave.matches(&k(KeyCode::Char('5'), C)),
        "0x1D decoded as Ctrl-5"
    );
    assert_eq!(
        leave.bytes(),
        vec![0x1d],
        "pressing it twice sends one Ctrl-]"
    );
    for not in [
        k(KeyCode::Esc, N),
        k(KeyCode::Char('c'), C),
        k(KeyCode::Char('q'), C),
        k(KeyCode::Char('p'), C),
        k(KeyCode::Tab, N),
        k(KeyCode::Char(']'), N),
        k(KeyCode::F(1), N),
    ] {
        assert!(!leave.matches(&not), "{not:?}");
    }
    let f12 = LeaveKey::parse("F12").unwrap();
    assert!(f12.matches(&k(KeyCode::F(12), N)));
    assert!(!f12.matches(&k(KeyCode::Char(']'), C)));
    assert_eq!(LeaveKey::parse("ctrl-\\"), Some(LeaveKey::Ctrl('\\')));
    assert_eq!(LeaveKey::parse("ctrl-xx"), None);
    assert_eq!(LeaveKey::parse("f99"), None);
    assert_eq!(LeaveKey::default().label(), "Ctrl-]");
}

#[test]
fn pastes_are_bracketed_when_the_agent_asked() {
    assert_eq!(paste("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
    assert_eq!(paste("a\nb", false), b"a\rb");
    assert_eq!(
        paste("x\x1b[201~y", true),
        b"\x1b[200~xy\x1b[201~",
        "an embedded end marker can't end the paste early"
    );
}
