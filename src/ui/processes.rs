//! The process panel (FR-010, FR-011, FR-013, FR-018, FR-019, FR-051).
//!
//! Groups are listed largest first, collapsed by default. The columns that fit are shown in
//! priority order; the selected row's full details (every FR-011 field) are always shown
//! under the list, so narrow panes lose no information.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::time::UNIX_EPOCH;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row as TableRow, Table, TableState};

use super::layout::pane_block;
use crate::app::state::{AppState, Pane};
use crate::procmon::snapshot::{
    Confidence, CwdState, FootprintMetric, Kind, NoteKind, ProcessGroup, ProcessKey, SignalKind,
    Snapshot, Status,
};

/// One visible line of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub key: ProcessKey,
    pub group_root: ProcessKey,
    /// 0 for a group's root, then the distance below it.
    pub depth: u8,
    pub expandable: bool,
}

/// The panel's sort: a column and whether its natural order is reversed (FR-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: Col,
    pub reversed: bool,
}

impl Default for Sort {
    /// Largest memory first, so the heaviest group is on top (US2).
    fn default() -> Self {
        Sort {
            column: Col::Mem,
            reversed: false,
        }
    }
}

impl Sort {
    /// Sorting by the same column again reverses it; a new column starts in its natural order.
    pub fn select(self, column: Col) -> Sort {
        if self.column == column {
            Sort {
                column,
                reversed: !self.reversed,
            }
        } else {
            Sort {
                column,
                reversed: false,
            }
        }
    }

    /// `▾` while the shown order is descending, `▴` while ascending.
    pub fn arrow(self) -> &'static str {
        if self.column.natural_descending() != self.reversed {
            "▾"
        } else {
            "▴"
        }
    }
}

impl Col {
    /// Memory, CPU and uptime list the largest first, status the most active first,
    /// confidence `certain` first; text columns are alphabetical.
    fn natural_descending(self) -> bool {
        matches!(
            self,
            Col::Mem | Col::Cpu | Col::Up | Col::Status | Col::Conf
        )
    }
}

fn status_rank(s: Status) -> u8 {
    match s {
        Status::Serving => 0,
        Status::LlmConnected => 1,
        Status::Busy => 2,
        Status::Idle => 3,
    }
}

/// The groups' order for a sort. Memory and CPU use group totals; ties keep a stable order by
/// (PID, start time) in either direction.
fn compare_groups(s: &Snapshot, sort: Sort, a: &ProcessGroup, b: &ProcessGroup) -> Ordering {
    let rec = |g: &ProcessGroup| s.records.get(&g.root);
    let llm = |g: &ProcessGroup| s.llm.get(&g.root);
    let text = |v: Option<String>| v.unwrap_or_default().to_lowercase();
    // Natural order, ascending meaning "comes first".
    let natural = match sort.column {
        Col::Mem => b.total_footprint.cmp(&a.total_footprint),
        Col::Cpu => b.total_cpu.total_cmp(&a.total_cpu),
        Col::Up => a.root.start_time.cmp(&b.root.start_time),
        Col::Status => {
            let r = |g| llm(g).map_or(u8::MAX, |l| status_rank(l.status));
            r(a).cmp(&r(b))
        }
        Col::Conf => {
            let r = |g| llm(g).map(|l| l.confidence);
            r(b).cmp(&r(a))
        }
        Col::Name => {
            text(rec(a).map(|r| r.name.to_string())).cmp(&text(rec(b).map(|r| r.name.to_string())))
        }
        Col::Pid => a.root.pid.cmp(&b.root.pid),
        Col::User => text(
            rec(a)
                .and_then(|r| r.owner.as_ref())
                .map(|u| u.0.to_string()),
        )
        .cmp(&text(
            rec(b)
                .and_then(|r| r.owner.as_ref())
                .map(|u| u.0.to_string()),
        )),
        Col::Type => text(llm(a).map(|l| kind(l.kind).to_string()))
            .cmp(&text(llm(b).map(|l| kind(l.kind).to_string()))),
        Col::ProviderModel => {
            let pm = |g| {
                llm(g)
                    .map(|l| provider_model(l.provider.as_deref(), l.model.as_deref()))
                    .filter(|v| !v.is_empty())
            };
            // Unknown provider/model last.
            match (pm(a), pm(b)) {
                (Some(x), Some(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }
        Col::Dir => {
            let d = |g| match rec(g).map(|r| &r.cwd) {
                Some(CwdState::Known(p)) => p.to_string_lossy().to_lowercase(),
                _ => String::from("\u{10ffff}"),
            };
            d(a).cmp(&d(b))
        }
    };
    let primary = if sort.reversed {
        natural.reverse()
    } else {
        natural
    };
    primary.then(a.root.cmp(&b.root))
}

/// The rows: each group in sort order and, when expanded, its members in tree order.
pub fn rows(s: &Snapshot, expanded: &HashSet<ProcessKey>, sort: Sort) -> Vec<Row> {
    let mut groups: Vec<_> = s.groups.iter().collect();
    groups.sort_by(|a, b| compare_groups(s, sort, a, b));
    let mut out = Vec::new();
    for g in groups {
        out.push(Row {
            key: g.root,
            group_root: g.root,
            depth: 0,
            expandable: !g.members.is_empty(),
        });
        if !expanded.contains(&g.root) {
            continue;
        }
        // Members are stored in depth-first order; depth from the parent chain.
        let mut depth: HashMap<ProcessKey, u8> = HashMap::from([(g.root, 0)]);
        for m in &g.members {
            let parent = s.records.get(m).and_then(|r| r.parent);
            let d = parent
                .and_then(|p| depth.get(&p))
                .map_or(1, |d| d.saturating_add(1));
            depth.insert(*m, d);
            out.push(Row {
                key: *m,
                group_root: g.root,
                depth: d,
                expandable: false,
            });
        }
    }
    out
}

/// Rows of the table that fit in `list` (border, header and the details area excluded).
fn table_rows(list: &Rect) -> usize {
    let inner = list.height.saturating_sub(2) as usize;
    let details = if inner >= 12 {
        6
    } else if inner >= 7 {
        3
    } else {
        0
    };
    // header and the coverage line
    inner.saturating_sub(details + 2).max(1)
}

/// First visible row so the selection stays on screen.
fn offset_for(selected: usize, visible: usize) -> usize {
    selected.saturating_sub(visible.saturating_sub(1))
}

/// The row index under screen row `y`, if any.
pub fn row_at(list: &Rect, selected: usize, y: u16) -> Option<usize> {
    let first = list.y + 2; // top border, header
    let visible = table_rows(list);
    if y < first || (y - first) as usize >= visible {
        return None;
    }
    Some(offset_for(selected, visible) + (y - first) as usize)
}

pub fn draw(f: &mut Frame<'_>, state: &AppState, area: Rect) {
    let mut title = vec![Span::raw(" LLM Processes ")];
    if state.all_users {
        title.push(Span::styled(
            " elevated: all users ",
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
    }
    let block = pane_block(state, Pane::Processes, Line::from(title));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let coverage = coverage_line(state);

    let Some(snap) = state.snapshot.as_deref() else {
        let lines = vec![Line::raw("scanning…"), Line::raw(""), coverage];
        f.render_widget(Paragraph::new(lines), inner);
        return;
    };
    let rows = rows(snap, &state.panel.expanded, state.panel.sort);
    if rows.is_empty() {
        let lines = vec![
            Line::raw("No LLM processes found."),
            Line::styled(
                "Agent CLIs, local model servers and processes connected to LLM providers appear here.",
                Style::default().add_modifier(Modifier::DIM),
            ),
            Line::raw(""),
            coverage,
        ];
        f.render_widget(
            Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
            inner,
        );
        return;
    }

    let detail_rows = if inner.height >= 16 {
        8
    } else if inner.height >= 7 {
        3
    } else {
        0
    };
    let [table_area, details, note] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(detail_rows),
        Constraint::Length(1),
    ])
    .areas(inner);

    let selected = state.panel.selected.min(rows.len() - 1);
    let mut ctx = Ctx::new(snap, state.all_users);
    ctx.sessions = snap
        .sessions
        .iter()
        .map(|(k, id)| {
            let project = state
                .sessions
                .get(*id)
                .map_or("", |s| s.project_name.as_str());
            (*k, format!(" #{id} {project}"))
        })
        .collect();
    let cols = choose_columns(table_area.width, state.all_users);
    let header = TableRow::new(
        cols.iter()
            .map(|c| Cell::from(c.title(&ctx, state.panel.sort))),
    )
    .style(Style::default().add_modifier(Modifier::BOLD));
    let body = rows.iter().map(|r| {
        let cells = cols
            .iter()
            .map(|c| Cell::from(ctx.cell(*c, r, state.panel.expanded.contains(&r.group_root))));
        let mut row = TableRow::new(cells);
        if r.depth > 0 && !snap.llm.contains_key(&r.key) {
            row = row.style(Style::default().add_modifier(Modifier::DIM));
        }
        row
    });
    let widths: Vec<Constraint> = cols.iter().map(|c| c.constraint()).collect();
    let table = Table::new(body, widths)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let visible = table_area.height.saturating_sub(1) as usize;
    let mut ts = TableState::default()
        .with_selected(Some(selected))
        .with_offset(offset_for(selected, visible.max(1)));
    f.render_stateful_widget(table, table_area, &mut ts);

    if detail_rows > 0 {
        f.render_widget(
            Paragraph::new(ctx.details(&rows[selected]))
                .wrap(ratatui::widgets::Wrap { trim: false }),
            details,
        );
    }
    f.render_widget(Paragraph::new(coverage_line(state)), note);
}

/// FR-018: always visible.
fn coverage_line(state: &AppState) -> Line<'static> {
    let text = state
        .snapshot
        .as_deref()
        .and_then(|s| {
            s.notes
                .iter()
                .find(|n| n.kind == NoteKind::Coverage)
                .map(|n| n.text.to_string())
        })
        .unwrap_or_else(|| {
            let mut t = String::from("Not covered: containers, remote machines");
            if cfg!(windows) {
                t.push_str(", WSL2");
            }
            t
        });
    Line::styled(text, Style::default().add_modifier(Modifier::DIM))
}

/// A panel column; every column can be the sort column (FR-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Col {
    Name,
    Mem,
    Cpu,
    Status,
    Conf,
    Up,
    Pid,
    User,
    Type,
    ProviderModel,
    Dir,
}

impl Col {
    fn width(self) -> u16 {
        match self {
            Col::Name => 8,
            Col::Mem => 8,
            Col::Cpu => 5,
            Col::Status => 13,
            Col::Conf => 7,
            Col::Up => 6,
            Col::Pid => 7,
            Col::User => 10,
            Col::Type => 12,
            Col::ProviderModel => 14,
            Col::Dir => 18,
        }
    }

    fn constraint(self) -> Constraint {
        match self {
            Col::Name => Constraint::Min(self.width()),
            _ => Constraint::Length(self.width()),
        }
    }

    fn title(self, ctx: &Ctx<'_>, sort: Sort) -> String {
        let arrow = if sort.column == self {
            sort.arrow()
        } else {
            ""
        };
        match self {
            Col::Name => format!("Name{arrow}"),
            Col::Mem => format!("Mem{arrow}{}", ctx.metric_suffix()),
            Col::Cpu => format!("CPU%{arrow}"),
            Col::Status => format!("Status{arrow}"),
            Col::Conf => format!("Conf{arrow}"),
            Col::Up => format!("Up{arrow}"),
            Col::Pid => format!("PID{arrow}"),
            Col::User => format!("User{arrow}"),
            Col::Type => format!("Type{arrow}"),
            Col::ProviderModel => format!("Provider/model{arrow}"),
            Col::Dir => format!("Directory{arrow}"),
        }
    }
}

/// The column whose header is at screen column `x` in the process list `list` (FR-063 header
/// click), or `None` if `y` is not the header row.
pub fn header_at(list: &Rect, all_users: bool, x: u16, y: u16) -> Option<Col> {
    if y != list.y + 1 || x <= list.x || x + 1 >= list.right() {
        return None;
    }
    let width = list.width.saturating_sub(2);
    let cols = choose_columns(width, all_users);
    let fixed: u16 = cols
        .iter()
        .filter(|c| **c != Col::Name)
        .map(|c| c.width())
        .sum::<u16>()
        + cols.len().saturating_sub(1) as u16;
    let mut left = list.x + 1;
    for c in cols {
        let w = if c == Col::Name {
            width.saturating_sub(fixed)
        } else {
            c.width()
        };
        if x >= left && x < left + w {
            return Some(c);
        }
        left += w + 1;
    }
    None
}

/// The columns that fit, in priority order (Name always; the rest while there is room).
fn choose_columns(width: u16, all_users: bool) -> Vec<Col> {
    let mut order = vec![
        Col::Mem,
        Col::Cpu,
        Col::Status,
        Col::Conf,
        Col::Up,
        Col::Pid,
    ];
    if all_users {
        order.push(Col::User);
    }
    order.extend([Col::Type, Col::ProviderModel, Col::Dir]);
    let mut used = Col::Name.width();
    let mut chosen = vec![Col::Name];
    for c in order {
        if used + 1 + c.width() <= width {
            used += 1 + c.width();
            chosen.push(c);
        }
    }
    // Keep the table's column order stable regardless of priority.
    let position = |c: &Col| {
        [
            Col::Name,
            Col::Type,
            Col::Pid,
            Col::User,
            Col::Dir,
            Col::Mem,
            Col::Cpu,
            Col::Up,
            Col::Status,
            Col::ProviderModel,
            Col::Conf,
        ]
        .iter()
        .position(|x| x == c)
    };
    chosen.sort_by_key(position);
    chosen
}

struct Ctx<'a> {
    s: &'a Snapshot,
    now: u64,
    metric: FootprintMetric,
    home: Option<String>,
    totals: HashMap<ProcessKey, (u64, f32, bool)>,
    all_users: bool,
    /// Agents started as Fallow sessions: `#n project` (002 FR-013).
    sessions: HashMap<ProcessKey, String>,
}

impl<'a> Ctx<'a> {
    fn new(s: &'a Snapshot, all_users: bool) -> Self {
        let totals = s
            .groups
            .iter()
            .map(|g| {
                (
                    g.root,
                    (g.total_footprint, g.total_cpu, !g.members.is_empty()),
                )
            })
            .collect();
        Ctx {
            s,
            now: s
                .taken_at
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            metric: s.summary.fallow_ram.metric,
            home: home_dir(),
            totals,
            all_users,
            sessions: HashMap::new(),
        }
    }

    fn metric_suffix(&self) -> String {
        format!(" {}", self.metric.label())
    }

    fn cell(&self, c: Col, r: &Row, expanded: bool) -> String {
        let rec = self.s.records.get(&r.key);
        let llm = self.s.llm.get(&r.key);
        let group = if r.depth == 0 {
            self.totals.get(&r.key).copied()
        } else {
            None
        };
        match c {
            Col::Name => {
                let name = rec.map_or("?", |x| x.name.as_str());
                // `.exe` carries no information on Windows and costs a narrow column 4 cells.
                let name = name
                    .strip_suffix(".exe")
                    .or_else(|| name.strip_suffix(".EXE"))
                    .unwrap_or(name);
                let label = self.sessions.get(&r.key).map_or("", String::as_str);
                let owned = format!("{name}{label}");
                let name = owned.as_str();
                if r.depth == 0 {
                    let mark = match (r.expandable, expanded) {
                        (false, _) => "  ",
                        (true, true) => "▾ ",
                        (true, false) => "▸ ",
                    };
                    format!("{mark}{name}")
                } else {
                    let indent = "  ".repeat(r.depth.min(6) as usize);
                    let nested = if llm.is_some() { "◆ " } else { "└ " };
                    format!("{indent}{nested}{name}")
                }
            }
            Col::Mem => {
                let fp = rec.and_then(|x| x.footprint);
                match (group, fp) {
                    (Some((total, _, true)), _) => format!("Σ{}", bytes(total)),
                    (_, Some(f)) if f.metric != self.metric => {
                        format!("{} {}", bytes(f.bytes), f.metric.label())
                    }
                    (_, Some(f)) => bytes(f.bytes),
                    (_, None) => "–".into(),
                }
            }
            Col::Cpu => {
                let cpu = match group {
                    Some((_, cpu, true)) => cpu,
                    _ => rec.map_or(0.0, |x| x.mean_cpu()),
                };
                format!("{cpu:.1}")
            }
            Col::Status => llm.map_or(String::new(), |l| status(l.status).into()),
            Col::Conf => llm.map_or(String::new(), |l| confidence(l.confidence).into()),
            Col::Up => uptime(self.now.saturating_sub(r.key.start_time)),
            Col::Pid => r.key.pid.to_string(),
            Col::User => rec
                .and_then(|x| x.owner.as_ref())
                .map_or(String::new(), |u| u.0.to_string()),
            Col::Type => llm.map_or(String::new(), |l| kind(l.kind).into()),
            Col::ProviderModel => llm.map_or(String::new(), |l| {
                provider_model(l.provider.as_deref(), l.model.as_deref())
            }),
            Col::Dir => rec.map_or(String::new(), |x| self.cwd(&x.cwd)),
        }
    }

    fn cwd(&self, c: &CwdState) -> String {
        match c {
            CwdState::Known(p) => {
                let p = p.to_string_lossy();
                match &self.home {
                    Some(h) if p.starts_with(h.as_str()) => format!("~{}", &p[h.len()..]),
                    _ => p.into_owned(),
                }
            }
            CwdState::Unknown => "?".into(),
            CwdState::Pending => "…".into(),
            CwdState::Unavailable(why) => format!("unavailable ({why})"),
        }
    }

    /// Every FR-011 field of the selected row, plus its signals (FR-025).
    fn details(&self, r: &Row) -> Vec<Line<'static>> {
        let dim = Style::default().add_modifier(Modifier::DIM);
        let Some(rec) = self.s.records.get(&r.key) else {
            return Vec::new();
        };
        let mut first = vec![Span::styled(
            rec.name.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )];
        let mut push = |s: String| {
            first.push(Span::styled(" · ", dim));
            first.push(Span::raw(s));
        };
        let llm = self.s.llm.get(&r.key);
        if let Some(l) = llm {
            push(kind(l.kind).into());
        }
        push(format!("PID {}", r.key.pid));
        push(format!(
            "up {}",
            uptime(self.now.saturating_sub(r.key.start_time))
        ));
        if let Some(u) = rec.owner.as_ref().filter(|_| self.all_users) {
            push(format!("user {}", u.0));
        }
        let mem = rec.footprint.map_or("–".into(), |f| {
            format!("{} {}", bytes(f.bytes), f.metric.label())
        });
        let mut out = vec![
            Line::from(first),
            Line::raw(format!("dir {}", self.cwd(&rec.cwd))),
        ];
        if let Some(l) = llm {
            let mut third = vec![
                Span::styled(
                    confidence(l.confidence),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", dim),
                Span::raw(status(l.status)),
            ];
            let pm = provider_model(l.provider.as_deref(), l.model.as_deref());
            if !pm.is_empty() {
                third.push(Span::styled(" · ", dim));
                third.push(Span::raw(pm));
            }
            out.push(Line::from(third));
        }
        out.push(Line::raw(format!("mem {mem} · cpu {:.1}%", rec.mean_cpu())));
        if let Some(l) = llm {
            let signals: Vec<String> = l
                .signals
                .iter()
                .map(|s| match &s.kind {
                    SignalKind::Signature { id } => format!("signature {id}"),
                    SignalKind::Session { id } => format!("started in Fallow as session #{id}"),
                    SignalKind::ProviderConnection { provider_id, via } => format!(
                        "{provider_id} connection via {}",
                        if *via == crate::procmon::snapshot::Via::Resolved {
                            "resolved"
                        } else {
                            "range"
                        }
                    ),
                    SignalKind::LocalModelConnection { server } => {
                        format!("local model server PID {}", server.pid)
                    }
                })
                .collect();
            out.push(Line::styled(
                format!("signals: {}", signals.join(", ")),
                dim,
            ));
        }
        out
    }
}

fn home_dir() -> Option<String> {
    let v = if cfg!(windows) {
        std::env::var("USERPROFILE")
    } else {
        std::env::var("HOME")
    };
    v.ok().filter(|h| h.len() > 1)
}

pub fn status(s: Status) -> &'static str {
    match s {
        Status::Serving => "serving",
        Status::LlmConnected => "LLM connected",
        Status::Busy => "busy",
        Status::Idle => "idle",
    }
}

pub fn confidence(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Likely => "likely",
    }
}

pub fn kind(k: Kind) -> &'static str {
    match k {
        Kind::AgentCli => "agent CLI",
        Kind::Script => "script",
        Kind::Editor => "editor",
        Kind::LocalModelServer => "local model server",
        Kind::Unknown => "unknown",
        Kind::Browser => "browser",
        Kind::Session => "session",
    }
}

fn provider_model(provider: Option<&str>, model: Option<&str>) -> String {
    match (provider, model) {
        (Some(p), Some(m)) => format!("{p}/{m}"),
        (Some(p), None) => p.to_string(),
        (None, Some(m)) => m.to_string(),
        (None, None) => String::new(),
    }
}

/// Compact binary units: 512K, 87M, 1.2G.
pub fn bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let b = b as f64;
    if b == 0.0 {
        "0".into()
    } else if b < K * K {
        format!("{:.0}K", b / K)
    } else if b < K * K * K {
        format!("{:.0}M", b / (K * K))
    } else {
        format!("{:.1}G", b / (K * K * K))
    }
}

pub fn uptime(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
        s => format!("{}d{:02}h", s / 86_400, (s % 86_400) / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(bytes(512 * 1024), "512K");
        assert_eq!(bytes(87 * 1024 * 1024), "87M");
        assert_eq!(bytes(1288490188), "1.2G");
        assert_eq!(uptime(59), "59s");
        assert_eq!(uptime(3599), "59m");
        assert_eq!(uptime(3 * 3600 + 5 * 60), "3h05m");
        assert_eq!(uptime(2 * 86_400 + 4 * 3600), "2d04h");
    }

    #[test]
    fn narrow_panes_keep_the_most_important_columns() {
        assert_eq!(
            choose_columns(40, false),
            [Col::Name, Col::Mem, Col::Cpu, Col::Status]
        );
        let wide = choose_columns(200, true);
        assert_eq!(wide.len(), 11);
        assert_eq!(wide[0], Col::Name);
    }
}
