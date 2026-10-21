//! Settings, signatures and providers: built-in defaults merged with user files (R-9).
//!
//! Files are read once at startup and again only by the "Reload configuration" command.

pub mod merge;
pub mod schema;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

pub use schema::{CwdPolicy, Kind, LocalServer, NameGlob, Provider, Settings, Signature};

use schema::{
    PROVIDERS_SHAPE, RawProviderFile, RawSettings, RawSignatureFile, SETTINGS_SHAPE,
    SIGNATURES_SHAPE,
};

pub const DEFAULT_SETTINGS: &str = include_str!("../../assets/defaults/settings.toml");
pub const DEFAULT_SIGNATURES: &str = include_str!("../../assets/defaults/signatures.toml");
pub const DEFAULT_PROVIDERS: &str = include_str!("../../assets/defaults/providers.toml");

const SETTINGS_FILE: &str = "settings.toml";
const SIGNATURES_FILE: &str = "signatures.toml";
const PROVIDERS_FILE: &str = "providers.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Warning,
    Error,
}

/// A configuration problem, shown in the UI as `file:line: message`.
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

/// The resolved configuration directory and the three file paths in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    /// `None` when no config directory can be determined (no home directory).
    pub dir: Option<PathBuf>,
}

impl ConfigPaths {
    /// `--config-dir` if given, otherwise `<platform config dir>/fallow/` (contracts/config.md).
    pub fn resolve(override_dir: Option<&Path>) -> Self {
        let dir = match override_dir {
            Some(d) => Some(d.to_path_buf()),
            None => platform_config_dir().map(|d| d.join("fallow")),
        };
        ConfigPaths { dir }
    }

    pub fn settings(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(SETTINGS_FILE))
    }

    pub fn signatures(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(SIGNATURES_FILE))
    }

    pub fn providers(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(PROVIDERS_FILE))
    }
}

/// `$XDG_CONFIG_HOME` or `~/.config` on Linux, `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows. Reads only Fallow's own environment.
fn platform_config_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    };
    let var = std::env::var_os;
    config_dir_for(os, var("HOME"), var("XDG_CONFIG_HOME"), var("APPDATA"))
}

/// Platform families for config-directory resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

/// The platform config directory (without the `fallow` component) from the given environment
/// values, per contracts/config.md "Location". Empty values count as unset. A relative
/// `XDG_CONFIG_HOME` is ignored, as the XDG Base Directory spec requires.
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
        // Unix rules, checked by bytes so the result doesn't depend on the host OS.
        Os::Linux => set(xdg_config_home)
            .filter(|p| p.as_os_str().as_encoded_bytes().first() == Some(&b'/'))
            .or_else(|| set(home).map(|h| h.join(".config"))),
    }
}

/// The platform cache directory (without the `fallow` component), by the same rules as
/// `config_dir_for`: `%LOCALAPPDATA%`, `~/Library/Caches`, or `$XDG_CACHE_HOME` (absolute) else
/// `~/.cache`. Fallow keeps only transient run files there (003 contracts/config.md).
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

/// `<platform cache dir>/fallow/run/`: the orchestration channel and per-run agent files live
/// here while Fallow runs (003 research R-2).
pub fn run_dir() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Linux
    };
    let var = std::env::var_os;
    cache_dir_for(os, var("HOME"), var("XDG_CACHE_HOME"), var("LOCALAPPDATA"))
        .map(|d| d.join("fallow").join("run"))
}

/// The merged configuration plus every problem found while loading it.
#[derive(Debug, Clone)]
pub struct Config {
    pub settings: Settings,
    pub signatures: Vec<Signature>,
    pub providers: Vec<Provider>,
    pub notices: Vec<Notice>,
}

/// A user file's display name and text.
#[derive(Debug, Clone, Copy)]
pub struct UserFile<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// Reads the user files under `paths` (missing files are fine) and merges them with the
/// built-in defaults.
pub fn load(paths: &ConfigPaths) -> Config {
    let mut notices = Vec::new();
    let mut read = |p: Option<PathBuf>| -> Option<(String, String)> {
        let p = p?;
        let name = p.display().to_string();
        match std::fs::read_to_string(&p) {
            Ok(text) => Some((name, text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                notices.push(Notice {
                    file: name,
                    line: None,
                    level: Level::Error,
                    message: format!("cannot read: {e} (this file's settings are ignored)"),
                });
                None
            }
        }
    };
    let s = read(paths.settings());
    let g = read(paths.signatures());
    let p = read(paths.providers());
    let mut config = load_from(as_file(&s), as_file(&g), as_file(&p));
    notices.append(&mut config.notices);
    config.notices = notices;
    config
}

/// Merges the built-in defaults with the given user files. Never fails: every problem becomes
/// a notice and the affected value or entry falls back to the default.
pub fn load_from(
    settings: Option<UserFile<'_>>,
    signatures: Option<UserFile<'_>>,
    providers: Option<UserFile<'_>>,
) -> Config {
    let mut notices = Vec::new();

    let mut merged_settings = Settings::default();
    let builtin = format!("<built-in> {SETTINGS_FILE}");
    for (file, text) in [(builtin.as_str(), DEFAULT_SETTINGS)]
        .into_iter()
        .chain(settings.map(|f| (f.name, f.text)))
    {
        if let Some(raw) =
            merge::parse_file::<RawSettings>(file, text, &SETTINGS_SHAPE, &mut notices)
        {
            for bad in schema::apply_settings(&mut merged_settings, raw) {
                notices.push(invalid(file, text, bad));
            }
        }
    }

    let builtin = format!("<built-in> {PROVIDERS_FILE}");
    let builtin_providers = load_providers(&builtin, DEFAULT_PROVIDERS, &mut notices);
    let user_providers = providers
        .map(|f| load_providers(f.name, f.text, &mut notices))
        .unwrap_or_default();
    let providers = merge::merge_by_id(builtin_providers, user_providers);

    let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
    let builtin = format!("<built-in> {SIGNATURES_FILE}");
    let builtin_signatures = load_signatures(&builtin, DEFAULT_SIGNATURES, &ids, &mut notices);
    let user_signatures = signatures
        .map(|f| load_signatures(f.name, f.text, &ids, &mut notices))
        .unwrap_or_default();
    let signatures = merge::merge_by_id(builtin_signatures, user_signatures);

    Config {
        settings: merged_settings,
        signatures,
        providers,
        notices,
    }
}

fn invalid(file: &str, text: &str, bad: schema::Invalid) -> Notice {
    Notice {
        file: file.to_string(),
        line: Some(merge::line_of(text, bad.span.start)),
        level: Level::Error,
        message: format!("{} (the default is kept)", bad.message),
    }
}

fn load_providers(file: &str, text: &str, notices: &mut Vec<Notice>) -> Vec<Provider> {
    let Some(raw) = merge::parse_file::<RawProviderFile>(file, text, &PROVIDERS_SHAPE, notices)
    else {
        return Vec::new();
    };
    let mut valid = Vec::new();
    for entry in raw.provider {
        let span = entry.span();
        let line = merge::line_of(text, span.start);
        match schema::validate_provider(entry.into_inner(), span) {
            Ok(p) => valid.push((p, line)),
            Err(bad) => notices.push(invalid(file, text, bad)),
        }
    }
    merge::dedup_last(valid, file, notices)
}

fn load_signatures(
    file: &str,
    text: &str,
    provider_ids: &[&str],
    notices: &mut Vec<Notice>,
) -> Vec<Signature> {
    let Some(raw) = merge::parse_file::<RawSignatureFile>(file, text, &SIGNATURES_SHAPE, notices)
    else {
        return Vec::new();
    };
    let mut valid = Vec::new();
    for entry in raw.signature {
        let span = entry.span();
        let line = merge::line_of(text, span.start);
        match schema::validate_signature(entry.into_inner(), span, provider_ids) {
            Ok(s) => valid.push((s, line)),
            Err(bad) => notices.push(invalid(file, text, bad)),
        }
    }
    merge::dedup_last(valid, file, notices)
}

fn as_file(f: &Option<(String, String)>) -> Option<UserFile<'_>> {
    f.as_ref().map(|(name, text)| UserFile {
        name: name.as_str(),
        text: text.as_str(),
    })
}

/// Text for `--print-config-paths`.
pub fn describe_paths(paths: &ConfigPaths) -> String {
    match &paths.dir {
        None => "config directory: (none: no home directory found; built-in defaults only)\n"
            .to_string(),
        Some(dir) => {
            let mut out = format!("config directory: {}\n", dir.display());
            for p in [paths.settings(), paths.signatures(), paths.providers()]
                .into_iter()
                .flatten()
            {
                let state = if p.is_file() {
                    "present"
                } else {
                    "not present"
                };
                out.push_str(&format!("{} ({state})\n", p.display()));
            }
            out
        }
    }
}
