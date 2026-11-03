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
    let (_, events, notes) = Log::<Event>::open(&d).unwrap();
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
    let (log, _, _) = Log::<Event>::open(&d).unwrap();
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

#[test]
fn paused_time_does_not_count_against_the_time_limit() {
    use fallow::run::events::Logged;
    use fallow::run::time::rfc3339;
    let t0 = 1_800_000_000;
    let at = |s: u64| rfc3339(t0 + s);
    let stage = |seq: u64, s: u64, to: Stage| Logged {
        seq,
        at: at(s),
        event: Event::Stage {
            to,
            why: String::new(),
        },
    };
    let mut events = vec![
        Logged {
            seq: 1,
            at: at(0),
            event: created(),
        },
        stage(2, 0, Stage::Building),
        stage(3, 600, Stage::Paused),
    ];
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(
        s.minutes_used(t0 + 2000),
        10,
        "the pause so far is left out"
    );
    events.push(stage(4, 4200, Stage::Building));
    let s = RunState::fold(1, &events).unwrap();
    assert_eq!(s.minutes_used(t0 + 4800), 20);
    assert!(s.limit_hit(t0 + 4800).is_none());
    let (which, used) = s.limit_hit(t0 + 3600 + 7300).unwrap();
    assert_eq!(which, "time");
    assert!(used.starts_with("121 of 120"));
}

#[test]
fn a_loop_folds_its_phases_tasks_questions_and_outcome() {
    // 007 T014: the loop events after run_created (contracts/storage.md).
    use fallow::run::events::{
        Level, Logged, PhaseSpec, PhaseState, QuestionState, TaskSpec, TaskState,
    };
    let task = |id: &str, story: Option<&str>| TaskSpec {
        id: id.into(),
        story: story.map(str::to_string),
        parallel: false,
        text: format!("do {id}"),
    };
    let events: Vec<Event> = vec![
        created(),
        Event::LoopCreated {
            project: "shop".into(),
            feature: "007-cart".into(),
            lock: 1,
            lock_commit: "l1".into(),
            base_branch: "master".into(),
            build_branch: "fallow/007/build".into(),
            pipeline: "name = \"standard\"".into(),
            level: Level::Normal,
            budget_usd: 2.5,
            budget_minutes: 100,
            phases: vec![
                PhaseSpec {
                    n: 1,
                    label: "1".into(),
                    title: "Setup".into(),
                    story: None,
                    tasks: vec![task("T001", None)],
                },
                PhaseSpec {
                    n: 3,
                    label: "3".into(),
                    title: "User Story 1".into(),
                    story: Some("US1".into()),
                    tasks: vec![task("T010", Some("US1")), task("T011", Some("US1"))],
                },
            ],
            when_verified: Default::default(),
            update_limit: 3,
            run_after: None,
            switches: Default::default(),
        },
        Event::Phase {
            n: 1,
            state: PhaseState::Building,
            session: Some("s1".into()),
            handover: None,
        },
        Event::Task {
            id: "T001".into(),
            state: TaskState::Accepted,
            commit: Some("c1".into()),
            repairs: 1,
        },
        Event::Phase {
            n: 1,
            state: PhaseState::Done,
            session: None,
            handover: Some("specs/007-cart/handover/phase-1.md".into()),
        },
        Event::Question {
            id: "q1".into(),
            step: 4,
            task: Some("T010".into()),
            text: "Refuse or add what is left?".into(),
            options: vec!["refuse".into(), "add what is left".into()],
            state: QuestionState::WaitingEngineer,
            by: None,
            answer: None,
            commit: None,
            affects: Some("US1".into()),
            recommended: Some("refuse".into()),
            cost_usd: 0.02,
            reopened: Vec::new(),
            why: Some("the lead relayed it".into()),
        },
        Event::Unlocked,
        Event::Relocked {
            lock: 2,
            commit: "l2".into(),
            affected: vec!["US1".into()],
            phases: Vec::new(),
            reconciled_target: None,
        },
        Event::Outcome {
            verified: false,
            missing: vec!["no review at checkpoints".into()],
        },
    ];
    let d = dir("loop");
    let (mut log, _, _) = Log::open(&d).unwrap();
    for e in events {
        log.append(e).unwrap();
    }
    drop(log);
    let (_, logged, notes) = Log::open(&d).unwrap();
    assert!(notes.is_empty());
    let logged: Vec<Logged> = logged;
    let s = RunState::fold(1, &logged).unwrap();
    let f = s.feature_loop.expect("a loop");
    assert_eq!(f.feature, "007-cart");
    assert_eq!(f.lock, 2);
    assert_eq!(f.lock_commit, "l2");
    assert!(!f.unlocked);
    assert_eq!(f.affected, ["US1"]);
    assert_eq!(f.phases[0].state, PhaseState::Done);
    assert_eq!(
        f.phases[0].session.as_deref(),
        Some("s1"),
        "kept when a later event omits it"
    );
    assert!(f.phases[0].handover.is_some());
    assert_eq!(f.tasks["T001"].state, TaskState::Accepted);
    assert_eq!(f.tasks["T001"].repairs, 1);
    assert_eq!(f.tasks["T011"].phase, 3);
    assert_eq!(f.questions["q1"].state, QuestionState::WaitingEngineer);
    assert_eq!(
        f.outcome,
        Some((false, vec!["no review at checkpoints".to_string()]))
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_update_folds_into_the_loop_and_reopens_every_story() {
    // 011 T005: an update's events (data-model.md), from the lock to ready again.
    use fallow::run::events::{
        ConflictKindRec, Level, Logged, PhaseSpec, PhaseState, Switches, TaskSpec, UpdateConflict,
        WaitFrom,
    };
    let task = |id: &str, story: Option<&str>, text: &str| TaskSpec {
        id: id.into(),
        story: story.map(str::to_string),
        parallel: false,
        text: text.into(),
    };
    let events: Vec<Event> = vec![
        created(),
        Event::LoopCreated {
            project: "shop".into(),
            feature: "011-x".into(),
            lock: 1,
            lock_commit: "l1".into(),
            base_branch: "main".into(),
            build_branch: "fallow/011-x/build".into(),
            pipeline: "name = \"standard\"".into(),
            level: Level::Free,
            budget_usd: 2.0,
            budget_minutes: 60,
            phases: vec![
                PhaseSpec {
                    n: 1,
                    label: "1".into(),
                    title: "Setup".into(),
                    story: None,
                    tasks: vec![task("T001", None, "Create calc/__init__.py")],
                },
                PhaseSpec {
                    n: 2,
                    label: "2".into(),
                    title: "User Story 1".into(),
                    story: Some("US1".into()),
                    tasks: vec![task("T002", Some("US1"), "add() in calc/__init__.py")],
                },
            ],
            when_verified: Default::default(),
            update_limit: 2,
            run_after: None,
            switches: Switches {
                auto: true,
                line: false,
                overlap: true,
            },
        },
        Event::Stage {
            to: Stage::Building,
            why: "lock".into(),
        },
        Event::Touched {
            files: vec!["calc/__init__.py".into()],
        },
        Event::Stage {
            to: Stage::Ready,
            why: "verified".into(),
        },
        Event::Stage {
            to: Stage::Building,
            why: "update".into(),
        },
        Event::Updated {
            n: 1,
            target: "main".into(),
            target_commit: "t1".into(),
            loop_head: "h1".into(),
            remote: false,
        },
        Event::UpdateConflicted {
            n: 1,
            working: "w1".into(),
            branch: "fallow/011-x/update-1".into(),
            conflicts: vec![UpdateConflict {
                path: "calc/__init__.py".into(),
                kind: ConflictKindRec::Text,
            }],
        },
        Event::UpdateMerged {
            n: 1,
            commit: "m1".into(),
        },
        Event::Overtaken {
            n: 1,
            target_commit: "t2".into(),
        },
        Event::Lined { at_ms: 1 },
        Event::Overlap {
            with: 7,
            files: vec!["calc/__init__.py".into()],
        },
        Event::WaitsFor {
            run: 7,
            from: WaitFrom::Engineer,
        },
    ];
    let d = dir("update");
    let (mut log, _, _) = Log::open(&d).unwrap();
    for e in events {
        log.append(e).unwrap();
    }
    drop(log);
    let (_, logged, _) = Log::open(&d).unwrap();
    let logged: Vec<Logged> = logged;
    let s = RunState::fold(1, &logged).unwrap();
    let f = s.feature_loop.as_ref().expect("a loop");
    assert_eq!(f.update_limit, 2);
    assert!(!f.switches.line);
    assert!(
        f.task_paths.contains("calc/__init__.py"),
        "{:?}",
        f.task_paths
    );
    assert!(f.touched.contains("calc/__init__.py"));
    let u = f.update.as_ref().expect("in progress until ready");
    assert!(u.merged);
    assert_eq!(u.branch.as_deref(), Some("fallow/011-x/update-1"));
    assert_eq!(f.base_commit, "t1");
    assert!(
        f.phases[0].reopened.is_none(),
        "the setup phase is not checked again"
    );
    assert_eq!(
        f.phases[1].state,
        PhaseState::Pending,
        "the story's phase is checked again"
    );
    assert!(f.phases[1].reopened.is_some());
    assert!(f.affected.is_empty(), "no relock repair");
    assert_eq!(f.overtaken, 1);
    assert!(f.lined_at.is_some());
    assert_eq!(f.overlaps[0].with, 7);
    assert_eq!(f.waits_for, Some((7, WaitFrom::Engineer)));
    assert!(
        f.update_steps
            .iter()
            .any(|(_, t, _)| t.contains("conflicts in calc/__init__.py"))
    );
    let _ = std::fs::remove_dir_all(&d);
}
