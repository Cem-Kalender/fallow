//! System summary (FR-016, FR-019).

use std::collections::{HashMap, HashSet};

use super::snapshot::{Footprint, Kind, LlmProcess, ProcessGroup, ProcessKey, SystemSummary};

/// `footprints` holds the figures of group members; `fallow` is Fallow's own, in the same
/// metric.
pub fn summarize(
    groups: &[ProcessGroup],
    llm: &HashMap<ProcessKey, LlmProcess>,
    footprints: &HashMap<ProcessKey, Footprint>,
    total_ram: u64,
    available_ram: u64,
    fallow: Footprint,
) -> SystemSummary {
    let sum = |keys: &HashSet<ProcessKey>| -> u64 {
        keys.iter()
            .filter_map(|k| footprints.get(k))
            .map(|f| f.bytes)
            .sum()
    };
    let all: HashSet<ProcessKey> = groups.iter().flat_map(members_of).collect();
    let local: HashSet<ProcessKey> = groups
        .iter()
        .filter(|g| {
            llm.get(&g.root)
                .is_some_and(|l| l.kind == Kind::LocalModelServer)
        })
        .flat_map(members_of)
        .collect();
    SystemSummary {
        total_ram,
        available_ram,
        llm_ram: sum(&all),
        local_model_ram: sum(&local),
        fallow_ram: fallow,
        low_memory: SystemSummary::is_low_memory(total_ram, available_ram),
    }
}

fn members_of(g: &ProcessGroup) -> impl Iterator<Item = ProcessKey> + '_ {
    std::iter::once(g.root).chain(g.members.iter().copied())
}
