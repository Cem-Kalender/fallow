//! 002 T013: replies to the queries agents and ConPTY wait for (research R-6), and the signals
//! the attention logic uses (R-4).

use fallow::session::term::{Signal, Term};

fn reply(input: &[u8]) -> Vec<u8> {
    let mut t = Term::new(24, 80, 1000);
    t.process(input);
    t.take_replies()
}

#[test]
fn queries_get_the_documented_replies() {
    let mut t = Term::new(24, 80, 1000);
    t.process(b"abc\r\nde");
    t.process(b"\x1b[6n");
    assert_eq!(
        t.take_replies(),
        b"\x1b[2;3R",
        "cursor at row 2, column 3 (1-based)"
    );
    assert_eq!(reply(b"\x1b[5n"), b"\x1b[0n");
    assert_eq!(reply(b"\x1b[c"), b"\x1b[?62;22c");
    assert_eq!(reply(b"\x1b[0c"), b"\x1b[?62;22c");
    assert_eq!(reply(b"\x1b[>c"), b"\x1b[>0;10;0c");
    let v = reply(b"\x1b[>q");
    assert!(
        v.starts_with(b"\x1bP>|fallow ") && v.ends_with(b"\x1b\\"),
        "{v:?}"
    );
    assert_eq!(
        reply(b"\x1b]10;?\x1b\\"),
        b"\x1b]10;rgb:d0d0/d0d0/d0d0\x1b\\"
    );
    assert_eq!(reply(b"\x1b]11;?\x07"), b"\x1b]11;rgb:0000/0000/0000\x1b\\");
    assert_eq!(reply(b"\x1b[?u"), b"\x1b[?0u", "kitty keyboard: no flags");
    assert!(
        reply(b"\x1b[>1u\x1b[<u").is_empty(),
        "push and pop are ignored"
    );
    assert_eq!(reply(b"\x1b[?2026$p"), b"\x1b[?2026;2$y");
    assert_eq!(reply(b"\x1b[?2026h\x1b[?2026$p"), b"\x1b[?2026;1$y");
}

#[test]
fn modes_and_signals_are_tracked() {
    let mut t = Term::new(24, 80, 1000);
    t.process(b"\x1b[?2026h");
    assert!(t.holding(std::time::Duration::from_secs(1)));
    t.process(b"\x1b[?2026l\x1b[?1004h");
    assert!(!t.holding(std::time::Duration::from_secs(1)));
    assert!(t.hooks().focus_reporting);
    t.process(b"\x07\x1b]9;done\x07\x1b]9;4;1;50\x07\x1b]777;notify;t;b\x07\x1b]2;\xe2\x9c\x8b Action Required\x07");
    t.process(b"\x1b]7501;state=waiting\x07\x1b]7501;state=working\x07");
    assert_eq!(
        t.take_signals(),
        vec![
            Signal::Bell,
            Signal::Notify,
            Signal::Working,
            Signal::Notify,
            Signal::Title("✋ Action Required".into()),
            Signal::Notify,
            Signal::Working,
        ]
    );
    assert!(t.take_signals().is_empty());
    assert!(t.take_replies().is_empty(), "signals produce no replies");
}

#[test]
fn osc52_copy_requests_from_the_agent_are_ignored() {
    let mut t = Term::new(24, 80, 1000);
    t.process(b"\x1b]52;c;aGVsbG8=\x07");
    assert!(t.take_replies().is_empty());
    assert!(t.take_signals().is_empty());
}
