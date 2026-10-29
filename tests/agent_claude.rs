//! 006 T022: an agent call with the fake agent: activity lines and the result are read; a crash,
//! an error and a time-out are each a failed step; refused commands are reported.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use fallow::agent::{Agent, Call, Session, claude};

fn call(scenario: &std::path::Path, cwd: &std::path::Path, limit: Duration) -> Call {
    Call {
        agent: Agent {
            command: env!("CARGO_BIN_EXE_fallow-fake-agent").into(),
            args: vec!["--scenario".into(), scenario.display().to_string()],
        },
        instruction: "Fallow step: build (part main)\n\nDo it.".into(),
        schema: None,
        allowed: vec!["Read".into()],
        mcp_config: None,
        session: Session::New("sess-1".into()),
        model: None,
        budget_usd: Some(1.0),
        cwd: cwd.to_path_buf(),
        time_limit: limit,
        system: None,
        unrestricted: false,
    }
}

fn setup(name: &str, scenario: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("fallow-ag-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let s = d.join("scenario.toml");
    std::fs::write(&s, scenario).unwrap();
    (d, s)
}

fn run(c: &Call) -> (fallow::agent::Outcome, Vec<fallow::agent::Activity>) {
    let mut lines = Vec::new();
    let stop = Arc::new(AtomicBool::new(false));
    let o = claude::run(c, &mut |a| lines.push(a), &stop);
    (o, lines)
}

#[test]
fn a_good_call_gives_activity_a_result_and_the_cost() {
    let (d, s) = setup(
        "good",
        "[[step]]\nkind = \"build\"\nfiles = [{ path = \"x.txt\", text = \"x\" }]\noutput = { status = \"done\", summary = \"ok\" }\ncost = 0.25\ndenied = [\"git push\"]\n",
    );
    let (o, lines) = run(&call(&s, &d, Duration::from_secs(30)));
    assert!(o.failure.is_none(), "{:?}", o.failure);
    assert_eq!(o.result.unwrap()["status"], "done");
    assert_eq!(o.cost_usd, Some(0.25));
    assert_eq!(o.session.as_deref(), Some("sess-1"));
    assert_eq!(o.denials, ["git push"]);
    assert!(
        lines.iter().any(|l| l.kind == "write" && l.text == "x.txt"),
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_crash_an_error_and_a_time_out_are_failed_steps() {
    let (d, s) = setup("crash", "[[step]]\nkind = \"build\"\nexit = 3\n");
    let (o, _) = run(&call(&s, &d, Duration::from_secs(30)));
    assert!(o.result.is_none());
    assert!(o.failure.unwrap().contains("without a result (exit 3)"));
    let _ = std::fs::remove_dir_all(&d);

    let (d, s) = setup(
        "error",
        "[[step]]\nkind = \"build\"\nis_error = true\noutput = { status = \"done\", summary = \"x\" }\n",
    );
    let (o, _) = run(&call(&s, &d, Duration::from_secs(30)));
    assert!(o.failure.unwrap().contains("ended with an error"));
    let _ = std::fs::remove_dir_all(&d);

    let (d, s) = setup(
        "slow",
        "[[step]]\nkind = \"build\"\nsleep_ms = 30000\noutput = { status = \"done\", summary = \"x\" }\n",
    );
    let start = std::time::Instant::now();
    let (o, _) = run(&call(&s, &d, Duration::from_secs(2)));
    assert!(o.failure.unwrap().contains("time limit"));
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "the process tree was ended"
    );
    let _ = std::fs::remove_dir_all(&d);

    let (d, s) = setup("noscript", "[[step]]\nkind = \"review\"\n");
    let (o, _) = run(&call(&s, &d, Duration::from_secs(30)));
    assert!(o.result.is_none() && o.failure.is_some());
    let _ = std::fs::remove_dir_all(&d);
}
