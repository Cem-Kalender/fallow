//! The command registry (FR-060, FR-061, FR-062): every command in contracts/ui-commands.md
//! with its title, context and default key. The palette, the help overlay and the hint bar are
//! all generated from this table, so no command is defined anywhere else.

/// Where a command applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Global,
    Processes,
    Files,
    Editor,
    FindBar,
    Dialog,
}

impl Context {
    pub fn title(self) -> &'static str {
        match self {
            Context::Global => "Global",
            Context::Processes => "Process panel",
            Context::Files => "Files pane",
            Context::Editor => "Editor",
            Context::FindBar => "Find bar",
            Context::Dialog => "Dialogs",
        }
    }

    pub const ALL: [Context; 6] = [
        Context::Global,
        Context::Processes,
        Context::Files,
        Context::Editor,
        Context::FindBar,
        Context::Dialog,
    ];
}

/// A position in the hint bar: order, key text, label.
pub type Hint = (u8, &'static str, &'static str);

#[derive(Debug, Clone, Copy)]
pub struct Command {
    pub id: &'static str,
    pub title: &'static str,
    pub context: Context,
    /// The default key or gesture as shown in help and the palette ("" = palette only).
    pub keys: &'static str,
    /// Shown in the hint bar of its context.
    pub hint: Option<Hint>,
    /// Implemented in this build; others say they are not available yet.
    pub available: bool,
}

const fn cmd(
    id: &'static str,
    title: &'static str,
    context: Context,
    keys: &'static str,
) -> Command {
    Command {
        id,
        title,
        context,
        keys,
        hint: None,
        available: false,
    }
}

const fn on(mut c: Command) -> Command {
    c.available = true;
    c
}

const fn hint(mut c: Command, order: u8, key: &'static str, label: &'static str) -> Command {
    c.hint = Some((order, key, label));
    c
}

use Context::*;

pub const COMMANDS: &[Command] = &[
    // Global
    on(hint(
        cmd("help", "Help", Global, "F1, ? (outside text input)"),
        0,
        "?",
        "Help",
    )),
    on(hint(
        cmd("palette", "Command palette", Global, "Ctrl-P"),
        1,
        "Ctrl-P",
        "Commands",
    )),
    on(cmd("pane.next", "Next pane", Global, "Tab")),
    on(cmd("pane.prev", "Previous pane", Global, "Shift-Tab")),
    on(cmd("pane.files", "Focus Files", Global, "Alt-1")),
    on(cmd("pane.editor", "Focus Editor", Global, "Alt-2")),
    on(cmd("pane.processes", "Focus Processes", Global, "Alt-3")),
    cmd("project.open", "Open project", Global, "Ctrl-O"),
    cmd("config.reload", "Reload configuration", Global, ""),
    on(cmd("quit", "Quit", Global, "Ctrl-Q")),
    // Process panel
    on(hint(
        cmd("proc.sort.mem", "Sort by memory", Processes, "m"),
        2,
        "m",
        "Sort mem",
    )),
    on(cmd("proc.sort.cpu", "Sort by CPU", Processes, "c")),
    on(cmd("proc.sort.uptime", "Sort by uptime", Processes, "u")),
    on(cmd("proc.sort.name", "Sort by name", Processes, "n")),
    on(cmd(
        "proc.sort.status",
        "Sort by status",
        Processes,
        "click header",
    )),
    on(cmd(
        "proc.sort.confidence",
        "Sort by confidence",
        Processes,
        "click header",
    )),
    on(cmd("proc.up", "Move selection up", Processes, "↑")),
    on(cmd("proc.down", "Move selection down", Processes, "↓")),
    on(cmd("proc.page_up", "Page up", Processes, "PgUp")),
    on(cmd("proc.page_down", "Page down", Processes, "PgDn")),
    on(cmd("proc.home", "First process", Processes, "Home")),
    on(cmd("proc.end", "Last process", Processes, "End")),
    on(cmd("proc.expand", "Expand group", Processes, "→")),
    on(cmd("proc.collapse", "Collapse group", Processes, "←")),
    on(cmd(
        "proc.toggle",
        "Expand / collapse group",
        Processes,
        "Space",
    )),
    hint(
        cmd("proc.details", "Details (signals)", Processes, "Enter"),
        3,
        "Enter",
        "Details",
    ),
    hint(
        cmd("proc.goto_dir", "Go to working directory", Processes, "g"),
        4,
        "g",
        "Go to dir",
    ),
    hint(cmd("proc.pause", "Pause", Processes, "p"), 5, "p", "Pause"),
    cmd("proc.resume", "Resume", Processes, "r"),
    hint(
        cmd("proc.kill", "Kill", Processes, "Shift-K"),
        6,
        "Shift-K",
        "Kill",
    ),
    cmd("proc.browsers", "Toggle browser category", Processes, ""),
    // Files pane
    cmd(
        "files.move",
        "Move selection",
        Files,
        "↑ / ↓, PgUp / PgDn, Home / End",
    ),
    hint(
        cmd(
            "files.open",
            "Open file / expand folder",
            Files,
            "Enter, double-click",
        ),
        2,
        "Enter",
        "Open",
    ),
    cmd(
        "files.collapse",
        "Collapse folder / go to parent",
        Files,
        "←",
    ),
    cmd("files.expand", "Expand folder", Files, "→"),
    hint(
        cmd("files.new_file", "New file", Files, "a"),
        3,
        "a",
        "New file",
    ),
    cmd("files.new_folder", "New folder", Files, "Shift-A"),
    hint(
        cmd("files.rename", "Rename", Files, "F2, r"),
        4,
        "F2",
        "Rename",
    ),
    hint(
        cmd("files.delete", "Delete", Files, "Delete, d"),
        5,
        "d",
        "Delete",
    ),
    // Editor
    hint(
        cmd("editor.save", "Save", Editor, "Ctrl-S"),
        2,
        "Ctrl-S",
        "Save",
    ),
    cmd("editor.undo", "Undo", Editor, "Ctrl-Z"),
    cmd("editor.redo", "Redo", Editor, "Ctrl-Y"),
    hint(
        cmd("editor.find", "Find", Editor, "Ctrl-F"),
        3,
        "Ctrl-F",
        "Find",
    ),
    hint(
        cmd("editor.goto_line", "Go to line", Editor, "Ctrl-G"),
        4,
        "Ctrl-G",
        "Go to line",
    ),
    cmd("editor.close", "Close buffer", Editor, "Ctrl-W"),
    cmd("editor.next_buffer", "Next buffer", Editor, "Ctrl-PgDn"),
    cmd("editor.prev_buffer", "Previous buffer", Editor, "Ctrl-PgUp"),
    cmd(
        "editor.indent",
        "Indent / outdent",
        Editor,
        "Tab / Shift-Tab",
    ),
    hint(
        cmd("editor.release", "Release editor focus", Editor, "Esc"),
        5,
        "Esc Tab",
        "Next pane",
    ),
    cmd(
        "editor.select",
        "Extend selection",
        Editor,
        "Shift-←/→/↑/↓, Shift-Home / Shift-End",
    ),
    cmd("editor.select_all", "Select all", Editor, "Ctrl-A"),
    cmd("editor.copy", "Copy", Editor, "Ctrl-C"),
    cmd("editor.cut", "Cut", Editor, "Ctrl-X"),
    cmd("editor.paste", "Paste", Editor, "Ctrl-V"),
    // Find bar
    hint(
        cmd(
            "find.scope",
            "Toggle scope: this file / whole project",
            FindBar,
            "Tab",
        ),
        2,
        "Tab",
        "Scope: this file/project",
    ),
    hint(
        cmd(
            "find.next",
            "Next match / run project search",
            FindBar,
            "Enter, ↓",
        ),
        3,
        "Enter",
        "Next/Search",
    ),
    cmd("find.prev", "Previous match", FindBar, "↑"),
    hint(
        cmd(
            "find.close",
            "Cancel search / close find bar",
            FindBar,
            "Esc",
        ),
        4,
        "Esc",
        "Close",
    ),
    // Dialogs
    hint(
        cmd("dialog.confirm", "Confirm", Dialog, "y, Enter on Confirm"),
        2,
        "y",
        "Confirm",
    ),
    hint(
        cmd("dialog.cancel", "Cancel", Dialog, "Esc, n, q"),
        3,
        "Esc",
        "Cancel",
    ),
];

pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The hint bar for a context: the global help and palette hints, then the context's primary
/// commands in order.
pub fn hints(context: Context, text_input: bool) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![
        if text_input {
            ("Ctrl-P ›", "Help")
        } else {
            ("?", "Help")
        },
        ("Ctrl-P", "Commands"),
    ];
    let mut own: Vec<Hint> = COMMANDS
        .iter()
        .filter(|c| c.context == context && context != Global)
        .filter_map(|c| c.hint)
        .collect();
    own.sort_by_key(|h| h.0);
    out.extend(own.into_iter().map(|(_, k, l)| (k, l)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn hint_order_matches_the_contract() {
        let text = |c| {
            hints(c, false)
                .iter()
                .map(|(k, l)| format!("{k} {l}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        assert_eq!(
            text(Processes),
            "? Help · Ctrl-P Commands · m Sort mem · Enter Details · g Go to dir · p Pause · Shift-K Kill"
        );
        assert_eq!(
            text(Files),
            "? Help · Ctrl-P Commands · Enter Open · a New file · F2 Rename · d Delete"
        );
        assert_eq!(
            text(Editor),
            "? Help · Ctrl-P Commands · Ctrl-S Save · Ctrl-F Find · Ctrl-G Go to line · Esc Tab Next pane"
        );
        assert_eq!(
            text(Dialog),
            "? Help · Ctrl-P Commands · y Confirm · Esc Cancel"
        );
    }
}
