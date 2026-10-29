//! The memory budget (005 research R-14, FR-037, FR-038): a session starts only when the measured
//! footprint of the running sessions plus its expected size fits.

impl super::Hub {
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
        serde_json::json!({ "budget_mb": self.settings.memory_budget_mb, "waiting": waiting })
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
