//! Status (FR-012, data-model.md): first match wins, recomputed from scratch every snapshot.

use std::collections::{HashMap, HashSet};

use super::detect::ProviderSet;
use super::platform::parse::is_local;
use super::snapshot::{ConnState, Connection, Kind, ProcessKey, Status};

/// "busy" means CPU averaged above this, in % of one core.
pub const BUSY_CPU: f32 = 2.0;

/// What status evaluation needs to know about one LLM process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusInput {
    pub kind: Kind,
    /// At least one established incoming connection on one of its listening ports.
    pub incoming: bool,
    /// At least one established outgoing connection to a provider:443 or a local model server.
    pub outgoing: bool,
    /// Mean of the CPU samples, % of one core.
    pub mean_cpu: f32,
}

pub fn evaluate(i: &StatusInput) -> Status {
    if i.kind == Kind::LocalModelServer && i.incoming {
        Status::Serving
    } else if i.outgoing {
        Status::LlmConnected
    } else if i.mean_cpu > BUSY_CPU {
        Status::Busy
    } else {
        Status::Idle
    }
}

/// Per-process connection facts from the snapshot's connections.
#[derive(Debug, Default)]
pub struct ConnectionFacts {
    incoming: HashSet<ProcessKey>,
    outgoing: HashSet<ProcessKey>,
}

impl ConnectionFacts {
    /// `servers` are the detected local model servers.
    pub fn new(
        conns: &[Connection],
        servers: &HashSet<ProcessKey>,
        providers: &ProviderSet,
    ) -> Self {
        let mut listen: HashMap<u16, ProcessKey> = HashMap::new();
        let mut own_listen: HashSet<(ProcessKey, u16)> = HashSet::new();
        for c in conns.iter().filter(|c| c.state == ConnState::Listen) {
            own_listen.insert((c.owner, c.local.port()));
            if servers.contains(&c.owner) {
                listen.insert(c.local.port(), c.owner);
            }
        }
        let mut facts = ConnectionFacts::default();
        for c in conns.iter().filter(|c| c.state == ConnState::Established) {
            if own_listen.contains(&(c.owner, c.local.port())) {
                // Accepted on one of its own listening ports: incoming, never "connected".
                facts.incoming.insert(c.owner);
                continue;
            }
            let to_provider = c.remote.port() == 443 && providers.lookup(c.remote.ip()).is_some();
            let to_server = listen
                .get(&c.remote.port())
                .is_some_and(|&s| s != c.owner && is_local(c.local, c.remote));
            if to_provider || to_server {
                facts.outgoing.insert(c.owner);
            }
        }
        facts
    }

    pub fn input(&self, key: ProcessKey, kind: Kind, mean_cpu: f32) -> StatusInput {
        StatusInput {
            kind,
            incoming: self.incoming.contains(&key),
            outgoing: self.outgoing.contains(&key),
            mean_cpu,
        }
    }
}
