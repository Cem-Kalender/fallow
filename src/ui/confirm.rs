//! The shared confirmation dialog (FR-032, FR-068, T069; contracts/ui-commands.md
//! "Confirmation dialogs").
//!
//! Default focus is **Cancel**. `y`, or `Enter` on Confirm, confirms; `Esc`, `n` and `q`
//! cancel. There is no "don't ask again". Confirming yields a [`ConfirmedToken`] naming what
//! was confirmed; only this module can construct one, so any action that requires a token is
//! unreachable without the dialog.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// Proof that the user confirmed an action on `subject`.
#[derive(Debug, PartialEq, Eq)]
pub struct ConfirmedToken {
    subject: String,
}

impl ConfirmedToken {
    /// What was confirmed (for a file delete: the full path).
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Cancel,
    Confirm,
}

/// A pending confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    /// What the dialog is about; carried into the token.
    pub subject: String,
    pub body: Vec<String>,
    /// The confirm button's label ("Delete", "Quit", …).
    pub confirm_label: String,
    pub cancel_label: String,
    pub focus: Button,
}

/// The result of a key press in a dialog.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Cancelled,
    Confirmed(ConfirmedToken),
}

impl Dialog {
    pub fn new(title: &str, subject: &str, body: Vec<String>, confirm_label: &str) -> Dialog {
        Dialog {
            title: title.to_string(),
            subject: subject.to_string(),
            body,
            confirm_label: confirm_label.to_string(),
            cancel_label: "Cancel".to_string(),
            focus: Button::Cancel,
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if key.kind == KeyEventKind::Release {
            return Outcome::Pending;
        }
        match key.code {
            KeyCode::Char('y' | 'Y') => self.confirm(),
            KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q' | 'Q') => Outcome::Cancelled,
            KeyCode::Enter => match self.focus {
                Button::Confirm => self.confirm(),
                Button::Cancel => Outcome::Cancelled,
            },
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Button::Cancel => Button::Confirm,
                    Button::Confirm => Button::Cancel,
                };
                Outcome::Pending
            }
            _ => Outcome::Pending,
        }
    }

    /// A click on a button (mouse, FR-063).
    pub fn click(&mut self, button: Button) -> Outcome {
        match button {
            Button::Confirm => self.confirm(),
            Button::Cancel => Outcome::Cancelled,
        }
    }

    fn confirm(&self) -> Outcome {
        Outcome::Confirmed(ConfirmedToken {
            subject: self.subject.clone(),
        })
    }
}

/// Where the dialog and its buttons are drawn.
pub fn geometry(area: Rect, d: &Dialog) -> (Rect, Rect, Rect) {
    let width = 70.min(area.width.saturating_sub(2)).max(20);
    let inner_w = width.saturating_sub(4) as usize;
    let lines: usize = d
        .body
        .iter()
        .map(|l| (l.chars().count() / inner_w.max(1)) + 1)
        .sum();
    let height = (lines as u16 + 5).min(area.height.saturating_sub(2)).max(5);
    let [v] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(v);
    let row = r.bottom().saturating_sub(2);
    let cancel_w = d.cancel_label.chars().count() as u16 + 4;
    let confirm_w = d.confirm_label.chars().count() as u16 + 4;
    let cancel = Rect::new(r.x + 2, row, cancel_w, 1);
    let confirm = Rect::new(r.x + 2 + cancel_w + 2, row, confirm_w, 1);
    (r, cancel, confirm)
}

pub fn draw(f: &mut Frame<'_>, area: Rect, d: &Dialog) {
    let (r, cancel, confirm) = geometry(area, d);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .title(format!(" {} ", d.title))
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let body: Vec<Line<'_>> = d.body.iter().map(|l| Line::raw(l.as_str())).collect();
    let text_area = Rect {
        height: inner.height.saturating_sub(2),
        ..inner
    };
    f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), text_area);
    let button = |label: &str, focused: bool| {
        let style = if focused {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
        };
        Paragraph::new(Line::from(Span::styled(format!("[ {label} ]"), style)))
    };
    f.render_widget(button(&d.cancel_label, d.focus == Button::Cancel), cancel);
    f.render_widget(
        button(&d.confirm_label, d.focus == Button::Confirm),
        confirm,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn default_focus_is_cancel() {
        let mut d = Dialog::new("Delete", "/p/x", vec![], "Delete");
        assert_eq!(d.focus, Button::Cancel);
        assert_eq!(d.key(k(KeyCode::Enter)), Outcome::Cancelled);
    }

    #[test]
    fn confirm_keys() {
        let mut d = Dialog::new("Delete", "/p/x", vec![], "Delete");
        match d.key(k(KeyCode::Char('y'))) {
            Outcome::Confirmed(t) => assert_eq!(t.subject(), "/p/x"),
            o => panic!("{o:?}"),
        }
        assert_eq!(d.key(k(KeyCode::Right)), Outcome::Pending);
        assert!(matches!(d.key(k(KeyCode::Enter)), Outcome::Confirmed(_)));
        for c in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('q')] {
            assert_eq!(d.key(k(c)), Outcome::Cancelled);
        }
        assert_eq!(d.key(k(KeyCode::Char('x'))), Outcome::Pending);
    }
}
