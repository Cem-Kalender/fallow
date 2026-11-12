//! 013: the page's themes, checked from `src/service/web/app.css` itself.
//! - Every colour comes from a name (FR-004).
//! - The light values are exactly the colours of before (FR-006).
//! - The dark theme names every colour, in two identical blocks (research R-2).
//! - Text and controls meet WCAG AA in both themes (FR-012, SC-002).

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
