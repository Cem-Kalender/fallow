//! 013: the page's themes, checked from `src/service/web/app.css` itself.
//! - Every colour comes from a name (FR-004).
//! - The light values are exactly the colours of before (FR-006).
//! - The dark theme names every colour, in two identical blocks (research R-2).
//! - Text and controls meet WCAG AA in both themes (FR-012, SC-002).

mod common;

use std::collections::BTreeMap;

const CSS: &str = include_str!("../src/service/web/app.css");

/// Custom properties the page sets at run time, or that are not colours.
const NOT_COLOURS: &[&str] = &["--share", "--top-h", "--sans", "--mono"];

/// The declarations `--name: value;` of the first block opened by `head` (the text up to its
/// `{`), and the CSS without that block.
fn take_block(css: &str, head: &str) -> Option<(BTreeMap<String, String>, String)> {
    let at = css.find(head)?;
    let open = at + css[at..].find('{')?;
    let close = open + css[open..].find('}')?;
    let mut decls = BTreeMap::new();
    for d in css[open + 1..close].split(';') {
        let d = d.trim();
        if let Some((k, v)) = d.split_once(':') {
            let k = k.trim();
            if k.starts_with("--") || k == "color-scheme" {
                decls.insert(k.to_string(), v.trim().to_string());
            }
        }
    }
    let mut rest = css[..at].to_string();
    rest.push_str(&css[close + 1..]);
    Some((decls, rest))
}

/// The CSS without comments, with `\r` dropped.
fn clean(css: &str) -> String {
    let css = css.replace('\r', "");
    let mut out = String::new();
    let mut rest = css.as_str();
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i..].find("*/") {
            Some(j) => rest = &rest[i + j + 2..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

struct Themes {
    light: BTreeMap<String, String>,
    /// The System dark block (`@media (prefers-color-scheme: dark) { :root:not(...) {...} }`).
    dark_media: Option<BTreeMap<String, String>>,
    /// The engineer's Dark (`:root[data-theme="dark"]`).
    dark_attr: Option<BTreeMap<String, String>>,
    /// Everything else.
    rest: String,
}

fn themes() -> Themes {
    let css = clean(CSS);
    let (light, mut rest) = take_block(&css, ":root {").expect("a :root block");
    let mut dark_media = None;
    if let Some(at) = rest.find("@media (prefers-color-scheme: dark)") {
        let inner = &rest[at..];
        let (decls, _) = take_block(inner, ":root:not([data-theme=\"light\"])").expect("the System dark block");
        // One rule only inside it, so all of it is compared with the Dark block (review).
        let block_end = {
            let open = inner.find('{').unwrap();
            let first_close = open + inner[open..].find('}').unwrap();
            first_close + 1 + inner[first_close + 1..].find('}').unwrap()
        };
        assert_eq!(inner[..block_end].matches('{').count(), 2, "the System dark block holds one rule");
        // Drop the whole @media block: its `:root` rule and the closing brace.
        let open = at + rest[at..].find('{').unwrap();
        let first_close = open + rest[open..].find('}').unwrap();
        let close = first_close + 1 + rest[first_close + 1..].find('}').unwrap();
        rest = format!("{}{}", &rest[..at], &rest[close + 1..]);
        dark_media = Some(decls);
    }
    let mut dark_attr = None;
    if let Some((decls, r)) = take_block(&rest, ":root[data-theme=\"dark\"] {") {
        dark_attr = Some(decls);
        rest = r;
    }
    if let Some((_, r)) = take_block(&rest, ":root[data-theme=\"light\"] {") {
        rest = r;
    }
    Themes { light, dark_media, dark_attr, rest }
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// The hex colours (`#abc`, `#abcd`, `#aabbcc`, `#aabbccdd`), `rgb(`, `rgba(`, `hsl(`, `hsla(`,
/// `color-mix(` and the plain colour keywords written in `text`.
fn literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '#' {
            let n = b[i + 1..].iter().take_while(|c| is_hex(**c)).count();
            // `#name` selectors start with a letter past `f`, or are followed by `-`/`_`.
            let next = b.get(i + 1 + n).copied().unwrap_or(' ');
            if (n == 3 || n == 4 || n == 6 || n == 8) && !(next.is_alphanumeric() || next == '-' || next == '_') {
                out.push(b[i..i + 1 + n].iter().collect());
            }
            i += 1 + n;
            continue;
        }
        i += 1;
    }
    let low = text.to_ascii_lowercase();
    for w in ["rgb(", "rgba(", "hsl(", "hsla(", "color-mix(", "hwb(", "lab(", "lch(", "oklab(", "oklch("] {
        for (k, _) in low.match_indices(w) {
            // A function name ending in the word (`--x-rgb(`) is not a colour.
            if k > 0 && (low.as_bytes()[k - 1].is_ascii_alphanumeric() || low.as_bytes()[k - 1] == b'-') {
                continue;
            }
            out.push(text[k..].chars().take(24).collect());
        }
    }
    // Keywords as a value: after `:` or a space, before `;`, a space, `)` or `!`.
    for w in ["white", "black", "red", "green", "blue", "gray", "grey", "silver", "orange", "yellow", "navy", "purple"] {
        for (k, _) in low.match_indices(w) {
            let before = low[..k].chars().next_back().unwrap_or(' ');
            let after = low[k + w.len()..].chars().next().unwrap_or(' ');
            if matches!(before, ':' | ' ' | '\t') && matches!(after, ';' | ' ' | ')' | '!' | ',' | '}' | '\n' | '\t') {
                // `.pill.grey` and `white-space` are not values.
                let line = low[..k].rsplit('\n').next().unwrap_or("");
                if line.contains(':') {
                    out.push(text[k..k + w.len()].to_string());
                }
            }
        }
    }
    out
}

#[test]
fn every_colour_comes_from_a_name() {
    let t = themes();
    // The one fallback allowed: an engine before Chrome 122 passes no custom property to
    // `::backdrop` (013 Phase 2 review), so the dialog's backdrop keeps the light one there.
    let rest = t.rest.replace("var(--backdrop, rgba(28, 28, 26, .35))", "var(--backdrop)");
    let left = literals(&rest);
    assert!(left.is_empty(), "colours written outside the theme blocks: {left:?}");
    // Every name used is defined, for the light theme at least.
    let mut missing = Vec::new();
    for (k, _) in t.rest.match_indices("var(--") {
        let name: String = t.rest[k + 4..].chars().take_while(|c| *c == '-' || c.is_ascii_alphanumeric()).collect();
        if !t.light.contains_key(&name) && !NOT_COLOURS.contains(&name.as_str()) {
            missing.push(name);
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "names used but not defined on :root: {missing:?}");
}

/// The light values of the names 013 added: exactly the literals they replaced (research R-1).
#[test]
fn the_light_values_are_the_colours_of_before() {
    let t = themes();
    let before = [
        ("--on-ink", "#FFFFFF"),
        ("--solid-hover", "#000000"),
        // Raised to AA's 3:1 (research R-6): before, #C9C8C1, #A3A29B, and for control borders
        // --line-3's #D4D3CC.
        ("--dot-grey", "#878680"),
        ("--arrow", "#878680"),
        ("--control-line", "#878680"),
        ("--bad-control", "#C1726B"),
        ("--backdrop", "rgba(28, 28, 26, .35)"),
        ("--marker", "#FDF1DC"),
        ("--minus", "#B42318"),
        ("--bar-rest", "#DCE3F8"),
        ("--phase-bg", "#F8FAFF"),
        ("--grid-dot", "#E9E8E2"),
        // Raised from #9C9B94 (R-6).
        ("--edge", "#878680"),
        ("--shadow-menu", "0 8px 24px rgba(0, 0, 0, .12)"),
        ("--shadow-panel", "0 10px 28px rgba(0, 0, 0, .14)"),
        // A few of the names that were there before 013, unchanged.
        ("--ground", "#F6F6F3"),
        ("--panel", "#FFFFFF"),
        ("--ink", "#1C1C1A"),
        ("--accent", "#2F52C9"),
        // Raised from #6E6D67 (R-6).
        ("--faint", "#6A6963"),
        // Decorative lines keep their grey: only controls moved to --control-line.
        ("--line-3", "#D4D3CC"),
        ("--term", "#1C1C1A"),
    ];
    for (k, v) in before {
        assert_eq!(t.light.get(k).map(String::as_str), Some(v), "{k}");
    }
    assert_eq!(t.light.get("color-scheme").map(String::as_str), Some("light"));
}

#[test]
fn the_two_dark_blocks_are_the_same_and_name_every_colour() {
    let t = themes();
    let media = t.dark_media.expect("the System dark block");
    let attr = t.dark_attr.expect("the Dark block");
    assert_eq!(media, attr, "the two dark blocks differ");
    let missing: Vec<&String> = t.light.keys().filter(|k| !NOT_COLOURS.contains(&k.as_str()) && !attr.contains_key(*k)).collect();
    assert!(missing.is_empty(), "names without a dark value: {missing:?}");
    assert_eq!(attr.get("color-scheme").map(String::as_str), Some("dark"));
}

/// WCAG 2.x relative luminance of `#rrggbb`.
fn luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    assert_eq!(h.len(), 6, "not #rrggbb: {hex}");
    let c = |i: usize| {
        let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * c(0) + 0.7152 * c(2) + 0.0722 * c(4)
}

fn ratio(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// Text on its backgrounds: at least 4.5:1 (FR-012).
const TEXT: &[(&str, &[&str])] = &[
    ("--ink", &["--ground", "--panel", "--panel-2", "--hover", "--chip", "--accent-bg", "--accent-bg-2", "--phase-bg", "--needs-panel", "--ok-panel", "--marker"]),
    ("--ink-2", &["--ground", "--panel", "--panel-2", "--hover", "--chip"]),
    ("--body", &["--ground", "--panel", "--panel-2", "--hover", "--chip", "--marker", "--needs-bg", "--needs-panel"]),
    ("--muted", &["--ground", "--panel", "--panel-2", "--hover", "--chip"]),
    ("--faint", &["--ground", "--panel", "--panel-2", "--hover", "--chip"]),
    ("--on-ink", &["--ink", "--solid-hover", "--spec"]),
    ("--accent", &["--ground", "--panel", "--panel-2", "--accent-bg", "--accent-bg-2"]),
    ("--accent-hover", &["--ground", "--panel"]),
    ("--needs", &["--needs-bg", "--panel", "--needs-panel"]),
    ("--needs-ink", &["--needs-bg", "--needs-panel", "--panel"]),
    ("--ok", &["--ok-bg", "--ok-panel", "--panel"]),
    ("--spec", &["--spec-bg", "--panel"]),
    ("--bad", &["--bad-bg", "--panel", "--ground"]),
    ("--warn", &["--warn-bg", "--panel"]),
    ("--teal", &["--teal-bg", "--panel"]),
    ("--diff-add-ink", &["--diff-add"]),
    ("--diff-del-ink", &["--diff-del"]),
    ("--term-ink", &["--term"]),
    ("--minus", &["--panel", "--panel-2"]),
];

/// Borders of controls, the focus outline and the parts of graphs that carry meaning: at least
/// 3:1 against what they sit on (FR-012; research R-6).
const PARTS: &[(&str, &[&str])] = &[
    ("--control-line", &["--ground", "--panel", "--panel-2", "--hover", "--chip", "--needs-bg", "--accent-bg", "--accent-bg-2", "--diff-add", "--ok-bg", "--ok-panel", "--spec-bg", "--warn-bg", "--bad-bg", "--teal-bg", "--phase-bg"]),
    ("--edge", &["--ground", "--panel", "--panel-2", "--phase-bg", "--accent-bg", "--accent-bg-2"]),
    ("--dot-grey", &["--ground", "--panel", "--panel-2", "--phase-bg", "--needs-bg"]),
    ("--arrow", &["--ground", "--panel", "--panel-2"]),
    ("--accent", &["--ground", "--panel", "--panel-2"]),
    ("--ok-dot", &["--ground", "--panel"]),
    ("--bad-control", &["--panel", "--chip"]),
];

fn check_pairs(name: &str, theme: &BTreeMap<String, String>, pairs: &[(&str, &[&str])], need: f64, bad: &mut Vec<String>) {
    for (fg, bgs) in pairs {
        for bg in *bgs {
            let (Some(a), Some(b)) = (theme.get(*fg), theme.get(*bg)) else {
                bad.push(format!("{name}: {fg} on {bg}: not defined"));
                continue;
            };
            let r = ratio(a, b);
            if r < need {
                bad.push(format!("{name}: {fg} {a} on {bg} {b}: {r:.2} < {need}"));
            }
        }
    }
}

#[test]
fn text_and_controls_meet_wcag_aa_in_both_themes() {
    let t = themes();
    let dark = t.dark_attr.clone().expect("the Dark block");
    let mut bad = Vec::new();
    for (name, theme) in [("light", &t.light), ("dark", &dark)] {
        check_pairs(name, theme, TEXT, 4.5, &mut bad);
        check_pairs(name, theme, PARTS, 3.0, &mut bad);
    }
    assert!(bad.is_empty(), "under WCAG AA:\n{}", bad.join("\n"));
}

/// A rule that sets both a text colour and a background from names must have that pair listed
/// in `TEXT` (research R-6: the guard sees only pairs in one rule).
#[test]
fn every_pair_in_one_rule_is_checked() {
    let t = themes();
    let name_of = |decl: &str| -> Option<String> {
        let i = decl.find("var(--")?;
        Some(decl[i + 4..].chars().take_while(|c| *c == '-' || c.is_ascii_alphanumeric()).collect())
    };
    let mut unlisted = Vec::new();
    for rule in t.rest.split('}') {
        let Some((sel, body)) = rule.split_once('{') else { continue };
        // A rule first inside a media block: the block's own header comes before it (review).
        let (sel, body) = match body.split_once('{') {
            Some((s, b)) if sel.trim_start().starts_with('@') => (s, b),
            _ => (sel, body),
        };
        let mut fg = None;
        let mut bg = None;
        for d in body.split(';') {
            let d = d.trim();
            if let Some(v) = d.strip_prefix("color:") {
                fg = name_of(v);
            } else if let Some(v) = d.strip_prefix("background:").or_else(|| d.strip_prefix("background-color:")) {
                bg = name_of(v);
            }
        }
        if let (Some(f), Some(b)) = (fg, bg) {
            let listed = TEXT.iter().any(|(x, bgs)| *x == f && bgs.contains(&b.as_str()));
            if !listed {
                unlisted.push(format!("{} {{ color: {f}; background: {b} }}", sel.trim()));
            }
        }
    }
    unlisted.sort();
    unlisted.dedup();
    assert!(unlisted.is_empty(), "pairs not in TEXT:\n{}", unlisted.join("\n"));
}

// ---- 013 US2: the engineer's choice, kept by Fallow and served on the page ------------------

/// One HTTP request to the page server; the status line, the headers and the body.
fn http(port: u16, method: &str, path: &str, cookie: &str, body: &str) -> (String, String) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    // The head, then as many bytes as Content-Length says: the server may keep the connection
    // open.
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut byte).unwrap();
        buf.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let len = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).unwrap();
    (head, String::from_utf8_lossy(&body).to_string())
}

#[test]
fn the_choice_is_kept_served_and_refused_when_unknown() {
    use fallow::studio::Req;
    let root = common::scratch("web-theme");
    let sc = root.join("scenario.toml");
    std::fs::write(&sc, "").unwrap();
    // The editor is kept beside the theme, untouched by it.
    std::fs::create_dir_all(root.join("data")).unwrap();
    let e = common::start_studio(&root, &sc, None);
    let tx = &e.tx;
    assert_eq!(common::sreq(tx, Req::GetTheme).unwrap()["theme"], "system", "System when nothing was chosen");
    let web = fallow::service::http::Web::start(e.tx.clone(), "b".repeat(64)).unwrap();
    let (head, _) = http(web.port, "GET", &format!("/?k={}", "b".repeat(64)), "", "");
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: ").or_else(|| l.strip_prefix("set-cookie: ")))
        .and_then(|c| c.split(';').next())
        .unwrap()
        .to_string();
    let page = |cookie: &str| http(web.port, "GET", "/", cookie, "").1;
    let html = page(&cookie);
    assert!(html.contains("<html lang=\"en\">") && !html.contains("data-theme"), "System is served as written");
    assert!(html.contains("<meta name=\"color-scheme\" content=\"light dark\">"));

    let (_, b) = http(web.port, "POST", "/api/theme", &cookie, r#"{"theme":"dark"}"#);
    assert!(b.contains("\"theme\":\"dark\""), "{b}");
    let html = page(&cookie);
    assert!(html.contains("<html lang=\"en\" data-theme=\"dark\">"), "Dark is on the page as served");
    assert!(html.contains("<meta name=\"color-scheme\" content=\"dark\">"));

    let (head, b) = http(web.port, "POST", "/api/theme", &cookie, r#"{"theme":"blue"}"#);
    assert!(b.contains("the theme is system, light or dark"), "{head} {b}");
    assert_eq!(common::sreq(tx, Req::GetTheme).unwrap()["theme"], "dark", "a refused value changes nothing");

    // The editor and the theme share prefs.json: one does not drop the other.
    let prefs = root.join("data").join("prefs.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    v["editor"] = serde_json::json!({ "program": "code", "args": [] });
    std::fs::write(&prefs, v.to_string()).unwrap();
    common::sreq(tx, Req::SetTheme { theme: "light".into() }).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&prefs).unwrap()).unwrap();
    assert_eq!(v["editor"]["program"], "code", "{v}");
    assert_eq!(v["theme"], "light");
    drop(web);
    common::stop(e);

    // Kept across a restart, on the same data.
    let e = common::start_studio(&root, &sc, None);
    assert_eq!(common::sreq(&e.tx, Req::GetTheme).unwrap()["theme"], "light");
    // A theme of the wrong type means System, and the editor beside it is kept (review).
    std::fs::write(&prefs, r#"{"editor":{"program":"code","args":[]},"theme":1}"#).unwrap();
    assert_eq!(common::sreq(&e.tx, Req::GetTheme).unwrap()["theme"], "system");
    assert_eq!(common::sreq(&e.tx, Req::GetEditor).unwrap()["editor"]["program"], "code");
    // An unreadable file means System.
    std::fs::write(&prefs, "not json").unwrap();
    assert_eq!(common::sreq(&e.tx, Req::GetTheme).unwrap()["theme"], "system");
    common::stop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn light_and_dark_are_written_on_the_page_as_served() {
    use fallow::service::http::page_with_theme;
    let light = page_with_theme("light");
    assert!(light.contains("<html lang=\"en\" data-theme=\"light\">"));
    assert!(light.contains("<meta name=\"color-scheme\" content=\"light\">"));
    let system = page_with_theme("system");
    assert!(!system.contains("data-theme"));
    // The control is in the top bar with its three choices, and no inline script was added.
    for v in ["system", "light", "dark"] {
        assert!(system.contains(&format!("name=\"theme\" value=\"{v}\"")), "{v}");
    }
    assert!(system.contains("<legend class=\"sr\">Theme</legend>"));
    assert!(!system.contains("<script>"));
}
