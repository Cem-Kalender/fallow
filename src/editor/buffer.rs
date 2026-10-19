//! An open file (data-model.md "Buffer", FR-003, FR-006, T066): a ropey rope, a capped undo log,
//! the cursor and selection, and the disk-change state machine.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ropey::{Rope, RopeSlice};

use super::selection::Selection;
use super::syntax::{LanguageId, SyntaxState};

/// "Undo is an edit log capped at 10 MB per buffer" (R-8).
pub const UNDO_CAP_BYTES: usize = 10 * 1024 * 1024;
/// Binary detection looks for a NUL byte in this many leading bytes.
pub const BINARY_SNIFF: usize = 8 * 1024;
/// Bookkeeping cost of one undo entry, counted against the cap with the text it holds.
const EDIT_OVERHEAD: usize = 48;

/// Where the buffer stands relative to its file (FR-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    InSync,
    /// The file changed while the buffer had unsaved edits; the user chooses.
    ChangedOnDisk,
    /// The file was deleted or renamed away.
    DeletedOnDisk,
}

/// Why a file was not opened as text.
#[derive(Debug)]
pub enum OpenError {
    Io(io::Error),
    Binary,
    NotUtf8,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Io(e) => write!(f, "{e}"),
            OpenError::Binary => write!(f, "binary file, not opened as text"),
            OpenError::NotUtf8 => write!(f, "not UTF-8 text, not opened"),
        }
    }
}

/// What a disk check did (FR-006 transitions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskEvent {
    /// Nothing to do: the content on disk is what the buffer last loaded or saved.
    Unchanged,
    /// `dirty = false` and the file changed: reloaded.
    Reloaded,
    /// `dirty = true` and the file changed: now `ChangedOnDisk`, ask "keep mine / load disk".
    Conflict,
    /// The file is gone: now `DeletedOnDisk` and dirty.
    Deleted,
}

/// One replacement: `removed` at `at` became `inserted`.
#[derive(Debug, Clone)]
struct Edit {
    at: usize,
    removed: String,
    inserted: String,
}

/// One undo step: edits applied in order, with the selection before and after.
#[derive(Debug, Clone)]
struct Change {
    id: u64,
    edits: Vec<Edit>,
    before: Selection,
    after: Selection,
}

impl Change {
    fn cost(&self) -> usize {
        self.edits
            .iter()
            .map(|e| e.removed.len() + e.inserted.len() + EDIT_OVERHEAD)
            .sum()
    }
}

/// The capped undo/redo log. The oldest entries are dropped first.
#[derive(Debug, Clone)]
pub struct EditLog {
    undo: VecDeque<Change>,
    redo: Vec<Change>,
    bytes: usize,
    cap: usize,
    next_id: u64,
    /// The version the buffer is at when every remaining undo step is undone.
    floor: u64,
    /// Typing may extend the newest change while this is set.
    open: bool,
}

impl EditLog {
    pub fn new(cap: usize) -> Self {
        EditLog {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            cap,
            next_id: 1,
            floor: 0,
            open: false,
        }
    }

    /// Bytes held by undo and redo entries.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    fn version(&self) -> u64 {
        self.undo.back().map_or(self.floor, |c| c.id)
    }

    fn seal(&mut self) {
        self.open = false;
    }

    fn push(&mut self, mut c: Change) {
        for r in self.redo.drain(..) {
            self.bytes -= r.cost();
        }
        c.id = self.next_id;
        self.next_id += 1;
        self.bytes += c.cost();
        self.undo.push_back(c);
        self.trim();
    }

    fn trim(&mut self) {
        while self.bytes > self.cap {
            match self.undo.pop_front() {
                Some(old) => {
                    self.bytes -= old.cost();
                    self.floor = old.id;
                }
                None => break,
            }
        }
    }
}

/// An open file.
#[derive(Debug)]
pub struct Buffer {
    pub path: Option<PathBuf>,
    text: Rope,
    pub read_only: bool,
    pub language: Option<LanguageId>,
    pub syntax: Option<SyntaxState>,
    undo: EditLog,
    saved_version: u64,
    /// Set when the file disappeared from disk: the content exists only here (FR-006).
    force_dirty: bool,
    pub disk_state: DiskState,
    /// Hash of the content last read from or written to disk.
    disk_hash: Option<u64>,
    pub sel: Selection,
    /// Display column kept across vertical moves.
    want_col: Option<usize>,
    /// First visible line and column (viewport).
    pub top: usize,
    pub left: usize,
    /// Incremented on every change to the text.
    pub rev: u64,
    /// The line ending inserted by Enter: the file's own.
    eol: &'static str,
    /// Indent with tabs (the file already does) or spaces.
    tabs: bool,
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

impl Buffer {
    /// An unnamed buffer holding `text` (tests, scratch).
    pub fn from_text(text: &str) -> Self {
        let mut b = Buffer {
            path: None,
            text: Rope::from_str(text),
            read_only: false,
            language: None,
            syntax: None,
            undo: EditLog::new(UNDO_CAP_BYTES),
            saved_version: 0,
            force_dirty: false,
            disk_state: DiskState::InSync,
            disk_hash: None,
            sel: Selection::default(),
            want_col: None,
            top: 0,
            left: 0,
            rev: 0,
            eol: "\n",
            tabs: false,
        };
        b.detect_style();
        b
    }

    /// Opens `path`. Files over `read_only_bytes` open read-only; binary and non-UTF-8 files are
    /// refused (spec edge case "large or binary file").
    pub fn open(path: &Path, read_only_bytes: u64) -> Result<Buffer, OpenError> {
        let bytes = std::fs::read(path).map_err(OpenError::Io)?;
        let text = decode(&bytes)?;
        let mut b = Buffer::from_text(text);
        b.path = Some(path.to_path_buf());
        b.read_only = bytes.len() as u64 > read_only_bytes;
        b.language = LanguageId::from_path(path);
        b.disk_hash = Some(hash_bytes(&bytes));
        Ok(b)
    }

    fn detect_style(&mut self) {
        self.eol = "\n";
        for line in self.text.lines().take(1000) {
            let n = line.len_chars();
            if n >= 2 && line.char(n - 2) == '\r' && line.char(n - 1) == '\n' {
                self.eol = "\r\n";
                break;
            }
            if n >= 1 && line.char(n - 1) == '\n' {
                break;
            }
        }
        self.tabs = self
            .text
            .lines()
            .take(1000)
            .any(|l| l.len_chars() > 0 && l.char(0) == '\t');
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    pub fn dirty(&self) -> bool {
        self.force_dirty || self.undo.version() != self.saved_version
    }

    pub fn undo_log(&self) -> &EditLog {
        &self.undo
    }

    /// Replaces the undo cap (tests).
    pub fn set_undo_cap(&mut self, cap: usize) {
        self.undo.cap = cap;
        self.undo.trim();
    }

    // ----- positions -----

    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }

    pub fn cursor(&self) -> usize {
        self.sel.head
    }

    /// The cursor as a 0-based (line, char column).
    pub fn line_col(&self) -> (usize, usize) {
        let c = self.sel.head.min(self.text.len_chars());
        let line = self.text.char_to_line(c);
        (line, c - self.text.line_to_char(line))
    }

    /// Chars in `line` without its line ending.
    pub fn line_len(&self, line: usize) -> usize {
        if line >= self.text.len_lines() {
            return 0;
        }
        content_len(self.text.line(line))
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.text.line_to_char(line.min(self.text.len_lines()))
    }

    fn pos(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.text.len_lines().saturating_sub(1));
        self.line_start(line) + col.min(self.line_len(line))
    }

    /// The selected range, if not empty.
    pub fn selection(&self) -> Option<Range<usize>> {
        (!self.sel.is_empty()).then(|| self.sel.range())
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|r| self.text.slice(r).to_string())
    }

    // ----- editing -----

    /// The one place the rope changes. Keeps the syntax tree in step.
    fn replace_raw(&mut self, range: Range<usize>, s: &str) -> Edit {
        let removed = self.text.slice(range.clone()).to_string();
        if let Some(syn) = &mut self.syntax {
            syn.edit(&self.text, range.clone(), s);
        }
        self.text.remove(range.clone());
        self.text.insert(range.start, s);
        self.rev += 1;
        Edit {
            at: range.start,
            removed,
            inserted: s.to_string(),
        }
    }

    /// Applies `edits` (ranges in the coordinates current at each step) as one undo step.
    fn change(&mut self, edits: Vec<(Range<usize>, String)>, after: Selection, merge: bool) {
        if self.read_only {
            return;
        }
        let before = self.sel;
        let mut done = Vec::with_capacity(edits.len());
        for (r, s) in edits {
            done.push(self.replace_raw(r, &s));
        }
        self.sel = after;
        self.want_col = None;
        if merge && self.undo.open && self.undo.redo.is_empty() {
            if let Some(last) = self.undo.undo.back_mut() {
                let extra: usize = done
                    .iter()
                    .map(|e| e.removed.len() + e.inserted.len() + EDIT_OVERHEAD)
                    .sum();
                last.edits.extend(done);
                last.after = after;
                self.undo.bytes += extra;
                self.undo.trim();
                return;
            }
        }
        self.undo.push(Change {
            id: 0,
            edits: done,
            before,
            after,
        });
        self.undo.open = merge;
    }

    /// Replaces the selection (or inserts at the cursor) with `s`, literally. Used for paste.
    pub fn insert(&mut self, s: &str) {
        let r = self.sel.range();
        let after = Selection::caret(r.start + s.chars().count());
        self.change(vec![(r, s.to_string())], after, false);
    }

    /// Types one character. Consecutive typing is one undo step per word: a space or tab ends
    /// the current step.
    pub fn type_char(&mut self, c: char) {
        let r = self.sel.range();
        let merge = r.is_empty();
        let after = Selection::caret(r.start + 1);
        self.change(vec![(r, c.to_string())], after, merge);
        if c.is_whitespace() {
            self.undo.seal();
        }
    }

    /// Enter: a line break in the file's style, keeping the current line's indentation.
    pub fn newline(&mut self) {
        let (line, _) = self.line_col();
        let indent: String = self
            .text
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let s = format!("{}{indent}", self.eol);
        self.insert(&s);
        self.undo.seal();
    }

    pub fn backspace(&mut self) {
        if let Some(r) = self.selection() {
            return self.change(
                vec![(r.clone(), String::new())],
                Selection::caret(r.start),
                false,
            );
        }
        let c = self.sel.head;
        if c == 0 {
            return;
        }
        let from = self.step_left(c);
        self.change(
            vec![(from..c, String::new())],
            Selection::caret(from),
            false,
        );
    }

    pub fn delete_forward(&mut self) {
        if let Some(r) = self.selection() {
            return self.change(
                vec![(r.clone(), String::new())],
                Selection::caret(r.start),
                false,
            );
        }
        let c = self.sel.head;
        if c >= self.text.len_chars() {
            return;
        }
        let to = self.step_right(c);
        self.change(vec![(c..to, String::new())], Selection::caret(c), false);
    }

    /// Removes the selection and returns it.
    pub fn cut(&mut self) -> Option<String> {
        let r = self.selection()?;
        let t = self.text.slice(r.clone()).to_string();
        self.change(
            vec![(r.clone(), String::new())],
            Selection::caret(r.start),
            false,
        );
        Some(t)
    }

    fn indent_unit(&self, tab_width: usize) -> String {
        if self.tabs {
            "\t".into()
        } else {
            " ".repeat(tab_width.max(1))
        }
    }

    fn selected_lines(&self) -> Range<usize> {
        let r = self.sel.range();
        let first = self.text.char_to_line(r.start);
        let mut last = self.text.char_to_line(r.end);
        if last > first && r.end == self.text.line_to_char(last) {
            last -= 1;
        }
        first..last + 1
    }

    /// Tab: indents the selected lines, or inserts spaces to the next tab stop.
    pub fn indent(&mut self, tab_width: usize) {
        let multi = self.selection().is_some_and(|_| {
            let l = self.selected_lines();
            l.end - l.start > 1 || self.sel.range().len() == self.line_len(l.start)
        });
        if !multi {
            let s = if self.tabs {
                "\t".to_string()
            } else {
                let (_, col) = self.line_col();
                let w = tab_width.max(1);
                " ".repeat(w - col % w)
            };
            return self.insert(&s);
        }
        let unit = self.indent_unit(tab_width);
        let lines = self.selected_lines();
        let n = unit.chars().count();
        let mut edits = Vec::new();
        let mut added = 0;
        for (k, line) in lines.clone().enumerate() {
            if self.line_len(line) == 0 {
                continue;
            }
            let at = self.line_start(line) + added;
            edits.push((at..at, unit.clone()));
            added += n;
            let _ = k;
        }
        let (a, h) = (self.sel.anchor, self.sel.head);
        let first = self.line_start(lines.start);
        let shift = |p: usize| if p > first { p + added } else { p };
        let after = Selection {
            anchor: shift(a),
            head: shift(h),
        };
        self.change(edits, after, false);
    }

    /// Shift-Tab: removes one indent level from the selected lines (or the cursor's line).
    pub fn outdent(&mut self, tab_width: usize) {
        let lines = self.selected_lines();
        let mut edits = Vec::new();
        let mut removed_before = Vec::new();
        let mut removed = 0;
        for line in lines {
            let start = self.line_start(line);
            let l = self.text.line(line);
            let n = if l.len_chars() > 0 && l.char(0) == '\t' {
                1
            } else {
                l.chars()
                    .take(tab_width.max(1))
                    .take_while(|c| *c == ' ')
                    .count()
            };
            if n > 0 {
                let at = start - removed;
                edits.push((at..at + n, String::new()));
                removed_before.push((start, n));
                removed += n;
            }
        }
        if edits.is_empty() {
            return;
        }
        let map = |p: usize| {
            let mut out = p;
            for (start, n) in &removed_before {
                if p >= start + n {
                    out -= n;
                } else if p > *start {
                    out -= p - start;
                }
            }
            out
        };
        let after = Selection {
            anchor: map(self.sel.anchor),
            head: map(self.sel.head),
        };
        self.change(edits, after, false);
    }

    pub fn undo(&mut self) -> bool {
        self.undo.seal();
        let Some(c) = self.undo.undo.pop_back() else {
            return false;
        };
        for e in c.edits.iter().rev() {
            let end = e.at + e.inserted.chars().count();
            self.replace_raw(e.at..end, &e.removed);
        }
        self.sel = c.before;
        self.want_col = None;
        self.undo.redo.push(c);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.undo.seal();
        let Some(c) = self.undo.redo.pop() else {
            return false;
        };
        for e in &c.edits {
            let end = e.at + e.removed.chars().count();
            self.replace_raw(e.at..end, &e.inserted);
        }
        self.sel = c.after;
        self.want_col = None;
        self.undo.undo.push_back(c);
        true
    }

    // ----- movement -----

    /// One step left, treating CRLF as one character.
    fn step_left(&self, c: usize) -> usize {
        if c >= 2 && self.text.char(c - 1) == '\n' && self.text.char(c - 2) == '\r' {
            c - 2
        } else {
            c.saturating_sub(1)
        }
    }

    fn step_right(&self, c: usize) -> usize {
        let n = self.text.len_chars();
        if c + 1 < n && self.text.char(c) == '\r' && self.text.char(c + 1) == '\n' {
            c + 2
        } else {
            (c + 1).min(n)
        }
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.undo.seal();
        self.sel.move_to(to, extend);
    }

    pub fn left(&mut self, extend: bool) {
        let to = match self.selection() {
            Some(r) if !extend => r.start,
            _ => self.step_left(self.sel.head),
        };
        self.want_col = None;
        self.move_to(to, extend);
    }

    pub fn right(&mut self, extend: bool) {
        let to = match self.selection() {
            Some(r) if !extend => r.end,
            _ => self.step_right(self.sel.head),
        };
        self.want_col = None;
        self.move_to(to, extend);
    }

    /// Moves `by` lines, keeping the display column.
    pub fn vertical(&mut self, by: isize, extend: bool, tab_width: usize) {
        let (line, col) = self.line_col();
        let want = self
            .want_col
            .unwrap_or_else(|| display_col(self.text.line(line), col, tab_width));
        let last = self.len_lines().saturating_sub(1) as isize;
        let target = (line as isize + by).clamp(0, last) as usize;
        let to = if by < 0 && line == 0 {
            0
        } else if by > 0 && line as isize == last {
            self.text.len_chars()
        } else {
            let c =
                char_at_display(self.text.line(target), want, tab_width).min(self.line_len(target));
            self.line_start(target) + c
        };
        self.move_to(to, extend);
        self.want_col = Some(want);
    }

    /// Home: first non-blank character, then column 0.
    pub fn home(&mut self, extend: bool) {
        let (line, col) = self.line_col();
        let indent = self
            .text
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count()
            .min(self.line_len(line));
        let to = if col == indent { 0 } else { indent };
        self.want_col = None;
        self.move_to(self.line_start(line) + to, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let (line, _) = self.line_col();
        self.want_col = None;
        self.move_to(self.line_start(line) + self.line_len(line), extend);
    }

    pub fn doc_start(&mut self, extend: bool) {
        self.want_col = None;
        self.move_to(0, extend);
    }

    pub fn doc_end(&mut self, extend: bool) {
        self.want_col = None;
        self.move_to(self.text.len_chars(), extend);
    }

    pub fn select_all(&mut self) {
        self.undo.seal();
        self.sel = Selection {
            anchor: 0,
            head: self.text.len_chars(),
        };
    }

    /// Puts the cursor at a 0-based line and char column (clamped).
    pub fn set_cursor(&mut self, line: usize, col: usize, extend: bool) {
        self.want_col = None;
        let to = self.pos(line, col);
        self.move_to(to, extend);
    }

    /// Puts the cursor at a 0-based line and display column (mouse).
    pub fn set_cursor_display(&mut self, line: usize, dcol: usize, tab_width: usize, extend: bool) {
        let line = line.min(self.len_lines().saturating_sub(1));
        let c = char_at_display(self.text.line(line), dcol, tab_width).min(self.line_len(line));
        self.set_cursor(line, c, extend);
    }

    /// Go to line (FR-009): 1-based line and optional 1-based column. Returns an error message
    /// and moves nothing when the input is out of range.
    pub fn goto(&mut self, line: usize, col: Option<usize>) -> Result<(), String> {
        let lines = self.len_lines();
        if line == 0 || line > lines {
            return Err(format!("Line must be between 1 and {lines}"));
        }
        if col == Some(0) {
            return Err("Column starts at 1".into());
        }
        self.set_cursor(line - 1, col.map_or(0, |c| c - 1), false);
        Ok(())
    }

    /// Scrolls so the cursor is visible in a `rows` × `cols` text area.
    pub fn scroll_to_cursor(&mut self, rows: usize, cols: usize, tab_width: usize) {
        let (line, col) = self.line_col();
        let rows = rows.max(1);
        if line < self.top {
            self.top = line;
        } else if line >= self.top + rows {
            self.top = line + 1 - rows;
        }
        let d = display_col(self.text.line(line), col, tab_width);
        let cols = cols.max(1);
        if d < self.left {
            self.left = d;
        } else if d >= self.left + cols {
            self.left = d + 1 - cols;
        }
    }

    /// Puts the cursor's line near the middle of the view (go to line, search results).
    pub fn center_on_cursor(&mut self, rows: usize) {
        let (line, _) = self.line_col();
        self.top = line.saturating_sub(rows / 3);
    }

    // ----- find in this file (FR-005) -----

    /// Selects the next (or previous) match of `query`, wrapping around. Case-insensitive
    /// unless the query has an uppercase letter.
    pub fn find(&mut self, query: &str, forward: bool) -> bool {
        if query.is_empty() {
            return false;
        }
        let smart = !query.chars().any(char::is_uppercase);
        let norm = |s: &str| {
            if smart {
                s.to_lowercase()
            } else {
                s.to_string()
            }
        };
        let q = norm(query);
        let qlen = query.chars().count();
        let lines = self.len_lines();
        let r = self.sel.range();
        let (start_line, _) = (self.text.char_to_line(r.start), 0);
        // Matches within one line, as char ranges.
        let line_matches = |line: usize| -> Vec<usize> {
            let text = self.text.line(line).to_string();
            let hay = norm(&text);
            if hay.len() != text.len() {
                // Lowercasing changed byte lengths: fall back to exact matching.
                return text
                    .match_indices(query)
                    .map(|(b, _)| text[..b].chars().count())
                    .collect();
            }
            hay.match_indices(&q)
                .map(|(b, _)| text[..b].chars().count())
                .collect()
        };
        for step in 0..=lines {
            let line = if forward {
                (start_line + step) % lines
            } else {
                (start_line + lines - step % lines) % lines
            };
            let base = self.line_start(line);
            let mut hits: Vec<usize> = line_matches(line).into_iter().map(|c| base + c).collect();
            if !forward {
                hits.reverse();
            }
            let pick = hits.into_iter().find(|&h| {
                if step > 0 {
                    true
                } else if !forward {
                    h < r.start
                } else if r.is_empty() {
                    // Nothing selected: a match right at the cursor counts.
                    h >= r.start
                } else {
                    // Skip the match that is selected now.
                    h > r.start
                }
            });
            if let Some(h) = pick {
                self.undo.seal();
                self.sel = Selection {
                    anchor: h,
                    head: h + qlen,
                };
                self.want_col = None;
                return true;
            }
        }
        // The only match may be the current selection itself.
        let cur = self.selected_text().map(|t| norm(&t));
        cur.as_deref() == Some(q.as_str())
    }

    // ----- disk -----

    /// Writes the buffer to its file (FR-003).
    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Err(io::Error::other("buffer has no file"));
        };
        if self.read_only {
            return Err(io::Error::other("read-only (file over the size threshold)"));
        }
        let mut bytes = Vec::with_capacity(self.text.len_bytes());
        for chunk in self.text.chunks() {
            bytes.extend_from_slice(chunk.as_bytes());
        }
        std::fs::write(&path, &bytes)?;
        self.disk_hash = Some(hash_bytes(&bytes));
        self.undo.seal();
        self.saved_version = self.undo.version();
        self.force_dirty = false;
        self.disk_state = DiskState::InSync;
        Ok(())
    }

    /// Re-reads the file after a change notification and applies the FR-006 transitions.
    /// Nothing is ever written here.
    pub fn check_disk(&mut self) -> DiskEvent {
        let Some(path) = self.path.clone() else {
            return DiskEvent::Unchanged;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if self.disk_state == DiskState::DeletedOnDisk {
                    return DiskEvent::Unchanged;
                }
                self.disk_state = DiskState::DeletedOnDisk;
                self.force_dirty = true;
                return DiskEvent::Deleted;
            }
            // Unreadable for now (locked mid-write on Windows): try again on the next event.
            Err(_) => return DiskEvent::Unchanged,
        };
        let h = hash_bytes(&bytes);
        if Some(h) == self.disk_hash && self.disk_state != DiskState::DeletedOnDisk {
            return DiskEvent::Unchanged;
        }
        if self.disk_state == DiskState::DeletedOnDisk {
            // The file came back. Same handling as a change, and the buffer is no longer
            // forced dirty if its content equals the file.
            self.force_dirty = false;
            self.disk_state = DiskState::InSync;
            if Some(h) == self.disk_hash && !self.dirty() {
                return DiskEvent::Unchanged;
            }
        }
        if !self.dirty() {
            match decode(&bytes) {
                Ok(text) => {
                    self.load_text(text, h);
                    DiskEvent::Reloaded
                }
                Err(_) => {
                    self.disk_state = DiskState::ChangedOnDisk;
                    DiskEvent::Conflict
                }
            }
        } else {
            self.disk_state = DiskState::ChangedOnDisk;
            DiskEvent::Conflict
        }
    }

    /// "keep mine": the buffer stays as it is and unsaved; the next save overwrites the file.
    pub fn keep_mine(&mut self) {
        self.disk_state = DiskState::InSync;
        self.force_dirty = true;
        if let Some(p) = &self.path {
            if let Ok(b) = std::fs::read(p) {
                self.disk_hash = Some(hash_bytes(&b));
            }
        }
    }

    /// "load disk": replaces the buffer with the file. The replaced text stays undoable.
    pub fn load_disk(&mut self) -> Result<(), OpenError> {
        let Some(p) = self.path.clone() else {
            return Ok(());
        };
        let bytes = std::fs::read(&p).map_err(OpenError::Io)?;
        let text = decode(&bytes)?;
        self.load_text(text, hash_bytes(&bytes));
        Ok(())
    }

    /// Replaces the whole text with the disk version as one undoable change and marks it saved.
    fn load_text(&mut self, text: &str, hash: u64) {
        let (line, col) = self.line_col();
        let ro = self.read_only;
        self.read_only = false;
        let all = 0..self.text.len_chars();
        self.change(vec![(all, text.to_string())], Selection::caret(0), false);
        self.read_only = ro;
        self.undo.seal();
        self.saved_version = self.undo.version();
        self.force_dirty = false;
        self.disk_hash = Some(hash);
        self.disk_state = DiskState::InSync;
        if let Some(s) = &mut self.syntax {
            s.reset();
        }
        self.set_cursor(line, col, false);
    }

    /// The file was renamed from the Files pane: follow it.
    pub fn renamed(&mut self, to: PathBuf) {
        self.language = LanguageId::from_path(&to);
        if self.syntax.as_ref().map(|s| s.language) != self.language {
            self.syntax = None;
        }
        self.path = Some(to);
    }

    /// The file was deleted from the Files pane: the buffer becomes unsaved (FR-006, FR-068).
    pub fn deleted(&mut self) {
        self.disk_state = DiskState::DeletedOnDisk;
        self.force_dirty = true;
    }
}

fn decode(bytes: &[u8]) -> Result<&str, OpenError> {
    if bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0) {
        return Err(OpenError::Binary);
    }
    std::str::from_utf8(bytes).map_err(|_| OpenError::NotUtf8)
}

/// Chars in a line slice without its line ending.
pub fn content_len(line: RopeSlice<'_>) -> usize {
    let mut n = line.len_chars();
    if n > 0 && line.char(n - 1) == '\n' {
        n -= 1;
    }
    if n > 0 && line.char(n - 1) == '\r' {
        n -= 1;
    }
    n
}

/// Display width of one char at display column `at`.
pub fn char_width(c: char, at: usize, tab_width: usize) -> usize {
    if c == '\t' {
        let w = tab_width.max(1);
        w - at % w
    } else {
        unicode_width::UnicodeWidthChar::width(c)
            .unwrap_or(0)
            .max(1)
    }
}

/// The display column of char `col` in `line`.
pub fn display_col(line: RopeSlice<'_>, col: usize, tab_width: usize) -> usize {
    let mut d = 0;
    for c in line.chars().take(col) {
        d += char_width(c, d, tab_width);
    }
    d
}

/// The char column at display column `dcol` in `line`.
pub fn char_at_display(line: RopeSlice<'_>, dcol: usize, tab_width: usize) -> usize {
    let mut d = 0;
    for (i, c) in line.chars().enumerate() {
        if c == '\n' || c == '\r' {
            return i;
        }
        let w = char_width(c, d, tab_width);
        if d + w > dcol {
            return i;
        }
        d += w;
    }
    line.len_chars()
}
