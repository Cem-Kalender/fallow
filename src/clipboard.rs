//! The clipboard (FR-008, T075; contracts/ui-commands.md "Selection and clipboard").
//!
//! Copy and cut **always** emit an OSC 52 sequence **and** write the platform clipboard through
//! `arboard`. The platform clipboard is opened lazily on first use, never at startup (plan.md
//! Dependency budget: on X11 arboard keeps a helper thread alive after a copy). A failure on one
//! side is silent; only when both fail does the caller say so.

use std::io::Write;

/// Encodes `bytes` as standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The OSC 52 "set clipboard" sequence: `ESC ] 52 ; c ; <base64> BEL`.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// What a copy reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyResult {
    pub osc52: bool,
    pub platform: bool,
}

impl CopyResult {
    pub fn failed(&self) -> bool {
        !self.osc52 && !self.platform
    }
}

enum Platform {
    /// Not opened yet.
    Unopened,
    Open(arboard::Clipboard),
    /// Could not be opened (no display, over SSH, …).
    Unavailable,
    /// Tests: never touch the system clipboard.
    Disabled,
}

/// Where OSC 52 sequences go.
enum Osc {
    Stdout,
    Capture(Vec<u8>),
    Broken,
}

pub struct Clipboard {
    platform: Platform,
    osc: Osc,
    /// Copies made (for `--bench-stats`).
    pub copies: u64,
}

impl std::fmt::Debug for Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let p = match self.platform {
            Platform::Unopened => "unopened",
            Platform::Open(_) => "open",
            Platform::Unavailable => "unavailable",
            Platform::Disabled => "disabled",
        };
        write!(f, "Clipboard({p})")
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Clipboard {
            platform: Platform::Unopened,
            osc: Osc::Stdout,
            copies: 0,
        }
    }
}

impl Clipboard {
    /// A clipboard that records OSC 52 output and never touches the system clipboard (tests).
    pub fn detached() -> Self {
        Clipboard {
            platform: Platform::Disabled,
            osc: Osc::Capture(Vec::new()),
            copies: 0,
        }
    }

    /// Like `detached`, with neither side working (tests of the "both failed" message).
    pub fn broken() -> Self {
        Clipboard {
            platform: Platform::Disabled,
            osc: Osc::Broken,
            copies: 0,
        }
    }

    /// The OSC 52 bytes written so far by a detached clipboard.
    pub fn captured(&self) -> &[u8] {
        match &self.osc {
            Osc::Capture(v) => v,
            _ => &[],
        }
    }

    /// True once the platform clipboard has been opened (or tried).
    pub fn platform_touched(&self) -> bool {
        matches!(self.platform, Platform::Open(_) | Platform::Unavailable)
    }

    fn platform(&mut self) -> Option<&mut arboard::Clipboard> {
        if let Platform::Unopened = self.platform {
            self.platform = match arboard::Clipboard::new() {
                Ok(c) => Platform::Open(c),
                Err(_) => Platform::Unavailable,
            };
        }
        match &mut self.platform {
            Platform::Open(c) => Some(c),
            _ => None,
        }
    }

    pub fn copy(&mut self, text: &str) -> CopyResult {
        self.copies += 1;
        let seq = osc52(text);
        let osc52 = match &mut self.osc {
            Osc::Stdout => {
                let mut out = std::io::stdout().lock();
                out.write_all(seq.as_bytes())
                    .and_then(|_| out.flush())
                    .is_ok()
            }
            Osc::Capture(v) => {
                v.extend_from_slice(seq.as_bytes());
                true
            }
            Osc::Broken => false,
        };
        let platform = self
            .platform()
            .is_some_and(|c| c.set_text(text.to_string()).is_ok());
        CopyResult { osc52, platform }
    }

    /// Reads the platform clipboard (`Ctrl-V` as a key). `None` if it is unavailable.
    pub fn paste(&mut self) -> Option<String> {
        self.platform()?.get_text().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }
}
