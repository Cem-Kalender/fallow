//! Spike S-3 (research.md R-12). It approximates Fallow's idle state with the real crate set:
//! a ratatui/crossterm UI (alternate screen, raw mode, mouse capture, bracketed paste, kitty
//! keyboard flags on Unix), a procmon thread that refreshes sysinfo every 2 s with the R-1
//! refresh kind, Fallow-owned command-line, footprint and TCP readers, a DNS thread, and one
//! tree-sitter grammar that is linked but never used.
//!
//! Throwaway code. It is not the product architecture.

mod platform;

use std::collections::{HashMap, HashSet};
use std::io::stdout;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};
use regex::Regex;
use sysinfo::{MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const REFRESH: Duration = Duration::from_secs(2);
const PROVIDER_HOSTS: &[&str] = &["api.anthropic.com:443", "api.openai.com:443"];
const LOCAL_MODEL_PORTS: &[u16] = &[11434, 8080, 1234];

pub struct Conn {
    pub pid: u32,
    pub remote_port: u16,
    pub local_port: u16,
    pub established: bool,
    pub listening: bool,
}

struct LlmRow {
    pid: u32,
    name: String,
    footprint: Option<u64>,
    cpu: f32,
    status: &'static str,
}

struct Snapshot {
    total_ram: u64,
    available_ram: u64,
    processes: usize,
    cached_cmdlines: usize,
    connections: usize,
    llm: Vec<LlmRow>,
}

/// Evidence that the procmon loop ran, written to `--stats <path>` on exit (research R-12).
#[derive(Default)]
struct Stats {
    cycles: AtomicU64,
    procs_last: AtomicU64,
    procs_min: AtomicU64,
    procs_max: AtomicU64,
    mine_last: AtomicU64,
    mine_max: AtomicU64,
    llm_max: AtomicU64,
    conns_max: AtomicU64,
    cmdlines_last: AtomicU64,
}

impl Stats {
    fn record(&self, procs: u64, mine: u64, llm: u64, conns: u64, cmdlines: u64) {
        let first = self.cycles.fetch_add(1, Ordering::Relaxed) == 0;
        self.procs_last.store(procs, Ordering::Relaxed);
        if first {
            self.procs_min.store(procs, Ordering::Relaxed);
        } else {
            self.procs_min.fetch_min(procs, Ordering::Relaxed);
        }
        self.procs_max.fetch_max(procs, Ordering::Relaxed);
        self.mine_last.store(mine, Ordering::Relaxed);
        self.mine_max.fetch_max(mine, Ordering::Relaxed);
        self.llm_max.fetch_max(llm, Ordering::Relaxed);
        self.conns_max.fetch_max(conns, Ordering::Relaxed);
        self.cmdlines_last.store(cmdlines, Ordering::Relaxed);
    }

    fn render(&self) -> String {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        format!(
            "pid={} cycles={} processes_last={} processes_min={} processes_max={} own_user_last={} own_user_max={} llm_detected_max={} interesting_tcp_max={} cmdlines_cached_last={}
",
            std::process::id(),
            g(&self.cycles),
            g(&self.procs_last),
            g(&self.procs_min),
            g(&self.procs_max),
            g(&self.mine_last),
            g(&self.mine_max),
            g(&self.llm_max),
            g(&self.conns_max),
            g(&self.cmdlines_last),
        )
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let stats_path = args.iter().position(|a| a == "--stats").and_then(|i| args.get(i + 1)).cloned();
    let stats = Arc::new(Stats::default());
    // Keep tree-sitter and one grammar linked without touching their tables at idle.
    if args.iter().any(|a| a == "--parse-self-test") {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok();
        let _ = parser.parse("fn main() {}", None);
        return;
    }
    std::hint::black_box(&tree_sitter_rust::LANGUAGE);

    let (tx, rx) = mpsc::sync_channel::<Snapshot>(1);
    thread::Builder::new()
        .name("procmon".into())
        .stack_size(256 * 1024)
        .spawn({
            let stats = stats.clone();
            move || procmon(tx, stats)
        })
        .expect("spawn procmon");
    thread::Builder::new()
        .name("dns".into())
        .stack_size(128 * 1024)
        .spawn(dns)
        .expect("spawn dns");

    let mut terminal = ratatui::init();
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    #[cfg(unix)]
    {
        use ratatui::crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    run(&mut terminal, rx);

    #[cfg(unix)]
    {
        use ratatui::crossterm::event::PopKeyboardEnhancementFlags;
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();

    if let Some(path) = stats_path {
        let _ = std::fs::write(path, stats.render());
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, rx: Receiver<Snapshot>) {
    let mut snap: Option<Snapshot> = None;
    let mut dirty = true;
    loop {
        if dirty {
            let _ = terminal.draw(|f| draw(f, snap.as_ref()));
            dirty = false;
        }
        if event::poll(Duration::from_millis(250)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k))
                    if k.code == KeyCode::Char('q')
                        || (k.code == KeyCode::Char('q')
                            && k.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    return;
                }
                Ok(_) => dirty = true,
                Err(_) => return,
            }
        }
        match rx.try_recv() {
            Ok(s) => {
                snap = Some(s);
                dirty = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return,
        }
    }
}

fn draw(f: &mut ratatui::Frame, snap: Option<&Snapshot>) {
    let [main, hints] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    let [files, editor, procs] = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(45),
        Constraint::Percentage(35),
    ])
    .areas(main);
    f.render_widget(Block::default().borders(Borders::ALL).title("Files"), files);
    f.render_widget(Block::default().borders(Borders::ALL).title("Editor"), editor);

    let [list, summary] = Layout::vertical([Constraint::Min(3), Constraint::Length(6)]).areas(procs);
    match snap {
        None => f.render_widget(
            Paragraph::new("scanning…").block(Block::default().borders(Borders::ALL).title("LLM Processes")),
            list,
        ),
        Some(s) => {
            let rows = s.llm.iter().map(|r| {
                Row::new(vec![
                    r.pid.to_string(),
                    r.name.clone(),
                    r.footprint.map(|b| format!("{} MB", b / 1_048_576)).unwrap_or_else(|| "?".into()),
                    format!("{:.1}", r.cpu),
                    r.status.to_string(),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Length(7),
                    Constraint::Min(8),
                    Constraint::Length(8),
                    Constraint::Length(6),
                    Constraint::Length(13),
                ],
            )
            .header(Row::new(vec!["PID", "Name", "Mem", "CPU", "Status"]))
            .block(Block::default().borders(Borders::ALL).title("LLM Processes"));
            f.render_widget(table, list);
            let text = format!(
                "total {} MB  available {} MB\nprocesses {}  cmdlines cached {}\ninteresting tcp {}",
                s.total_ram / 1_048_576,
                s.available_ram / 1_048_576,
                s.processes,
                s.cached_cmdlines,
                s.connections
            );
            f.render_widget(
                Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("System")),
                summary,
            );
        }
    }
    f.render_widget(Paragraph::new("? Help  Ctrl-P Commands  Tab Next pane  q Quit (spike)"), hints);
}

fn procmon(tx: SyncSender<Snapshot>, stats: Arc<Stats>) {
    let name_sig = Regex::new(r"(?i)^(claude|codex|aider|gemini|ollama|llama-server|lm-studio|cursor)(\.exe)?$")
        .expect("regex");
    let cmd_sig = Regex::new(r"(?i)(^|[\\/ ])(claude|codex|aider|ollama|llama-server)(\.exe|\.js)?( |$)")
        .expect("regex");

    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing()
        .with_cpu()
        .with_user(UpdateKind::OnlyIfNotSet);
    let mut plat = platform::Platform::new();
    let mut cmdlines: HashMap<(u32, u64), Box<[Box<str>]>> = HashMap::new();
    let mut cpu_hist: HashMap<u32, [f32; 5]> = HashMap::new();
    let mut tick: usize = 0;

    loop {
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());

        let me = sysinfo::get_current_pid().ok();
        let my_uid = me.and_then(|p| sys.process(p)).and_then(|p| p.user_id().cloned());

        let mut mine: Vec<u32> = Vec::new();
        for (pid, p) in sys.processes() {
            if my_uid.is_some() && p.user_id() != my_uid.as_ref() {
                continue;
            }
            mine.push(pid.as_u32());
            let key = (pid.as_u32(), p.start_time());
            if !cmdlines.contains_key(&key) {
                let cmd = plat.cmdline(pid.as_u32()).unwrap_or_default();
                cmdlines.insert(key, cmd);
            }
        }
        let alive: HashSet<(u32, u64)> = sys
            .processes()
            .iter()
            .map(|(pid, p)| (pid.as_u32(), p.start_time()))
            .collect();
        cmdlines.retain(|k, _| alive.contains(k));
        cpu_hist.retain(|pid, _| sys.process(Pid::from_u32(*pid)).is_some());

        let conns = plat.tcp(&mine);
        let mut provider_conn: HashSet<u32> = HashSet::new();
        let mut serving: HashSet<u32> = HashSet::new();
        for c in &conns {
            if c.established && c.remote_port == 443 {
                provider_conn.insert(c.pid);
            }
            if c.established && LOCAL_MODEL_PORTS.contains(&c.local_port) {
                serving.insert(c.pid);
            }
        }

        let mut llm = Vec::new();
        for pid in &mine {
            let Some(p) = sys.process(Pid::from_u32(*pid)) else { continue };
            let name = p.name().to_string_lossy();
            let cmd = cmdlines.get(&(*pid, p.start_time()));
            let joined = cmd.map(|c| c.join(" ")).unwrap_or_default();
            if !(name_sig.is_match(&name) || cmd_sig.is_match(&joined)) {
                continue;
            }
            let hist = cpu_hist.entry(*pid).or_insert([0.0; 5]);
            hist[tick % 5] = p.cpu_usage();
            let cpu = hist.iter().sum::<f32>() / 5.0;
            let status = if serving.contains(pid) {
                "serving"
            } else if provider_conn.contains(pid) {
                "LLM connected"
            } else if cpu > 2.0 {
                "busy"
            } else {
                "idle"
            };
            llm.push(LlmRow {
                pid: *pid,
                name: name.into_owned(),
                footprint: plat.footprint(*pid),
                cpu,
                status,
            });
        }

        stats.record(
            sys.processes().len() as u64,
            mine.len() as u64,
            llm.len() as u64,
            conns.len() as u64,
            cmdlines.len() as u64,
        );
        let snap = Snapshot {
            total_ram: sys.total_memory(),
            available_ram: sys.available_memory(),
            processes: sys.processes().len(),
            cached_cmdlines: cmdlines.len(),
            connections: conns.len(),
            llm,
        };
        if tx.send(snap).is_err() {
            return;
        }
        tick = tick.wrapping_add(1);
        thread::sleep(REFRESH);
    }
}

fn dns() {
    loop {
        for host in PROVIDER_HOSTS {
            let _ = std::hint::black_box(host.to_socket_addrs().map(|a| a.count()));
        }
        thread::sleep(Duration::from_secs(15 * 60));
    }
}
