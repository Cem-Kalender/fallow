//! The memory budget (005 research R-14, FR-037, FR-038): a session starts only when the measured
//! footprint of the running sessions plus its expected size fits.

/// What Fallow measured (the process monitor's footprints), in MB.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sample {
    pub total_mb: u64,
    pub fallow_mb: u64,
    /// Every running session's process tree (gate sessions included): id, profile id, MB.
    pub sessions: Vec<(u32, String, u64)>,
}

impl super::Hub {
    /// The budget in MB: the engineer's setting, or 75% of the machine's memory minus Fallow's
    /// own (research R-14).
    pub fn memory_budget(&self, m: &Sample) -> u64 {
        match self.settings.memory_budget_mb {
            0 => (m.total_mb * 3 / 4).saturating_sub(m.fallow_mb),
            mb => mb,
        }
    }

    /// The expected size of a new session of `profile`: the largest seen, else the setting.
    pub fn expected_size(&self, profile: &str) -> u64 {
        self.profile_sizes
            .get(profile)
            .copied()
            .unwrap_or(self.settings.session_size_mb)
    }

    /// How many MB a new session of `profile` lacks to fit the budget, if any (FR-037).
    pub fn memory_shortfall(&mut self, m: &Sample, profile: &str) -> Option<u64> {
        let budget = self.memory_budget(m);
        self.memory_budget_seen = budget;
        let used: u64 = m.sessions.iter().map(|(_, _, mb)| *mb).sum();
        let need = used + self.expected_size(profile);
        (need > budget).then(|| need - budget)
    }

    /// Learns sizes from the sample and lets waiting starts go when they fit (FR-038).
    pub(crate) fn tick_memory(&mut self, sessions: &dyn super::Sessions) -> bool {
        let Some(m) = sessions.memory() else {
            return false;
        };
        for (_, profile, mb) in &m.sessions {
            let e = self.profile_sizes.entry(profile.clone()).or_insert(0);
            *e = (*e).max(*mb);
        }
        if self.memory_waits.is_empty() {
            return false;
        }
        let mut freed = Vec::new();
        for (id, _) in self.memory_waits.clone() {
            let Some(profile) = self
                .starts
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.profile.clone())
            else {
                freed.push(id);
                continue;
            };
            match self.memory_shortfall(&m, &profile) {
                None => freed.push(id),
                Some(mb) => {
                    if let Some(w) = self.memory_waits.iter_mut().find(|(x, _)| *x == id) {
                        w.1 = mb;
                    }
                }
            }
            // One start at a time: the next is decided on a new sample.
            if !freed.is_empty() {
                break;
            }
        }
        let changed = !freed.is_empty();
        self.memory_waits.retain(|(id, _)| !freed.contains(id));
        for id in freed {
            if self.starts.iter().any(|s| s.id == id) {
                self.notices
                    .push(format!("Memory is free: start {id} goes ahead"));
            }
        }
        changed
    }

    /// The memory line for `squad_status` (FR-037, FR-038): starts waiting and their shortfall.
    pub fn memory_status(&self, project: crate::projects::ProjectId) -> serde_json::Value {
        let waiting: Vec<serde_json::Value> = self
            .memory_waits
            .iter()
            .filter(|(id, _)| {
                self.starts
                    .iter()
                    .any(|s| s.id == *id && s.project == project)
            })
            .map(|(id, mb)| serde_json::json!({"start": id, "short_mb": mb}))
            .collect();
        serde_json::json!({
            "budget_mb": self.memory_budget_seen,
            "setting_mb": self.settings.memory_budget_mb,
            "waiting": waiting,
        })
    }

    /// Whether a start of this project waits for memory (FR-038).
    pub fn memory_waiting(&self, project: crate::projects::ProjectId) -> bool {
        self.memory_waits.iter().any(|(id, _)| {
            self.starts
                .iter()
                .any(|s| s.id == *id && s.project == project)
        })
    }
}
