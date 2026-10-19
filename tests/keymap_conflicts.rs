//! T064: key bindings against contracts/ui-commands.md (FR-064, FR-062).
//!
//! The expected bindings below are transcribed from the contract's tables, not generated from
//! the keymap, so a change to either one shows up here.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fallow::app::commands::{COMMANDS, Context, find};
use fallow::app::keymap::lookup;
use fallow::session::keys::LeaveKey;
use fallow::ui::help::{CONFLICTS, CONFLICTS_002};
use fallow::ui::palette;

fn parse_key(s: &str) -> KeyEvent {
    let mut m = KeyModifiers::NONE;
    let mut rest = s;
    loop {
        if let Some(r) = rest.strip_prefix("Ctrl-") {
            m |= KeyModifiers::CONTROL;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Shift-") {
            m |= KeyModifiers::SHIFT;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("Alt-") {
            m |= KeyModifiers::ALT;
            rest = r;
        } else {
            break;
        }
    }
    let code = match rest {
        "Tab" if m.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
        "Tab" => KeyCode::Tab,
        "Esc" => KeyCode::Esc,
        "Enter" => KeyCode::Enter,
        "Delete" => KeyCode::Delete,
        "Space" => KeyCode::Char(' '),
        "PgUp" => KeyCode::PageUp,
        "PgDn" => KeyCode::PageDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "↑" => KeyCode::Up,
        "↓" => KeyCode::Down,
        "←" => KeyCode::Left,
        "→" => KeyCode::Right,
        f if f.starts_with('F') && f.len() > 1 => KeyCode::F(f[1..].parse().unwrap()),
        c if c.chars().count() == 1 => {
            let ch = c.chars().next().unwrap();
            if m.contains(KeyModifiers::SHIFT) && ch.is_ascii_alphabetic() {
                KeyCode::Char(ch.to_ascii_uppercase())
            } else {
                KeyCode::Char(ch)
            }
        }
        other => panic!("unknown key {other:?} in {s:?}"),
    };
    KeyEvent::new(code, m)
}

/// Expands "Ctrl-PgUp/PgDn", "Alt-1/2/3", "↑↓←→" and comma lists into single keys.
fn expand(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    if cell.starts_with("Mouse") {
        return out;
    }
    for tok in cell.split(", ").map(str::trim) {
        if tok == "↑↓←→" {
            out.extend(["↑", "↓", "←", "→"].map(String::from));
            continue;
        }
        match tok.find('/') {
            Some(slash) if tok.len() > 1 => {
                let prefix_end = tok[..slash].rfind('-').map_or(0, |i| i + 1);
                let (prefix, alts) = tok.split_at(prefix_end);
                out.extend(alts.split('/').map(|a| format!("{prefix}{a}")));
            }
            _ => out.push(tok.to_string()),
        }
    }
    out
}

/// contracts/ui-commands.md: (context, key, command).
const CONTRACT: &[(Context, &str, &str)] = &[
    // Panes and focus
    (Context::Files, "Tab", "pane.next"),
    (Context::Processes, "Shift-Tab", "pane.prev"),
    (Context::Files, "Alt-1", "pane.files"),
    (Context::Files, "Alt-2", "pane.editor"),
    (Context::Files, "Alt-3", "pane.processes"),
    (Context::Editor, "Esc", "editor.release"),
    // Global
    (Context::Files, "F1", "help"),
    (Context::Processes, "?", "help"),
    (Context::Editor, "Ctrl-P", "palette"),
    (Context::Files, "Ctrl-O", "project.open"),
    (Context::Editor, "Ctrl-Q", "quit"),
    // Editor
    (Context::Editor, "Ctrl-S", "editor.save"),
    (Context::Editor, "Ctrl-Z", "editor.undo"),
    (Context::Editor, "Ctrl-Y", "editor.redo"),
    (Context::Editor, "Ctrl-F", "editor.find"),
    (Context::Editor, "Ctrl-G", "editor.goto_line"),
    (Context::Editor, "Ctrl-W", "editor.close"),
    (Context::Editor, "Ctrl-PgDn", "editor.next_buffer"),
    (Context::Editor, "Ctrl-PgUp", "editor.prev_buffer"),
    (Context::Editor, "Tab", "editor.indent"),
    (Context::Editor, "Shift-Tab", "editor.outdent"),
    // Selection and clipboard
    (Context::Editor, "Shift-←", "editor.select"),
    (Context::Editor, "Shift-→", "editor.select"),
    (Context::Editor, "Shift-↑", "editor.select"),
    (Context::Editor, "Shift-↓", "editor.select"),
    (Context::Editor, "Shift-Home", "editor.select"),
    (Context::Editor, "Shift-End", "editor.select"),
    (Context::Editor, "Ctrl-A", "editor.select_all"),
    (Context::Editor, "Ctrl-C", "editor.copy"),
    (Context::Editor, "Ctrl-X", "editor.cut"),
    (Context::Editor, "Ctrl-V", "editor.paste"),
    // Find bar
    (Context::FindBar, "Tab", "find.scope"),
    (Context::FindBar, "Enter", "find.next"),
    (Context::FindBar, "↓", "find.next"),
    (Context::FindBar, "↑", "find.prev"),
    (Context::FindBar, "Esc", "find.close"),
    // Files pane
    (Context::Files, "↑", "files.up"),
    (Context::Files, "↓", "files.down"),
    (Context::Files, "PgUp", "files.page_up"),
    (Context::Files, "PgDn", "files.page_down"),
    (Context::Files, "Home", "files.home"),
    (Context::Files, "End", "files.end"),
    (Context::Files, "Enter", "files.open"),
    (Context::Files, "←", "files.collapse"),
    (Context::Files, "→", "files.expand"),
    (Context::Files, "a", "files.new_file"),
    (Context::Files, "Shift-A", "files.new_folder"),
    (Context::Files, "F2", "files.rename"),
    (Context::Files, "r", "files.rename"),
    (Context::Files, "Delete", "files.delete"),
    (Context::Files, "d", "files.delete"),
    // Process panel
    (Context::Processes, "m", "proc.sort.mem"),
    (Context::Processes, "c", "proc.sort.cpu"),
    (Context::Processes, "u", "proc.sort.uptime"),
    (Context::Processes, "n", "proc.sort.name"),
    (Context::Processes, "↑", "proc.up"),
    (Context::Processes, "↓", "proc.down"),
    (Context::Processes, "PgUp", "proc.page_up"),
    (Context::Processes, "PgDn", "proc.page_down"),
    (Context::Processes, "Home", "proc.home"),
    (Context::Processes, "End", "proc.end"),
    (Context::Processes, "→", "proc.expand"),
    (Context::Processes, "←", "proc.collapse"),
    (Context::Processes, "Space", "proc.toggle"),
    (Context::Processes, "Enter", "proc.details"),
    (Context::Processes, "g", "proc.goto_dir"),
    (Context::Processes, "p", "proc.pause"),
    (Context::Processes, "r", "proc.resume"),
    (Context::Processes, "Shift-K", "proc.kill"),
    // Dialogs
    (Context::Dialog, "y", "dialog.confirm"),
    (Context::Dialog, "Esc", "dialog.cancel"),
    (Context::Dialog, "n", "dialog.cancel"),
    (Context::Dialog, "q", "dialog.cancel"),
    // 002: global (both views)
    (Context::Projects, "Ctrl-E", "view.toggle"),
    (Context::Files, "Ctrl-E", "view.toggle"),
    (Context::Sessions, "Ctrl-T", "sessions.overview"),
    (Context::Projects, "Ctrl-N", "session.next"),
    (Context::Files, "Ctrl-B", "session.prev"),
    // 002: projects list
    (Context::Projects, "↑", "projects.up"),
    (Context::Projects, "↓", "projects.down"),
    (Context::Projects, "Enter", "projects.open"),
    (Context::Projects, "n", "session.new"),
    (Context::Projects, "a", "project.add"),
    (Context::Projects, "Ctrl-O", "project.open"),
    (Context::Projects, "Home", "projects.home"),
    (Context::Projects, "End", "projects.end"),
    (Context::Projects, "Delete", "project.remove"),
    (Context::Projects, "d", "project.remove"),
    (Context::Projects, "F2", "project.rename"),
    (Context::Projects, "r", "project.rename"),
    (Context::Projects, "Shift-↑", "project.move_up"),
    (Context::Projects, "Shift-↓", "project.move_down"),
    (Context::Projects, "f", "project.filter"),
    (Context::Projects, "e", "project.edit"),
    // 002: sessions list
    (Context::Sessions, "↑", "sessions.up"),
    (Context::Sessions, "↓", "sessions.down"),
    (Context::Sessions, "Enter", "session.focus"),
    (Context::Sessions, "n", "session.new_here"),
    (Context::Sessions, "s", "session.split"),
    (Context::Sessions, "Shift-R", "session.restart"),
    (Context::Sessions, "Shift-K", "session.stop"),
    (Context::Sessions, "w", "session.close"),
    (Context::Sessions, "p", "session.process"),
    (Context::Sessions, "Shift-PgUp", "session.scroll_up"),
    (Context::Sessions, "Shift-PgDn", "session.scroll_down"),
    (Context::Sessions, "Ctrl-F", "session.search"),
    (Context::Sessions, "Ctrl-C", "session.copy"),
    // 002: process panel
    (Context::Processes, "j", "proc.jump_session"),
];

/// Contexts that are text inputs (FR-066: `?` is text there).
fn text_input(c: Context) -> bool {
    matches!(c, Context::Editor | Context::FindBar)
}

#[test]
fn every_default_key_is_bound_to_exactly_the_listed_command() {
    for (ctx, key, id) in CONTRACT {
        let got = lookup(&parse_key(key), *ctx, text_input(*ctx));
        assert_eq!(got, Some(*id), "{ctx:?} {key}");
        assert!(find(id).is_some(), "{id} is not registered");
    }
}

#[test]
fn no_key_is_bound_twice_in_one_context() {
    let mut seen: HashMap<(String, String), &str> = HashMap::new();
    for (ctx, key, id) in CONTRACT {
        let k = (format!("{ctx:?}"), key.to_string());
        if let Some(prev) = seen.insert(k.clone(), id) {
            assert_eq!(prev, *id, "{k:?} bound to {prev} and {id}");
        }
    }
    // The registry's own key column: no two commands claim the same key in one context.
    let mut claimed: HashMap<(String, String), &str> = HashMap::new();
    for c in COMMANDS {
        for k in c.keys.split([',']).map(str::trim) {
            if k.is_empty() || k.contains("click") || k.contains("drag") || k.contains('/') {
                continue;
            }
            if let Some(prev) = claimed.insert((format!("{:?}", c.context), k.to_string()), c.id) {
                panic!("{:?} {k}: {prev} and {}", c.context, c.id);
            }
        }
    }
}

#[test]
fn every_key_in_the_known_conflicts_table_maps_to_a_registered_command() {
    let contexts = [
        Context::Global,
        Context::Files,
        Context::Editor,
        Context::FindBar,
        Context::Processes,
        Context::Dialog,
        Context::Projects,
        Context::Sessions,
    ];
    let mut checked = 0;
    for row in CONFLICTS {
        for key in expand(row[0]) {
            let ev = parse_key(&key);
            let hit = contexts
                .iter()
                .find_map(|c| lookup(&ev, *c, text_input(*c)).filter(|id| find(id).is_some()));
            assert!(
                hit.is_some(),
                "{key:?} (row {:?}) maps to no command",
                row[0]
            );
            checked += 1;
        }
    }
    assert!(checked >= 40, "only {checked} keys checked");
}

#[test]
fn every_002_key_in_the_conflicts_table_is_bound() {
    // The leave key is matched by the focused session itself, not the keymap.
    let leave = LeaveKey::default();
    assert_eq!(leave.label(), "Ctrl-]");
    let contexts = [Context::Global, Context::Projects, Context::Sessions];
    for row in CONFLICTS_002 {
        for key in expand(row[0]) {
            if key == "Ctrl-]" {
                assert!(leave.matches(&parse_key(&key)));
                continue;
            }
            let ev = parse_key(&key);
            let hit = contexts
                .iter()
                .find_map(|c| lookup(&ev, *c, false).filter(|id| find(id).is_some()));
            assert!(hit.is_some(), "{key:?} maps to no command");
        }
    }
}

#[test]
fn every_command_has_a_palette_entry() {
    let all: Vec<&str> = palette::matches("").iter().map(|c| c.id).collect();
    for c in COMMANDS {
        assert!(all.contains(&c.id), "{} missing from the palette", c.id);
        assert!(!c.title.is_empty());
    }
    for (_, _, id) in CONTRACT {
        assert!(all.contains(id), "{id} missing from the palette");
    }
}
