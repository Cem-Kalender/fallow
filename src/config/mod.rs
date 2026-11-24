//! Where Fallow keeps things, and its settings (006 contracts/config.md; 007 research S-16,
//! contracts/storage.md).
//!
//! The engineer's own configuration lives in `~/.fallow/`: `settings.toml`, `agents/` and
//! `pipelines/`. 006 kept the settings in the platform config directory; they are moved once.
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
    /// 006's settings file, moved into `dir` once (`None` with `--config-dir`).
    pub legacy_settings: Option<PathBuf>,
}

impl ConfigPaths {
    /// `--config-dir` if given, otherwise `~/.fallow/`.
    pub fn resolve(override_dir: Option<&Path>) -> Self {
        match override_dir {
            Some(d) => ConfigPaths {
                dir: Some(d.to_path_buf()),
                legacy_settings: None,
            },
            None => ConfigPaths {
                dir: home_config_dir(),
                legacy_settings: platform_config_dir()
                    .map(|d| d.join("fallow").join("settings.toml")),
            },
        }
    }

    pub fn settings(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("settings.toml"))
    }

    /// The engineer's agent definitions (contracts/agent-definition.md).
    pub fn agents(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("agents"))
    }

    /// The engineer's pipelines (contracts/pipeline.md).
    pub fn pipelines(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("pipelines"))
    }

    /// Creates the directory, readable by the current user only, and moves 006's settings file
    /// into it when it has none yet. Returns what it did, for the service's log.
    pub fn prepare(&self) -> Result<Option<String>, String> {
        let Some(dir) = &self.dir else {
            return Ok(None);
        };
        crate::platform::private_dir(dir)?;
        let (Some(new), Some(old)) = (self.settings(), self.legacy_settings.as_ref()) else {
            return Ok(None);
        };
        if new.exists() || !old.is_file() {
            return Ok(None);
        }
        std::fs::copy(old, &new)
            .map_err(|e| format!("cannot move {} to {}: {e}", old.display(), new.display()))?;
        let _ = std::fs::remove_file(old);
        Ok(Some(format!(
            "moved the settings from {} to {}",
            old.display(),
            new.display()
        )))
    }
}

/// `~/.fallow/`: `%USERPROFILE%\.fallow` on Windows, `$HOME/.fallow` elsewhere.
fn home_config_dir() -> Option<PathBuf> {
    let var = std::env::var_os;
    home_config_dir_for(this_os(), var("HOME"), var("USERPROFILE"))
}

/// `~/.fallow/` from the given environment values. Empty values count as unset.
pub fn home_config_dir_for(
    os: Os,
    home: Option<OsString>,
    userprofile: Option<OsString>,
) -> Option<PathBuf> {
    let set = |v: Option<OsString>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        Os::Windows => set(userprofile).or_else(|| set(home)),
        Os::MacOs | Os::Linux => set(home),
    }
    .map(|h| h.join(".fallow"))
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

/// 006's config directory, read only to move its settings: `%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config` on Linux. Reads
/// only these path variables, never stores or shows them.
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

/// Fallow's data folder (runs, records and the service file, contracts/storage.md) from the given
/// environment values: `<platform data dir>/fallow` on Windows and Linux; on macOS
/// `~/Library/Application Support/Fallow` (016 research M-14: never in Caches, which macOS may
/// empty).
pub fn data_dir_for(
    os: Os,
    home: Option<OsString>,
    xdg_cache_home: Option<OsString>,
    localappdata: Option<OsString>,
) -> Option<PathBuf> {
    match os {
        Os::MacOs => home
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join("Library/Application Support/Fallow")),
        _ => cache_dir_for(os, home, xdg_cache_home, localappdata).map(|d| d.join("fallow")),
    }
}

/// Fallow's data folder on this machine (`data_dir_for`).
pub fn data_dir() -> Option<PathBuf> {
    let var = std::env::var_os;
    data_dir_for(
        this_os(),
        var("HOME"),
        var("XDG_CACHE_HOME"),
        var("LOCALAPPDATA"),
    )
}

/// 016 (research M-14): where Fallow kept its data on macOS before 016,
/// `~/Library/Caches/fallow`; `None` elsewhere.
pub fn old_data_dir() -> Option<PathBuf> {
    if this_os() != Os::MacOs {
        return None;
    }
    cache_dir_for(Os::MacOs, std::env::var_os("HOME"), None, None).map(|d| d.join("fallow"))
}

/// Whether a folder holds any of Fallow's data. What a folder may hold without counting: 006's
/// settings file (macOS kept it in `~/Library/Application Support/fallow`, the same folder as
/// `…/Fallow` on a case-insensitive volume, until it moved to `~/.fallow`), Finder's `.DS_Store`
/// and the service's sockets (a test run makes one).
fn holds_data(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        !(name == "settings.toml"
            || name == ".DS_Store"
            || name == MOVING
            || name.starts_with("service.sock"))
    })
}

/// 016: in the new data folder while the old one's entries are moved into it one by one; a
/// start that finds it finishes the move (review: never two folders after a failure midway).
const MOVING: &str = ".moving-from-caches";

/// 016 (FR-032, research M-14): moves Fallow's data from `old` to `new` once, at the service's
/// start: when `old` exists and `new` holds none of Fallow's data. A rename on the same volume;
/// when `new` exists (with nothing of Fallow's in it) the old folder's entries are renamed into
/// it, marked as under way so that a start after a failure midway finishes it. Returns the line for the service log; an error stops the service (it never runs on two
/// folders).
pub fn move_old_data(old: &Path, new: &Path) -> Result<Option<String>, String> {
    let unfinished = new.join(MOVING).exists();
    if !old.is_dir() || (holds_data(new) && !unfinished) {
        return Ok(None);
    }
    let fail = |e: std::io::Error| {
        format!(
            "cannot move Fallow's data from {} to {}: {e}",
            old.display(),
            new.display()
        )
    };
    if !new.exists() {
        if let Some(parent) = new.parent() {
            std::fs::create_dir_all(parent).map_err(fail)?;
        }
        std::fs::rename(old, new).map_err(fail)?;
    } else {
        std::fs::write(new.join(MOVING), old.display().to_string()).map_err(fail)?;
        for e in std::fs::read_dir(old).map_err(fail)? {
            let e = e.map_err(fail)?;
            let name = e.file_name();
            // A stale socket stays behind; the new service makes its own.
            if name.to_string_lossy().starts_with("service.sock") {
                let _ = std::fs::remove_file(e.path());
                continue;
            }
            std::fs::rename(e.path(), new.join(&name)).map_err(fail)?;
        }
        let _ = std::fs::remove_dir(old);
        std::fs::remove_file(new.join(MOVING)).map_err(fail)?;
    }
    Ok(Some(format!(
        "moved Fallow's data from {} to {}",
        old.display(),
        new.display()
    )))
}
