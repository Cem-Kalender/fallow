//! The command palette (FR-062, FR-066): `Ctrl-P` opens it, typing filters, and every command
//! shows its key binding. Generated from the command registry.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};

use crate::app::commands::{COMMANDS, Command};

/// Commands whose title contains every word of `query` (case-insensitive), in registry order.
pub fn matches(query: &str) -> Vec<&'static Command> {
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    COMMANDS
        .iter()
        .filter(|c| {
            let title = c.title.to_lowercase();
            words.iter().all(|w| title.contains(w))
        })
        .collect()
}

pub fn draw(f: &mut Frame<'_>, area: Rect, query: &str, selected: usize) {
    let [v] = Layout::vertical([Constraint::Percentage(70)])
        .flex(Flex::Start)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(70.min(area.width))])
        .flex(Flex::Center)
        .areas(v);
    let r = Rect {
        y: r.y + 1,
        height: r.height.saturating_sub(1),
        ..r
    };
    f.render_widget(Clear, r);
    let block = Block::bordered().title(" Commands ");
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [input, list] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(format!("> {query}▏")), input);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let width = list.width as usize;
    let items: Vec<ListItem<'_>> = matches(query)
        .into_iter()
        .map(|c| {
            let keys = if c.keys.is_empty() { "" } else { c.keys };
            let title = if c.available {
                c.title.to_string()
            } else {
                format!("{} (not available yet)", c.title)
            };
            let pad = width.saturating_sub(title.chars().count() + keys.chars().count() + 1);
            let style = if c.available { Style::default() } else { dim };
            ListItem::new(Line::from(vec![
                Span::styled(title, style),
                Span::raw(" ".repeat(pad.max(1))),
                Span::styled(keys, Style::default().add_modifier(Modifier::BOLD)),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(
        List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        list,
        &mut st,
    );
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn filters_by_words() {
        let ids: Vec<&str> = matches("help").iter().map(|c| c.id).collect();
        assert_eq!(ids, ["help"]);
        assert!(matches("reload").iter().any(|c| c.id == "config.reload"));
        assert!(
            matches("sort memory")
                .iter()
                .any(|c| c.id == "proc.sort.mem")
        );
        assert_eq!(matches("").len(), crate::app::commands::COMMANDS.len());
    }
}
