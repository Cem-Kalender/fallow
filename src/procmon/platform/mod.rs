//! The Fallow-owned platform layer (plan.md, Project Structure; research R-1).
//!
//! The sensitive and platform-specific reads live here, behind one trait with one file per OS,
//! so that Fallow controls exactly what is read from other processes.
//!
//! No method may request or read a process environment (FR-024, Constitution VII).

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use super::snapshot::{Connection, Footprint, ProcessKey};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::OsPlatform;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::OsPlatform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::OsPlatform;

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

pub type Result<T> = std::result::Result<T, PlatformError>;

/// Per-OS reads and actions. Every method takes the full `ProcessKey` so implementations can
/// check that the PID still belongs to the same process.
///
/// No method may request or read a process environment (FR-024, Constitution VII).
pub trait Platform {
    /// The argv of the process, without its environment.
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>>;

    /// The working directory.
    fn cwd(&mut self, key: ProcessKey) -> Result<PathBuf>;

    /// Memory in the FR-019 metric for this platform.
    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint>;

    /// TCP connections owned by the given processes (keyed by PID), filtered to the states
    /// data-model.md keeps.
    fn tcp_table(&mut self, owners: &HashMap<u32, ProcessKey>) -> Result<Vec<Connection>>;

    /// Pause (SIGSTOP; NtSuspendProcess on Windows, R-7).
    fn suspend(&mut self, key: ProcessKey) -> Result<()>;

    /// Resume (SIGCONT; NtResumeProcess on Windows, R-7).
    fn resume(&mut self, key: ProcessKey) -> Result<()>;

    /// SIGTERM, or SIGKILL when `force` (Linux and macOS); TerminateProcess on Windows.
    fn kill(&mut self, key: ProcessKey, force: bool) -> Result<()>;
}
