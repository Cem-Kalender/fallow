//! Selection (FR-008, T074): the anchor/head model shared by the editor and the single-line text
//! inputs (find box, name prompts, go to line, open project). Selection, copy, cut and paste
//! work the same way in all of them.

use std::ops::Range;

/// A selection between `anchor` and `head` (char indexes). Empty when they are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Selection {
            anchor: at,
            head: at,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// Moves the head to `to`; without `extend` the anchor follows (the selection collapses).
    pub fn move_to(&mut self, to: usize, extend: bool) {
        self.head = to;
        if !extend {
            self.anchor = to;
        }
    }
}

/// A one-line text input with a cursor and a selection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineInput {
    text: String,
    sel: Selection,
}

impl LineInput {
    pub fn new(text: &str) -> Self {
        let n = text.chars().count();
        LineInput {
            text: text.to_string(),
            sel: Selection::caret(n),
        }
    }

    /// Starts with `text` fully selected, so typing replaces it.
    pub fn selected(text: &str) -> Self {
        let mut i = LineInput::new(text);
        i.select_all();
        i
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Selection {
        self.sel
    }

    /// The cursor as a char index.
    pub fn cursor(&self) -> usize {
        self.sel.head
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map_or(self.text.len(), |(b, _)| b)
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.sel.is_empty() {
            return None;
        }
        let r = self.sel.range();
        Some(self.text[self.byte(r.start)..self.byte(r.end)].to_string())
    }

    /// Replaces the selection (or inserts at the cursor). Line breaks become spaces: this is a
    /// one-line input.
    pub fn insert(&mut self, s: &str) {
        let clean: String = s
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        let r = self.sel.range();
        let (a, b) = (self.byte(r.start), self.byte(r.end));
        self.text.replace_range(a..b, &clean);
        self.sel = Selection::caret(r.start + clean.chars().count());
    }

    pub fn backspace(&mut self) {
        if self.sel.is_empty() {
            if self.sel.head == 0 {
                return;
            }
            self.sel.anchor = self.sel.head - 1;
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if self.sel.is_empty() {
            if self.sel.head >= self.len() {
                return;
            }
            self.sel.anchor = self.sel.head + 1;
        }
        self.insert("");
    }

    /// Removes and returns the selection.
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        self.insert("");
        Some(t)
    }

    pub fn left(&mut self, extend: bool) {
        let to = if !extend && !self.sel.is_empty() {
            self.sel.range().start
        } else {
            self.sel.head.saturating_sub(1)
        };
        self.sel.move_to(to, extend);
    }

    pub fn right(&mut self, extend: bool) {
        let to = if !extend && !self.sel.is_empty() {
            self.sel.range().end
        } else {
            (self.sel.head + 1).min(self.len())
        };
        self.sel.move_to(to, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.sel.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let n = self.len();
        self.sel.move_to(n, extend);
    }

    pub fn select_all(&mut self) {
        self.sel = Selection {
            anchor: 0,
            head: self.len(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_replaces_the_selection() {
        let mut i = LineInput::selected("old");
        i.insert("n");
        i.insert("ew");
        assert_eq!(i.text(), "new");
        i.left(true);
        i.left(true);
        assert_eq!(i.selected_text().as_deref(), Some("ew"));
        assert_eq!(i.cut().as_deref(), Some("ew"));
        assert_eq!(i.text(), "n");
        i.insert("a\nb");
        assert_eq!(i.text(), "na b", "one line");
    }

    #[test]
    fn unicode_and_editing_keys() {
        let mut i = LineInput::new("héllo");
        i.home(false);
        i.right(false);
        i.delete();
        assert_eq!(i.text(), "hllo");
        i.end(false);
        i.backspace();
        assert_eq!(i.text(), "hll");
        i.home(false);
        i.backspace();
        assert_eq!(i.text(), "hll");
    }
}
