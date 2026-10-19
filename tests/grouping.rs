//! Grouping (FR-013) and summary totals (FR-016).

use std::collections::{HashMap, HashSet};

use fallow::procmon::group::{self, Parents};
use fallow::procmon::snapshot::{
    Confidence, Footprint, FootprintMetric, Kind, LlmProcess, ProcessKey, Status,
};
use fallow::procmon::summary;

fn k(pid: u32) -> ProcessKey {
    ProcessKey { pid, start_time: 7 }
}

/// 1 init ─ 10 claude (LLM) ─ 11 bash ─ 12 pytest
///                          └ 13 ollama-client (LLM, nested)
///        ─ 20 ollama (LLM) ─ 21 runner
///        ─ 30 vim
fn tree() -> Parents {
    [
        (k(1), None),
        (k(10), Some(k(1))),
        (k(11), Some(k(10))),
        (k(12), Some(k(11))),
        (k(13), Some(k(10))),
        (k(20), Some(k(1))),
        (k(21), Some(k(20))),
        (k(30), Some(k(1))),
    ]
    .into()
}

fn llm_set() -> HashSet<ProcessKey> {
    [k(10), k(13), k(20)].into()
}

#[test]
fn descendants_group_under_the_nearest_llm_ancestor() {
    let groups = group::build(&tree(), &llm_set());
    let roots: Vec<u32> = groups.iter().map(|g| g.root.pid).collect();
    assert_eq!(
        roots,
        [10, 20],
        "a nested LLM process does not root its own group"
    );
    let g10 = &groups[0];
    let mut members: Vec<u32> = g10.members.iter().map(|m| m.pid).collect();
    members.sort();
    assert_eq!(members, [11, 12, 13]);
    assert_eq!(g10.nested, [k(13)]);
    assert_eq!(groups[1].members, [k(21)]);
    assert!(groups.iter().all(|g| !g.members.contains(&k(30))));
}

#[test]
fn group_totals_count_each_member_once() {
    let mut groups = group::build(&tree(), &llm_set());
    let mb = |pid: u32| Some(pid as u64 * 1_000_000);
    group::totals(&mut groups, |key| mb(key.pid), |key| key.pid as f32 / 10.0);
    assert_eq!(groups[0].total_footprint, (10 + 11 + 12 + 13) * 1_000_000);
    assert!((groups[0].total_cpu - 4.6).abs() < 1e-4);
    assert_eq!(groups[1].total_footprint, (20 + 21) * 1_000_000);
}

fn llm(pid: u32, kind: Kind) -> (ProcessKey, LlmProcess) {
    (
        k(pid),
        LlmProcess {
            key: k(pid),
            kind,
            signals: Default::default(),
            confidence: Confidence::Certain,
            provider: None,
            model: None,
            status: Status::Idle,
        },
    )
}

#[test]
fn llm_ram_counts_members_unique_across_groups() {
    let mut groups = group::build(&tree(), &llm_set());
    // A process listed in both groups (shared by a broken tree) still counts once.
    groups[1].members.push(k(12));
    let fp = |b: u64| Footprint {
        bytes: b,
        metric: FootprintMetric::Pss,
    };
    let footprints: HashMap<ProcessKey, Footprint> = [10, 11, 12, 13, 20, 21]
        .into_iter()
        .map(|p| (k(p), fp(p as u64)))
        .collect();
    let llms: HashMap<ProcessKey, LlmProcess> = [
        llm(10, Kind::AgentCli),
        llm(13, Kind::Script),
        llm(20, Kind::LocalModelServer),
    ]
    .into();
    let s = summary::summarize(&groups, &llms, &footprints, 1000, 50, fp(5));
    assert_eq!(s.llm_ram, 10 + 11 + 12 + 13 + 20 + 21);
    assert_eq!(s.local_model_ram, 20 + 21 + 12);
    assert!(s.low_memory, "50 of 1000 is under 10%");
    assert_eq!(s.fallow_ram, fp(5));
}

#[test]
fn parent_loops_do_not_hang() {
    let parents: Parents = [(k(1), Some(k(2))), (k(2), Some(k(1)))].into();
    let groups = group::build(&parents, &[k(1)].into());
    assert!(groups.len() <= 1);
}

#[test]
fn a_reused_pid_does_not_adopt_older_processes() {
    // Seen on Windows: OneDrive (started at t=100) still reports parent PID 27, which exited;
    // PID 27 now belongs to fallow.exe, started at t=500 under an agent. OneDrive must not
    // join the agent's group.
    let agent = ProcessKey {
        pid: 20,
        start_time: 50,
    };
    let fallow = ProcessKey {
        pid: 27,
        start_time: 500,
    };
    let onedrive = ProcessKey {
        pid: 84,
        start_time: 100,
    };
    let child = ProcessKey {
        pid: 90,
        start_time: 600,
    };
    let by_pid: HashMap<u32, ProcessKey> = [agent, fallow, onedrive, child]
        .iter()
        .map(|k| (k.pid, *k))
        .collect();
    let parents = group::link_parents(
        [
            (agent, None),
            (fallow, Some(20)),
            (onedrive, Some(27)),
            (child, Some(27)),
        ],
        &by_pid,
    );
    assert_eq!(parents[&fallow], Some(agent));
    assert_eq!(
        parents[&onedrive], None,
        "the PID-27 parent is younger than OneDrive"
    );
    assert_eq!(
        parents[&child],
        Some(fallow),
        "a real child keeps its parent"
    );
    let groups = group::build(&parents, &[agent].into_iter().collect());
    let members: HashSet<ProcessKey> = groups[0].members.iter().copied().collect();
    assert!(members.contains(&fallow) && members.contains(&child));
    assert!(!members.contains(&onedrive));
}
