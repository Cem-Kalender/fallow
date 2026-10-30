//! Config directory resolution per platform (contracts/config.md "Location"). The resolver is
//! pure, so every platform's rules are tested on every host.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use fallow::config::{ConfigPaths, Os, config_dir_for};

fn s(v: &str) -> Option<OsString> {
    Some(OsString::from(v))
}

#[test]
fn linux_prefers_absolute_xdg_config_home() {
    assert_eq!(
        config_dir_for(Os::Linux, s("/home/u"), s("/xdg/conf"), None),
        Some(PathBuf::from("/xdg/conf"))
    );
}

#[test]
fn linux_falls_back_to_dot_config() {
    let want = Some(PathBuf::from("/home/u/.config"));
    assert_eq!(
        config_dir_for(Os::Linux, s("/home/u"), None, None),
        want,
        "unset"
    );
    assert_eq!(
        config_dir_for(Os::Linux, s("/home/u"), s(""), None),
        want,
        "empty"
    );
    assert_eq!(
        config_dir_for(Os::Linux, s("/home/u"), s("relative/conf"), None),
        want,
        "a relative XDG_CONFIG_HOME is ignored"
    );
}

#[test]
fn linux_without_home_or_xdg_has_no_config_dir() {
    assert_eq!(config_dir_for(Os::Linux, None, None, None), None);
    assert_eq!(config_dir_for(Os::Linux, s(""), None, None), None);
}

#[test]
fn macos_uses_application_support_and_ignores_xdg() {
    assert_eq!(
        config_dir_for(Os::MacOs, s("/Users/u"), s("/xdg/conf"), None),
        Some(
            Path::new("/Users/u")
                .join("Library")
                .join("Application Support")
        )
    );
    assert_eq!(config_dir_for(Os::MacOs, None, s("/xdg/conf"), None), None);
}

#[test]
fn windows_uses_appdata_only() {
    let appdata = r"C:\Users\u\AppData\Roaming";
    assert_eq!(
        config_dir_for(Os::Windows, s(r"C:\Users\u"), s("/xdg"), s(appdata)),
        Some(PathBuf::from(appdata))
    );
    assert_eq!(
        config_dir_for(Os::Windows, s(r"C:\Users\u"), None, None),
        None
    );
    assert_eq!(config_dir_for(Os::Windows, None, None, s("")), None);
}

#[test]
fn config_dir_override_wins_and_files_live_inside() {
    let p = ConfigPaths::resolve(Some(Path::new("/custom")));
    assert_eq!(p.dir, Some(PathBuf::from("/custom")));
    assert_eq!(
        p.settings(),
        Some(Path::new("/custom").join("settings.toml"))
    );
}

#[test]
fn default_dir_is_dot_fallow_in_home() {
    // 007 research S-16: the engineer's configuration lives in ~/.fallow/.
    if let Some(dir) = ConfigPaths::resolve(None).dir {
        assert_eq!(dir.file_name().unwrap(), ".fallow");
    }
    use fallow::config::home_config_dir_for;
    let s = |v: &str| Some(std::ffi::OsString::from(v));
    assert_eq!(
        home_config_dir_for(Os::Windows, s("/h"), s(r"C:\Users\u")),
        Some(PathBuf::from(r"C:\Users\u").join(".fallow"))
    );
    assert_eq!(
        home_config_dir_for(Os::Linux, s("/home/u"), None),
        Some(PathBuf::from("/home/u/.fallow"))
    );
    assert_eq!(home_config_dir_for(Os::Linux, s(""), None), None);
    let p = ConfigPaths::resolve(Some(Path::new("/custom")));
    assert_eq!(p.agents(), Some(Path::new("/custom").join("agents")));
    assert_eq!(p.pipelines(), Some(Path::new("/custom").join("pipelines")));
    assert_eq!(p.legacy_settings, None);
}

#[test]
fn the_006_settings_move_once() {
    let root = std::env::temp_dir().join(format!("fallow-cfgmove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old = root.join("old").join("settings.toml");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, "[defaults]\nrepair_cap = 4\n").unwrap();
    let p = ConfigPaths {
        dir: Some(root.join("new")),
        legacy_settings: Some(old.clone()),
    };
    let said = p.prepare().unwrap();
    assert!(said.unwrap().contains("moved"));
    assert!(!old.exists());
    assert!(std::fs::read_to_string(p.settings().unwrap())
        .unwrap()
        .contains("repair_cap = 4"));
    // A second start finds the new file and moves nothing.
    std::fs::write(&old, "stale").unwrap();
    assert_eq!(p.prepare().unwrap(), None);
    assert!(old.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cache_dir_per_platform() {
    // 003 contracts/config.md "Files Fallow creates outside the config directory".
    use fallow::config::cache_dir_for;
    let s = |v: &str| Some(std::ffi::OsString::from(v));
    assert_eq!(
        cache_dir_for(Os::Linux, s("/home/u"), s("/xdg/cache"), None),
        Some("/xdg/cache".into())
    );
    assert_eq!(
        cache_dir_for(Os::Linux, s("/home/u"), s("rel"), None),
        Some(std::path::Path::new("/home/u").join(".cache"))
    );
    assert_eq!(
        cache_dir_for(Os::MacOs, s("/Users/u"), None, None),
        Some(
            std::path::Path::new("/Users/u")
                .join("Library")
                .join("Caches")
        )
    );
    assert_eq!(
        cache_dir_for(Os::Windows, None, None, s(r"C:\Users\u\AppData\Local")),
        Some(r"C:\Users\u\AppData\Local".into())
    );
    assert_eq!(cache_dir_for(Os::Windows, s("/x"), None, None), None);
}
