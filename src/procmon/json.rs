//! The hidden test interface's JSON (contracts/cli.md, T048). A small hand-written writer: the
//! product links no JSON crate (serde_json is dev-only).

use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use super::snapshot::{
    Confidence, CwdState, Footprint, Kind, LlmProcess, Note, NoteKind, ProcessGroup, ProcessKey,
    ProcessRecord, SignalKind, Snapshot, Status, SystemSummary, Via,
};

/// A JSON string literal.
pub fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn opt_string(out: &mut String, s: Option<&str>) {
    match s {
        Some(s) => string(out, s),
        None => out.push_str("null"),
    }
}

fn number(out: &mut String, v: f64) {
    if v.is_finite() {
        let _ = write!(out, "{v}");
    } else {
        out.push_str("null");
    }
}

pub fn key(out: &mut String, k: ProcessKey) {
    let _ = write!(out, "{{\"pid\":{},\"start_time\":{}}}", k.pid, k.start_time);
}

fn list<T>(
    out: &mut String,
    items: impl IntoIterator<Item = T>,
    mut each: impl FnMut(&mut String, T),
) {
    out.push('[');
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(out, item);
    }
    out.push(']');
}

pub fn unix_seconds(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::AgentCli => "agent_cli",
        Kind::Script => "script",
        Kind::Editor => "editor",
        Kind::LocalModelServer => "local_model_server",
        Kind::Unknown => "unknown",
        Kind::Browser => "browser",
        Kind::Session => "session",
    }
}

pub fn status_name(s: Status) -> &'static str {
    match s {
        Status::Serving => "serving",
        Status::LlmConnected => "llm_connected",
        Status::Busy => "busy",
        Status::Idle => "idle",
    }
}

pub fn confidence_name(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Likely => "likely",
    }
}

fn via_name(v: Via) -> &'static str {
    match v {
        Via::Resolved => "resolved",
        Via::Range => "range",
    }
}

fn footprint(out: &mut String, f: Option<Footprint>) {
    match f {
        Some(f) => {
            let _ = write!(out, "{{\"bytes\":{},\"metric\":", f.bytes);
            string(out, f.metric.label());
            out.push('}');
        }
        None => out.push_str("null"),
    }
}

fn group(out: &mut String, g: &ProcessGroup) {
    out.push_str("{\"root\":");
    key(out, g.root);
    out.push_str(",\"members\":");
    list(out, &g.members, |o, k| key(o, *k));
    out.push_str(",\"nested\":");
    list(out, &g.nested, |o, k| key(o, *k));
    let _ = write!(
        out,
        ",\"total_footprint\":{},\"total_cpu\":",
        g.total_footprint
    );
    number(out, g.total_cpu as f64);
    out.push('}');
}

fn llm(out: &mut String, l: &LlmProcess) {
    out.push_str("{\"key\":");
    key(out, l.key);
    out.push_str(",\"kind\":");
    string(out, kind_name(l.kind));
    out.push_str(",\"signals\":");
    list(out, &l.signals, |o, s| {
        match &s.kind {
            SignalKind::Signature { id } => {
                o.push_str("{\"kind\":\"signature\",\"id\":");
                string(o, id);
            }
            SignalKind::Session { id } => {
                o.push_str("{\"kind\":\"session\",\"id\":");
                number(o, *id as f64);
            }
            SignalKind::ProviderConnection { provider_id, via } => {
                o.push_str("{\"kind\":\"provider_connection\",\"provider_id\":");
                string(o, provider_id);
                o.push_str(",\"via\":");
                string(o, via_name(*via));
            }
            SignalKind::LocalModelConnection { server } => {
                o.push_str("{\"kind\":\"local_model_connection\",\"server\":");
                key(o, *server);
            }
        }
        o.push_str(",\"confidence\":");
        string(o, confidence_name(s.confidence));
        o.push('}');
    });
    out.push_str(",\"confidence\":");
    string(out, confidence_name(l.confidence));
    out.push_str(",\"provider\":");
    opt_string(out, l.provider.as_deref());
    out.push_str(",\"model\":");
    opt_string(out, l.model.as_deref());
    out.push_str(",\"status\":");
    string(out, status_name(l.status));
    out.push('}');
}

fn record(out: &mut String, r: &ProcessRecord) {
    out.push_str("{\"key\":");
    key(out, r.key);
    out.push_str(",\"parent\":");
    match r.parent {
        Some(p) => key(out, p),
        None => out.push_str("null"),
    }
    out.push_str(",\"name\":");
    string(out, &r.name);
    out.push_str(",\"cmdline\":");
    match &r.cmdline {
        Some(args) => list(out, args.iter(), |o, a| string(o, a)),
        None => out.push_str("null"),
    }
    out.push_str(",\"owner\":");
    opt_string(out, r.owner.as_ref().map(|u| u.0.as_str()));
    out.push_str(",\"cpu_samples\":");
    list(out, r.cpu_samples, |o, v| number(o, v as f64));
    out.push_str(",\"footprint\":");
    footprint(out, r.footprint);
    out.push_str(",\"cwd\":");
    match &r.cwd {
        CwdState::Unknown => out.push_str("{\"state\":\"unknown\"}"),
        CwdState::Pending => out.push_str("{\"state\":\"pending\"}"),
        CwdState::Known(p) => {
            out.push_str("{\"state\":\"known\",\"path\":");
            string(out, &p.to_string_lossy());
            out.push('}');
        }
        CwdState::Unavailable(why) => {
            out.push_str("{\"state\":\"unavailable\",\"reason\":");
            string(out, why);
            out.push('}');
        }
    }
    out.push('}');
}

fn summary(out: &mut String, s: &SystemSummary) {
    let _ = write!(
        out,
        "{{\"total_ram\":{},\"available_ram\":{},\"llm_ram\":{},\"local_model_ram\":{},\"fallow_ram\":",
        s.total_ram, s.available_ram, s.llm_ram, s.local_model_ram
    );
    footprint(out, Some(s.fallow_ram));
    let _ = write!(out, ",\"low_memory\":{}}}", s.low_memory);
}

fn note(out: &mut String, n: &Note) {
    out.push_str("{\"kind\":");
    string(
        out,
        match n.kind {
            NoteKind::Coverage => "coverage",
            NoteKind::Degraded => "degraded",
        },
    );
    out.push_str(",\"text\":");
    string(out, &n.text);
    out.push('}');
}

/// The whole snapshot (data-model.md shape; `records` are LLM group members only).
pub fn snapshot(s: &Snapshot) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str("{\"taken_at\":");
    number(&mut out, unix_seconds(s.taken_at));
    out.push_str(",\"groups\":");
    list(&mut out, &s.groups, group);
    let mut llms: Vec<&LlmProcess> = s.llm.values().collect();
    llms.sort_by_key(|l| l.key);
    out.push_str(",\"llm\":");
    list(&mut out, llms, llm);
    let mut records: Vec<&ProcessRecord> = s.records.values().collect();
    records.sort_by_key(|r| r.key);
    out.push_str(",\"records\":");
    list(&mut out, records, record);
    out.push_str(",\"summary\":");
    summary(&mut out, &s.summary);
    out.push_str(",\"notes\":");
    list(&mut out, &s.notes, note);
    out.push('}');
    out
}

/// One `--snapshot-stream` line: time, cycle number and the detected LLM processes' keys.
pub fn stream_line(s: &Snapshot, cycle: u64) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("{\"t\":");
    number(&mut out, unix_seconds(s.taken_at));
    let _ = write!(out, ",\"cycle\":{cycle},\"llm\":");
    let mut keys: Vec<ProcessKey> = s.llm.keys().copied().collect();
    keys.sort();
    list(&mut out, keys, key);
    out.push('}');
    out
}
