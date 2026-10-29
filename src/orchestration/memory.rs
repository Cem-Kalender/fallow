//! The memory budget (005 research R-14, FR-037, FR-038): a session starts only when the measured
//! footprint of the running sessions plus its expected size fits.

use std::time::{Duration, Instant};

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

    /// Memory promised but not yet in the sample (005 review): approved starts not carried out
    /// and not waiting (with `before`, only those requested earlier), and running sessions the
    /// sample does not show yet.
    pub fn reserved_mb(
        &self,
        sessions: &dyn super::Sessions,
        m: &Sample,
        before: Option<u64>,
    ) -> u64 {
        let unmeasured: u64 = sessions
            .all()
            .iter()
            .filter(|s| s.running && !m.sessions.iter().any(|(id, _, _)| *id == s.id))
            .map(|s| self.expected_size(&s.profile_id))
            .sum();
        self.approved_mb(before) + unmeasured
    }

    /// The expected size of approved starts not carried out and not waiting for memory.
    pub fn approved_mb(&self, before: Option<u64>) -> u64 {
        self.starts
            .iter()
            .filter(|s| s.state == super::StartState::Approved && before.is_none_or(|b| s.id < b))
            .filter(|s| !self.memory_waits.iter().any(|(id, _, _)| *id == s.id))
            .map(|s| self.expected_size(&s.profile))
            .sum()
    }

    /// A gate run of `project` waits for memory, or no longer does: the engineer and the lead
    /// are told once per wait (005 review).
    pub fn gate_memory_wait(&mut self, project: crate::projects::ProjectId, short: Option<u64>) {
        let waiting = self.gate_memory_waits.contains(&project);
        match short {
            Some(mb) if !waiting => {
                self.gate_memory_waits.push(project);
                let text = format!(
                    "A gate run waits for memory: {mb} MB short of the budget. It starts when a session ends or memory frees up."
                );
                self.alerts.push(text.clone());
                self.tell_lead(project, text);
            }
            None if waiting => self.gate_memory_waits.retain(|p| *p != project),
            _ => {}
        }
    }

    /// How many MB a new session of `profile` lacks to fit the budget, if any (FR-037), with
    /// `reserved` MB already promised to others.
    pub fn memory_shortfall(&mut self, m: &Sample, profile: &str, reserved: u64) -> Option<u64> {
        let budget = self.memory_budget(m);
        self.memory_budget_seen = budget;
        let used: u64 = m.sessions.iter().map(|(_, _, mb)| *mb).sum::<u64>() + reserved;
        let need = used + self.expected_size(profile);
        (need > budget).then(|| need - budget)
    }

    /// Starts that waited longer than `request_timeout_s` are refused, and the lead is told.
    fn memory_timeouts(&mut self, now: Instant) -> bool {
        let mut changed = false;
        // Starts that waited too long are refused, and the lead is told.
        let limit = Duration::from_secs(self.settings.request_timeout_s);
        let stale: Vec<u64> = self
            .memory_waits
            .iter()
            .filter(|(_, _, since)| now.duration_since(*since) >= limit)
            .map(|(id, _, _)| *id)
            .collect();
        for id in stale {
            self.memory_waits.retain(|(x, _, _)| *x != id);
            let Some(s) = self.starts.iter_mut().find(|s| s.id == id) else {
                continue;
            };
            s.state = super::StartState::Refused("no room in the memory budget".into());
            let (project, text) = (
                s.project,
                format!(
                    "Starting {} ({}) in {} was refused: it waited {} s for memory. End a session or raise orchestration.memory_budget_mb.",
                    s.profile_name,
                    s.role.as_deref().unwrap_or("no role"),
                    s.project_name,
                    limit.as_secs()
                ),
            );
            self.start_outcome(id);
            self.alerts.push(text.clone());
            self.tell_lead(project, text);
            changed = true;
        }
        changed
    }

    /// Learns sizes from the sample, holds approved starts that do not fit (also those the
    /// engineer approved later), lets waiting starts go when they fit, and refuses a start that
    /// waited longer than `request_timeout_s` (FR-038).
    pub(crate) fn tick_memory(&mut self, sessions: &dyn super::Sessions, now: Instant) -> bool {
        let mut changed = self.memory_timeouts(now);
        let Some(m) = sessions.memory() else {
            return changed;
        };
        for (_, profile, mb) in &m.sessions {
            let e = self.profile_sizes.entry(profile.clone()).or_insert(0);
            *e = (*e).max(*mb);
        }
        // Approved starts not checked yet (approved by the engineer after the request).
        let unchecked: Vec<(u64, String)> = self
            .starts
            .iter()
            .filter(|s| s.state == super::StartState::Approved)
            .filter(|s| !self.memory_waits.iter().any(|(id, _, _)| *id == s.id))
            .map(|s| (s.id, s.profile.clone()))
            .collect();
        for (id, profile) in unchecked {
            let reserved = self.reserved_mb(sessions, &m, Some(id));
            if let Some(mb) = self.memory_shortfall(&m, &profile, reserved) {
                let first = !self.memory_told.contains(&id);
                let since = self.memory_held.remove(&id).unwrap_or(now);
                self.memory_waits.push((id, mb, since));
                self.memory_told.insert(id);
                if let Some(s) = self.starts.iter().find(|s| s.id == id).filter(|_| first) {
                    let (project, text) = (
                        s.project,
                        format!(
                            "Starting {} ({}) in {} waits for memory: {mb} MB short of the budget. It starts by itself when a session ends or memory frees up.",
                            s.profile_name,
                            s.role.as_deref().unwrap_or("no role"),
                            s.project_name
                        ),
                    );
                    self.alerts.push(text.clone());
                    self.tell_lead(project, text);
                }
                changed = true;
            }
        }
        let mut freed = None;
        for (id, _, _) in self.memory_waits.clone() {
            let Some(profile) = self
                .starts
                .iter()
                .find(|s| s.id == id && s.state == super::StartState::Approved)
                .map(|s| s.profile.clone())
            else {
                freed = Some((id, false));
                break;
            };
            let reserved = self.reserved_mb(sessions, &m, None);
            match self.memory_shortfall(&m, &profile, reserved) {
                // One start at a time: the next is decided on a new sample.
                None => {
                    freed = Some((id, true));
                    break;
                }
                Some(mb) => {
                    if let Some(w) = self.memory_waits.iter_mut().find(|(x, _, _)| *x == id) {
                        w.1 = mb;
                    }
                }
            }
        }
        if let Some((id, goes)) = freed {
            // When it waited, so a start held again keeps counting toward the timeout.
            if let Some(w) = self.memory_waits.iter().find(|(x, _, _)| *x == id) {
                self.memory_held.insert(id, w.2);
            }
            self.memory_waits.retain(|(x, _, _)| *x != id);
            if goes {
                self.notices
                    .push(format!("Memory is free: start {id} goes ahead"));
            }
            changed = true;
        }
        changed
    }

    /// The memory line for `squad_status` (FR-037, FR-038): starts waiting and their shortfall.
    pub fn memory_status(&self, project: crate::projects::ProjectId) -> serde_json::Value {
        let waiting: Vec<serde_json::Value> = self
            .memory_waits
            .iter()
            .filter(|(id, _, _)| {
                self.starts
                    .iter()
                    .any(|s| s.id == *id && s.project == project)
            })
            .map(|(id, mb, _)| serde_json::json!({"start": id, "short_mb": mb}))
            .collect();
        serde_json::json!({
            "budget_mb": self.memory_budget_seen,
            "setting_mb": self.settings.memory_budget_mb,
            "waiting": waiting,
        })
    }

    /// Whether a start of this project waits for memory (FR-038).
    pub fn memory_waiting(&self, project: crate::projects::ProjectId) -> bool {
        self.memory_waits.iter().any(|(id, _, _)| {
            self.starts
                .iter()
                .any(|s| s.id == *id && s.project == project)
        })
    }
}
