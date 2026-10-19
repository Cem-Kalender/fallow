//! Keys and pastes for the agent (002 FR-003, research R-3, R-6).
//!
//! Every key except the leave key goes to the agent, encoded the way a standard xterm-compatible
//! terminal encodes it in legacy mode. Application cursor mode (DECCKM) changes the arrow keys and
//! `Home`/`End`, as the agent requested. Sessions do not use the kitty keyboard protocol in v1.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// The key that leaves a focused session (`sessions.leave_key`, default `ctrl-]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveKey {
    /// `Ctrl` with a character: `]`, `\`, `5`, …
    Ctrl(char),
    F(u8),
}

impl Default for LeaveKey {
    fn default() -> Self {
        LeaveKey::Ctrl(']')
    }
}

impl LeaveKey {
    /// Parses the settings form: `ctrl-]`, `ctrl-\`, `ctrl-5`, `f12`.
    pub fn parse(s: &str) -> Option<LeaveKey> {
        let s = s.trim().to_ascii_lowercase();
        if let Some(c) = s.strip_prefix("ctrl-") {
            let mut it = c.chars();
            let ch = it.next()?;
            return it.next().is_none().then_some(LeaveKey::Ctrl(ch));
        }
        if let Some(n) = s.strip_prefix('f') {
            let n: u8 = n.parse().ok()?;
            return (1..=24).contains(&n).then_some(LeaveKey::F(n));
        }
        None
    }

    pub fn label(self) -> String {
        match self {
            LeaveKey::Ctrl(c) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            LeaveKey::F(n) => format!("F{n}"),
        }
    }

    /// True for this key in any of its encodings. `Ctrl-]` is 0x1D, which legacy decoders may
    /// report as `Ctrl-]` or as `Ctrl-5` (0x1C–0x1F map to `Ctrl-4`…`Ctrl-7` in crossterm's
    /// Unix parser).
    pub fn matches(self, key: &KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        match (self, key.code) {
            (LeaveKey::F(n), KeyCode::F(k)) => n == k && key.modifiers.is_empty(),
            (LeaveKey::Ctrl(c), KeyCode::Char(k))
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let k = k.to_ascii_lowercase();
                k == c || ctrl_byte(k).is_some_and(|b| Some(b) == ctrl_byte(c))
            }
            _ => false,
        }
    }

    /// The bytes that send this key itself to the agent (the leave key pressed twice).
    pub fn bytes(self) -> Vec<u8> {
        match self {
            LeaveKey::Ctrl(c) => ctrl_byte(c).map_or_else(Vec::new, |b| vec![b]),
            LeaveKey::F(n) => function_key(n, false).unwrap_or_default(),
        }
    }
}

/// The control byte for `Ctrl` + `c` (`a`…`z`, `@[\]^_`, and the digit aliases `2`…`8`).
fn ctrl_byte(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

/// The xterm modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4).
fn modifier_param(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
}

fn function_key(n: u8, _app: bool) -> Option<Vec<u8>> {
    Some(match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => return None,
    })
}

/// Encodes a key for the agent. `app_cursor`: the agent enabled application cursor keys.
/// Returns `None` for key releases and keys with no terminal encoding.
pub fn encode(key: &KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let m = key.modifiers;
    let ctrl = m.contains(KeyModifiers::CONTROL);
    let alt = m.contains(KeyModifiers::ALT);
    let shift = m.contains(KeyModifiers::SHIFT);
    let modded = m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let mp = modifier_param(m);
    // CSI letter keys: arrows, Home, End.
    let csi_letter = |l: u8| -> Vec<u8> {
        if modded {
            format!("\x1b[1;{mp}{}", l as char).into_bytes()
        } else if app_cursor {
            vec![0x1b, b'O', l]
        } else {
            vec![0x1b, b'[', l]
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modded {
            format!("\x1b[{n};{mp}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let mut out = match key.code {
        KeyCode::Char(c) => {
            if ctrl && !alt {
                vec![ctrl_byte(c)?]
            } else if ctrl && alt {
                // AltGr on Windows arrives as Ctrl+Alt and produces text.
                c.to_string().into_bytes()
            } else {
                let mut b = c.to_string().into_bytes();
                if alt {
                    b.insert(0, 0x1b);
                }
                return Some(b);
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab if shift => b"\x1b[Z".to_vec(),
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace if ctrl => vec![0x08],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi_letter(b'A'),
        KeyCode::Down => csi_letter(b'B'),
        KeyCode::Right => csi_letter(b'C'),
        KeyCode::Left => csi_letter(b'D'),
        KeyCode::Home => csi_letter(b'H'),
        KeyCode::End => csi_letter(b'F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n) => {
            if modded {
                let base = function_key(n, false)?;
                // ESC O P → ESC [ 1 ; m P ; ESC [ 15 ~ → ESC [ 15 ; m ~
                if base.get(1) == Some(&b'O') {
                    format!("\x1b[1;{mp}{}", base[2] as char).into_bytes()
                } else {
                    let num = std::str::from_utf8(&base[2..base.len() - 1]).ok()?;
                    format!("\x1b[{num};{mp}~").into_bytes()
                }
            } else {
                function_key(n, app_cursor)?
            }
        }
        _ => return None,
    };
    // Alt with a non-character key that has no modifier form yet: ESC prefix.
    if alt && matches!(key.code, KeyCode::Enter | KeyCode::Backspace | KeyCode::Esc) {
        out.insert(0, 0x1b);
    }
    Some(out)
}

/// A paste for the agent: wrapped in bracketed-paste markers when the agent enabled them
/// (`?2004`), otherwise sent as typed text with line breaks as Enter.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    // The paste end marker inside pasted text would end the paste early; drop it.
    let clean = text.replace("\x1b[201~", "");
    if bracketed {
        let mut v = b"\x1b[200~".to_vec();
        v.extend_from_slice(clean.as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        clean.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}
