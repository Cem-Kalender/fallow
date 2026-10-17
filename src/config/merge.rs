//! Parsing with source lines, unknown-key warnings and merging by `id` (research R-9).

use std::collections::HashSet;

use serde::de::DeserializeOwned;
use toml::de::{DeTable, DeValue};

use super::schema::Shape;
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

/// Parses one file. A syntax or type error is reported as `file:line: message` and the file's
/// content is ignored (`None`). Unknown keys produce warnings only.
pub(crate) fn parse_file<T: DeserializeOwned>(
    file: &str,
    text: &str,
    shape: &Shape,
    notices: &mut Vec<Notice>,
) -> Option<T> {
    let table = match DeTable::parse(text) {
        Ok(t) => t,
        Err(e) => {
            notices.push(parse_error(file, text, &e));
            return None;
        }
    };
    if let Shape::Table(keys) = shape {
        unknown_keys(table.get_ref(), keys, "", file, text, notices);
    }
    match toml::from_str::<T>(text) {
        Ok(v) => Some(v),
        Err(e) => {
            notices.push(parse_error(file, text, &e));
            None
        }
    }
}

fn parse_error(file: &str, text: &str, e: &toml::de::Error) -> Notice {
    let message = format!("{} (this file's settings are ignored)", e.message().trim());
    Notice {
        file: file.to_string(),
        line: e.span().map(|s| line_of(text, s.start)),
        level: Level::Error,
        message,
    }
}

fn unknown_keys(
    table: &DeTable<'_>,
    keys: &[(&'static str, Shape)],
    prefix: &str,
    file: &str,
    text: &str,
    notices: &mut Vec<Notice>,
) {
    for (k, v) in table.iter() {
        let name: &str = k.get_ref();
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        let Some((_, shape)) = keys.iter().find(|(known, _)| *known == name) else {
            notices.push(Notice {
                file: file.to_string(),
                line: Some(line_of(text, k.span().start)),
                level: Level::Warning,
                message: format!("unknown key `{path}` (ignored)"),
            });
            continue;
        };
        match (shape, v.get_ref()) {
            (Shape::Table(sub), DeValue::Table(t)) => {
                unknown_keys(t, sub, &path, file, text, notices)
            }
            (Shape::Tables(sub), DeValue::Array(items)) => {
                for item in items.iter() {
                    if let DeValue::Table(t) = item.get_ref() {
                        unknown_keys(t, sub, &path, file, text, notices);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Something merged by `id`.
pub trait Keyed {
    fn id(&self) -> &str;
}

impl Keyed for super::Signature {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Keyed for super::Provider {
    fn id(&self) -> &str {
        &self.id
    }
}

/// "A user entry with the same `id` replaces the built-in entry completely." New ids are
/// appended in file order. Disabled entries are kept with `enabled = false`.
pub fn merge_by_id<T: Keyed>(builtin: Vec<T>, user: Vec<T>) -> Vec<T> {
    let mut out = builtin;
    for entry in user {
        match out.iter_mut().find(|b| b.id() == entry.id()) {
            Some(slot) => *slot = entry,
            None => out.push(entry),
        }
    }
    out
}

/// Keeps the last entry for each id within one file and warns about the earlier ones.
pub(crate) fn dedup_last<T: Keyed>(
    entries: Vec<(T, usize)>,
    file: &str,
    notices: &mut Vec<Notice>,
) -> Vec<T> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for (entry, line) in entries.into_iter().rev() {
        if seen.insert(entry.id().to_string()) {
            kept.push(entry);
        } else {
            notices.push(Notice {
                file: file.to_string(),
                line: Some(line),
                level: Level::Warning,
                message: format!("duplicate id {:?}: the later entry is used", entry.id()),
            });
        }
    }
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::line_of;

    #[test]
    fn lines() {
        let t = "a\nb\nc";
        assert_eq!(line_of(t, 0), 1);
        assert_eq!(line_of(t, 2), 2);
        assert_eq!(line_of(t, 4), 3);
        assert_eq!(line_of(t, 99), 3);
    }
}
