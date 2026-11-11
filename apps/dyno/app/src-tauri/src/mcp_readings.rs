//! Dyno's read-only reading tools for the family MCP server (ADR 0007; ADR 0032 D7 as amended for 1.1.0).
//!
//! Eight tools let an AI client ask what the Dashboard, Disks, Network, Alarms, History and Sensors pages show: `system_summary`,
//! `cpu_memory`, `disks`, `top_processes`, `network_summary`, `alarms`, `history` and `sensors`. They compute nothing the engine
//! does not: each one asks the agent through [`AgentCalls`] (the same requests the pages send, so the agent's validation and
//! logs apply) or reads the newest live update the window and the tray draw, and only shapes the values for a model: units in
//! the names, long lists cut to a bound, a reason wherever a reading is not available.
//!
//! What the client never gets (rule 56, the output goes to an AI client): command lines, user names, executable paths, the host
//! name, network addresses, port numbers, alarm details and alarm keys (they carry paths). Free text that is the user's own (a volume
//! label, a custom alarm rule's name) is passed through, with the user-name segment of any home or media path replaced. Process rows hold a name, the main process id and
//! numbers. Every list has a row bound and every answer a byte bound. The server logs each call (tool, client, duration, outcome,
//! size; never the output) and keeps it in the audit log the settings page shows.

use crate::mcp::AgentCalls;
use dyno_core::alarms::{Alarm, AlarmChange, AlarmFilter, AlarmState, ChangeKind};
use dyno_core::grouping::AppGroup;
use dyno_core::hardware::Confidence;
use dyno_core::health::Severity;
use dyno_core::history::{HistoryPoint, HistoryRange};
use dyno_core::inventory::SystemInventory;
use dyno_core::lifespan::{LifeForecast, LifeOutcome, LifeSubject, LifespanOverview};
use dyno_core::metrics::{MemoryPressure, PowerSource, Reading};
use dyno_core::network::{Exposure, NetworkQuery, NetworkSnapshot, SocketState};
use dyno_core::process::{Measured, UnavailableReason};
use dyno_core::protocol::{History, LiveUpdate, Reply, Request};
use dyno_core::read_via::{ReadVia, NEEDS_ADMIN};
use paddock_mcp::{CallCtx, ToolDef, ToolError, ToolOutput};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

/// The most rows of any list in an answer (processes, alarms, changes).
pub const MAX_ROWS: usize = 50;
/// The most points of one history answer.
pub const MAX_POINTS: usize = 120;
/// The points of a history answer when the client does not say.
const DEFAULT_POINTS: usize = 60;
/// The most volumes, interfaces and sensors of one answer.
const MAX_ITEMS: usize = 64;
/// The most cores of one answer.
const MAX_CORES: usize = 256;
/// The longest name or title the client sees, in characters.
const MAX_TEXT: usize = 120;
/// The most bytes of text any reading tool returns (the rows above keep every answer far below it).
const MAX_BYTES: usize = 32 * 1024;
/// The time limit of one call; an answer comes from memory or the agent's own store.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The reading tools over `calls`.
pub fn tools(calls: Arc<dyn AgentCalls>) -> Vec<ToolDef> {
    let none = || json!({"type": "object", "additionalProperties": false});
    let mut v = Vec::new();
    let c = calls.clone();
    v.push(read(
        "system_summary",
        "System summary",
        "A short summary of this computer right now: operating system, uptime in seconds, CPU load in percent, memory use in percent and bytes, and swap in bytes. \
         Start here for a general question about how the computer is doing. A value that cannot be read says why instead of being left out.",
        none(),
        move |_| system_summary(&c),
    ));
    let c = calls.clone();
    v.push(read(
        "cpu_memory",
        "CPU and memory",
        "Current processor and memory readings: CPU load in percent (total, and per core when per_core is true), frequency in MHz, memory total, used and available in bytes, \
         used percent, swap in bytes, commit where the system reports it, and the memory pressure (normal, elevated or critical).",
        json!({"type": "object", "properties": {"per_core": {"type": "boolean", "description": "Also give the load of each logical core, in percent. Default false."}}, "additionalProperties": false}),
        move |a| cpu_memory(&c, flag(a, "per_core")),
    ));
    let c = calls.clone();
    v.push(read(
        "disks",
        "Disks",
        "The volumes with total, free and used bytes and used percent, the current disk read and write speed in bytes per second, and the health of each solid-state drive \
         (a forecast with its confidence) where the computer reports it. Health that needs administrator readings says so.",
        none(),
        move |_| disks(&c),
    ));
    let c = calls.clone();
    v.push(read(
        "top_processes",
        "Top processes",
        "The busiest programs right now, ranked by CPU or by memory. Each row is a program (all its processes added together) with its name, main process id, process count, \
         CPU in percent of the whole computer and resident memory in bytes. Never command lines, user names or paths. limit is 1 to 50 (default 10); \
         while Dyno's task manager is closed Dyno keeps only a short list, and the answer says how many it could rank.",
        json!({"type": "object", "properties": {
            "sort": {"type": "string", "enum": ["cpu", "memory"], "description": "Rank by cpu (default) or memory."},
            "limit": {"type": "integer", "minimum": 1, "maximum": MAX_ROWS, "description": "How many programs to return (default 10, at most 50)."}
        }, "additionalProperties": false}),
        move |a| top_processes(&c, a),
    ));
    let c = calls.clone();
    v.push(read(
        "network_summary",
        "Network summary",
        "Network traffic now: bytes per second received and sent in total and per interface, and counts of listening ports (and how many listen on every network interface), \
         open connections and established connections. No addresses, ports or program names are given.",
        none(),
        move |_| network_summary(&c),
    ));
    let c = calls.clone();
    v.push(read(
        "alarms",
        "Alarms",
        "Dyno's alarms: the active ones that still need attention (rule, severity, state, title, when first and last seen as epoch milliseconds, how often seen, latest value) \
         and the most recent changes (raised, acknowledged, returned to normal). active_limit is 1 to 50 (default 20), history_limit is 0 to 50 (default 10). \
         No alarms is a normal answer.",
        json!({"type": "object", "properties": {
            "active_limit": {"type": "integer", "minimum": 1, "maximum": MAX_ROWS, "description": "How many active alarms to list (default 20)."},
            "history_limit": {"type": "integer", "minimum": 0, "maximum": MAX_ROWS, "description": "How many recent changes to list (default 10; 0 for none)."}
        }, "additionalProperties": false}),
        move |a| alarms(&c, a),
    ));
    let c = calls.clone();
    v.push(read(
        "history",
        "History of one metric",
        "One metric over the last hour, day, week or month, as at most 120 points of {atMs (epoch milliseconds), value, max}. Metrics: cpu, memory (percent), \
         disk_read, disk_write, network_receive, network_send (bytes per second), gpu (percent) and temperature (degrees Celsius). \
         Dyno keeps history only while its agent runs; a metric nobody measured says so.",
        json!({"type": "object", "required": ["metric"], "properties": {
            "metric": {"type": "string", "enum": METRICS.iter().map(|m| m.name).collect::<Vec<_>>(), "description": "Which reading."},
            "range": {"type": "string", "enum": ["hour", "day", "week", "month"], "description": "How far back (default hour)."},
            "points": {"type": "integer", "minimum": 2, "maximum": MAX_POINTS, "description": "At most this many points (default 60, at most 120)."}
        }, "additionalProperties": false}),
        move |a| history(&c, a),
    ));
    let c = calls;
    v.push(read(
        "sensors",
        "Sensors",
        "Temperatures in degrees Celsius, fan speeds in rpm, the battery (charge and wear in percent, whether it is plugged in) and the graphics card (load in percent, memory in bytes), \
         where the computer has them. A sensor Dyno cannot read says why, for example that it needs administrator readings.",
        none(),
        move |_| sensors(&c),
    ));
    v
}

/// A read-only tool with Dyno's bounds.
fn read(
    name: &str,
    title: &str,
    description: &str,
    schema: Value,
    f: impl Fn(&Value) -> Result<Value, ToolError> + Send + Sync + 'static,
) -> ToolDef {
    ToolDef::read_only(name, description, schema, move |_: &CallCtx, args: &Value| f(args).map(answer))
        .title(title)
        .max_result_bytes(MAX_BYTES)
        .timeout(TIMEOUT)
}

/// An answer: the JSON as the text a model reads and as structured data.
fn answer(mut v: Value) -> ToolOutput {
    fit(&mut v);
    ToolOutput {
        text: v.to_string(),
        structured: Some(v),
        next_cursor: None,
    }
}

/// Keeps the answer under the byte bound as valid JSON: while it is too big, the longest list is cut in half and the answer says
/// `truncated`. (The server would otherwise cut the text mid-value.)
fn fit(v: &mut Value) {
    const ROOM: usize = MAX_BYTES - 512;
    let mut cut = false;
    while v.to_string().len() > ROOM {
        let Some(list) = longest_list(v) else { break };
        if list.len() <= 1 {
            break;
        }
        list.truncate(list.len() / 2);
        cut = true;
    }
    if cut {
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "truncated".into(),
                json!("The answer was cut to fit; ask for fewer rows or a narrower query."),
            );
        }
    }
}

fn longest_list(v: &mut Value) -> Option<&mut Vec<Value>> {
    match v {
        Value::Array(a) => {
            let best = a.iter().map(longest_len).max().unwrap_or(0);
            if a.len() >= best {
                return Some(a);
            }
            a.iter_mut().max_by_key(|x| longest_len(x)).and_then(longest_list)
        }
        Value::Object(o) => o.values_mut().max_by_key(|x| longest_len(x)).and_then(longest_list),
        _ => None,
    }
}

fn longest_len(v: &Value) -> usize {
    match v {
        Value::Array(a) => a.len().max(a.iter().map(longest_len).max().unwrap_or(0)),
        Value::Object(o) => o.values().map(longest_len).max().unwrap_or(0),
        _ => 0,
    }
}

// ---- arguments (the server's schema check has refused anything else; these re-check what the handler relies on) ----

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn count(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| ToolError(format!("{key} is {min} to {max}."))),
    }
}

fn word<'a>(args: &'a Value, key: &str, allowed: &[&'a str], default: &'a str) -> Result<&'a str, ToolError> {
    match args.get(key).and_then(Value::as_str) {
        None => Ok(default),
        Some(w) => allowed
            .iter()
            .copied()
            .find(|a| *a == w)
            .ok_or_else(|| ToolError(format!("{key} is one of {}.", allowed.join(", ")))),
    }
}

// ---- small shapers ----

/// Text for a client: control characters removed, cut to [`MAX_TEXT`] characters.
fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    hide_user(&s).chars().take(MAX_TEXT).collect()
}

/// Replaces the user-name segment of any home or media path in free text (`/Users/bob/x`, `/home/bob`, `/media/bob/DISK`,
/// `/run/media/bob`, `C:\Users\bob`) with `<user>`: a volume label or a rule's text can carry one, and a client never gets a user name.
fn hide_user(s: &str) -> String {
    const PREFIXES: [&str; 5] = ["/Users/", "/home/", "/media/", "/run/media/", "\\Users\\"];
    let mut out = s.to_string();
    for prefix in PREFIXES {
        let lower_prefix = prefix.to_ascii_lowercase();
        let mut from = 0;
        while let Some(i) = out.to_ascii_lowercase()[from..].find(&lower_prefix).map(|i| i + from) {
            let start = i + prefix.len();
            let end = out[start..].find(['/', '\\', ' ', '"', '\'']).map_or(out.len(), |e| start + e);
            if start < end && &out[start..end] != "<user>" {
                out.replace_range(start..end, "<user>");
            }
            from = start;
        }
    }
    out
}

/// A mount point a client may see: `/` or a drive letter. Any other mount point is a path (`/media/bob/DISK`, `/Volumes/Work`)
/// that can carry a user name or a label, so it is left out.
fn public_mount(mount: &str) -> Option<String> {
    let m = mount.trim();
    let bytes = m.as_bytes();
    let drive = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || (bytes.len() == 3 && (bytes[2] == b'\\' || bytes[2] == b'/')));
    (m == "/" || drive).then(|| m.to_string())
}

/// Removes " on port 1234" from an alarm's title: a client gets the program, never a port number.
fn no_port(title: &str) -> String {
    let mut out = title.to_string();
    while let Some(i) = out.find(" on port ") {
        let digits = out[i + 9..].chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            break;
        }
        out.replace_range(i..i + 9 + digits, "");
    }
    out
}

/// A number rounded to one decimal (the pages show one).
fn r1(v: f64) -> f64 {
    if v.is_finite() {
        (v * 10.0).round() / 10.0
    } else {
        0.0
    }
}

fn not_available(reason: &str) -> Value {
    json!({"available": false, "reason": clean(reason)})
}

/// A reading, or the reason it is not there.
fn reading<T>(r: &Reading<T>, f: impl FnOnce(&T) -> Value) -> Value {
    match r {
        Reading::Available(v) => f(v),
        Reading::Unavailable(why) => not_available(why),
    }
}

/// `reason`, with what the helper's state adds ("Turn on administrator readings in Settings ...") when it differs.
fn with_via(reason: &str, via: &ReadVia) -> String {
    match via.reason() {
        Some(extra) if !reason.contains(extra) => format!("{} {}", dyno_core::read_via::sentence(reason), extra),
        _ => reason.to_string(),
    }
}

fn why_unread(r: UnavailableReason) -> &'static str {
    match r {
        UnavailableReason::NeedsAdmin => "Needs administrator readings; turn them on in Dyno's Settings.",
        UnavailableReason::NotPermitted => "The system does not let Dyno read this program.",
        UnavailableReason::NotSupported => "Not on this operating system.",
        UnavailableReason::ResidentMemoryOnly => "Only resident memory is available for this program on this system.",
    }
}

fn pressure_word(p: MemoryPressure) -> &'static str {
    match p {
        MemoryPressure::Normal => "normal",
        MemoryPressure::Elevated => "elevated",
        MemoryPressure::Critical => "critical",
    }
}

fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    }
}

fn state_word(s: AlarmState) -> &'static str {
    match s {
        AlarmState::Active => "active",
        AlarmState::Acknowledged => "acknowledged",
        AlarmState::Cleared => "cleared",
        AlarmState::ClearedUnacknowledged => "clearedUnacknowledged",
    }
}

fn change_word(k: ChangeKind) -> &'static str {
    match k {
        ChangeKind::Raised => "raised",
        ChangeKind::Repeated => "repeated",
        ChangeKind::Returned => "returned",
        ChangeKind::Acknowledged => "acknowledged",
        ChangeKind::Shelved => "shelved",
        ChangeKind::Unshelved => "unshelved",
        ChangeKind::Muted => "muted",
        ChangeKind::Noted => "noted",
        ChangeKind::Chattering => "chattering",
        ChangeKind::Pruned => "pruned",
    }
}

fn confidence_word(c: Confidence) -> &'static str {
    match c {
        Confidence::Low => "low",
        Confidence::Medium => "medium",
        Confidence::High => "high",
    }
}

fn percent(part: u64, whole: u64) -> f64 {
    r1(f64::from(dyno_core::metrics::percent(part, whole)))
}

// ---- asking the agent ----

fn unexpected() -> ToolError {
    ToolError("The agent answered something unexpected.".into())
}

fn live(calls: &Arc<dyn AgentCalls>) -> Result<LiveUpdate, ToolError> {
    calls.live().map_err(ToolError)
}

fn inventory(calls: &Arc<dyn AgentCalls>) -> Result<SystemInventory, String> {
    match calls.call(Request::Inventory)? {
        Reply::Inventory(i) => Ok(*i),
        _ => Err("The agent answered something unexpected.".into()),
    }
}

// ---- system_summary ----

fn system_summary(calls: &Arc<dyn AgentCalls>) -> Result<Value, ToolError> {
    let l = live(calls)?;
    let inv = inventory(calls);
    let (os, cpu_name) = match &inv {
        Ok(i) => (json!(clean(&i.os)), json!(clean(&i.cpu.name))),
        Err(why) => (not_available(why), not_available(why)),
    };
    let s = &l.system;
    Ok(json!({
        "os": os,
        "uptimeSeconds": s.uptime_secs,
        "cpu": {"name": cpu_name, "percent": r1(f64::from(s.cpu.total)), "logicalCores": s.cpu.per_core.len()},
        "memory": {
            "usedPercent": percent(s.memory.used, s.memory.total),
            "usedBytes": s.memory.used,
            "totalBytes": s.memory.total,
            "pressure": pressure_word(l.pressure),
        },
        "swap": {"usedBytes": s.memory.swap_used, "totalBytes": s.memory.swap_total},
        "takenAtMs": s.at.millis(),
    }))
}

// ---- cpu_memory ----

fn cpu_memory(calls: &Arc<dyn AgentCalls>, per_core: bool) -> Result<Value, ToolError> {
    let l = live(calls)?;
    let s = &l.system;
    let mut cpu = Map::new();
    cpu.insert("percent".into(), json!(r1(f64::from(s.cpu.total))));
    cpu.insert("logicalCores".into(), json!(s.cpu.per_core.len()));
    cpu.insert(
        "frequencyMhz".into(),
        if s.cpu.frequency_mhz > 0 {
            json!(s.cpu.frequency_mhz)
        } else {
            not_available("The system does not report the frequency.")
        },
    );
    if s.cpu.base_frequency_mhz > 0 {
        cpu.insert("baseFrequencyMhz".into(), json!(s.cpu.base_frequency_mhz));
    }
    if per_core {
        cpu.insert(
            "perCorePercent".into(),
            json!(s.cpu.per_core.iter().take(MAX_CORES).map(|c| r1(f64::from(*c))).collect::<Vec<_>>()),
        );
    }
    let m = &s.memory;
    Ok(json!({
        "cpu": Value::Object(cpu),
        "memory": {
            "totalBytes": m.total,
            "usedBytes": m.used,
            "availableBytes": m.available,
            "usedPercent": percent(m.used, m.total),
            "swapUsedBytes": m.swap_used,
            "swapTotalBytes": m.swap_total,
            "commit": reading(&m.commit, |c| json!({"usedBytes": c.used, "limitBytes": c.limit})),
            "pressure": pressure_word(l.pressure),
        },
        "takenAtMs": s.at.millis(),
    }))
}

// ---- disks ----

fn lifespan(calls: &Arc<dyn AgentCalls>) -> Result<LifespanOverview, String> {
    match calls.call(Request::Lifespan)? {
        Reply::Lifespan(o) => Ok(*o),
        _ => Err("The agent answered something unexpected.".into()),
    }
}

fn health_row(f: &LifeForecast) -> Value {
    match &f.outcome {
        LifeOutcome::Unavailable { reason } => json!({"name": clean(&f.name), "available": false, "reason": clean(reason)}),
        _ => json!({"name": clean(&f.name), "summary": clean(&f.headline), "confidence": confidence_word(f.confidence)}),
    }
}

fn disks(calls: &Arc<dyn AgentCalls>) -> Result<Value, ToolError> {
    let l = live(calls)?;
    let s = &l.system;
    let volumes: Vec<Value> = s
        .disks
        .iter()
        .take(MAX_ITEMS)
        .map(|d| {
            let used = d.total.saturating_sub(d.free);
            let mut v = json!({
                "name": clean(&d.name), "totalBytes": d.total, "freeBytes": d.free,
                "usedBytes": used, "usedPercent": percent(used, d.total), "removable": d.removable,
            });
            if let Some(m) = public_mount(&d.mount) {
                v["mount"] = json!(m);
            }
            v
        })
        .collect();
    let health = match lifespan(calls) {
        Ok(o) => {
            let rows: Vec<Value> = o
                .forecasts
                .iter()
                .filter(|f| f.subject == LifeSubject::Ssd)
                .take(MAX_ITEMS)
                .map(health_row)
                .collect();
            if rows.is_empty() {
                let extra = [&l.via.processes, &l.via.temperatures]
                    .iter()
                    .find_map(|v| v.reason().map(|_| NEEDS_ADMIN));
                not_available(&format!(
                    "Dyno has no health reading for these drives.{}",
                    extra.map_or(String::new(), |e| format!(" {e}"))
                ))
            } else {
                Value::Array(rows)
            }
        }
        Err(why) => not_available(&why),
    };
    let volumes = if volumes.is_empty() {
        not_available("Dyno has not found any volumes on this computer.")
    } else {
        Value::Array(volumes)
    };
    Ok(json!({
        "volumes": volumes,
        "volumeCount": s.disks.len(),
        "io": {
            "readBytesPerSecond": r1(s.disk_io.read_bps),
            "writeBytesPerSecond": r1(s.disk_io.write_bps),
            "busyPercent": reading(&s.disk_io.busy, |b| json!(r1(f64::from(*b)))),
        },
        "health": health,
        "takenAtMs": s.at.millis(),
    }))
}

// ---- top_processes ----

fn measured<T: Copy>(m: &Measured<T>, f: impl Fn(T) -> Value, name: &str, row: &mut Map<String, Value>, key: &str) {
    match m {
        Measured::Value(v) => {
            row.insert(key.into(), f(*v));
        }
        Measured::Unavailable { reason } => {
            row.insert(key.into(), Value::Null);
            row.insert(name.into(), json!(why_unread(*reason)));
        }
    }
}

fn process_row(g: &AppGroup) -> Value {
    let mut row = Map::new();
    row.insert("name".into(), json!(clean(&g.name)));
    row.insert("pid".into(), json!(g.pids.first().copied()));
    row.insert("processCount".into(), json!(g.pids.len()));
    measured(&g.cpu, |c| json!(r1(f64::from(c))), "cpuUnavailable", &mut row, "cpuPercent");
    measured(&g.memory, |m| json!(m), "memoryUnavailable", &mut row, "memoryBytes");
    Value::Object(row)
}

fn top_processes(calls: &Arc<dyn AgentCalls>, args: &Value) -> Result<Value, ToolError> {
    let sort = word(args, "sort", &["cpu", "memory"], "cpu")?;
    let limit = count(args, "limit", 10, 1, MAX_ROWS)?;
    let l = live(calls)?;
    // The full list when the task manager has asked for it; otherwise the agent's short top lists (one per ranking).
    let (mut ranked, full): (Vec<AppGroup>, bool) = match &l.groups {
        Some(g) => (g.clone(), true),
        None if sort == "memory" => (l.top_memory.clone(), false),
        None => (l.top_cpu.clone(), false),
    };
    if sort == "memory" {
        ranked.sort_by_key(|g| std::cmp::Reverse(g.memory_read()));
    } else {
        ranked.sort_by(|a, b| b.cpu_read().total_cmp(&a.cpu_read()));
    }
    let total = ranked.len();
    let rows: Vec<Value> = ranked.iter().take(limit).map(process_row).collect();
    let mut out = json!({"sortedBy": sort, "shown": rows.len(), "ranked": total, "processes": rows, "takenAtMs": l.system.at.millis()});
    if !full && limit > total {
        out["note"] = json!(format!(
            "Dyno keeps only a short list ({total} programs) while its task manager is closed, so no more can be ranked."
        ));
    }
    if let Some(why) = l.via.processes.reason() {
        out["processesNote"] = json!(clean(why));
    }
    Ok(out)
}

// ---- network_summary ----

fn network(calls: &Arc<dyn AgentCalls>) -> Result<NetworkSnapshot, String> {
    match calls.call(Request::Network {
        query: NetworkQuery {
            show_loopback: true,
            ..NetworkQuery::default()
        },
    })? {
        Reply::Network(n) => Ok(*n),
        _ => Err("The agent answered something unexpected.".into()),
    }
}

fn network_summary(calls: &Arc<dyn AgentCalls>) -> Result<Value, ToolError> {
    let l = live(calls)?;
    let (rx, tx) = l.system.network_totals();
    let interfaces: Vec<Value> = l
        .system
        .network
        .iter()
        .take(MAX_ITEMS)
        .map(|n| json!({"name": clean(&n.name), "receiveBytesPerSecond": r1(n.rx_bps), "sendBytesPerSecond": r1(n.tx_bps)}))
        .collect();
    let connections = match network(calls) {
        Ok(n) => {
            let established = n.connections.iter().filter(|c| c.state == SocketState::Established).count();
            let open = n.listeners.iter().filter(|x| x.exposure == Exposure::AllInterfaces).count();
            let mut v = json!({
                "listening": n.listeners.len(),
                "listeningOnAllInterfaces": open,
                "total": n.connections.len(),
                "established": established,
                "hiddenByRowLimit": n.truncated,
                "programUnknown": n.unknown_owners,
            });
            if let Some(note) = &n.note {
                v["note"] = json!(clean(note));
            }
            v
        }
        Err(why) => not_available(&why),
    };
    Ok(json!({
        "totals": {"receiveBytesPerSecond": r1(rx), "sendBytesPerSecond": r1(tx)},
        "interfaces": interfaces,
        "connections": connections,
        "takenAtMs": l.system.at.millis(),
    }))
}

// ---- alarms ----

fn alarm_row(a: &Alarm) -> Value {
    json!({
        "id": a.id, "rule": clean(&a.rule), "severity": severity_word(a.severity), "state": state_word(a.state), "title": clean(&no_port(&a.title)),
        "firstSeenMs": a.first_seen.millis(), "lastSeenMs": a.last_seen.millis(), "repeatCount": a.repeat_count,
        "currentValue": a.current_value.map(r1), "muted": a.muted, "chattering": a.chattering,
    })
}

fn change_row(c: &AlarmChange) -> Value {
    json!({
        "atMs": c.at.millis(), "change": change_word(c.kind), "alarmId": c.alarm.id, "rule": clean(&c.alarm.rule),
        "severity": severity_word(c.alarm.severity), "title": clean(&no_port(&c.alarm.title)),
    })
}

fn alarms(calls: &Arc<dyn AgentCalls>, args: &Value) -> Result<Value, ToolError> {
    let active_limit = count(args, "active_limit", 20, 1, MAX_ROWS)?;
    let history_limit = count(args, "history_limit", 10, 0, MAX_ROWS)?;
    // Every open alarm (the agent's own cap is 500) so that `activeCount` is true; only the first `active_limit` are shown.
    let filter = AlarmFilter {
        limit: 500,
        ..AlarmFilter::default()
    };
    let active = match calls.call(Request::Alarms { filter }).map_err(ToolError)? {
        Reply::Alarms(a) => a,
        _ => return Err(unexpected()),
    };
    let changes = if history_limit == 0 {
        Vec::new()
    } else {
        match calls
            .call(Request::AlarmHistory {
                alarm_id: None,
                limit: u32::try_from(history_limit).unwrap_or(10),
            })
            .map_err(ToolError)?
        {
            Reply::AlarmChanges(c) => c,
            _ => return Err(unexpected()),
        }
    };
    Ok(json!({
        "activeCount": active.len(),
        "active": active.iter().take(active_limit).map(alarm_row).collect::<Vec<_>>(),
        "recentChanges": changes.iter().take(history_limit).map(change_row).collect::<Vec<_>>(),
    }))
}

// ---- history ----

/// A history metric: its name, unit and where it is in a point.
struct HistoryMetric {
    name: &'static str,
    unit: &'static str,
    value: fn(&HistoryPoint) -> Option<f64>,
    max: fn(&HistoryPoint) -> Option<f64>,
    missing: &'static str,
}

const METRICS: [HistoryMetric; 8] = [
    HistoryMetric {
        name: "cpu",
        unit: "percent",
        value: |p| Some(f64::from(p.cpu)),
        max: |p| Some(f64::from(p.cpu_max)),
        missing: "",
    },
    HistoryMetric {
        name: "memory",
        unit: "percent",
        value: |p| Some(f64::from(p.memory)),
        max: |p| Some(f64::from(p.memory_max)),
        missing: "",
    },
    HistoryMetric {
        name: "disk_read",
        unit: "bytes per second",
        value: |p| Some(p.disk_read_bps),
        max: |p| Some(p.disk_read_bps),
        missing: "",
    },
    HistoryMetric {
        name: "disk_write",
        unit: "bytes per second",
        value: |p| Some(p.disk_write_bps),
        max: |p| Some(p.disk_write_bps),
        missing: "",
    },
    HistoryMetric {
        name: "network_receive",
        unit: "bytes per second",
        value: |p| Some(p.net_rx_bps),
        max: |p| Some(p.net_rx_bps),
        missing: "",
    },
    HistoryMetric {
        name: "network_send",
        unit: "bytes per second",
        value: |p| Some(p.net_tx_bps),
        max: |p| Some(p.net_tx_bps),
        missing: "",
    },
    HistoryMetric {
        name: "gpu",
        unit: "percent",
        value: |p| p.gpu.map(f64::from),
        max: |p| p.gpu.map(f64::from),
        missing: "Dyno has no GPU readings in this range: this computer's graphics card is not measured, or Dyno was not running.",
    },
    HistoryMetric {
        name: "temperature",
        unit: "degrees Celsius",
        value: |p| p.temperature.map(f64::from),
        max: |p| p.temperature.map(f64::from),
        missing: "Dyno has no temperature readings in this range: this computer does not report them, or they need administrator readings.",
    },
];

fn history(calls: &Arc<dyn AgentCalls>, args: &Value) -> Result<Value, ToolError> {
    let names: Vec<&str> = METRICS.iter().map(|m| m.name).collect();
    let metric_name = args
        .get("metric")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError(format!("metric is one of {}.", names.join(", "))))?;
    let metric = METRICS
        .iter()
        .find(|m| m.name == metric_name)
        .ok_or_else(|| ToolError(format!("metric is one of {}.", names.join(", "))))?;
    let range_name = word(args, "range", &["hour", "day", "week", "month"], "hour")?;
    let range = match range_name {
        "day" => HistoryRange::Day,
        "week" => HistoryRange::Week,
        "month" => HistoryRange::Month,
        _ => HistoryRange::Hour,
    };
    let want = count(args, "points", DEFAULT_POINTS, 2, MAX_POINTS)?;
    let h: History = match calls.call(Request::History { range, end: None }).map_err(ToolError)? {
        Reply::History(h) => *h,
        _ => return Err(unexpected()),
    };
    if h.points.is_empty() {
        return Err(ToolError(format!("Dyno has no history for the last {range_name} yet. It keeps history while its agent runs; try a shorter range or ask again later.")));
    }
    if h.points.iter().all(|p| (metric.value)(p).is_none()) {
        return Err(ToolError(metric.missing.into()));
    }
    // Buckets of whole source points, so at most `want` points come out; each is the average of its bucket with the highest value.
    let per_bucket = h.points.len().div_ceil(want);
    let points: Vec<Value> = h
        .points
        .chunks(per_bucket)
        .filter_map(|chunk| {
            let values: Vec<f64> = chunk.iter().filter_map(|p| (metric.value)(p)).collect();
            if values.is_empty() {
                return None;
            }
            let max = chunk.iter().filter_map(|p| (metric.max)(p)).fold(f64::MIN, f64::max);
            Some(json!({"atMs": chunk[0].at.millis(), "value": r1(values.iter().sum::<f64>() / values.len() as f64), "max": r1(max)}))
        })
        .collect();
    Ok(json!({
        "metric": metric.name,
        "unit": metric.unit,
        "range": range_name,
        "fromMs": h.from.millis(),
        "toMs": h.to.millis(),
        "resolutionMs": h.resolution_ms.saturating_mul(i64::try_from(per_bucket).unwrap_or(1)),
        "points": points,
    }))
}

// ---- sensors ----

fn sensors(calls: &Arc<dyn AgentCalls>) -> Result<Value, ToolError> {
    let l = live(calls)?;
    let s = &l.system;
    let temperatures = match &s.temperatures {
        Reading::Available(t) if t.is_empty() => {
            not_available(&with_via("This computer reports no temperature sensors.", &l.via.temperatures))
        }
        Reading::Available(t) => json!({"readings": t.iter().take(MAX_ITEMS).map(|t| {
            let mut v = json!({"label": clean(&t.label), "celsius": r1(f64::from(t.celsius))});
            if let Some(c) = t.critical {
                v["criticalCelsius"] = json!(r1(f64::from(c)));
            }
            v
        }).collect::<Vec<_>>()}),
        Reading::Unavailable(why) => not_available(&with_via(why, &l.via.temperatures)),
    };
    let fans = match &s.fans {
        Reading::Available(f) if f.is_empty() => not_available(&with_via("This computer reports no fans.", &l.via.temperatures)),
        Reading::Available(f) => {
            json!({"readings": f.iter().take(MAX_ITEMS).map(|f| json!({"label": clean(&f.label), "rpm": f.rpm})).collect::<Vec<_>>()})
        }
        Reading::Unavailable(why) => not_available(&with_via(why, &l.via.temperatures)),
    };
    let battery = reading(&s.battery, |b| {
        json!({
            "chargePercent": r1(f64::from(b.charge)),
            "pluggedIn": b.source == PowerSource::Ac,
            "healthPercent": r1(f64::from(b.health)),
            "cycles": b.cycles,
            "secondsToEmpty": b.seconds_to_empty,
        })
    });
    let gpu = reading(
        &s.gpu,
        |g| json!({"name": clean(&g.name), "loadPercent": r1(f64::from(g.utilization)), "memoryUsedBytes": g.memory_used, "memoryTotalBytes": g.memory_total}),
    );
    let out = json!({"temperatures": temperatures, "fans": fans, "battery": battery, "gpu": gpu, "takenAtMs": s.at.millis()});
    // Nothing at all to read: say why, never an empty success.
    let sections = ["temperatures", "fans", "battery", "gpu"];
    if sections.iter().all(|k| out[*k]["available"] == json!(false)) {
        let reasons: Vec<String> = sections
            .iter()
            .map(|k| format!("{k}: {}", out[*k]["reason"].as_str().unwrap_or("not available")))
            .collect();
        return Err(ToolError(format!(
            "This computer has no sensor readings Dyno can show. {}",
            reasons.join(" ")
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
