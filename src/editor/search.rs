//! Project search (FR-005, FR-042, R-8, T076), with ripgrep's library crates.
//!
//! A search runs only when the user submits it, on a thread spawned for that search. It
//! respects `.gitignore`, streams results to the UI as each file is searched, can be cancelled,
//! and holds at most [`RESULT_CAP`] results, after which it stops with state `Capped`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, SearcherBuilder};

/// "Holds at most 10,000 results in memory" (R-8).
pub const RESULT_CAP: usize = 10_000;
/// A result line is kept up to this many chars.
const LINE_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchState {
    Running,
    Done,
    Cancelled,
    Capped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: Arc<Path>,
    /// 1-based.
    pub line: u64,
    /// Byte offset of the match in the line.
    pub col: usize,
    pub text: String,
}

enum Msg {
    Hits(Vec<SearchHit>),
    Finished(SearchState, Option<String>),
}

/// A submitted search (data-model.md "SearchSession").
#[derive(Debug)]
pub struct SearchSession {
    pub query: String,
    pub root: PathBuf,
    pub results: Vec<SearchHit>,
    pub state: SearchState,
    pub error: Option<String>,
    pub started: Instant,
    pub finished: Option<Duration>,
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::Hits(h) => write!(f, "Hits({})", h.len()),
            Msg::Finished(s, _) => write!(f, "Finished({s:?})"),
        }
    }
}

impl SearchSession {
    /// Starts searching `root` for `query` (a literal, case-insensitive unless it has an
    /// uppercase letter).
    pub fn start(root: &Path, query: &str) -> SearchSession {
        let (tx, rx) = sync_channel(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let s = SearchSession {
            query: query.to_string(),
            root: root.to_path_buf(),
            results: Vec::new(),
            state: SearchState::Running,
            error: None,
            started: Instant::now(),
            finished: None,
            rx,
            cancel: cancel.clone(),
        };
        let (root, query) = (root.to_path_buf(), query.to_string());
        let spawned = std::thread::Builder::new()
            .name("fallow-search".into())
            .spawn(move || run(&root, &query, &tx, &cancel));
        if let Err(e) = spawned {
            let mut s = s;
            s.state = SearchState::Failed;
            s.error = Some(e.to_string());
            return s;
        }
        s
    }

    pub fn running(&self) -> bool {
        self.state == SearchState::Running
    }

    /// Takes what the search thread has sent. Returns true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Hits(h)) => {
                    let room = RESULT_CAP - self.results.len();
                    self.results.extend(h.into_iter().take(room));
                    changed = true;
                }
                Ok(Msg::Finished(st, err)) => {
                    if self.state == SearchState::Running {
                        self.state = st;
                        self.error = err;
                        self.finished = Some(self.started.elapsed());
                    }
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.state == SearchState::Running {
                        self.state = SearchState::Done;
                        self.finished = Some(self.started.elapsed());
                        changed = true;
                    }
                    break;
                }
            }
        }
        changed
    }

    /// Stops the search. Results so far are kept.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if self.state == SearchState::Running {
            self.state = SearchState::Cancelled;
            self.finished = Some(self.started.elapsed());
        }
    }

    /// Polls until the search finishes or `timeout` passes (tests and benchmarks).
    pub fn wait(&mut self, timeout: Duration) -> SearchState {
        let until = Instant::now() + timeout;
        while self.running() && Instant::now() < until {
            if !self.poll() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        self.poll();
        self.state
    }

    /// `path` relative to the searched root, for display.
    pub fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.root).unwrap_or(path)
    }
}

impl Drop for SearchSession {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn run(root: &Path, query: &str, tx: &SyncSender<Msg>, cancel: &AtomicBool) {
    let matcher = match RegexMatcherBuilder::new()
        .case_smart(true)
        .fixed_strings(true)
        .build(query)
    {
        Ok(m) => m,
        Err(e) => {
            let _ = tx.send(Msg::Finished(SearchState::Failed, Some(e.to_string())));
            return;
        }
    };
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut sent = 0usize;
    let walk = ignore::WalkBuilder::new(root).require_git(false).build();
    for entry in walk {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(Msg::Finished(SearchState::Cancelled, None));
            return;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path: Arc<Path> = Arc::from(entry.path());
        let mut hits = Vec::new();
        let room = RESULT_CAP - sent;
        let _ = searcher.search_path(
            &matcher,
            entry.path(),
            Lossy(|line, text| {
                if cancel.load(Ordering::Relaxed) || hits.len() >= room {
                    return Ok(false);
                }
                let col = matcher
                    .find(text.as_bytes())
                    .ok()
                    .flatten()
                    .map_or(0, |m| m.start());
                let text = text.trim_end_matches(['\n', '\r']);
                hits.push(SearchHit {
                    path: path.clone(),
                    line,
                    col,
                    text: text.chars().take(LINE_CHARS).collect(),
                });
                Ok(true)
            }),
        );
        if hits.is_empty() {
            continue;
        }
        sent += hits.len();
        if tx.send(Msg::Hits(hits)).is_err() {
            return;
        }
        if sent >= RESULT_CAP {
            let _ = tx.send(Msg::Finished(SearchState::Capped, None));
            return;
        }
    }
    let _ = tx.send(Msg::Finished(SearchState::Done, None));
}
