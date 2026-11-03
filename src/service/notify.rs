//! Notifications (007 research S-14, FR-003a): each new "Needs you" item raises one desktop
//! notification naming the project, the feature and what is needed, never code or output.
//!
//! An item is known by its key (the loop, what it needs, and which question, merge or stop), so
//! a step tried again or a page read again raises nothing new. The items raised are kept on disk,
//! so an item raised before Fallow stopped is not raised again, and one that came while it was
//! stopped, or while it started, is.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::platform::notify::{Notice, Notify};

pub struct Notifier {
    to: Arc<dyn Notify>,
    /// The switch of the icon's menu, for every project.
    pub on: Arc<AtomicBool>,
    /// The page's address with its secret, to which an item's hash is added.
    page: Option<String>,
    seen: BTreeSet<String>,
    /// Where `seen` is kept (`notified.json`), when there is a data folder.
    file: Option<PathBuf>,
}

/// What a notification says an item needs: fixed words, never the question, an agent's text or
/// a command's output (FR-003a).
fn what(item: &Value) -> &'static str {
    match item["kind"].as_str() {
        Some("question") => "a question for you; the loop waits",
        Some("ready") => "ready to merge",
        Some("unverified") => "done, not verified by Fallow: review it before merging",
        Some("pr_open") => "pull request opened: review and merge it on the remote",
        Some("pr_merged") => "merged on the remote",
        Some("docs_changed") => "the target changed this feature's documents: lock again or stop",
        _ => match item["what"].as_str() {
            Some("stopped at a limit") => "stopped at a limit",
            Some("stopped at the repair cap") => "stopped at the repair cap",
            _ => "stopped and needs you",
        },
    }
}

/// The notification for a "Needs you" item.
pub fn notice(item: &Value, page: Option<&str>) -> Notice {
    let project = item["project_name"].as_str().unwrap_or_default();
    let feature = item["feature"].as_str().unwrap_or_default();
    let hash = match item["project"].as_str().filter(|p| !p.is_empty()) {
        Some(p) => format!("#/p/{}/f/{}/loop", enc(p), enc(feature)),
        // A run of 006, outside any project: the dashboard.
        None => "#/".to_string(),
    };
    Notice {
        title: if project.is_empty() {
            "Fallow".to_string()
        } else {
            format!("Fallow · {project}")
        },
        body: match item["kind"].as_str() {
            // 011 (contracts/web-api.md): counts only, never commits or code.
            Some("update_limit") => format!(
                "{feature}: updated {} times and overtaken; your decision",
                item["updates"].as_u64().unwrap_or(0)
            ),
            Some("update") => format!(
                "{feature}: {} moved; bring it up to date",
                item["target"].as_str().unwrap_or("the target")
            ),
            Some("merged_unchecked") => {
                let n = item["unchecked"].as_u64().unwrap_or(0);
                format!(
                    "{feature} merged without the latest check ({n} commit{})",
                    if n == 1 { "" } else { "s" }
                )
            }
            _ => format!("{feature}: {}", what(item)),
        },
        url: format!("{}{hash}", page.unwrap_or("")),
    }
}

impl Notifier {
    pub fn new(
        to: Arc<dyn Notify>,
        on: Arc<AtomicBool>,
        page: Option<String>,
        file: Option<PathBuf>,
    ) -> Notifier {
        Notifier {
            to,
            on,
            page,
            seen: BTreeSet::new(),
            file,
        }
    }

    /// Reads the items raised before Fallow stopped. `false` when there is no record: the first
    /// start, whose items are then `seed`ed.
    pub fn load(&mut self) -> bool {
        let Some(f) = &self.file else { return false };
        match std::fs::read_to_string(f)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            Some(seen) => {
                self.seen = seen;
                true
            }
            None => false,
        }
    }

    /// Takes `items` as raised, without raising them.
    pub fn seed(&mut self, items: &[Value]) {
        self.seen = keys(items);
        self.save();
    }

    fn save(&self) {
        if let Some(f) = &self.file {
            let _ = std::fs::write(f, serde_json::to_string(&self.seen).unwrap_or_default());
        }
    }

    /// Raises one notification for each item not seen before, unless notifications are off,
    /// for every project or for the item's (`enabled`).
    /// Only the items still there are kept: one that goes and comes back is new.
    pub fn update(&mut self, items: &[Value], enabled: impl Fn(&str) -> bool) {
        let now = keys(items);
        if now == self.seen {
            return;
        }
        for item in items {
            let Some(key) = item["key"].as_str() else {
                continue;
            };
            if self.seen.contains(key) {
                continue;
            }
            if !self.on.load(Ordering::Relaxed)
                || !enabled(item["project"].as_str().unwrap_or_default())
            {
                continue;
            }
            self.to.show(&notice(item, self.page.as_deref()));
        }
        self.seen = now;
        self.save();
    }
}

fn keys(items: &[Value]) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|i| i["key"].as_str().map(str::to_string))
        .collect()
}

/// A part of the page's hash, percent-encoded.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests_011 {
    use super::*;
    use serde_json::json;

    /// 011 (contracts/web-api.md): counts only, the project and the feature, never code (V).
    #[test]
    fn the_limit_and_an_unchecked_merge_are_said_in_fixed_words() {
        let base = json!({ "project": "p", "project_name": "shop", "feature": "011-x" });
        let mut a = base.clone();
        a["kind"] = json!("update_limit");
        a["updates"] = json!(3);
        assert_eq!(
            notice(&a, None).body,
            "011-x: updated 3 times and overtaken; your decision"
        );
        let mut b = base;
        b["kind"] = json!("merged_unchecked");
        b["unchecked"] = json!(1);
        assert_eq!(
            notice(&b, None).body,
            "011-x merged without the latest check (1 commit)"
        );
        let mut c = json!({ "project": "p", "project_name": "shop", "feature": "011-x" });
        c["kind"] = json!("update");
        c["target"] = json!("main");
        assert_eq!(
            notice(&c, None).body,
            "011-x: main moved; bring it up to date"
        );
    }
}
