//! Default key bindings (FR-064, FR-065, FR-066, contracts/ui-commands.md). Keys resolve to
//! command ids from the registry (`commands.rs`).
//!
//! The focused context's own bindings are looked up first, then the global ones. That order
//! is what lets `Tab` indent in the editor while it switches panes everywhere else.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::commands::Context;

/// Resolves a key press for the focused context. `text_input` disables `?` (FR-066).
pub fn lookup(key: &KeyEvent, context: Context, text_input: bool) -> Option<&'static str> {
    own(key, context).or_else(|| global(key, text_input))
}

fn global(key: &KeyEvent, text_input: bool) -> Option<&'static str> {
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('q' | 'Q') if ctrl && !alt => Some("quit"),
        KeyCode::Char('p' | 'P') if ctrl && !alt => Some("palette"),
        KeyCode::Char('o' | 'O') if ctrl && !alt => Some("project.open"),
        KeyCode::Char('e' | 'E') if ctrl && !alt => Some("view.toggle"),
        KeyCode::F(1) => Some("help"),
        KeyCode::Char('?') if !text_input && !ctrl && !alt => Some("help"),
        KeyCode::BackTab => Some("pane.prev"),
        KeyCode::Tab if shift => Some("pane.prev"),
        KeyCode::Tab => Some("pane.next"),
        KeyCode::Char('1') if alt && !ctrl => Some("pane.files"),
        KeyCode::Char('2') if alt && !ctrl => Some("pane.editor"),
        KeyCode::Char('3') if alt && !ctrl => Some("pane.processes"),
        _ => None,
    }
}

fn own(key: &KeyEvent, context: Context) -> Option<&'static str> {
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    let plain = !ctrl && !alt;
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
            KeyCode::Char(' ') if plain => Some("proc.toggle"),
            KeyCode::Enter => Some("proc.details"),
            KeyCode::Char('m') if plain => Some("proc.sort.mem"),
            KeyCode::Char('c') if plain => Some("proc.sort.cpu"),
            KeyCode::Char('u') if plain => Some("proc.sort.uptime"),
            KeyCode::Char('n') if plain => Some("proc.sort.name"),
            KeyCode::Char('g') if plain => Some("proc.goto_dir"),
            KeyCode::Char('p') if plain => Some("proc.pause"),
            KeyCode::Char('r') if plain => Some("proc.resume"),
            KeyCode::Char('K') if plain => Some("proc.kill"),
            _ => None,
        },
        Context::Files => match key.code {
            KeyCode::Up if plain => Some("files.up"),
            KeyCode::Down if plain => Some("files.down"),
            KeyCode::PageUp if plain => Some("files.page_up"),
            KeyCode::PageDown if plain => Some("files.page_down"),
            KeyCode::Home if plain => Some("files.home"),
            KeyCode::End if plain => Some("files.end"),
            KeyCode::Left if plain => Some("files.collapse"),
            KeyCode::Right if plain => Some("files.expand"),
            KeyCode::Enter => Some("files.open"),
            KeyCode::Char('a') if plain => Some("files.new_file"),
            KeyCode::Char('A') if plain => Some("files.new_folder"),
            KeyCode::F(2) => Some("files.rename"),
            KeyCode::Char('r') if plain => Some("files.rename"),
            KeyCode::Delete => Some("files.delete"),
            KeyCode::Char('d') if plain => Some("files.delete"),
            _ => None,
        },
        Context::Editor => match key.code {
            KeyCode::Char('s' | 'S') if ctrl && !alt => Some("editor.save"),
            KeyCode::Char('z' | 'Z') if ctrl && !alt => Some("editor.undo"),
            KeyCode::Char('y' | 'Y') if ctrl && !alt => Some("editor.redo"),
            KeyCode::Char('f' | 'F') if ctrl && !alt && !shift => Some("editor.find"),
            KeyCode::Char('g' | 'G') if ctrl && !alt => Some("editor.goto_line"),
            KeyCode::Char('w' | 'W') if ctrl && !alt => Some("editor.close"),
            KeyCode::Char('a' | 'A') if ctrl && !alt => Some("editor.select_all"),
            KeyCode::Char('c' | 'C') if ctrl && !alt => Some("editor.copy"),
            KeyCode::Char('x' | 'X') if ctrl && !alt => Some("editor.cut"),
            KeyCode::Char('v' | 'V') if ctrl && !alt => Some("editor.paste"),
            KeyCode::PageDown if ctrl => Some("editor.next_buffer"),
            KeyCode::PageUp if ctrl => Some("editor.prev_buffer"),
            KeyCode::BackTab => Some("editor.outdent"),
            KeyCode::Tab if shift => Some("editor.outdent"),
            KeyCode::Tab => Some("editor.indent"),
            KeyCode::Esc => Some("editor.release"),
            // Selection keys are listed as a command for help and the conflicts check; the
            // editor applies them as cursor movement.
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
                if shift && !ctrl && !alt =>
            {
                Some("editor.select")
            }
            _ => None,
        },
        Context::FindBar => match key.code {
            KeyCode::Tab | KeyCode::BackTab => Some("find.scope"),
            KeyCode::Enter | KeyCode::Down => Some("find.next"),
            KeyCode::Up => Some("find.prev"),
            KeyCode::Esc => Some("find.close"),
            _ => None,
        },
        Context::Dialog => match key.code {
            KeyCode::Char('y' | 'Y') if plain => Some("dialog.confirm"),
            KeyCode::Esc => Some("dialog.cancel"),
            KeyCode::Char('n' | 'N' | 'q' | 'Q') if plain => Some("dialog.cancel"),
            _ => None,
        },
        Context::Projects => match key.code {
            KeyCode::Up if plain => Some("projects.up"),
            KeyCode::Down if plain => Some("projects.down"),
            KeyCode::Enter | KeyCode::Right => Some("projects.open"),
            KeyCode::Char('n') if plain => Some("session.new"),
            KeyCode::Char('a') if plain => Some("project.add"),
            _ => None,
        },
        Context::Sessions => match key.code {
            KeyCode::Up if plain => Some("sessions.up"),
            KeyCode::Down if plain => Some("sessions.down"),
            KeyCode::Enter => Some("session.focus"),
            KeyCode::Char('n') if plain => Some("session.new_here"),
            KeyCode::Char('R') if !ctrl && !alt => Some("session.restart"),
            KeyCode::Char('w') if plain => Some("session.close"),
            KeyCode::PageUp if shift => Some("session.scroll_up"),
            KeyCode::PageDown if shift => Some("session.scroll_down"),
            KeyCode::Char('f' | 'F') if ctrl && !alt => Some("session.search"),
            KeyCode::Char('c' | 'C') if ctrl && !alt => Some("session.copy"),
            KeyCode::Esc | KeyCode::Left => Some("sessions.back"),
            _ => None,
        },
        // A focused session handles its own keys (only the leave key is Fallow's).
        Context::Session => None,
        Context::Global => None,
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
            lookup(&k(KeyCode::Tab, KeyModifiers::NONE), Context::Editor, true),
            Some("editor.indent"),
            "Tab indents in the editor"
        );
        assert_eq!(
            lookup(&k(KeyCode::Tab, KeyModifiers::NONE), Context::Files, false),
            Some("pane.next")
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
