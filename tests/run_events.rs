//! 006 T020: the event log. Append, then fold; a torn last line is dropped with a notice and cut
//! off; the stage machine refuses illegal transitions.

use std::io::Write;
use std::path::PathBuf;

use fallow::config::Autonomy;
use fallow::run::events::{Event, Limits, Log, Mode, Stage};
use fallow::run::state::RunState;

fn created() -> Event {
    Event::RunCreated {
        repo: PathBuf::from("C:/r"),
        target: "main".into(),
        start_commit: "a".repeat(40),
        task: "t".into(),
        mode: Mode::Solo,
        autonomy: Autonomy::Medium,
        limits: Limits {
            usd: 10.0,
            minutes: 120,
            step_minutes: 30,
        },
        roles: Default::default(),
        repair_cap: 2,
        gates: Vec::new(),
        allow: Default::default(),
        test_patterns: Vec::new(),
        recorded: false,
        left_out: Vec::new(),
    }
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-ev-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn events_are_appended_and_folded_again_after_a_restart() {
    let d = dir("fold");
    let (mut log, events, notes) = Log::open(&d).unwrap();
    assert!(events.is_empty() && notes.is_empty());
    log.append(created()).unwrap();
    log.append(Event::Stage {
        to: Stage::Building,
        why: "approved".into(),
    })
    .unwrap();
    drop(log);
    let (mut log, events, _) = Log::open(&d).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].seq, 2);
    let s = RunState::fold(7, &events).unwrap();
    assert_eq!(s.stage, Stage::Building);
    assert!(s.building_started.is_some());
    let next = log.append(Event::Note { text: "x".into() }).unwrap();
    assert_eq!(next.seq, 3, "numbering continues after a restart");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_torn_last_line_is_dropped_and_cut_off() {
    let d = dir("torn");
    let (mut log, _, _) = Log::open(&d).unwrap();
    log.append(created()).unwrap();
    let path = log.path().to_path_buf();
    drop(log);
    // A crash in the middle of a write.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"{\"seq\":2,\"at\":\"2026").unwrap();
    drop(f);
    let (mut log, events, notes) = Log::open(&d).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(notes.len(), 1, "{notes:?}");
    log.append(Event::Note {
        text: "after".into(),
    })
    .unwrap();
    drop(log);
    let (_, events, notes) = Log::open(&d).unwrap();
    assert_eq!(events.len(), 2, "the next event starts on a clean line");
    assert!(notes.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn illegal_stage_changes_are_refused_by_the_fold() {
    let d = dir("illegal");
    let (mut log, _, _) = Log::open(&d).unwrap();
    log.append(created()).unwrap();
    // Planning cannot go straight to ready.
    log.append(Event::Stage {
        to: Stage::Ready,
        why: "skip".into(),
    })
    .unwrap();
    log.append(Event::Stage {
        to: Stage::Stopped,
        why: "stop".into(),
    })
    .unwrap();
    // Nothing leaves a final stage.
    log.append(Event::Stage {
        to: Stage::Building,
        why: "again".into(),
    })
    .unwrap();
    drop(log);
    let (_, events, _) = Log::open(&d).unwrap();
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(s.stage, Stage::Stopped);
    assert_eq!(s.notes.len(), 2, "{:?}", s.notes);
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(windows)]
#[test]
fn the_run_folder_is_private_to_the_user() {
    let d = dir("acl");
    let (log, _, _) = Log::open(&d).unwrap();
    drop(log);
    let out = std::process::Command::new("icacls")
        .arg(&d)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    // One access entry, for the current user, inherited by what is inside.
    let entries: Vec<&str> = text.lines().filter(|l| l.contains(":(")).collect();
    assert_eq!(entries.len(), 1, "{text}");
    assert!(entries[0].contains("(OI)(CI)"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}
