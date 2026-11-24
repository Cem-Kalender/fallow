//! Parsing TOML with source lines and unknown-key warnings.

use serde::de::DeserializeOwned;

use super::{Level, Notice};

/// 1-based line of a byte offset.
pub fn line_of(text: &str, offset: usize) -> usize {
    let end = offset.min(text.len());
    text.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// The first line on which `key` appears as a key (`key =` or `[key` / `[[key`).
fn line_of_key(text: &str, key: &str) -> Option<usize> {
    text.lines()
        .position(|l| {
            let t = l.trim_start();
            t.strip_prefix(key)
                .is_some_and(|r| r.trim_start().starts_with('='))
                || t.starts_with(&format!("[{key}"))
                || t.starts_with(&format!("[[{key}"))
        })
        .map(|i| i + 1)
}

/// Parses `text` as `T`. A syntax or type error is reported as `file:line: message` and gives
/// `None`. Top-level keys outside `known` give a warning (or, if listed in `legacy`, a warning
/// that the key is no longer read).
pub fn parse<T: DeserializeOwned>(
    file: &str,
    text: &str,
    known: &[&str],
    legacy: &[&str],
    notices: &mut Vec<Notice>,
) -> Option<T> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            notices.push(Notice {
                file: file.into(),
                line: e.span().map(|s| line_of(text, s.start)),
                level: Level::Error,
                message: e.message().to_string(),
            });
            return None;
        }
    };
    let mut kept = toml::Table::new();
    for (k, v) in table {
        if known.contains(&k.as_str()) {
            kept.insert(k, v);
        } else {
            notices.push(Notice {
                file: file.into(),
                line: line_of_key(text, &k),
                level: Level::Warning,
                message: if legacy.contains(&k.as_str()) {
                    format!("{k} is no longer read (006)")
                } else {
                    format!("unknown key {k}")
                },
            });
        }
    }
    match toml::Value::Table(kept).try_into::<T>() {
        Ok(t) => Some(t),
        Err(e) => {
            notices.push(Notice {
                file: file.into(),
                line: None,
                level: Level::Error,
                message: e.message().to_string(),
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct T {
        #[serde(default)]
        a: i64,
    }

    #[test]
    fn unknown_and_legacy_keys_warn() {
        let mut n = Vec::new();
        let t: T = parse(
            "f.toml",
            "a = 3\nb = 1\nname = \"x\"\n",
            &["a"],
            &["name"],
            &mut n,
        )
        .unwrap();
        assert_eq!(t.a, 3);
        assert_eq!(n.len(), 2);
        assert!(
            n.iter()
                .any(|x| x.message == "unknown key b" && x.line == Some(2))
        );
        assert!(
            n.iter()
                .any(|x| x.message.contains("no longer read") && x.line == Some(3))
        );
    }

    #[test]
    fn errors_have_lines() {
        let mut n = Vec::new();
        let t: Option<T> = parse("f.toml", "a = 1\na = \n", &["a"], &[], &mut n);
        assert!(t.is_none());
        assert_eq!(n[0].line, Some(2));
    }
}
