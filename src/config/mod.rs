//! Where Fallow keeps things, and its settings (006 contracts/config.md).
//!
//! The more specific value wins: Fallow's settings, then the project's squad file, then the
//! run's own options (FR-037, FR-038).

pub mod merge;
pub mod settings;
pub mod squad;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

pub use settings::{AgentDef, Autonomy, Defaults, RoleChoice, Settings};
pub use squad::{Failures, GateDef, Squad};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warning,
    Error,
}

/// A configuration problem, shown as `file:line: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub file: String,
    pub line: Option<usize>,
    pub level: Level,
    pub message: String,
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}: {}", self.file, line, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// The resolved configuration directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    /// `None` when no config directory can be determined.
    pub dir: Option<PathBuf>,
}

impl ConfigPaths {
    /// `--config-dir` if given, otherwise `<platform config dir>/fallow/`.
    pub fn resolve(override_dir: Option<&Path>) -> Self {
        let dir = match override_dir {
            Some(d) => Some(d.to_path_buf()),
            None => platform_config_dir().map(|d| d.join("fallow")),
        };
        ConfigPaths { dir }
    }

    pub fn settings(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("settings.toml"))
    }
}

fn this_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    }
}

/// `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or
/// `~/.config` on Linux. Reads only these path variables, never stores or shows them.
fn platform_config_dir() -> Option<PathBuf> {
    let var = std::env::var_os;
    config_dir_for(
        this_os(),
        var("HOME"),
        var("XDG_CONFIG_HOME"),
        var("APPDATA"),
    )
}

/// Platform families for directory resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

/// The platform config directory (without the `fallow` component) from the given environment
/// values. Empty values count as unset. A relative `XDG_CONFIG_HOME` is ignored, as the XDG Base
/// Directory spec requires.
pub fn config_dir_for(
    os: Os,
    home: Option<OsString>,
    xdg_config_home: Option<OsString>,
    appdata: Option<OsString>,
) -> Option<PathBuf> {
    let set = |v: Option<OsString>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        Os::Windows => set(appdata),
        Os::MacOs => set(home).map(|h| h.join("Library").join("Application Support")),
        Os::Linux => set(xdg_config_home)
            .filter(|p| p.as_os_str().as_encoded_bytes().first() == Some(&b'/'))
            .or_else(|| set(home).map(|h| h.join(".config"))),
    }
}

/// The platform data directory (without the `fallow` component): `%LOCALAPPDATA%`,
/// `~/Library/Caches`, or `$XDG_CACHE_HOME` (absolute) else `~/.cache`.
pub fn cache_dir_for(
    os: Os,
    home: Option<OsString>,
    xdg_cache_home: Option<OsString>,
    localappdata: Option<OsString>,
) -> Option<PathBuf> {
    let set = |v: Option<OsString>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        Os::Windows => set(localappdata),
        Os::MacOs => set(home).map(|h| h.join("Library").join("Caches")),
        Os::Linux => set(xdg_cache_home)
            .filter(|p| p.as_os_str().as_encoded_bytes().first() == Some(&b'/'))
            .or_else(|| set(home).map(|h| h.join(".cache"))),
    }
}

/// `<platform data dir>/fallow/`: runs, records and the service file (contracts/storage.md).
pub fn data_dir() -> Option<PathBuf> {
    let var = std::env::var_os;
    cache_dir_for(
        this_os(),
        var("HOME"),
        var("XDG_CACHE_HOME"),
        var("LOCALAPPDATA"),
    )
    .map(|d| d.join("fallow"))
}
