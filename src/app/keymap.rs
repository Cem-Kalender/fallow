//! Default key bindings (FR-065, FR-066, contracts/ui-commands.md). Keys resolve to command ids
//! from the registry (`commands.rs`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::commands::Context;

/// Resolves a key press for the focused context. `text_input` disables `?` (FR-066).
pub fn lookup(key: &KeyEvent, context: Context, text_input: bool) -> Option<&'static str> {
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    // Global keys first.
    let global = match key.code {
        KeyCode::Char('q' | 'Q') if ctrl => Some("quit"),
        KeyCode::Char('p' | 'P') if ctrl => Some("palette"),
        KeyCode::Char('o' | 'O') if ctrl => Some("project.open"),
        KeyCode::F(1) => Some("help"),
        KeyCode::Char('?') if !text_input && !ctrl && !alt => Some("help"),
        KeyCode::BackTab => Some("pane.prev"),
        KeyCode::Tab if shift => Some("pane.prev"),
        KeyCode::Tab => Some("pane.next"),
        KeyCode::Char('1') if alt => Some("pane.files"),
        KeyCode::Char('2') if alt => Some("pane.editor"),
        KeyCode::Char('3') if alt => Some("pane.processes"),
        _ => None,
    };
    if global.is_some() || text_input {
        return global;
    }
    match context {
        Context::Processes => match key.code {
            KeyCode::Up => Some("proc.up"),
            KeyCode::Down => Some("proc.down"),
            KeyCode::PageUp => Some("proc.page_up"),
            KeyCode::PageDown => Some("proc.page_down"),
            KeyCode::Home => Some("proc.home"),
            KeyCode::End => Some("proc.end"),
            KeyCode::Right => Some("proc.expand"),
            KeyCode::Left => Some("proc.collapse"),
            KeyCode::Char(' ') => Some("proc.toggle"),
            KeyCode::Enter => Some("proc.details"),
            KeyCode::Char('m') if !ctrl && !alt => Some("proc.sort.mem"),
            KeyCode::Char('c') if !ctrl && !alt => Some("proc.sort.cpu"),
            KeyCode::Char('u') if !ctrl && !alt => Some("proc.sort.uptime"),
            KeyCode::Char('n') if !ctrl && !alt => Some("proc.sort.name"),
            KeyCode::Char('g') if !ctrl && !alt => Some("proc.goto_dir"),
            KeyCode::Char('p') if !ctrl && !alt => Some("proc.pause"),
            KeyCode::Char('r') if !ctrl && !alt => Some("proc.resume"),
            KeyCode::Char('K') if !ctrl && !alt => Some("proc.kill"),
            _ => None,
        },
        Context::Files => match key.code {
            KeyCode::Enter => Some("files.open"),
            KeyCode::Char('a') => Some("files.new_file"),
            KeyCode::Char('A') => Some("files.new_folder"),
            KeyCode::F(2) | KeyCode::Char('r') => Some("files.rename"),
            KeyCode::Delete | KeyCode::Char('d') => Some("files.delete"),
            _ => None,
        },
        Context::Editor => match key.code {
            KeyCode::Char('s') if ctrl => Some("editor.save"),
            KeyCode::Char('f') if ctrl => Some("editor.find"),
            KeyCode::Char('g') if ctrl => Some("editor.goto_line"),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn globals_and_text_input() {
        assert_eq!(
            lookup(
                &k(KeyCode::Char('?'), KeyModifiers::SHIFT),
                Context::Processes,
                false
            ),
            Some("help")
        );
        assert_eq!(
            lookup(
                &k(KeyCode::Char('?'), KeyModifiers::NONE),
                Context::Processes,
                true
            ),
            None
        );
        assert_eq!(
            lookup(&k(KeyCode::F(1), KeyModifiers::NONE), Context::Files, true),
            Some("help")
        );
        assert_eq!(
            lookup(
                &k(KeyCode::Char('3'), KeyModifiers::ALT),
                Context::Files,
                false
            ),
            Some("pane.processes")
        );
        assert_eq!(
            lookup(
                &k(KeyCode::Down, KeyModifiers::NONE),
                Context::Processes,
                false
            ),
            Some("proc.down")
        );
        assert_eq!(
            lookup(&k(KeyCode::Down, KeyModifiers::NONE), Context::Files, false),
            None
        );
    }

    #[test]
    fn every_bound_id_is_registered() {
        use crate::app::commands::find;
        for (code, m, ctx) in [
            (KeyCode::Char('m'), KeyModifiers::NONE, Context::Processes),
            (KeyCode::Char('K'), KeyModifiers::SHIFT, Context::Processes),
            (KeyCode::Char('o'), KeyModifiers::CONTROL, Context::Global),
            (KeyCode::F(2), KeyModifiers::NONE, Context::Files),
            (KeyCode::Char('s'), KeyModifiers::CONTROL, Context::Editor),
        ] {
            let id = lookup(&k(code, m), ctx, false).unwrap();
            assert!(find(id).is_some(), "{id}");
        }
    }
}
