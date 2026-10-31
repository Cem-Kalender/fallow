//! 007 US5 (T067; FR-003a, constitution V): a "Needs you" item raises one desktop notification,
//! however often the loop and the page look again; it names the project, the feature and what
//! is needed, never the question's text, code or output (a marker test); and a project can turn
//! notifications off.

mod common;

use common::*;
use fallow::platform::notify::Recorder;
use fallow::run::engine::Request;
use fallow::studio::Req;
use std::sync::Arc;
use std::time::Duration;

const MARKER: &str = "MARKER-5f0c2d";

#[test]
fn one_notification_per_item_without_code_and_an_off_switch() {
    let root = scratch("notify");
    let question = format!("Should add() accept floats like {MARKER}?");
    let steps = [
        lead_writes_calc(),
        task_step("T001", &[("calc/__init__.py", &format!("# {MARKER}\n"))], &format!("ask = {question:?}\nsay = \"Working on {MARKER}\""))
            .replacen("status = \"done\"", "status = \"blocked\", question = \"$ANSWER\"", 1)
            .replacen("summary = \"did", &format!("summary = \"{MARKER} did"), 1),
        format!("[[step]]\nkind = \"lead_answer\"\npart = \"q1\"\noutput = {{ outcome = \"relay\", answer = \"{MARKER}\", basis = \"none\", reserved = false, changes_criterion = false, touches_interface = false, adds_dependency = false }}\n\n"),
        task_step("T001", &[("calc/__init__.py", "")], "when = \"The engineer answers\""),
        calc_loop_ok().split_once("[[step]]\nkind = \"handover\"").map(|(_, r)| format!("[[step]]\nkind = \"handover\"{r}")).unwrap(),
    ]
    .concat();
    let scenario = root.join("scenario.toml");
    std::fs::write(&scenario, steps).unwrap();
    let rec = Arc::new(Recorder::default());
    let e = start_studio_notify(&root, &scenario, None, rec.clone());
    let tx = &e.tx;
    let id = lock_calc_in(tx, &repo_named(&root, "shop"), None, Some((5.0, 60)), None);
    wait_for(tx, id, "the question", |v| !v["needs"].is_null());
    // The engine looks again every second, and the page asks again: still one notification.
    for _ in 0..4 {
        let _ = ask(tx, Request::Dashboard);
        std::thread::sleep(Duration::from_millis(600));
    }
    let shown = rec.0.lock().unwrap().clone();
    assert_eq!(shown.len(), 1, "{shown:?}");
    let n = &shown[0];
    assert_eq!(n.title, "Fallow · shop");
    assert_eq!(n.body, "001-calc: a question for you; the loop waits");
    assert_eq!(n.url, "http://fallow.test/#/p/shop/f/001-calc/loop");
    for n in &shown {
        let all = format!("{} {} {}", n.title, n.body, n.url);
        assert!(!all.contains(MARKER) && !all.contains("floats"), "no question text, code or output: {all}");
    }

    // The project turns notifications off: the merge that follows raises none.
    sreq(tx, Req::PatchProject { id: "shop".into(), patch: serde_json::json!({ "notifications": false }) }).unwrap();
    ask(tx, Request::QuestionAnswer { id, q: "q1".into(), answer: "Ints only".into() }).unwrap();
    wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(rec.0.lock().unwrap().len(), 1, "notifications are off for the project");
    drop(e);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn what_a_notification_says() {
    use fallow::service::notify::notice;
    use serde_json::json;
    let base = json!({ "project": "shop", "project_name": "Shop", "feature": "007-cart" });
    let with = |k: serde_json::Value| {
        let mut v = base.clone();
        for (a, b) in k.as_object().unwrap() {
            v[a] = b.clone();
        }
        notice(&v, Some("http://x/?k=s"))
    };
    assert_eq!(with(json!({ "kind": "ready" })).body, "007-cart: ready to merge");
    assert!(with(json!({ "kind": "unverified" })).body.contains("not verified by Fallow"));
    // A stop says only which kind of stop, never Fallow's longer text, which may name files.
    let s = with(json!({ "kind": "stopped", "what": "stopped at a limit", "text": "secret.rs failed" }));
    assert_eq!(s.body, "007-cart: stopped at a limit");
    assert_eq!(with(json!({ "kind": "stopped", "text": "x" })).body, "007-cart: stopped and needs you");
    assert_eq!(s.url, "http://x/?k=s#/p/shop/f/007-cart/loop");
}

#[test]
fn the_switches_and_what_was_raised_before_a_restart() {
    use fallow::service::notify::Notifier;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = scratch("notify-switches");
    let file = dir.join("notified.json");
    let item = |k: &str, p: &str| json!({ "key": k, "kind": "ready", "project": p, "project_name": p, "feature": "001-x" });
    let rec = Arc::new(Recorder::default());
    let on = Arc::new(AtomicBool::new(true));
    let mut n = Notifier::new(rec.clone(), on.clone(), None, Some(file.clone()));
    assert!(!n.load(), "no record on the first start");
    // What is there at the first start was raised before: not again.
    n.seed(&[item("a", "shop")]);
    n.update(&[item("a", "shop")], |_| true);
    assert_eq!(rec.0.lock().unwrap().len(), 0);
    // A new item is raised; one of a project that turned them off is not.
    n.update(&[item("a", "shop"), item("b", "shop"), item("c", "notes")], |p| p != "notes");
    assert_eq!(rec.0.lock().unwrap().len(), 1, "b only");
    // The global switch off: nothing; on again: the next one.
    on.store(false, Ordering::Relaxed);
    n.update(&[item("a", "shop"), item("b", "shop"), item("c", "notes"), item("d", "shop")], |_| true);
    assert_eq!(rec.0.lock().unwrap().len(), 1, "switched off");
    on.store(true, Ordering::Relaxed);
    let now = [item("b", "shop"), item("c", "notes"), item("d", "shop"), item("e", "shop")];
    n.update(&now, |_| true);
    assert_eq!(rec.0.lock().unwrap().len(), 2, "e");
    // After a restart: what was raised stays raised, and what came meanwhile is new.
    let mut n = Notifier::new(rec.clone(), on, None, Some(file));
    assert!(n.load());
    n.update(&[item("b", "shop"), item("e", "shop"), item("f", "shop")], |_| true);
    let shown = rec.0.lock().unwrap().clone();
    assert_eq!(shown.len(), 3, "f only: {shown:?}");
    // A run outside any project opens the dashboard; names are encoded in the address.
    let x = fallow::service::notify::notice(&json!({ "kind": "stopped" }), Some("http://x/"));
    assert_eq!((x.title.as_str(), x.url.as_str()), ("Fallow", "http://x/#/"));
    let y = fallow::service::notify::notice(&json!({ "kind": "ready", "project": "a b", "feature": "001-c#d" }), Some("http://x/"));
    assert_eq!(y.url, "http://x/#/p/a%20b/f/001-c%23d/loop");
    let _ = std::fs::remove_dir_all(&dir);
}
