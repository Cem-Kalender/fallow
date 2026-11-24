//! 016 (FR-033, research M-6, data-model "Page settings", T029): the chat panels' width per
//! screen is kept with Fallow's settings (`prefs.json`), on every platform, not in the web view's
//! storage (Fallow's Mac window keeps none).

use fallow::studio::files::{chat_wide, set_chat_wide, set_theme, theme};
use serde_json::{Map, Value, json};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-chat-wide-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn screens(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn missing_means_narrow_everywhere() {
    let d = dir("missing");
    assert_eq!(chat_wide(&d), json!({}));
    std::fs::write(d.join("prefs.json"), "not json").unwrap();
    assert_eq!(chat_wide(&d), json!({}));
    std::fs::write(d.join("prefs.json"), r#"{"chat_wide": "wide"}"#).unwrap();
    assert_eq!(chat_wide(&d), json!({}));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_width_is_kept_per_screen_and_read_back() {
    let d = dir("write");
    assert_eq!(
        set_chat_wide(&d, &screens(json!({ "project": true }))).unwrap(),
        json!({ "project": true })
    );
    assert_eq!(
        set_chat_wide(&d, &screens(json!({ "spec": true }))).unwrap(),
        json!({ "project": true, "spec": true })
    );
    assert_eq!(
        set_chat_wide(&d, &screens(json!({ "project": false }))).unwrap(),
        json!({ "project": false, "spec": true })
    );
    assert_eq!(chat_wide(&d), json!({ "project": false, "spec": true }));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unknown_screens_and_the_other_settings_are_kept() {
    let d = dir("keep");
    std::fs::write(
        d.join("prefs.json"),
        r#"{"theme":"dark","chat_wide":{"a-later-screen":true},"editor":{"program":"code","args":[]}}"#,
    )
    .unwrap();
    set_chat_wide(&d, &screens(json!({ "project": true }))).unwrap();
    assert_eq!(
        chat_wide(&d),
        json!({ "a-later-screen": true, "project": true })
    );
    assert_eq!(theme(&d), "dark");
    set_theme(&d, "light").unwrap();
    assert_eq!(chat_wide(&d)["project"], true);
    let text = std::fs::read_to_string(d.join("prefs.json")).unwrap();
    assert!(text.contains("\"program\": \"code\""), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn only_screen_names_to_true_or_false_are_taken() {
    let d = dir("bad");
    for bad in [
        json!({}),
        json!({ "project": "yes" }),
        json!({ "": true }),
        json!({ "x".repeat(65): true }),
    ] {
        assert!(set_chat_wide(&d, &screens(bad.clone())).is_err(), "{bad}");
    }
    assert!(!d.join("prefs.json").exists());
    let _ = std::fs::remove_dir_all(&d);
}
