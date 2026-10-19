//! The help overlay (FR-061, FR-066, FR-012): every binding grouped by pane, generated from the
//! command registry, plus the displayed-semantics legend, the notes and the known terminal
//! conflicts, verbatim from contracts/ui-commands.md.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::app::commands::{COMMANDS, Context};

/// contracts/ui-commands.md "Displayed semantics".
pub const SEMANTICS: &[(&str, &str)] = &[
    (
        "Status serving",
        "Local model server with at least one connected client. CPU may be low while the GPU works.",
    ),
    (
        "Status LLM connected",
        "Has an open connection to an LLM provider or local model server. This does not mean a request is in progress: Fallow can't see traffic without reading payloads or extra privileges.",
    ),
    (
        "Status busy",
        "Averaged above 2% of one CPU core over the last 10 s.",
    ),
    ("Status idle", "None of the above."),
    ("Confidence certain", "Matched a known signature."),
    (
        "Confidence likely",
        "Seen only through its connections. It may be a false positive, for example when providers share CDN addresses.",
    ),
    (
        "Memory metric label",
        "`footprint` (macOS), `private WS` (Windows), `PSS` / `RSS` (Linux, per process). The system summary lists the metric. \"GPU memory (VRAM) not included\" is shown where it applies.",
    ),
    (
        "Recent changes",
        "\"Changed in this directory in the last 30 min. Not proven to come from this process.\"",
    ),
];

/// contracts/ui-commands.md "Help overlay content".
pub const NOTES: &[(&str, &str)] = &[
    (
        "Selecting text with the terminal",
        "Fallow captures the mouse. To use your terminal's own text selection, hold a modifier while dragging. This is usually `Shift`; in iTerm2 it is `Option`, and in Terminal.app `Fn`.",
    ),
    (
        "Alt shortcuts on macOS",
        "`Alt-1/2/3` need 'Use Option as Meta key' (Terminal.app › Settings › Profiles › Keyboard) or 'Left Option key: Esc+' (iTerm2 › Settings › Profiles › Keys). `Tab` / `Shift-Tab` always work.",
    ),
    (
        "F-keys on Mac keyboards",
        "Hold `Fn` for `F1` / `F2`, or use `?` and the palette.",
    ),
    ("Leaving the editor", "`Esc`, then `Tab`."),
    (
        "Keys in a focused session",
        "Every key goes to the agent unchanged, including `Esc`, `Tab`, `Ctrl-C`, `Ctrl-Q`, `Ctrl-P` and `F1`, except the leave key (`Ctrl-]` by default, `sessions.leave_key`). Press it twice to send it to the agent. `Ctrl-E`, `Ctrl-T`, `Ctrl-N` and `Ctrl-B` are Fallow's only while no session has focus.",
    ),
    (
        "Known gaps in sessions",
        "No kitty keyboard protocol: agents use their legacy keys (in Claude Code, `Shift-Enter` does not insert a newline; use `Ctrl-J` or `\\` then `Enter`). No reflow on resize: the agent redraws at the new size, and older scrollback keeps its line breaks.",
    ),
    (
        "Windows: agents installed with npm",
        "npm installs agents as `.cmd` shims. Fallow runs the program the shim points to (`node` with its script, or the agent's `.exe`), never `cmd.exe`. A shim it cannot read is refused with the reason; point a profile in profiles.toml at the real executable.",
    ),
    (
        "Ctrl-] on AltGr layouts",
        "Where `]` needs AltGr (German, Nordic), many terminals also send `Ctrl-]` for `Ctrl-5`; Fallow accepts both. The key can be changed with `sessions.leave_key`.",
    ),
];

/// contracts/ui-commands.md "Known terminal conflicts": key, Windows Terminal, Terminal.app,
/// iTerm2.
pub const CONFLICTS: &[[&str; 4]] = &[
    ["Tab, Shift-Tab", "free", "free", "free"],
    ["Esc", "free", "free", "free"],
    [
        "Alt-1/2/3",
        "free (WT uses Ctrl-Alt-1…9 for tabs)",
        "needs Option as Meta, off by default",
        "needs Option = Esc+, off by default",
    ],
    [
        "F1",
        "free",
        "free; needs Fn on Mac keyboards (OS setting)",
        "free; needs Fn on Mac keyboards",
    ],
    ["F2", "free", "free; needs Fn", "free; needs Fn"],
    ["?", "free", "free", "free"],
    [
        "Ctrl-P",
        "free (WT palette is Ctrl-Shift-P)",
        "free",
        "free",
    ],
    [
        "Ctrl-O",
        "free",
        "free (raw mode disables the tty \"discard\" char)",
        "free (same)",
    ],
    [
        "Ctrl-Q",
        "free",
        "free (raw mode disables XON/XOFF)",
        "free (same)",
    ],
    [
        "Ctrl-S",
        "free",
        "free (raw mode disables XON/XOFF)",
        "free (same)",
    ],
    [
        "Ctrl-Z",
        "free",
        "free (raw mode disables job-control signals)",
        "free (same)",
    ],
    [
        "Ctrl-Y",
        "free",
        "free (raw mode disables the tty \"delayed suspend\" char)",
        "free (same)",
    ],
    ["Ctrl-F", "free (WT find is Ctrl-Shift-F)", "free", "free"],
    ["Ctrl-G", "free", "free", "free"],
    [
        "Ctrl-W",
        "free (WT close pane is Ctrl-Shift-W)",
        "free",
        "free",
    ],
    [
        "Ctrl-A",
        "free (WT select-all is Ctrl-Shift-A)",
        "free",
        "free",
    ],
    [
        "Ctrl-C",
        "conditional: WT's generated settings bind Ctrl-C to copy only while WT has its own selection; otherwise it passes through. Fallow's mouse capture means WT rarely has a selection.",
        "free (raw mode disables SIGINT)",
        "free (same)",
    ],
    ["Ctrl-X", "free", "free", "free"],
    [
        "Ctrl-V",
        "intercepted, same result: WT's generated settings bind Ctrl-V to paste. The text arrives as a bracketed paste, which Fallow handles as a paste.",
        "free (raw mode disables the tty \"literal next\" char)",
        "free (same)",
    ],
    [
        "Ctrl-PgUp/PgDn",
        "free (WT scroll is Ctrl-Shift-PgUp/PgDn)",
        "verify: Terminal.app's PgUp/PgDn handling may consume the combination",
        "verify",
    ],
    ["Shift-←/→", "free", "free", "free"],
    ["Shift-↑/↓", "free", "verify", "free"],
    [
        "Shift-Home/End",
        "free",
        "verify: Terminal.app maps Home/End to scrollback by default and may send unmodified Home/End when Shift is held",
        "verify",
    ],
    [
        "Home/End/PgUp/PgDn",
        "free",
        "intercepted by default: they scroll the scrollback. Fallow uses the alternate screen, where Terminal.app passes them to the app. verify",
        "free in alternate screen; verify",
    ],
    [
        "Delete",
        "free",
        "free (Fn-Delete on Mac keyboards)",
        "free (Fn-Delete)",
    ],
    [
        "Enter",
        "free (WT's Enter = copy applies only while WT has a selection)",
        "free",
        "free",
    ],
    [
        "↑↓←→",
        "free (WT uses Alt-arrows for its panes)",
        "free (macOS uses Ctrl-arrows for Spaces; Fallow doesn't use them)",
        "free",
    ],
    [
        "a, Shift-A, d, r, g, p, m, c, u, n, s, w, j, e, f, Shift-R, Shift-K, Space",
        "free",
        "free",
        "free",
    ],
    [
        "Mouse drag, click, wheel",
        "captured by Fallow; the terminal's own selection needs Shift",
        "captured; own selection needs Fn (verify)",
        "captured; own selection needs Option",
    ],
];

/// 002 contracts/ui-commands.md "Known terminal conflicts (002 keys)": key, Windows Terminal,
/// Terminal.app, iTerm2, built-in agents.
pub const CONFLICTS_002: &[[&str; 5]] = &[
    [
        "Ctrl-]",
        "free (verify)",
        "free (verify)",
        "free (verify)",
        "not bound by Claude Code, Codex, Gemini CLI; Aider (prompt_toolkit) uses it for character search, reachable by pressing it twice",
    ],
    ["Ctrl-E", "free", "free", "free", "n/a (outside sessions)"],
    [
        "Ctrl-T",
        "free (WT new tab is Ctrl-Shift-T)",
        "free",
        "free",
        "n/a (outside sessions)",
    ],
    [
        "Ctrl-N",
        "free (WT new window is Ctrl-Shift-N)",
        "free",
        "free",
        "n/a (outside sessions)",
    ],
    ["Ctrl-B", "free", "free", "free", "n/a (outside sessions)"],
    [
        "Shift-PgUp/PgDn",
        "free (WT scroll is Ctrl-Shift-PgUp/PgDn)",
        "verify: may scroll Terminal.app's own scrollback",
        "verify",
        "n/a (after leaving the session)",
    ],
];

pub const CONFLICTS_STATUS: &str = "Windows Terminal: verified against its published defaults.json. Terminal.app and iTerm2: checked against documented defaults; to be confirmed in the macOS hardware run-through.";

/// The overlay's text.
pub fn lines() -> Vec<Line<'static>> {
    let h = Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let b = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let mut out = Vec::new();
    for ctx in Context::ALL {
        out.push(Line::styled(ctx.title(), h));
        for c in COMMANDS.iter().filter(|c| c.context == ctx) {
            let keys = if c.keys.is_empty() { "palette" } else { c.keys };
            let mut spans = vec![Span::styled(format!("  {keys:<34}"), b), Span::raw(c.title)];
            if !c.available {
                spans.push(Span::styled("  (not available yet)", dim));
            }
            out.push(Line::from(spans));
        }
        out.push(Line::raw(""));
    }
    out.push(Line::styled("Status, confidence and memory", h));
    for (k, v) in SEMANTICS {
        out.push(Line::from(vec![
            Span::styled(format!("  {k}: "), b),
            Span::raw(*v),
        ]));
    }
    out.push(Line::raw(""));
    out.push(Line::styled("Notes", h));
    for (k, v) in NOTES {
        out.push(Line::from(vec![
            Span::styled(format!("  {k}: "), b),
            Span::raw(*v),
        ]));
    }
    out.push(Line::raw(""));
    out.push(Line::styled("Known terminal conflicts", h));
    out.push(Line::styled(format!("  {CONFLICTS_STATUS}"), dim));
    out.push(Line::styled(
        format!(
            "  {:<15}{:<40}{:<52}{}",
            "Key", "Windows Terminal", "Terminal.app", "iTerm2"
        ),
        b,
    ));
    for [k, wt, ta, it] in CONFLICTS {
        out.push(Line::raw(format!("  {k:<15}{wt:<40}{ta:<52}{it}")));
    }
    out.push(Line::raw(""));
    out.push(Line::styled("Known terminal conflicts: agent sessions", h));
    out.push(Line::styled(
        format!(
            "  {:<17}{:<40}{:<50}{:<16}{}",
            "Key", "Windows Terminal", "Terminal.app", "iTerm2", "Built-in agents"
        ),
        b,
    ));
    for [k, wt, ta, it, ag] in CONFLICTS_002 {
        out.push(Line::raw(format!("  {k:<17}{wt:<40}{ta:<50}{it:<16}{ag}")));
    }
    out
}

pub fn draw(f: &mut Frame<'_>, area: Rect, scroll: u16) {
    let [v] = Layout::vertical([Constraint::Percentage(90)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Percentage(92)])
        .flex(Flex::Center)
        .areas(v);
    let text = lines();
    let max = (text.len() as u16).saturating_sub(r.height.saturating_sub(2));
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((scroll.min(max), 0))
            .block(Block::bordered().title(" Help · Esc to close ")),
        r,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_text() -> String {
        lines()
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    #[test]
    fn explains_llm_connected_and_lists_every_command() {
        let t = all_text();
        assert!(t.contains("This does not mean a request is in progress"));
        for c in COMMANDS {
            assert!(t.contains(c.title), "help is missing {}", c.title);
        }
        for (k, _) in NOTES {
            assert!(t.contains(k));
        }
        assert!(t.contains("Known terminal conflicts"));
        for [k, ..] in CONFLICTS_002 {
            assert!(t.contains(k), "help is missing the {k} row");
        }
    }
}
