//! An agent call through the runtime interface with the fake agent (006 T022; 007 T009–T011):
//! events and the result are read; a crash, an error and a time-out are each a failed step;
//! refused commands are reported; a fork runs in a new session; replies stream as deltas; edits
//! carry their diff.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use fallow::agent::{Call, Event, Outcome, Runtime, Session, ToolKind, fake::Fake, rights};

fn call(cwd: &std::path::Path, limit: Duration) -> Call {
    Call {
        instruction: "Fallow step: build (part main)\n\nDo it.".into(),
        schema: None,
        rights: rights::maker(vec!["python -m unittest".into()], Vec::new()),
        mcp_config: None,
        session: Session::New("sess-1".into()),
        model: None,
        budget_usd: Some(1.0),
        cwd: cwd.to_path_buf(),
        time_limit: limit,
        system: None,
        partial: false,
        group: None,
        attachments: Vec::new(),
        web: false,
        user_settings: false,
    }
}

fn setup(name: &str, scenario: &str) -> (std::path::PathBuf, Fake) {
    let d = std::env::temp_dir().join(format!("fallow-ag-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let s = d.join("scenario.toml");
    std::fs::write(&s, scenario).unwrap();
    (d, Fake::new(env!("CARGO_BIN_EXE_fallow-fake-agent"), &s))
}

fn run(rt: &Fake, c: &Call) -> (Outcome, Vec<Event>) {
    let mut events = Vec::new();
    let stop = AtomicBool::new(false);
    let o = rt.start(c, &mut |e| events.push(e), &stop);
    (o, events)
}

#[test]
fn a_good_call_gives_events_a_result_and_the_cost() {
    let (d, rt) = setup(
        "good",
        "[[step]]\nkind = \"build\"\nfiles = [{ path = \"x.txt\", text = \"x\" }, { path = \"y.txt\", old = \"a\", text = \"b\" }]\noutput = { status = \"done\", summary = \"ok\" }\ncost = 0.25\ndenied = [\"git push\"]\n",
    );
    assert_eq!(rt.name(), "fake");
    assert!(rt.available().installed);
    let (o, events) = run(&rt, &call(&d, Duration::from_secs(30)));
    assert!(o.failure.is_none(), "{:?}", o.failure);
    assert_eq!(o.result.unwrap()["status"], "done");
    assert_eq!(o.cost_usd, Some(0.25));
    assert_eq!(o.session.as_deref(), Some("sess-1"));
    assert_eq!(o.denials, ["git push"]);
    assert!(
        events.iter().any(|e| matches!(e, Event::ToolCall { kind: ToolKind::Write, summary, .. } if summary == "x.txt")),
        "{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::ToolCall { kind: ToolKind::Edit, detail, .. } if detail == "-a\n+b\n")),
        "an edit carries its diff: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::ToolResult { ok: true, .. })),
        "{events:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_fork_runs_in_a_new_session_and_replies_stream() {
    let (d, rt) = setup(
        "fork",
        "[[step]]\nkind = \"lead_turn\"\nsay = \"It remembers the word.\"\noutput = { reply = \"x\" }\n",
    );
    let mut c = call(&d, Duration::from_secs(30));
    c.instruction = "Fallow step: lead_turn\n\nWhich word?".into();
    c.session = Session::Fork("sess-1".into());
    c.partial = true;
    let (o, events) = run(&rt, &c);
    assert!(o.failure.is_none(), "{:?}", o.failure);
    let s = o.session.unwrap();
    assert_ne!(s, "sess-1", "a fork reports its new session");
    assert!(s.starts_with("sess-1-fork"));
    let deltas: String = events
        .iter()
        .filter_map(|e| match e {
            Event::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "It remembers the word.");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_crash_an_error_and_a_time_out_are_failed_steps() {
    let (d, rt) = setup("crash", "[[step]]\nkind = \"build\"\nexit = 3\n");
    let (o, _) = run(&rt, &call(&d, Duration::from_secs(30)));
    assert!(o.result.is_none());
    assert!(o.failure.unwrap().contains("without a result (exit 3)"));
    let _ = std::fs::remove_dir_all(&d);

    let (d, rt) = setup(
        "error",
        "[[step]]\nkind = \"build\"\nis_error = true\noutput = { status = \"done\", summary = \"x\" }\n",
    );
    let (o, _) = run(&rt, &call(&d, Duration::from_secs(30)));
    assert!(o.failure.unwrap().contains("ended with an error"));
    let _ = std::fs::remove_dir_all(&d);

    let (d, rt) = setup(
        "slow",
        "[[step]]\nkind = \"build\"\nsleep_ms = 30000\noutput = { status = \"done\", summary = \"x\" }\n",
    );
    let start = std::time::Instant::now();
    let (o, _) = run(&rt, &call(&d, Duration::from_secs(2)));
    assert!(o.failure.unwrap().contains("time limit"));
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "the process tree was ended"
    );
    let _ = std::fs::remove_dir_all(&d);

    let (d, rt) = setup("noscript", "[[step]]\nkind = \"review\"\n");
    let (o, _) = run(&rt, &call(&d, Duration::from_secs(30)));
    assert!(o.result.is_none() && o.failure.is_some());
    let _ = std::fs::remove_dir_all(&d);
}

/// T086: Claude Code 2.1 ends a resume of an unknown session with an error result that has no
/// message, the reason on stderr only; Fallow reads it there, so the lost session is known.
#[test]
fn a_session_the_runtime_does_not_know_is_named_from_stderr() {
    let (d, rt) = setup(
        "lost",
        "[[step]]\nkind = \"review\"\noutput = { answers = [] }\n",
    );
    let mut c = call(&d, Duration::from_secs(30));
    c.session = Session::Resume("never-started".into());
    let (o, _) = run(&rt, &c);
    let why = o.failure.unwrap();
    assert!(
        why.contains("No conversation found with session ID: never-started"),
        "{why}"
    );
    let _ = std::fs::remove_dir_all(&d);
}
