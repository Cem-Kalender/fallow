//! The memory budget (005 research R-14, FR-037, FR-038): a session starts only when the measured
//! footprint of the running sessions plus its expected size fits.

impl super::Hub {
    /// Whether a start of this project waits for memory (FR-038).
    pub fn memory_waiting(&self, project: crate::projects::ProjectId) -> bool {
        self.memory_waits.iter().any(|(id, _)| {
            self.starts
                .iter()
                .any(|s| s.id == *id && s.project == project)
        })
    }
}
