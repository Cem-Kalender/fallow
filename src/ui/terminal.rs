//! Terminal lifecycle: raw mode, alternate screen, paste, mouse and keyboard flags, restored on
//! exit and from a panic hook (FR-001, FR-008, FR-063, FR-067).

use std::io::{self, Stdout, Write, stdout};
use std::sync::Once;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

static PANIC_HOOK: Once = Once::new();

/// Takes over the terminal. On failure, whatever was already enabled is undone.
pub fn enter(mouse: bool) -> io::Result<Tui> {
    install_panic_hook();
    let result = (|| {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen, EnableBracketedPaste)?;
        if mouse {
            execute!(out, EnableMouseCapture)?;
        }
        // Linux/macOS: ask for unambiguous modifier reporting without querying support first,
        // because a query round-trip would delay the first frame (FR-067, FR-041). Terminals
        // without the kitty protocol ignore it. Windows' console input API already reports
        // modifiers.
        #[cfg(unix)]
        {
            use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
            execute!(
                out,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
        }
        Terminal::new(CrosstermBackend::new(out))
    })();
    if result.is_err() {
        restore();
    }
    result
}

/// Gives the terminal back. Safe to call more than once and from the panic hook; every step
/// runs even if an earlier one fails.
pub fn restore() {
    let mut out = stdout();
    #[cfg(unix)]
    {
        use crossterm::event::PopKeyboardEnhancementFlags;
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(out, DisableMouseCapture);
    let _ = execute!(out, DisableBracketedPaste);
    let _ = execute!(out, LeaveAlternateScreen, cursor::Show);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// Restores the terminal before the default hook prints the panic message, so the message is
/// readable and the shell is usable. The release profile aborts on panic; the hook still runs.
fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}
