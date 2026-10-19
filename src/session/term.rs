//! A session's terminal (002 research R-1, R-2, R-4, R-6).
//!
//! `vt100` parses the agent's output into a screen. Fallow adds what vt100 leaves to its user:
//! - replies to the queries agents and ConPTY wait for (cursor position, device attributes,
//!   version, colors, kitty keyboard flags, mode reports);
//! - synchronized output (`?2026`) and focus reporting (`?1004`) tracking;
//! - attention signals (bell, OSC 9/777/7501, title);
//! - the compact scrollback tier fed by `on_scroll`, and one view over both tiers.

use std::num::NonZeroU16;
use std::time::Instant;

use ratatui::style::{Modifier, Style};

use super::scrollback::{self, Compact};

/// Rows of scrollback vt100 keeps as full cells; older rows go to the compact tier (R-2).
pub const CELL_TIER_ROWS: usize = 1_000;

/// Something the attention logic (US2) and the UI care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    Bell,
    /// OSC 9 / OSC 777 notification, or OSC 7501 "waiting"/"done": the agent wants the user.
    Notify,
    /// OSC 9;4 progress or OSC 7501 "working".
    Working,
    Title(String),
}

/// The vt100 callbacks: everything Fallow handles beyond the screen.
#[derive(Debug, Default)]
pub struct Hooks {
    /// Bytes to write back to the agent (query replies).
    pub replies: Vec<u8>,
    pub signals: Vec<Signal>,
    pub compact: Compact,
    /// Synchronized output is on since this moment (`?2026h`).
    pub sync_since: Option<Instant>,
    /// The agent asked for focus reports (`?1004h`).
    pub focus_reporting: bool,
}

fn has(params: &[&[u16]], n: u16) -> bool {
    params.iter().any(|p| p.first() == Some(&n))
}

fn first(params: &[&[u16]]) -> u16 {
    params.first().and_then(|p| p.first()).copied().unwrap_or(0)
}

impl vt100::Callbacks for Hooks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.signals.push(Signal::Bell);
    }

    fn visual_bell(&mut self, _: &mut vt100::Screen) {
        self.signals.push(Signal::Bell);
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let t: String = String::from_utf8_lossy(title).chars().take(200).collect();
        self.signals.push(Signal::Title(t));
    }

    // OSC 52 copy requests from the agent are ignored: the clipboard is written only when the
    // user copies (Principle VII).

    fn on_scroll(&mut self, row: vt100::capture::RowContents<'_>, alternate_screen: bool) {
        if !alternate_screen {
            self.compact.push(scrollback::capture(&row));
        }
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        match (i1, i2, c) {
            // DSR: cursor position, and "terminal OK".
            (None, None, 'n') => match first(params) {
                6 => {
                    let (row, col) = screen.cursor_position();
                    self.replies
                        .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
                }
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                _ => {}
            },
            // DA1: a VT220-class terminal with ANSI color.
            (None, None, 'c') if first(params) == 0 => {
                self.replies.extend_from_slice(b"\x1b[?62;22c");
            }
            // DA2.
            (Some(b'>'), None, 'c') => self.replies.extend_from_slice(b"\x1b[>0;10;0c"),
            // XTVERSION.
            (Some(b'>'), None, 'q') => {
                self.replies.extend_from_slice(
                    format!("\x1bP>|fallow {}\x1b\\", env!("CARGO_PKG_VERSION")).as_bytes(),
                );
            }
            // Kitty keyboard protocol query: no flags (legacy encoding in v1). Push and pop
            // (`CSI > … u`, `CSI < u`) are accepted and ignored.
            (Some(b'?'), None, 'u') => self.replies.extend_from_slice(b"\x1b[?0u"),
            // DECRQM for synchronized output.
            (Some(b'?'), Some(b'$'), 'p') if first(params) == 2026 => {
                let state = if self.sync_since.is_some() { 1 } else { 2 };
                self.replies
                    .extend_from_slice(format!("\x1b[?2026;{state}$y").as_bytes());
            }
            (Some(b'?'), None, 'h') => {
                if has(params, 2026) {
                    self.sync_since.get_or_insert_with(Instant::now);
                }
                if has(params, 1004) {
                    self.focus_reporting = true;
                }
            }
            (Some(b'?'), None, 'l') => {
                if has(params, 2026) {
                    self.sync_since = None;
                }
                if has(params, 1004) {
                    self.focus_reporting = false;
                }
            }
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [b"10", b"?"] => self
                .replies
                .extend_from_slice(b"\x1b]10;rgb:d0d0/d0d0/d0d0\x1b\\"),
            [b"11", b"?"] => self
                .replies
                .extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
            [b"9", b"4", ..] => self.signals.push(Signal::Working),
            [b"9", ..] => self.signals.push(Signal::Notify),
            [b"777", b"notify", ..] => self.signals.push(Signal::Notify),
            [b"7501", rest @ ..] => {
                let text = rest
                    .iter()
                    .map(|p| String::from_utf8_lossy(p).to_ascii_lowercase())
                    .collect::<Vec<_>>()
                    .join(";");
                if text.contains("working") {
                    self.signals.push(Signal::Working);
                } else if text.contains("waiting") || text.contains("done") {
                    self.signals.push(Signal::Notify);
                }
            }
            _ => {}
        }
    }
}

/// One styled row of the view.
pub type Row = Vec<(String, Style)>;

/// The terminal of one session.
pub struct Term {
    parser: vt100::Parser<Hooks>,
}

impl std::fmt::Debug for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (r, c) = self.size();
        write!(f, "Term({r}x{c})")
    }
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).unwrap_or(NonZeroU16::MIN)
}

impl Term {
    /// A terminal of `rows` × `cols` keeping `scrollback_lines` lines of history in total.
    pub fn new(rows: u16, cols: u16, scrollback_lines: usize) -> Term {
        let cells = scrollback_lines.min(CELL_TIER_ROWS);
        let hooks = Hooks {
            compact: Compact::new(scrollback_lines.saturating_sub(cells)),
            ..Hooks::default()
        };
        Term {
            parser: vt100::Parser::new_with_callbacks(nz(rows), nz(cols), cells, hooks),
        }
    }

    /// Parses agent output.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Takes the replies the agent is waiting for.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.parser.callbacks_mut().replies)
    }

    pub fn take_signals(&mut self) -> Vec<Signal> {
        std::mem::take(&mut self.parser.callbacks_mut().signals)
    }

    pub fn hooks(&self) -> &Hooks {
        self.parser.callbacks()
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn size(&self) -> (u16, u16) {
        let (r, c) = self.parser.screen().size();
        (r.get(), c.get())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.size() != (rows, cols) {
            self.parser.set_size(nz(rows), nz(cols));
        }
    }

    /// Lines above the screen: compact tier plus vt100's cell tier.
    pub fn scrollback_len(&mut self) -> usize {
        self.hooks().compact.len() + self.cell_scrollback()
    }

    fn cell_scrollback(&mut self) -> usize {
        self.parser.screen().scrollback_len()
    }

    /// Lines dropped at the scrollback cap.
    pub fn dropped(&self) -> u64 {
        self.hooks().compact.dropped
    }

    /// Synchronized output has been on for less than `limit`: hold redraws.
    pub fn holding(&self, limit: std::time::Duration) -> bool {
        self.hooks()
            .sync_since
            .is_some_and(|at| at.elapsed() < limit)
    }

    /// The rows to show when scrolled back `offset` lines from the live screen (0 = live).
    /// Rows come from the compact tier first, then vt100's cell tier and screen.
    pub fn view(&mut self, offset: usize) -> Vec<Row> {
        let (rows, cols) = self.size();
        let h = rows as usize;
        let c = self.hooks().compact.len();
        let b = self.cell_scrollback();
        let offset = offset.min(c + b);
        let top = c + b - offset;
        let mut out = Vec::with_capacity(h);
        for v in top..(c.min(top + h)) {
            let line = self
                .hooks()
                .compact
                .get(v)
                .map(|l| scrollback::runs(&l.formatted));
            out.push(line.unwrap_or_default());
        }
        if out.len() < h {
            let o = (c + b).saturating_sub(top.max(c)).min(b);
            let screen = self.parser.screen_mut();
            screen.set_scrollback(o);
            let need = h - out.len();
            for r in 0..need as u16 {
                out.push(row_runs(screen, r, cols));
            }
            screen.set_scrollback(0);
        }
        out
    }

    /// The plain text of virtual line `v` (0 = oldest kept line) for selection and search.
    pub fn line_text(&mut self, v: usize) -> String {
        let c = self.hooks().compact.len();
        if v < c {
            return self
                .hooks()
                .compact
                .get(v)
                .map(|l| scrollback::plain(&l.formatted))
                .unwrap_or_default();
        }
        let b = self.cell_scrollback();
        let (rows, cols) = self.size();
        let o = (c + b).saturating_sub(v).min(b);
        let idx = v - (c + b - o);
        let screen = self.parser.screen_mut();
        screen.set_scrollback(o);
        let text = if idx < rows as usize {
            screen
                .rows(0, cols)
                .nth(idx)
                .unwrap_or_default()
                .trim_end()
                .to_string()
        } else {
            String::new()
        };
        screen.set_scrollback(0);
        text
    }

    /// Total virtual lines: scrollback plus the screen.
    pub fn total_lines(&mut self) -> usize {
        self.scrollback_len() + self.size().0 as usize
    }

    /// Searches upward from line `from` (exclusive) for `query` (case-insensitive unless it has
    /// an uppercase letter). Returns the virtual line index.
    pub fn search_up(&mut self, query: &str, from: usize) -> Option<usize> {
        let smart = !query.chars().any(char::is_uppercase);
        let q = if smart {
            query.to_lowercase()
        } else {
            query.to_string()
        };
        (0..from.min(self.total_lines())).rev().find(|&v| {
            let t = self.line_text(v);
            if smart {
                t.to_lowercase().contains(&q)
            } else {
                t.contains(&q)
            }
        })
    }
}

/// One screen row as styled runs.
fn row_runs(screen: &vt100::Screen, row: u16, cols: u16) -> Row {
    let mut out: Row = Vec::new();
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let mut style = Style::default();
        if let Some(c) = scrollback::color(cell.fgcolor()) {
            style = style.fg(c);
        }
        if let Some(c) = scrollback::color(cell.bgcolor()) {
            style = style.bg(c);
        }
        if cell.bold() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if cell.dim() {
            style = style.add_modifier(Modifier::DIM);
        }
        if cell.italic() {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if cell.underline() {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if cell.inverse() {
            style = style.add_modifier(Modifier::REVERSED);
        }
        let text = if cell.has_contents() {
            cell.contents()
        } else {
            " "
        };
        match out.last_mut() {
            Some((s, st)) if *st == style => s.push_str(text),
            _ => out.push((text.to_string(), style)),
        }
    }
    out
}
