//! The snapshot model (data-model.md, "Process monitoring").
//!
//! Process identity is always (PID, start time), never the PID alone. There is deliberately no
//! environment-based detection signal (FR-024).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use compact_str::CompactString;
use smallvec::SmallVec;

pub use crate::config::Kind;

/// Identifies one process: the PID plus its start time (seconds since the epoch, from the OS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcessKey {
    pub pid: u32,
    pub start_time: u64,
}

/// The owning user in the OS's own form: a numeric uid on Linux and macOS, a SID on Windows.
/// Shown only in all-users mode (FR-031).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Uid(pub CompactString);

/// One visible process in a snapshot.
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    pub key: ProcessKey,
    pub parent: Option<ProcessKey>,
    pub name: CompactString,
    /// Read once per key and cached (R-1, R-2).
    pub cmdline: Option<Arc<[Box<str>]>>,
    pub owner: Option<Uid>,
    /// The last 5 refreshes, as % of one core.
    pub cpu_samples: [f32; 5],
    /// Only for LLM group members (R-3).
    pub footprint: Option<Footprint>,
    /// Only for LLM group members.
    pub cwd: CwdState,
}

impl ProcessRecord {
    /// Mean of the CPU samples (unfilled slots count as 0 for a new process).
    pub fn mean_cpu(&self) -> f32 {
        self.cpu_samples.iter().sum::<f32>() / self.cpu_samples.len() as f32
    }
}

/// Which memory metric a figure uses; shown in the UI (FR-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FootprintMetric {
    PhysFootprint,
    PrivateWorkingSet,
    Pss,
    Rss,
    WorkingSet,
}

impl FootprintMetric {
    /// The label shown in the UI (contracts/ui-commands.md "Displayed semantics").
    pub fn label(self) -> &'static str {
        match self {
            FootprintMetric::PhysFootprint => "footprint",
            FootprintMetric::PrivateWorkingSet => "private WS",
            FootprintMetric::Pss => "PSS",
            FootprintMetric::Rss => "RSS",
            FootprintMetric::WorkingSet => "WS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    pub bytes: u64,
    pub metric: FootprintMetric,
}

/// A process's working directory. On Windows it moves Unknown → Pending → Known or
/// Unavailable under the R-10 policy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CwdState {
    #[default]
    Unknown,
    Pending,
    Known(PathBuf),
    Unavailable(CompactString),
}

/// Only these three TCP states are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnState {
    Established,
    Listen,
    Other,
}

/// A TCP connection that could matter for detection (data-model.md, "Connection").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub owner: ProcessKey,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: ConnState,
}

/// How a provider address was recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Via {
    Resolved,
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignalKind {
    Signature {
        id: CompactString,
    },
    ProviderConnection {
        provider_id: CompactString,
        via: Via,
    },
    LocalModelConnection {
        server: ProcessKey,
    },
    /// The user started it as a Fallow session (002 FR-013).
    Session {
        id: u32,
    },
}

/// `Certain` ranks above `Likely`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Likely,
    Certain,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetectionSignal {
    pub kind: SignalKind,
    pub confidence: Confidence,
}

impl DetectionSignal {
    /// A signature match is certain; a connection is only likely.
    pub fn new(kind: SignalKind) -> Self {
        let confidence = match kind {
            SignalKind::Signature { .. } | SignalKind::Session { .. } => Confidence::Certain,
            SignalKind::ProviderConnection { .. } | SignalKind::LocalModelConnection { .. } => {
                Confidence::Likely
            }
        };
        DetectionSignal { kind, confidence }
    }
}

/// FR-012 status, recomputed from scratch every snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Serving,
    LlmConnected,
    Busy,
    Idle,
}

/// A detected LLM process.
#[derive(Debug, Clone)]
pub struct LlmProcess {
    pub key: ProcessKey,
    pub kind: Kind,
    /// At least one (FR-025).
    pub signals: SmallVec<[DetectionSignal; 2]>,
    /// The highest confidence among the signals.
    pub confidence: Confidence,
    pub provider: Option<CompactString>,
    pub model: Option<CompactString>,
    pub status: Status,
}

/// An LLM process with no LLM ancestor, and all its descendants.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessGroup {
    pub root: ProcessKey,
    pub members: Vec<ProcessKey>,
    /// Descendants that are themselves LLM processes (FR-013).
    pub nested: Vec<ProcessKey>,
    /// Each member counted once.
    pub total_footprint: u64,
    pub total_cpu: f32,
}

/// FR-016.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemSummary {
    pub total_ram: u64,
    pub available_ram: u64,
    pub llm_ram: u64,
    pub local_model_ram: u64,
    pub fallow_ram: Footprint,
    pub low_memory: bool,
}

impl SystemSummary {
    /// "available_ram < 10% of total_ram".
    pub fn is_low_memory(total_ram: u64, available_ram: u64) -> bool {
        available_ram.saturating_mul(10) < total_ram
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Container, remote and WSL2 statements (FR-018).
    Coverage,
    /// A reduced-capability mode, e.g. data that needs elevation.
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub kind: NoteKind,
    pub text: CompactString,
}

/// One refresh's view, sent from procmon to the UI as `Arc<Snapshot>`.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub taken_at: SystemTime,
    pub groups: Vec<ProcessGroup>,
    pub llm: HashMap<ProcessKey, LlmProcess>,
    /// LLM group members only.
    pub records: HashMap<ProcessKey, ProcessRecord>,
    pub summary: SystemSummary,
    pub notes: Vec<Note>,
    /// Agents started as Fallow sessions: process → session id (002 FR-013, R-8).
    pub sessions: HashMap<ProcessKey, u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_certain_connections_are_likely() {
        let s = DetectionSignal::new(SignalKind::Signature {
            id: "ollama".into(),
        });
        assert_eq!(s.confidence, Confidence::Certain);
        let c = DetectionSignal::new(SignalKind::ProviderConnection {
            provider_id: "openai".into(),
            via: Via::Resolved,
        });
        assert_eq!(c.confidence, Confidence::Likely);
        assert!(Confidence::Certain > Confidence::Likely);
    }

    #[test]
    fn low_memory_below_ten_percent() {
        assert!(SystemSummary::is_low_memory(1000, 99));
        assert!(!SystemSummary::is_low_memory(1000, 100));
    }

    #[test]
    fn keys_differ_by_start_time() {
        let a = ProcessKey {
            pid: 7,
            start_time: 1,
        };
        let b = ProcessKey {
            pid: 7,
            start_time: 2,
        };
        assert_ne!(a, b, "a reused PID is a different process");
    }
}
