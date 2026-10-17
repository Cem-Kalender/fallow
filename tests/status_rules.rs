//! FR-012 status table tests: first match wins, in the order serving → LLM connected → busy →
//! idle (data-model.md "Status").

use std::collections::HashSet;
use std::net::SocketAddr;

use fallow::config::{self, Kind};
use fallow::procmon::detect::ProviderSet;
use fallow::procmon::snapshot::{ConnState, Connection, ProcessKey, Status};
use fallow::procmon::status::{ConnectionFacts, StatusInput, evaluate};

fn input(kind: Kind, incoming: bool, outgoing: bool, mean_cpu: f32) -> StatusInput {
    StatusInput {
        kind,
        incoming,
        outgoing,
        mean_cpu,
    }
}

#[test]
fn first_match_wins() {
    use Kind::*;
    use Status::*;
    let table = [
        // kind, incoming, outgoing, cpu => status
        (LocalModelServer, true, false, 0.0, Serving),
        (LocalModelServer, true, true, 90.0, Serving), // serving regardless of CPU or outgoing
        (LocalModelServer, false, true, 0.0, LlmConnected),
        (LocalModelServer, false, false, 50.0, Busy),
        (LocalModelServer, false, false, 0.1, Idle),
        (AgentCli, true, false, 0.0, Idle), // only servers can be "serving"
        (AgentCli, false, true, 0.0, LlmConnected), // idle keep-alive stays connected
        (AgentCli, false, true, 99.0, LlmConnected),
        (Script, false, false, 2.01, Busy),
        (Script, false, false, 2.0, Idle), // busy is strictly above 2%
        (Unknown, false, false, 0.0, Idle),
    ];
    for (kind, inc, out, cpu, want) in table {
        assert_eq!(
            evaluate(&input(kind, inc, out, cpu)),
            want,
            "{kind:?} {inc} {out} {cpu}"
        );
    }
}

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

fn key(pid: u32) -> ProcessKey {
    ProcessKey {
        pid,
        start_time: 1000 + pid as u64,
    }
}

fn conn(owner: u32, local: &str, remote: &str, state: ConnState) -> Connection {
    Connection {
        owner: key(owner),
        local: a(local),
        remote: a(remote),
        state,
    }
}

fn providers() -> ProviderSet {
    ProviderSet::new(&config::load_from(None, None, None).providers, [])
}

#[test]
fn incoming_connections_never_make_a_server_llm_connected() {
    let server = 10;
    let conns = [
        conn(server, "127.0.0.1:11434", "0.0.0.0:0", ConnState::Listen),
        conn(
            server,
            "127.0.0.1:11434",
            "127.0.0.1:40000",
            ConnState::Established,
        ),
    ];
    let servers: HashSet<ProcessKey> = [key(server)].into();
    let facts = ConnectionFacts::new(&conns, &servers, &providers());
    let i = facts.input(key(server), Kind::LocalModelServer, 0.0);
    assert!(i.incoming && !i.outgoing, "{i:?}");
    assert_eq!(evaluate(&i), Status::Serving);
    // Without the server kind the same facts are idle, never "LLM connected".
    assert_eq!(
        evaluate(&facts.input(key(server), Kind::Unknown, 0.0)),
        Status::Idle
    );
}

#[test]
fn clients_of_servers_and_providers_are_llm_connected() {
    let (server, client, agent, other) = (10, 11, 12, 13);
    let conns = [
        conn(server, "127.0.0.1:11434", "0.0.0.0:0", ConnState::Listen),
        conn(
            client,
            "127.0.0.1:40000",
            "127.0.0.1:11434",
            ConnState::Established,
        ),
        conn(
            agent,
            "10.0.0.2:50000",
            "160.79.104.10:443",
            ConnState::Established,
        ),
        // Port 443 of a non-provider address does not count.
        conn(
            other,
            "10.0.0.2:50001",
            "93.184.216.34:443",
            ConnState::Established,
        ),
    ];
    let servers: HashSet<ProcessKey> = [key(server)].into();
    let facts = ConnectionFacts::new(&conns, &servers, &providers());
    assert_eq!(
        evaluate(&facts.input(key(client), Kind::Script, 0.0)),
        Status::LlmConnected
    );
    assert_eq!(
        evaluate(&facts.input(key(agent), Kind::AgentCli, 0.0)),
        Status::LlmConnected
    );
    assert_eq!(
        evaluate(&facts.input(key(other), Kind::Unknown, 0.0)),
        Status::Idle
    );
    assert_eq!(
        evaluate(&facts.input(key(server), Kind::LocalModelServer, 5.0)),
        Status::Busy
    );
}
