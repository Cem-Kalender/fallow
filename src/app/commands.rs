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
    /// Agent workspace: the project list (002).
    Projects,
    /// Agent workspace: the sessions of the selected project.
    Sessions,
    /// A focused session: every key but the leave key goes to the agent.
    Session,
    /// The orchestration view (003).
    Orchestration,
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
            Context::Projects => "Projects",
            Context::Sessions => "Sessions",
            Context::Session => "Focused session",
            Context::Orchestration => "Orchestration view",
        }
    }

    pub const ALL: [Context; 10] = [
        Context::Global,
        Context::Processes,
        Context::Files,
        Context::Editor,
        Context::FindBar,
        Context::Dialog,
        Context::Projects,
        Context::Sessions,
        Context::Session,
        Context::Orchestration,
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
    on(cmd("project.open", "Open project", Global, "Ctrl-O")),
    on(cmd(
        "config.reload",
        "Reload configuration (agent profiles)",
        Global,
        "",
    )),
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
    on(cmd("files.up", "Move selection up", Files, "↑")),
    on(cmd("files.down", "Move selection down", Files, "↓")),
    on(cmd("files.page_up", "Page up in Files", Files, "PgUp")),
    on(cmd("files.page_down", "Page down in Files", Files, "PgDn")),
    on(cmd("files.home", "First file", Files, "Home")),
    on(cmd("files.end", "Last file", Files, "End")),
    on(hint(
        cmd(
            "files.open",
            "Open file / expand folder",
            Files,
            "Enter, double-click",
        ),
        2,
        "Enter",
        "Open",
    )),
    on(cmd(
        "files.collapse",
        "Collapse folder / go to parent",
        Files,
        "←",
    )),
    on(cmd("files.expand", "Expand folder", Files, "→")),
    on(hint(
        cmd("files.new_file", "New file", Files, "a"),
        3,
        "a",
        "New file",
    )),
    on(cmd("files.new_folder", "New folder", Files, "Shift-A")),
    on(hint(
        cmd("files.rename", "Rename", Files, "F2, r"),
        4,
        "F2",
        "Rename",
    )),
    on(hint(
        cmd("files.delete", "Delete", Files, "Delete, d"),
        5,
        "d",
        "Delete",
    )),
    // Editor
    on(hint(
        cmd("editor.save", "Save", Editor, "Ctrl-S"),
        2,
        "Ctrl-S",
        "Save",
    )),
    on(cmd("editor.undo", "Undo", Editor, "Ctrl-Z")),
    on(cmd("editor.redo", "Redo", Editor, "Ctrl-Y")),
    on(hint(
        cmd("editor.find", "Find", Editor, "Ctrl-F"),
        3,
        "Ctrl-F",
        "Find",
    )),
    on(hint(
        cmd("editor.goto_line", "Go to line", Editor, "Ctrl-G"),
        4,
        "Ctrl-G",
        "Go to line",
    )),
    on(cmd("editor.close", "Close buffer", Editor, "Ctrl-W")),
    on(cmd(
        "editor.next_buffer",
        "Next buffer",
        Editor,
        "Ctrl-PgDn",
    )),
    on(cmd(
        "editor.prev_buffer",
        "Previous buffer",
        Editor,
        "Ctrl-PgUp",
    )),
    on(cmd("editor.indent", "Indent", Editor, "Tab")),
    on(cmd("editor.outdent", "Outdent", Editor, "Shift-Tab")),
    on(hint(
        cmd("editor.release", "Release editor focus", Editor, "Esc"),
        5,
        "Esc Tab",
        "Next pane",
    )),
    on(cmd(
        "editor.select",
        "Extend selection",
        Editor,
        "Shift-←/→/↑/↓, Shift-Home / Shift-End",
    )),
    on(cmd("editor.select_all", "Select all", Editor, "Ctrl-A")),
    on(cmd("editor.copy", "Copy", Editor, "Ctrl-C")),
    on(cmd("editor.cut", "Cut", Editor, "Ctrl-X")),
    on(cmd("editor.paste", "Paste", Editor, "Ctrl-V")),
    // Find bar
    on(hint(
        cmd(
            "find.scope",
            "Toggle scope: this file / whole project",
            FindBar,
            "Tab",
        ),
        2,
        "Tab",
        "Scope: this file/project",
    )),
    on(hint(
        cmd(
            "find.next",
            "Next match / run project search",
            FindBar,
            "Enter, ↓",
        ),
        3,
        "Enter",
        "Next/Search",
    )),
    on(cmd("find.prev", "Previous match", FindBar, "↑")),
    on(hint(
        cmd(
            "find.close",
            "Cancel search / close find bar",
            FindBar,
            "Esc",
        ),
        4,
        "Esc",
        "Close",
    )),
    // Agent workspace (002)
    on(hint(
        cmd(
            "view.toggle",
            "Switch agent workspace / editor view",
            Global,
            "Ctrl-E",
        ),
        4,
        "Ctrl-E",
        "Editor",
    )),
    on(cmd("projects.up", "Previous project", Projects, "↑")),
    on(cmd("projects.down", "Next project", Projects, "↓")),
    on(hint(
        cmd(
            "projects.open",
            "Show the project's sessions",
            Projects,
            "Enter",
        ),
        5,
        "Enter",
        "Sessions",
    )),
    on(hint(
        cmd("session.new", "New session", Projects, "n"),
        2,
        "n",
        "New session",
    )),
    on(cmd("project.add", "Add project", Projects, "a")),
    on(cmd("projects.home", "First project", Projects, "Home")),
    on(cmd("projects.end", "Last project", Projects, "End")),
    on(hint(
        cmd(
            "project.remove",
            "Remove project from the list (the folder is kept)",
            Projects,
            "Delete, d",
        ),
        7,
        "d",
        "Remove",
    )),
    on(cmd("project.rename", "Rename project", Projects, "F2, r")),
    on(cmd(
        "project.move_up",
        "Move project up",
        Projects,
        "Shift-↑",
    )),
    on(cmd(
        "project.move_down",
        "Move project down",
        Projects,
        "Shift-↓",
    )),
    on(hint(
        cmd(
            "project.filter",
            "Show only this project's LLM processes (toggle)",
            Projects,
            "f",
        ),
        6,
        "f",
        "Filter processes",
    )),
    on(cmd(
        "project.edit",
        "Open project in the editor view",
        Projects,
        "e",
    )),
    on(cmd(
        "project.relocate",
        "Point project to a new folder",
        Projects,
        "",
    )),
    on(cmd(
        "sessions.up",
        "Previous session in the list",
        Sessions,
        "↑",
    )),
    on(cmd(
        "sessions.down",
        "Next session in the list",
        Sessions,
        "↓",
    )),
    on(hint(
        cmd("session.focus", "Focus session", Sessions, "Enter"),
        5,
        "Enter",
        "Open",
    )),
    on(hint(
        cmd(
            "session.new_here",
            "New session in this project",
            Sessions,
            "n",
        ),
        2,
        "n",
        "New session",
    )),
    on(hint(
        cmd("session.restart", "Restart session", Sessions, "R"),
        6,
        "R",
        "Restart",
    )),
    on(cmd("session.close", "Close exited session", Sessions, "w")),
    on(cmd(
        "session.scroll_up",
        "Scroll session back",
        Sessions,
        "Shift-PgUp",
    )),
    on(cmd(
        "session.scroll_down",
        "Scroll session forward",
        Sessions,
        "Shift-PgDn",
    )),
    on(cmd(
        "session.search",
        "Search session scrollback",
        Sessions,
        "Ctrl-F",
    )),
    on(cmd(
        "session.copy",
        "Copy session selection",
        Sessions,
        "Ctrl-C",
    )),
    on(cmd(
        "sessions.back",
        "Back to the project list",
        Sessions,
        "Esc",
    )),
    on(cmd(
        "sessions.overview",
        "Session overview (all projects)",
        Global,
        "Ctrl-T",
    )),
    on(hint(
        cmd("session.next", "Next session", Global, "Ctrl-N"),
        3,
        "Ctrl-N/B",
        "Switch session",
    )),
    on(cmd("session.prev", "Previous session", Global, "Ctrl-B")),
    on(cmd(
        "session.split",
        "Split with the selected session",
        Sessions,
        "s",
    )),
    on(hint(
        cmd("session.stop", "Stop session", Sessions, "Shift-K"),
        7,
        "Shift-K",
        "Stop",
    )),
    on(cmd(
        "session.process",
        "Show the session's process",
        Sessions,
        "p",
    )),
    on(cmd(
        "proc.jump_session",
        "Go to the process's session",
        Processes,
        "j",
    )),
    on(hint(
        cmd("session.leave", "Leave the session", Session, "Ctrl-]"),
        0,
        "Ctrl-]",
        "Leave session",
    )),
    // Orchestration (003 contracts/ui-commands.md)
    on(hint(
        cmd(
            "orch.open",
            "Orchestration: open",
            Projects,
            "o (project and session lists)",
        ),
        8,
        "o",
        "Orchestration",
    )),
    on(cmd("session.details", "Session details", Sessions, "i")),
    on(hint(
        cmd("orch.tab_next", "Next tab", Orchestration, "Tab"),
        2,
        "Tab",
        "Next tab",
    )),
    on(cmd(
        "orch.tab_prev",
        "Previous tab",
        Orchestration,
        "Shift-Tab",
    )),
    on(hint(
        cmd("orch.write", "Write on the board", Orchestration, "w"),
        3,
        "w",
        "Write",
    )),
    on(hint(
        cmd(
            "orch.board_toggle",
            "Board: project / shared",
            Orchestration,
            "b",
        ),
        4,
        "b",
        "Shared/project",
    )),
    on(cmd(
        "orch.requests",
        "Orchestration: review requests",
        Global,
        "",
    )),
    on(cmd(
        "orch.revoke_all",
        "Revoke all permissions",
        Orchestration,
        "Shift-X",
    )),
    on(cmd(
        "orch.grant_new",
        "Grant standing permission…",
        Orchestration,
        "a",
    )),
    on(cmd(
        "orch.approve",
        "Approve the request",
        Orchestration,
        "y",
    )),
    on(cmd(
        "orch.edit",
        "Edit the request, then approve",
        Orchestration,
        "e",
    )),
    on(cmd("orch.reject", "Reject the request", Orchestration, "x")),
    on(cmd(
        "orch.grant",
        "Grant a permission for the request",
        Orchestration,
        "g",
    )),
    on(cmd(
        "orch.revoke",
        "Revoke the permission",
        Orchestration,
        "Delete",
    )),
    on(cmd("orch.signal", "Signal an event", Orchestration, "v")),
    on(cmd(
        "orch.clear",
        "Clear the selected event",
        Orchestration,
        "c",
    )),
    on(cmd("orch.up", "Scroll up / previous", Orchestration, "↑")),
    on(cmd("orch.down", "Scroll down / next", Orchestration, "↓")),
    on(cmd("orch.page_up", "Page up", Orchestration, "PgUp")),
    on(cmd("orch.page_down", "Page down", Orchestration, "PgDn")),
    on(hint(
        cmd(
            "orch.close",
            "Close the orchestration view",
            Orchestration,
            "Esc",
        ),
        9,
        "Esc",
        "Close",
    )),
    // Dialogs
    on(hint(
        cmd("dialog.confirm", "Confirm", Dialog, "y, Enter on Confirm"),
        2,
        "y",
        "Confirm",
    )),
    on(hint(
        cmd("dialog.cancel", "Cancel", Dialog, "Esc, n, q"),
        3,
        "Esc",
        "Cancel",
    )),
];

pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The hint bar for a context: the global help and palette hints, then the context's primary
/// commands in order. The agent workspace's lists also show the global workspace hints
/// (switch session, editor view; 002 FR-021).
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
        .filter(|c| {
            (c.context == context && context != Global)
                || (matches!(c.id, "session.next" | "view.toggle")
                    && matches!(context, Projects | Sessions))
        })
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

    #[test]
    fn workspace_hints_show_start_switch_editor_and_help() {
        // 002 FR-021.
        let text = |c| {
            hints(c, false)
                .iter()
                .map(|(k, l)| format!("{k} {l}"))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        assert_eq!(
            text(Projects),
            "? Help · Ctrl-P Commands · n New session · Ctrl-N/B Switch session · Ctrl-E Editor · Enter Sessions · f Filter processes · d Remove · o Orchestration"
        );
        assert_eq!(
            text(Sessions),
            "? Help · Ctrl-P Commands · n New session · Ctrl-N/B Switch session · Ctrl-E Editor · Enter Open · R Restart · Shift-K Stop"
        );
    }
}
