//! Sorting (FR-014, US2 AS1–AS2): group totals for memory and CPU, stable orders, repeat to
//! reverse, keys and header clicks.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use fallow::app::state::{AppState, Pane};
use fallow::procmon::snapshot::{
    Confidence, CwdState, Footprint, FootprintMetric, Kind, LlmProcess, ProcessGroup, ProcessKey,
    ProcessRecord, Snapshot, Status, SystemSummary,
};
use fallow::ui::layout;
use fallow::ui::processes::{self, Col};
use ratatui::layout::Rect;

fn k(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        start_time: 1_000 + pid as u64,
    }
}

struct G {
    root: u32,
    name: &'static str,
    own_mem: u64,
    member_mem: u64,
    cpu: f32,
    status: Status,
    confidence: Confidence,
}

/// Three groups. Root "small" has the largest own figure but the smallest group total, so
/// sorting by group totals is distinguishable from sorting by the root alone.
fn snapshot() -> Snapshot {
    let groups = [
        G {
            root: 10,
            name: "big",
            own_mem: 100,
            member_mem: 900,
            cpu: 1.0,
            status: Status::Idle,
            confidence: Confidence::Certain,
        },
        G {
            root: 20,
            name: "Alpha",
            own_mem: 300,
            member_mem: 0,
            cpu: 9.0,
            status: Status::Serving,
            confidence: Confidence::Likely,
        },
        G {
            root: 30,
            name: "small",
            own_mem: 400,
            member_mem: 50,
            cpu: 3.0,
            status: Status::LlmConnected,
            confidence: Confidence::Certain,
        },
    ];
    let fp = |b| {
        Some(Footprint {
            bytes: b,
            metric: FootprintMetric::Pss,
        })
    };
    let mut records = HashMap::new();
    let mut llm = HashMap::new();
    let mut out = Vec::new();
    for g in &groups {
        let member = g.root + 1;
        records.insert(k(g.root), record(g.root, None, g.name, fp(g.own_mem)));
        let mut members = Vec::new();
        if g.member_mem > 0 {
            records.insert(
                k(member),
                record(member, Some(k(g.root)), "child", fp(g.member_mem)),
            );
            members.push(k(member));
        }
        llm.insert(
            k(g.root),
            LlmProcess {
                key: k(g.root),
                kind: Kind::AgentCli,
                signals: Default::default(),
                confidence: g.confidence,
                provider: None,
                model: None,
                status: g.status,
            },
        );
        out.push(ProcessGroup {
            root: k(g.root),
            members,
            nested: Vec::new(),
            total_footprint: g.own_mem + g.member_mem,
            total_cpu: g.cpu,
        });
    }
    Snapshot {
        taken_at: UNIX_EPOCH + Duration::from_secs(10_000),
        groups: out,
        llm,
        records,
        summary: SystemSummary {
            total_ram: 1 << 34,
            available_ram: 1 << 33,
            llm_ram: 0,
            local_model_ram: 0,
            fallow_ram: Footprint {
                bytes: 1,
                metric: FootprintMetric::Pss,
            },
            low_memory: false,
        },
        notes: Vec::new(),
    }
}

fn record(
    pid: u32,
    parent: Option<ProcessKey>,
    name: &str,
    footprint: Option<Footprint>,
) -> ProcessRecord {
    ProcessRecord {
        key: k(pid),
        parent,
        name: name.into(),
        cmdline: None,
        owner: None,
        cpu_samples: [0.0; 5],
        footprint,
        cwd: CwdState::Unknown,
    }
}

fn state() -> AppState {
    let mut s = AppState::new(None, Vec::new());
    s.focus = Pane::Processes;
    s.area = Rect::new(0, 0, 120, 40);
    s.set_snapshot(Arc::new(snapshot()));
    s
}

fn order(s: &AppState) -> Vec<u32> {
    let snap = s.snapshot.as_deref().unwrap();
    processes::rows(snap, &s.panel.expanded, s.panel.sort)
        .iter()
        .filter(|r| r.depth == 0)
        .map(|r| r.key.pid)
        .collect()
}

fn key(s: &mut AppState, c: char) {
    s.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn memory_sorts_by_group_total_highest_first_and_reverses() {
    let mut s = state();
    assert_eq!(
        order(&s),
        [10, 30, 20],
        "default: memory, by group total (1000, 450, 300)"
    );
    key(&mut s, 'm');
    assert_eq!(order(&s), [20, 30, 10], "m again reverses");
    key(&mut s, 'm');
    assert_eq!(order(&s), [10, 30, 20]);
}

#[test]
fn cpu_sorts_by_group_total_highest_first() {
    let mut s = state();
    key(&mut s, 'c');
    assert_eq!(order(&s), [20, 30, 10]);
    key(&mut s, 'c');
    assert_eq!(order(&s), [10, 30, 20]);
}

#[test]
fn uptime_name_status_confidence() {
    let mut s = state();
    key(&mut s, 'u');
    assert_eq!(
        order(&s),
        [10, 20, 30],
        "longest running (earliest start) first"
    );
    key(&mut s, 'n');
    assert_eq!(
        order(&s),
        [20, 10, 30],
        "case-insensitive: Alpha, big, small"
    );
    assert!(s.run("proc.sort.status"));
    assert_eq!(order(&s), [20, 30, 10], "serving, LLM connected, idle");
    assert!(s.run("proc.sort.confidence"));
    assert_eq!(
        order(&s),
        [10, 30, 20],
        "certain first; ties keep a stable order by PID"
    );
    assert!(s.run("proc.sort.confidence"));
    assert_eq!(
        order(&s),
        [20, 10, 30],
        "reversed: likely first, ties still by PID"
    );
}

#[test]
fn confidence_has_no_single_key() {
    let mut s = state();
    let before = s.panel.sort;
    for c in ['f', 'C', 's', 'S'] {
        key(&mut s, c);
    }
    assert_eq!(s.panel.sort.column, before.column);
}

#[test]
fn clicking_a_header_sorts_by_it_and_clicking_again_reverses() {
    let mut s = state();
    let g = layout::geometry(s.area, s.sizes, s.focus);
    let y = g.list.y + 1;
    let x = (g.list.x..g.list.right())
        .find(|&x| processes::header_at(&g.list, false, x, y) == Some(Col::Status))
        .expect("Status header visible at 120x40");
    let click = |s: &mut AppState| {
        s.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    };
    click(&mut s);
    assert_eq!(s.panel.sort.column, Col::Status);
    assert_eq!(order(&s), [20, 30, 10]);
    assert_eq!(s.panel.sort.arrow(), "▾");
    click(&mut s);
    assert_eq!(order(&s), [10, 30, 20]);
    assert_eq!(s.panel.sort.arrow(), "▴");
}

#[test]
fn the_selected_process_stays_selected_when_the_order_changes() {
    let mut s = state();
    s.select(2); // pid 20 under the default order
    key(&mut s, 'm');
    let snap = s.snapshot.as_deref().unwrap();
    let rows = processes::rows(snap, &s.panel.expanded, s.panel.sort);
    assert_eq!(rows[s.panel.selected].key.pid, 20);
}

#[test]
fn the_active_header_shows_its_direction() {
    let s = state();
    let screen = {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        t.draw(|f| fallow::ui::draw(f, &s)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..40)
            .map(|y| {
                (0..120)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(screen.contains("Mem▾"), "{screen}");
}
