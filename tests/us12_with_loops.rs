//! 012 T051 (quickstart Q10; research R-10): sessions and 011's loops. A session's squash into a
//! ready loop's target is named on the loop page as the session's, not as from outside Fallow;
//! 011's rules apply to it as to any move of the target, and the session never joins the merge
//! line.

mod common;

use common::*;
use fallow::run::engine::Request;
use fallow::conversation::events::ApprovalAnswer;
use fallow::studio::Req;
use serde_json::Value;

type Tx = std::sync::mpsc::Sender<fallow::run::engine::Msg>;

fn allow_next(tx: &Tx, s: &str) {
    let w = wait_conv(tx, s, "a command waiting", |v| !v["pending_approval"].is_null());
    let id = w["pending_approval"]["id"].as_str().unwrap().to_string();
    sreq(tx, Req::Approve { conv: s.to_string(), id, answer: ApprovalAnswer::Once, prefix: None }).unwrap();
}

#[test]
fn a_sessions_squash_into_a_ready_loops_target_is_named_as_the_sessions() {
    let root = scratch("us12-loops-squash");
    let repo_path = root.join("repo").display().to_string().replace('\\', "/");
    let session_steps = format!(
        r#"
[[step]]
kind = "assistant_turn"
when = "make b"
files = [{{ path = "b.txt", text = "b\n" }}]
output = {{ reply = "made b" }}

[[step]]
kind = "assistant_turn"
when = "squash it into main"
exec = ["git -C {repo_path} merge --squash fallow/solo/make-b && git -C {repo_path} commit -q -m 'Make b'"]
output = {{ reply = "squashed" }}
"#
    );
    let (e, repo, id) = locked_calc(&root, &format!("{}{session_steps}", calc_loop_ok()), None);
    let tx = &e.tx;
    let v = wait_for(tx, id, "ready to merge", |v| v["stage"] == "ready");
    let target = v["target"]["branch"].as_str().unwrap_or("main").to_string();
    assert_eq!(target, "main", "{}", v["target"]);
    let project = ask(tx, Request::Data).unwrap()["projects"][0]["id"].as_str().unwrap().to_string();

    let s = start_session(tx, &project, "make b");
    wait_turn(tx, &s, 1);
    say_solo(tx, &s, "squash it into main");
    allow_next(tx, &s);
    let sv = wait_turn(tx, &s, 2);
    let main = git(&repo, &["rev-parse", "main"]);
    assert_eq!(sv["session"]["squashes"][0]["commit"], main.as_str(), "{}", sv["session"]);

    // The loop's target watch names the commit as the session's.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let v = loop {
        let v = ask(tx, Request::Loop { id }).unwrap();
        let seen = v["target"]["commits"].as_array().is_some_and(|c| c.iter().any(|c| c["sha"] == main.as_str() && c["from_kind"] == "session"));
        if seen {
            break v;
        }
        assert!(std::time::Instant::now() < end, "never named as the session's: {}", v["target"]);
        std::thread::sleep(std::time::Duration::from_millis(300));
    };
    let c: &Value = v["target"]["commits"].as_array().unwrap().iter().find(|c| c["sha"] == main.as_str()).unwrap();
    assert_eq!(c["from"], "make b", "{c}");
    assert_eq!(v["target"]["moved"], true);
    // The session never joins the merge line: the loop's line has no session in it.
    assert!(!v.to_string().contains(&format!("\"session\":\"{s}\"")), "{v}");
}
