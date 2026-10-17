//! Grouping (FR-013): each LLM process with no LLM ancestor roots a group of all its
//! descendants. Descendants that are LLM processes are listed as nested, not as groups.

use std::collections::{HashMap, HashSet};

use super::snapshot::{ProcessGroup, ProcessKey};

/// Parent links between visible processes.
pub type Parents = HashMap<ProcessKey, Option<ProcessKey>>;

/// Ancestor chains longer than this are treated as broken (guards against loops).
const MAX_DEPTH: usize = 512;

/// Builds the groups. Totals are left at zero; `totals` fills them in.
pub fn build(parents: &Parents, llm: &HashSet<ProcessKey>) -> Vec<ProcessGroup> {
    let mut children: HashMap<ProcessKey, Vec<ProcessKey>> = HashMap::new();
    for (&child, parent) in parents {
        if let Some(p) = parent {
            children.entry(*p).or_default().push(child);
        }
    }
    for list in children.values_mut() {
        list.sort();
    }
    let mut roots: Vec<ProcessKey> = llm
        .iter()
        .copied()
        .filter(|k| parents.contains_key(k) && !has_llm_ancestor(*k, parents, llm))
        .collect();
    roots.sort();
    roots
        .into_iter()
        .map(|root| {
            let mut members = Vec::new();
            let mut stack: Vec<ProcessKey> = children.get(&root).cloned().unwrap_or_default();
            stack.reverse();
            let mut seen: HashSet<ProcessKey> = HashSet::from([root]);
            while let Some(k) = stack.pop() {
                if !seen.insert(k) {
                    continue;
                }
                members.push(k);
                if let Some(c) = children.get(&k) {
                    stack.extend(c.iter().rev());
                }
            }
            let nested = members
                .iter()
                .copied()
                .filter(|m| llm.contains(m))
                .collect();
            ProcessGroup {
                root,
                members,
                nested,
                total_footprint: 0,
                total_cpu: 0.0,
            }
        })
        .collect()
}

fn has_llm_ancestor(key: ProcessKey, parents: &Parents, llm: &HashSet<ProcessKey>) -> bool {
    let mut cur = parents.get(&key).copied().flatten();
    for _ in 0..MAX_DEPTH {
        let Some(p) = cur else { return false };
        if p == key {
            return false;
        }
        if llm.contains(&p) {
            return true;
        }
        cur = parents.get(&p).copied().flatten();
    }
    false
}

/// Fills in each group's totals, counting the root and every member once.
pub fn totals(
    groups: &mut [ProcessGroup],
    footprint: impl Fn(ProcessKey) -> Option<u64>,
    cpu: impl Fn(ProcessKey) -> f32,
) {
    for g in groups {
        let all = std::iter::once(g.root).chain(g.members.iter().copied());
        let unique: HashSet<ProcessKey> = all.collect();
        g.total_footprint = unique.iter().filter_map(|k| footprint(*k)).sum();
        g.total_cpu = unique.iter().map(|k| cpu(*k)).sum();
    }
}
