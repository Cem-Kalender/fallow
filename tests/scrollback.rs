//! 002 T014: two-tier scrollback (research R-2, FR-004).

use fallow::session::scrollback::{plain, runs};
use fallow::session::term::{CELL_TIER_ROWS, Term};
use ratatui::style::{Color, Modifier};

fn lines(n: usize, from: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in from..from + n {
        v.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    v
}

fn text(row: &[(String, ratatui::style::Style)]) -> String {
    row.iter()
        .map(|(s, _)| s.as_str())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn old_rows_move_to_the_compact_tier_and_the_total_is_capped() {
    let mut t = Term::new(10, 40, 3000);
    t.process(&lines(5000, 0));
    // 5000 lines + the empty last screen row: 4991 rows have left the screen.
    assert_eq!(t.scrollback_len(), 3000, "capped at scrollback_lines");
    assert!(t.dropped() > 0, "the oldest lines were dropped first");
    let total = t.total_lines();
    assert_eq!(total, 3010);
    // Line addressing is continuous across both tiers.
    let first = t.line_text(0);
    let n: usize = first.trim_start_matches("line ").parse().unwrap();
    for v in [0, 1, 999, 1999, 2000, 2001, 2999, 3000, 3008] {
        assert_eq!(
            t.line_text(v),
            format!("line {}", n + v),
            "virtual line {v}"
        );
    }
    assert_eq!(t.line_text(3009), "", "the empty last screen row");
}

#[test]
fn the_view_spans_both_tiers() {
    let mut t = Term::new(10, 40, 3000);
    t.process(&lines(5000, 0));
    let live = t.view(0);
    assert_eq!(live.len(), 10);
    assert_eq!(text(&live[8]), "line 4999");
    // Scrolled far enough to show compact lines and cell rows in one view.
    let cells = t.screen().scrollback_len();
    assert_eq!(cells, CELL_TIER_ROWS);
    let offset = cells + 5;
    let view = t.view(offset);
    let top = t.total_lines() - 10 - offset;
    for (i, row) in view.iter().enumerate() {
        assert_eq!(text(row), t.line_text(top + i), "row {i}");
    }
    // Fully scrolled back: the oldest kept line on top.
    let oldest = t.view(usize::MAX);
    assert_eq!(text(&oldest[0]), t.line_text(0));
}

#[test]
fn compact_lines_keep_styles_and_drop_trailing_blanks() {
    let mut t = Term::new(2, 40, CELL_TIER_ROWS + 10);
    t.process(b"\x1b[1;31mred bold\x1b[0m plain     \r\n");
    t.process(&lines(CELL_TIER_ROWS + 5, 0));
    let mut found = None;
    for v in 0..t.total_lines() {
        if t.line_text(v).starts_with("red bold") {
            found = Some(v);
            break;
        }
    }
    let v = found.expect("styled line kept");
    assert!(v < 10, "it is in the compact tier");
    let view = t.view(usize::MAX);
    let row = &view[v];
    assert_eq!(text(row), "red bold plain");
    let (_, red) = row.iter().find(|(s, _)| s.contains("red")).unwrap();
    assert_eq!(red.fg, Some(Color::Indexed(1)));
    assert!(red.add_modifier.contains(Modifier::BOLD));
    // The plain helper strips escapes.
    assert_eq!(plain("\x1b[31mx\x1b[0my"), "xy");
    let r = runs("\x1b[38;2;1;2;3mrgb\x1b[48;5;200mbg");
    assert_eq!(r[0].1.fg, Some(Color::Rgb(1, 2, 3)));
    assert_eq!(r[1].1.bg, Some(Color::Indexed(200)));
}

#[test]
fn search_spans_both_tiers() {
    let mut t = Term::new(10, 40, 3000);
    t.process(&lines(4000, 0));
    let total = t.total_lines();
    let hit = t.search_up("line 1500", total).expect("found");
    assert_eq!(t.line_text(hit), "line 1500");
    assert!(hit < 2000, "found in the compact tier");
    let hit = t.search_up("LINE 3990", total);
    assert!(hit.is_none(), "uppercase makes it case-sensitive");
}

#[test]
fn alternate_screen_rows_are_not_kept() {
    let mut t = Term::new(5, 20, 3000);
    t.process(b"\x1b[?1049h");
    t.process(&lines(3000, 0));
    t.process(b"\x1b[?1049l");
    assert_eq!(t.hooks().compact.len(), 0);
}
