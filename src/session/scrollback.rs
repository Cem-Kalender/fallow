//! The compact scrollback tier (002 research R-2).
//!
//! vt100 keeps the newest rows of scrollback as full cells (32 bytes each). Rows that leave it
//! arrive through `Callbacks::on_scroll` and are kept here as one line of text with SGR escapes
//! ("basic formatted", each line self-contained), with trailing blanks dropped. 10,000 lines of
//! 200 columns cost about 1.5 MB here instead of 64 MB as cells.

use std::collections::VecDeque;

use ratatui::style::{Color, Modifier, Style};

/// One line of the compact tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Text with SGR escapes; self-contained (starts from default attributes).
    pub formatted: Box<str>,
    /// The line continues on the next one (soft wrap).
    pub wrapped: bool,
}

/// The oldest part of a session's scrollback.
#[derive(Debug, Default)]
pub struct Compact {
    lines: VecDeque<Line>,
    cap: usize,
    /// Lines dropped at the cap, for the "N lines kept" marker.
    pub dropped: u64,
}

impl Compact {
    pub fn new(cap: usize) -> Self {
        Compact {
            lines: VecDeque::new(),
            cap,
            dropped: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.trim();
    }

    pub fn push(&mut self, line: Line) {
        if self.cap == 0 {
            self.dropped += 1;
            return;
        }
        self.lines.push_back(line);
        self.trim();
    }

    fn trim(&mut self) {
        while self.lines.len() > self.cap {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }

    pub fn get(&self, i: usize) -> Option<&Line> {
        self.lines.get(i)
    }

    /// Bytes held by line text (for memory accounting in tests and benchmarks).
    pub fn bytes(&self) -> usize {
        self.lines.iter().map(|l| l.formatted.len()).sum()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

/// Builds a compact line from a row leaving vt100's scrollback.
pub fn capture(row: &vt100::capture::RowContents<'_>) -> Line {
    let mut state = vt100::capture::BasicFormattedCaptureState::new();
    let mut s = String::new();
    let _ = row.write_formatted_basic(&mut s, &mut state);
    // A row that is not wrapped asks for a newline before the next one.
    let wrapped = !next_needs_newline(&state);
    Line {
        formatted: trim_trailing_blanks(&s).into(),
        wrapped,
    }
}

/// `BasicFormattedCaptureState` doesn't expose `newline_pending` (set when the row is not soft
/// wrapped), and `RowContents` doesn't expose the row; its `Debug` form does. vt100 is pinned
/// (`=0.18.1`) and `tests/scrollback.rs` checks wrapped and unwrapped rows, so a change upstream
/// shows up as a test failure.
fn next_needs_newline(state: &vt100::capture::BasicFormattedCaptureState) -> bool {
    let dbg = format!("{state:?}");
    dbg.contains("newline_pending: true")
}

fn trim_trailing_blanks(s: &str) -> &str {
    // Trailing spaces may be followed by a reset escape; only plain trailing spaces are removed.
    s.trim_end_matches(' ')
}

/// The plain text of a formatted line.
pub fn plain(formatted: &str) -> String {
    vt100::capture::basic_formatted_to_plain(formatted).collect()
}

/// A formatted line as styled runs.
pub fn runs(formatted: &str) -> Vec<(String, Style)> {
    let mut out = Vec::new();
    let mut style = Style::default();
    let mut rest = formatted;
    while !rest.is_empty() {
        match rest.find('\x1b') {
            Some(0) => {
                // ESC [ params m
                let end = rest.find('m').unwrap_or(rest.len() - 1);
                let params = rest.get(2..end).unwrap_or("");
                style = apply_sgr(style, params);
                rest = &rest[end + 1..];
            }
            Some(i) => {
                out.push((rest[..i].to_string(), style));
                rest = &rest[i..];
            }
            None => {
                out.push((rest.to_string(), style));
                break;
            }
        }
    }
    out
}

/// Applies SGR parameters (`1;38;5;196`) to a style.
pub fn apply_sgr(mut style: Style, params: &str) -> Style {
    let nums: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params
            .split([';', ':'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let mut i = 0;
    while i < nums.len() {
        let n = nums[i];
        match n {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            30..=37 => style = style.fg(Color::Indexed((n - 30) as u8)),
            39 => style.fg = None,
            40..=47 => style = style.bg(Color::Indexed((n - 40) as u8)),
            49 => style.bg = None,
            90..=97 => style = style.fg(Color::Indexed((n - 90 + 8) as u8)),
            100..=107 => style = style.bg(Color::Indexed((n - 100 + 8) as u8)),
            38 | 48 => {
                let (color, used) = match nums.get(i + 1) {
                    Some(5) => (nums.get(i + 2).map(|&c| Color::Indexed(c as u8)), 2),
                    Some(2) => (
                        match (nums.get(i + 2), nums.get(i + 3), nums.get(i + 4)) {
                            (Some(&r), Some(&g), Some(&b)) => {
                                Some(Color::Rgb(r as u8, g as u8, b as u8))
                            }
                            _ => None,
                        },
                        4,
                    ),
                    _ => (None, 0),
                };
                if let Some(c) = color {
                    style = if n == 38 { style.fg(c) } else { style.bg(c) };
                }
                i += used;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// Converts a vt100 color to a ratatui color.
pub fn color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}
