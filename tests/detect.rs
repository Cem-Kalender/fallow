//! Detection (FR-020, FR-022, FR-023, FR-025, FR-026) through the real refresh logic
//! (`Monitor`), with a fake platform layer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use fallow::config::{self, Settings};
use fallow::dns;
use fallow::procmon::platform::parse::{self, TcpRow};
use fallow::procmon::platform::{Platform, RawConn, Result, TcpQuery};
use fallow::procmon::snapshot::{
    Confidence, ConnState, Footprint, FootprintMetric, Kind, ProcessKey, SignalKind, Snapshot,
    Status, Via,
};
use fallow::procmon::worker::{Monitor, ProcSample};

#[derive(Default)]
struct World {
    cmdlines: HashMap<u32, Vec<&'static str>>,
    conns: Vec<(u32, TcpRow)>,
    cmdline_reads: Vec<ProcessKey>,
    footprint_reads: Vec<ProcessKey>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<World>>);

impl Platform for Fake {
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>> {
        let mut w = self.0.borrow_mut();
        w.cmdline_reads.push(key);
        let args = w.cmdlines.get(&key.pid).cloned().unwrap_or_default();
        Ok(args.into_iter().map(Box::from).collect::<Vec<_>>().into())
    }
    fn cwd(&mut self, _key: ProcessKey) -> Result<PathBuf> {
        Ok(PathBuf::from("/work"))
    }
    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint> {
        self.0.borrow_mut().footprint_reads.push(key);
        Ok(Footprint {
            bytes: key.pid as u64 * 1024,
            metric: FootprintMetric::Pss,
        })
    }
    fn tcp_table(&mut self, q: &TcpQuery<'_>, out: &mut Vec<RawConn>) -> Result<()> {
        parse::select(&self.0.borrow().conns, q, out);
        Ok(())
    }
    fn suspend(&mut self, _key: ProcessKey) -> Result<()> {
        unreachable!("detection never acts on processes")
    }
    fn resume(&mut self, _key: ProcessKey) -> Result<()> {
        unreachable!("detection never acts on processes")
    }
    fn kill(&mut self, _key: ProcessKey, _force: bool) -> Result<()> {
        unreachable!("detection never acts on processes")
    }
}

fn monitor(fake: &Fake, browsers: bool) -> Monitor<Fake> {
    let cfg = config::load_from(None, None, None);
    let mut settings = Settings::default();
    settings.detection.browsers = browsers;
    Monitor::new(
        fake.clone(),
        &cfg.signatures,
        &settings,
        dns::initial(&cfg.providers),
        false,
        Vec::new(),
    )
}

fn p(pid: u32, start: u64, name: &str) -> ProcSample {
    ProcSample {
        key: ProcessKey {
            pid,
            start_time: start,
        },
        parent_pid: Some(1),
        name: name.into(),
        owner: None,
        cpu: 0.0,
    }
}

fn row(local: &str, remote: &str, state: ConnState) -> TcpRow {
    TcpRow {
        local: local.parse().unwrap(),
        remote: remote.parse().unwrap(),
        state,
        owner: 0,
    }
}

fn llm_of(s: &Snapshot, pid: u32) -> Option<&fallow::procmon::snapshot::LlmProcess> {
    s.llm.values().find(|l| l.key.pid == pid)
}

#[test]
fn signature_match_is_certain() {
    let fake = Fake::default();
    fake.0
        .borrow_mut()
        .cmdlines
        .insert(10, vec!["/home/u/.local/bin/claude", "--resume"]);
    let s = monitor(&fake, false).cycle(&[p(1, 1, "init"), p(10, 5, "claude")], 100, 50);
    let l = llm_of(&s, 10).expect("claude detected");
    assert_eq!(l.kind, Kind::AgentCli);
    assert_eq!(l.confidence, Confidence::Certain);
    assert_eq!(l.provider.as_deref(), Some("anthropic"));
    assert!(matches!(&l.signals[0].kind, SignalKind::Signature { id } if id == "claude-code"));
    assert!(llm_of(&s, 1).is_none());
}

#[test]
fn provider_match_needs_established_port_443_and_a_provider_address() {
    let fake = Fake::default();
    {
        let mut w = fake.0.borrow_mut();
        w.conns = vec![
            (
                20,
                row(
                    "10.0.0.2:50000",
                    "160.79.104.10:443",
                    ConnState::Established,
                ),
            ),
            (
                21,
                row("10.0.0.2:50001", "160.79.104.10:80", ConnState::Established),
            ), // wrong port
            (
                22,
                row("10.0.0.2:50002", "160.79.104.10:443", ConnState::Other),
            ), // closing
            (23, row("160.79.104.10:443", "0.0.0.0:0", ConnState::Listen)), // listening
            (
                24,
                row(
                    "10.0.0.2:50003",
                    "93.184.216.34:443",
                    ConnState::Established,
                ),
            ), // not a provider
        ];
    }
    let procs = [
        p(1, 1, "init"),
        p(20, 5, "python3"),
        p(21, 5, "a"),
        p(22, 5, "b"),
        p(23, 5, "c"),
        p(24, 5, "d"),
    ];
    let s = monitor(&fake, false).cycle(&procs, 100, 50);
    let l = llm_of(&s, 20).expect("provider connection detected");
    assert_eq!(l.confidence, Confidence::Likely);
    assert_eq!(
        l.kind,
        Kind::Script,
        "an interpreter seen only through a connection"
    );
    assert_eq!(l.provider.as_deref(), Some("anthropic"));
    assert!(matches!(
        &l.signals[0].kind,
        SignalKind::ProviderConnection { provider_id, via: Via::Range } if provider_id == "anthropic"
    ));
    assert_eq!(l.status, Status::LlmConnected);
    for pid in [21, 22, 23, 24] {
        assert!(llm_of(&s, pid).is_none(), "pid {pid} must not be detected");
    }
}

#[test]
fn local_model_client_is_likely_with_the_servers_model() {
    let fake = Fake::default();
    {
        let mut w = fake.0.borrow_mut();
        w.cmdlines.insert(30, vec!["ollama", "run", "llama3.2:3b"]);
        w.conns = vec![
            (30, row("127.0.0.1:11434", "0.0.0.0:0", ConnState::Listen)),
            (
                30,
                row("127.0.0.1:11434", "127.0.0.1:40000", ConnState::Established),
            ),
            (
                31,
                row("127.0.0.1:40000", "127.0.0.1:11434", ConnState::Established),
            ),
        ];
    }
    let s = monitor(&fake, false).cycle(
        &[p(1, 1, "init"), p(30, 5, "ollama"), p(31, 6, "node")],
        100,
        50,
    );
    let server = llm_of(&s, 30).unwrap();
    assert_eq!(server.kind, Kind::LocalModelServer);
    assert_eq!(server.model.as_deref(), Some("llama3.2:3b"));
    assert_eq!(server.status, Status::Serving);
    let client = llm_of(&s, 31).expect("client detected");
    assert_eq!(client.confidence, Confidence::Likely);
    assert_eq!(client.model.as_deref(), Some("llama3.2:3b"));
    assert!(
        matches!(client.signals[0].kind, SignalKind::LocalModelConnection { server } if server.pid == 30)
    );
    assert_eq!(client.status, Status::LlmConnected);
}

#[test]
fn browsers_are_dropped_unless_enabled() {
    let fake = Fake::default();
    fake.0.borrow_mut().conns = vec![(
        40,
        row(
            "10.0.0.2:50000",
            "160.79.104.10:443",
            ConnState::Established,
        ),
    )];
    let procs = [p(1, 1, "init"), p(40, 5, "chrome")];
    let s = monitor(&fake, false).cycle(&procs, 100, 50);
    assert!(
        llm_of(&s, 40).is_none(),
        "a browser is excluded, connections included"
    );
    let s = monitor(&fake, true).cycle(&procs, 100, 50);
    let l = llm_of(&s, 40).expect("detected with detection.browsers = true");
    assert_eq!(l.kind, Kind::Browser);
    assert_eq!(l.confidence, Confidence::Certain);
}

#[test]
fn a_reused_pid_never_inherits_the_old_record() {
    let fake = Fake::default();
    fake.0.borrow_mut().cmdlines.insert(50, vec!["claude"]);
    let mut m = monitor(&fake, false);
    let s = m.cycle(&[p(1, 1, "init"), p(50, 100, "claude")], 100, 50);
    assert!(llm_of(&s, 50).is_some());
    // Same PID, new start time, different program.
    fake.0.borrow_mut().cmdlines.insert(50, vec!["bash"]);
    let s = m.cycle(&[p(1, 1, "init"), p(50, 200, "bash")], 100, 50);
    assert!(
        llm_of(&s, 50).is_none(),
        "the new process is not the old claude"
    );
    let reads: Vec<u64> = fake
        .0
        .borrow()
        .cmdline_reads
        .iter()
        .filter(|k| k.pid == 50)
        .map(|k| k.start_time)
        .collect();
    assert_eq!(
        reads,
        [100, 200],
        "the command line is read once per (pid, start time)"
    );
}

#[test]
fn records_without_a_start_time_are_dropped() {
    let fake = Fake::default();
    fake.0.borrow_mut().cmdlines.insert(60, vec!["claude"]);
    let s = monitor(&fake, false).cycle(&[p(1, 1, "init"), p(60, 0, "claude")], 100, 50);
    assert!(s.llm.is_empty());
}

#[test]
fn footprints_are_read_at_most_every_footprint_interval() {
    let fake = Fake::default();
    fake.0.borrow_mut().cmdlines.insert(70, vec!["claude"]);
    let mut m = monitor(&fake, false); // default footprint_interval_ms = 10 000
    let t0 = std::time::Instant::now();
    let mut cpus = Vec::new();
    for i in 0..8u64 {
        let mut agent = p(70, 5, "claude");
        agent.cpu = i as f32;
        let s = m.cycle_at(
            &[p(1, 1, "init"), agent],
            100,
            50,
            t0 + std::time::Duration::from_secs(2 * i),
        );
        let rec = s.records.values().find(|r| r.key.pid == 70).unwrap();
        assert!(
            rec.footprint.is_some(),
            "the last figure is kept between reads"
        );
        cpus.push(rec.cpu_samples);
    }
    let reads = fake
        .0
        .borrow()
        .footprint_reads
        .iter()
        .filter(|k| k.pid == 70)
        .count();
    // Refreshes at 0, 2, …, 14 s: reads at 0 and 10 s only.
    assert_eq!(reads, 2);
    assert_ne!(cpus[0], cpus[7], "CPU still updates on every refresh");
}

#[test]
fn session_agents_are_tracked_as_their_own_groups() {
    // 002 R-8: outer claude (10) → fallow (20) → session shell (30) → child (31). The tracked
    // shell is listed even without a signature, is certain, and roots its own group: its
    // child is not counted in the outer claude's group.
    use fallow::procmon::worker::Request;
    let fake = Fake::default();
    fake.0
        .borrow_mut()
        .cmdlines
        .insert(10, vec!["/home/u/.local/bin/claude"]);
    let mut m = monitor(&fake, false);
    m.request(Request::Track {
        pid: 30,
        session: 4,
    });
    let mut fallow = p(20, 6, "fallow");
    fallow.parent_pid = Some(10);
    let mut shell = p(30, 7, "bash");
    shell.parent_pid = Some(20);
    let mut child = p(31, 8, "pytest");
    child.parent_pid = Some(30);
    let s = m.cycle(
        &[p(1, 1, "init"), p(10, 5, "claude"), fallow, shell, child],
        100,
        50,
    );
    let l = llm_of(&s, 30).expect("the session agent is listed");
    assert_eq!(l.kind, Kind::Session);
    assert_eq!(l.confidence, Confidence::Certain);
    assert!(matches!(l.signals[0].kind, SignalKind::Session { id: 4 }));
    assert_eq!(s.sessions.values().copied().collect::<Vec<_>>(), [4]);
    let outer = s.groups.iter().find(|g| g.root.pid == 10).unwrap();
    let inner = s.groups.iter().find(|g| g.root.pid == 30).unwrap();
    assert!(
        outer.members.iter().any(|k| k.pid == 20),
        "fallow stays in the outer group"
    );
    assert!(!outer.members.iter().any(|k| k.pid == 30 || k.pid == 31));
    assert_eq!(
        inner.members.iter().map(|k| k.pid).collect::<Vec<_>>(),
        [31]
    );
    m.request(Request::Untrack { pid: 30 });
    let s = m.cycle(&[p(1, 1, "init"), p(30, 7, "bash")], 100, 50);
    assert!(
        llm_of(&s, 30).is_none(),
        "untracked: an ordinary process again"
    );
}
