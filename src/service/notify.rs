//! Notifications (007 research S-14, FR-003a): each new "Needs you" item raises one desktop
//! notification naming the project, the feature and what is needed, never code or output.
//!
//! An item is known by its key (the loop, what it needs, and which question, merge or stop), so
//! a step tried again or a page read again raises nothing new. Items already there when Fallow
//! starts were raised before it stopped; they are not raised again.

use std::collections::BTreeSet;
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
    started: bool,
}

/// What a notification says an item needs: fixed words, never the question, an agent's text or
/// a command's output (FR-003a).
fn what(item: &Value) -> &'static str {
    match item["kind"].as_str() {
        Some("question") => "a question for you; the loop waits",
        Some("ready") => "ready to merge",
        Some("unverified") => "done, not verified by Fallow: review it before merging",
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
    let hash = format!(
        "#/p/{}/f/{}/loop",
        item["project"].as_str().unwrap_or_default(),
        feature
    );
    Notice {
        title: format!("Fallow · {project}"),
        body: format!("{feature}: {}", what(item)),
        url: format!("{}{hash}", page.unwrap_or("")),
    }
}

impl Notifier {
    pub fn new(to: Arc<dyn Notify>, on: Arc<AtomicBool>, page: Option<String>) -> Notifier {
        Notifier {
            to,
            on,
            page,
            seen: BTreeSet::new(),
            started: false,
        }
    }

    /// Raises one notification for each item not seen before, unless notifications are off,
    /// for every project or for the item's (`enabled`).
    pub fn update(&mut self, items: &[Value], enabled: impl Fn(&str) -> bool) {
        let first = !self.started;
        self.started = true;
        for item in items {
            let Some(key) = item["key"].as_str() else {
                continue;
            };
            if !self.seen.insert(key.to_string()) || first {
                continue;
            }
            if !self.on.load(Ordering::Relaxed) || !enabled(item["project"].as_str().unwrap_or_default()) {
                continue;
            }
            self.to.show(&notice(item, self.page.as_deref()));
        }
    }
}
