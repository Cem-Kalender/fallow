//! 002 T016: transcript replay (SC-002). Each `tests/fixtures/transcripts/*.bin` is a byte
//! stream an agent sent to its terminal; replaying it through Fallow's terminal must give the
//! screen in the matching `.golden` file (10 rows × 40 columns, trailing blanks trimmed).
//!
//! The fixtures here are synthetic and exercise the features agent CLIs rely on (research R-1):
//! alternate screen, 24-bit and 256 colors, cursor addressing, scroll regions, line erase,
//! synchronized output and wide characters. Transcripts of real agents can be added with
//! `cargo xtask record-transcript -- <agent>`; review and scrub them before committing, since
//! they may contain account names and paths.

use fallow::session::term::Term;

fn screen(bytes: &[u8]) -> String {
    let mut t = Term::new(10, 40, 1000);
    t.process(bytes);
    let mut out = String::new();
    for row in t.view(0) {
        let line: String = row.iter().map(|(s, _)| s.as_str()).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn transcripts_replay_to_their_golden_screens() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcripts");
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("bin") {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap();
        let golden = std::fs::read_to_string(p.with_extension("golden")).unwrap();
        let golden = golden.replace("\r\n", "\n");
        assert_eq!(screen(&bytes), golden, "{}", p.display());
        n += 1;
    }
    assert!(n >= 2, "no transcripts found in {}", dir.display());
}

#[test]
fn colors_and_wide_characters_survive() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/transcripts/fullscreen.bin"),
    )
    .unwrap();
    let mut t = Term::new(10, 40, 1000);
    t.process(&bytes);
    assert!(t.screen().alternate_screen());
    let rows = t.view(0);
    let (_, style) = rows[0].iter().find(|(s, _)| s.contains("Agent")).unwrap();
    assert_eq!(style.fg, Some(ratatui::style::Color::Rgb(217, 119, 87)));
    let (_, gray) = rows[2].iter().find(|(s, _)| s.contains("model")).unwrap();
    assert_eq!(gray.fg, Some(ratatui::style::Color::Indexed(244)));
    let text: String = rows[2].iter().map(|(s, _)| s.as_str()).collect();
    assert!(text.contains("中文"), "{text}");
}
