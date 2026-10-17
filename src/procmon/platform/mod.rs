//! The Fallow-owned platform layer (plan.md, Project Structure; research R-1).
//!
//! The sensitive and platform-specific reads live here, behind one trait with one file per OS,
//! so that Fallow controls exactly what is read from other processes.
//!
//! No method may request or read a process environment (FR-024, Constitution VII).

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use super::snapshot::{Footprint, ProcessKey};

pub mod parse;

pub use parse::{RawConn, TcpQuery};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::OsPlatform;
#[cfg(target_os = "linux")]
pub(crate) use linux::read_proc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::OsPlatform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{OsPlatform, split_command_line};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// Not implemented on this platform (yet).
    Unsupported,
    /// The OS refused; Fallow never asks for elevation (Constitution V).
    PermissionDenied,
    /// The process no longer exists, or its PID now belongs to a different process.
    Gone,
    Other(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlatformError::Unsupported => f.write_str("not supported on this platform"),
            PlatformError::PermissionDenied => f.write_str("permission denied"),
            PlatformError::Gone => f.write_str("process is gone"),
            PlatformError::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PlatformError {}

impl From<std::io::Error> for PlatformError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => PlatformError::Gone,
            std::io::ErrorKind::PermissionDenied => PlatformError::PermissionDenied,
            _ => PlatformError::Other(e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// Per-OS reads and actions. Callers pass keys from the current refresh; the worker drops
/// records whose (PID, start time) changed.
///
/// No method may request or read a process environment (FR-024, Constitution VII).
pub trait Platform {
    /// The argv of the process, without its environment.
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>>;

    /// The working directory.
    fn cwd(&mut self, key: ProcessKey) -> Result<PathBuf>;

    /// Memory in the FR-019 metric for this platform.
    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint>;

    /// The connections that can matter (data-model.md "Connection"), selected per
    /// `parse::select`, appended to `out`.
    fn tcp_table(&mut self, query: &TcpQuery<'_>, out: &mut Vec<RawConn>) -> Result<()>;

    /// Pause (SIGSTOP; NtSuspendProcess on Windows, R-7).
    fn suspend(&mut self, key: ProcessKey) -> Result<()>;

    /// Resume (SIGCONT; NtResumeProcess on Windows, R-7).
    fn resume(&mut self, key: ProcessKey) -> Result<()>;

    /// SIGTERM, or SIGKILL when `force` (Linux and macOS); TerminateProcess on Windows.
    fn kill(&mut self, key: ProcessKey, force: bool) -> Result<()>;
}

/// True when Fallow itself runs elevated (root, or an elevated Administrator token). Used only
/// to decide whether `--all-users` may take effect (FR-031); Fallow never asks for elevation.
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        // SAFETY: plain Win32 calls on our own process token, closed before returning.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut e as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            CloseHandle(token);
            ok != 0 && e.TokenIsElevated != 0
        }
    }
}
