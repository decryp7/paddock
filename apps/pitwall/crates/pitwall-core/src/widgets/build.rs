//! The popup's values: one pure function from the facts the live feed keeps and the clock to what each placed widget shows,
//! the header status and the surface notice. The UI draws these and decides nothing (AGENTS.md "Where logic lives"): what is
//! calm, what is stale, which tone a meter has, which row comes first and how many fit are all decided here.

use super::facts::Facts;
use super::popover::{DOWNLOAD_ROWS, ERROR_ROWS, MODEL_ROWS};
use super::values::{
    DownloadRow, DownloadRowState, EngineHealth, ErrorRow, MemoryLevel, MeterTone, ModelRow, ModelRowState, Page, PopoverHeader,
    PopoverState, PopoverStatus, PopoverValue, PopoverValues, ServerState, ServingToggle, VramReading, WidgetActionId,
};
use super::PopoverWidget;
use crate::mgmt::{Availability, Download, DownloadState, HotState, IssueState, MemoryPressure, ServingState};
use paddock_widgets::{Layout, PlacementState, Progress, SurfaceNotice, SurfaceValues, Tone, WidgetAction, WidgetState};

/// A meter is "getting full" above this share (percent) and "nearly full" above [`BAD_ABOVE`].
const WARN_ABOVE: u8 = 85;
/// See [`WARN_ABOVE`].
const BAD_ABOVE: u8 = 95;
/// Recent errors lists the errors of this long ago, at most (milliseconds): a day.
const ERROR_WINDOW_MS: i64 = 86_400_000;
const MB: u64 = 1024 * 1024;

fn act(action: WidgetActionId, label: &str) -> Option<WidgetAction<WidgetActionId>> {
    Some(WidgetAction {
        action,
        label: label.to_string(),
    })
}

fn open(page: Page, label: &str) -> Option<WidgetAction<WidgetActionId>> {
    act(WidgetActionId::OpenPage { page }, label)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The readings are older than the feed: the connection is gone, so every value is the last one known.
fn stale_since(f: &Facts) -> Option<i64> {
    if f.connected() {
        None
    } else {
        f.failed_ms
    }
}

fn ready(value: PopoverValue, f: &Facts) -> PopoverState {
    WidgetState::Ready {
        value,
        as_of: f.changed_ms,
        stale_since: stale_since(f),
    }
}

fn empty(title: &str, detail: &str, next: Option<WidgetAction<WidgetActionId>>) -> PopoverState {
    WidgetState::Empty {
        title: title.into(),
        detail: detail.into(),
        next,
        calm: false,
    }
}

/// "All good": the widget has looked and there is nothing to act on (drawn as one calm row with a check).
fn calm(title: &str, detail: &str, next: Option<WidgetAction<WidgetActionId>>) -> PopoverState {
    WidgetState::Empty {
        title: title.into(),
        detail: detail.into(),
        next,
        calm: true,
    }
}

/// "12 min ago", "3 h ago", "2 d ago".
fn ago(now_ms: i64, then_ms: i64) -> String {
    let minutes = (now_ms - then_ms).max(0) / 60_000;
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}

fn percent(used: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    let p = (u128::from(used) * 100 + u128::from(total) / 2) / u128::from(total);
    u8::try_from(p.min(100)).unwrap_or(100)
}

fn tone(percent: u8) -> MeterTone {
    if percent > BAD_ABOVE {
        MeterTone::Bad
    } else if percent > WARN_ABOVE {
        MeterTone::Warn
    } else {
        MeterTone::Neutral
    }
}

/// How the engines are doing, the most serious thing first.
fn engine_health(f: &Facts) -> EngineHealth {
    if f.engine_setup.state == crate::mgmt::EngineSetupState::Installing {
        return EngineHealth::Installing;
    }
    let failed = f.issues.iter().filter(|i| i.state == IssueState::Failed).count();
    if failed > 0 {
        return EngineHealth::Failed { count: count(failed) };
    }
    let restarting = f.issues.iter().filter(|i| i.state == IssueState::Restarting).count();
    if restarting > 0 {
        return EngineHealth::Restarting { count: count(restarting) };
    }
    let none_usable = !f.engines.is_empty() && f.engines.iter().all(|e| !matches!(e.availability, Availability::Available));
    if f.engine_setup.state == crate::mgmt::EngineSetupState::Failed || none_usable {
        return EngineHealth::Missing;
    }
    EngineHealth::Ready
}

/// How the local AI is doing overall, and which footer items work now: the header pill and the menu.
pub fn popover_header(f: &Facts) -> PopoverHeader {
    let reachable = f.connected() && f.known();
    let serving = f.server.as_ref().map(|s| s.serving);
    let problems = f.issues.len();
    let status = match (&f.server, reachable) {
        (None, _) if f.failed() => PopoverStatus::Unreachable,
        (None, _) => PopoverStatus::Unknown,
        (Some(_), false) => PopoverStatus::Unreachable,
        (Some(_), true) if serving == Some(ServingState::Stopped) => PopoverStatus::Stopped,
        (Some(_), true) if problems > 0 => PopoverStatus::Problems { count: count(problems) },
        (Some(_), true) if serving == Some(ServingState::Paused) => PopoverStatus::Paused,
        (Some(s), true) => PopoverStatus::Serving { port: s.port },
    };
    PopoverHeader {
        status,
        serving: match (reachable, serving) {
            (true, Some(ServingState::Serving)) => ServingToggle::Pause,
            (true, Some(_)) => ServingToggle::Resume,
            _ => ServingToggle::Off,
        },
        can_release_memory: reachable && !f.models.is_empty(),
    }
}

fn server(f: &Facts) -> PopoverState {
    let known = f.server.as_ref();
    let state = match known {
        Some(s) if f.connected() => match s.serving {
            ServingState::Serving => ServerState::Serving,
            ServingState::Paused => ServerState::Paused,
            ServingState::Stopped => ServerState::Stopped,
        },
        _ => ServerState::Unreachable,
    };
    let value = PopoverValue::Server {
        state,
        port: known.map_or(0, |s| s.port),
        // Only a feed that answers says how the engines are; a stale "ready" beside "not running" would be a lie.
        engine: known.filter(|_| f.connected()).map(|_| engine_health(f)),
    };
    ready(value, f)
}

fn activity(f: &Facts) -> PopoverState {
    let tokens_per_sec = if f.serving_off() {
        0
    } else {
        let t = f.throughput.gen_tps.round();
        if t.is_finite() && t > 0.0 {
            u32::try_from(t as u64).unwrap_or(u32::MAX)
        } else {
            0
        }
    };
    ready(
        PopoverValue::Activity {
            running: f.running,
            waiting: f.waiting,
            tokens_per_sec,
        },
        f,
    )
}

fn memory(f: &Facts) -> PopoverState {
    let Some(m) = &f.memory else {
        return empty("See memory in use", "No reading yet.", None);
    };
    let used = m.ram_total.saturating_sub(m.ram_available);
    let p = percent(used, m.ram_total);
    let vram = m.gpus.iter().find_map(|g| match (g.vram_used, g.vram_total) {
        (Some(used), Some(total)) if total > 0 => {
            let vp = percent(used, total);
            Some(VramReading {
                used,
                total,
                percent: vp,
                busy_percent: g.utilisation.map(|u| u.round().clamp(0.0, 100.0) as u8),
                tone: tone(vp),
            })
        }
        _ => None,
    });
    ready(
        PopoverValue::Memory {
            used,
            total: m.ram_total,
            percent: p,
            tone: tone(p),
            pressure: match m.pressure {
                MemoryPressure::Normal => MemoryLevel::Normal,
                MemoryPressure::Elevated => MemoryLevel::Elevated,
                MemoryPressure::Critical => MemoryLevel::Critical,
            },
            vram,
        },
        f,
    )
}

fn loaded_models(f: &Facts) -> PopoverState {
    if f.models.is_empty() {
        return empty("See which models are loaded", "No model is loaded.", open(Page::Models, "Open Models"));
    }
    let mut ordered: Vec<&crate::mgmt::LoadedModel> = f.models.iter().collect();
    // Busy first, then the most recently used; the id keeps equal models in a fixed order.
    ordered.sort_by(|a, b| {
        let active = |m: &crate::mgmt::LoadedModel| m.active_requests > 0 || m.state == HotState::Busy;
        active(b)
            .cmp(&active(a))
            .then(b.last_used_at.cmp(&a.last_used_at))
            .then(a.model_id.cmp(&b.model_id))
    });
    let rows: Vec<ModelRow> = ordered
        .iter()
        .take(MODEL_ROWS)
        .map(|m| ModelRow {
            id: m.model_id.clone(),
            name: m.name.clone(),
            bytes: m.ram_bytes.saturating_add(m.vram_bytes),
            state: match m.state {
                HotState::Ready => ModelRowState::Ready,
                HotState::Busy => ModelRowState::Busy,
                HotState::Loading => ModelRowState::Loading,
                HotState::ReleasingContext | HotState::Unloading => ModelRowState::Releasing,
                HotState::Failed => ModelRowState::Failed,
            },
            pinned: m.pinned,
            active: m.active_requests,
        })
        .collect();
    let bytes = f.models.iter().fold(0u64, |a, m| a.saturating_add(m.ram_bytes).saturating_add(m.vram_bytes));
    ready(
        PopoverValue::LoadedModels {
            total: count(f.models.len()),
            more: count(f.models.len().saturating_sub(rows.len())),
            rows,
            bytes,
        },
        f,
    )
}

fn is_running(d: &Download) -> bool {
    matches!(
        d.state,
        DownloadState::Queued | DownloadState::Downloading | DownloadState::Verifying | DownloadState::Unpacking | DownloadState::SelfTest
    )
}

/// "12 MB/s" (whole megabytes per second, "under 1 MB/s" below one).
fn speed(bps: u64) -> String {
    if bps >= MB {
        format!("{} MB/s", bps / MB)
    } else {
        "under 1 MB/s".to_string()
    }
}

fn phase_of(d: &Download) -> &'static str {
    match d.state {
        DownloadState::Queued => "Waiting in line",
        DownloadState::Verifying => "Checking the file",
        DownloadState::Unpacking => "Unpacking",
        DownloadState::SelfTest => "Testing the build",
        _ => "Downloading",
    }
}

fn downloads(f: &Facts) -> PopoverState {
    let mut running: Vec<&Download> = f.downloads.values().filter(|d| is_running(d)).collect();
    if let Some(d) = f.engine_setup.download.as_ref().filter(|d| is_running(d) && !f.downloads.contains_key(&d.id)) {
        running.push(d);
    }
    if !running.is_empty() {
        let done: u64 = running.iter().map(|d| d.bytes_done).sum();
        let total: u64 = running.iter().map(|d| d.bytes_total).sum();
        let bps: u64 = running.iter().map(|d| d.speed_bps).sum();
        let (title, phase, cancel) = match running.as_slice() {
            [d] => {
                let phase = if d.state == DownloadState::Downloading && bps > 0 {
                    format!("{} · {}", phase_of(d), speed(bps))
                } else {
                    phase_of(d).to_string()
                };
                (
                    format!("Downloading {}", d.name),
                    phase,
                    act(WidgetActionId::CancelDownload { id: d.id.clone() }, "Cancel download"),
                )
            }
            many => (
                format!("Downloading {} files", many.len()),
                if bps > 0 { speed(bps) } else { "Starting".to_string() },
                None,
            ),
        };
        let progress = if total == 0 {
            Progress::Unknown
        } else {
            Progress::Fraction {
                done: done.min(total) / MB,
                total: total / MB,
                unit: "MB".into(),
            }
        };
        return WidgetState::Working {
            title,
            phase,
            progress,
            last: None,
            cancel,
        };
    }
    let mut rest: Vec<&Download> = f
        .downloads
        .values()
        .filter(|d| matches!(d.state, DownloadState::Failed | DownloadState::Paused))
        .collect();
    // Failed ones first, then by name.
    rest.sort_by(|a, b| (a.state != DownloadState::Failed).cmp(&(b.state != DownloadState::Failed)).then(a.name.cmp(&b.name)));
    if rest.is_empty() && !f.downloads_read {
        return empty("See downloads as they run", "Not checked yet.", open(Page::Models, "Open Models"));
    }
    if rest.is_empty() {
        return calm("No downloads running", "Nothing to wait for", open(Page::Models, "Open Models"));
    }
    let rows: Vec<DownloadRow> = rest
        .iter()
        .take(DOWNLOAD_ROWS)
        .map(|d| DownloadRow {
            id: d.id.clone(),
            name: d.name.clone(),
            state: if d.state == DownloadState::Failed {
                DownloadRowState::Failed
            } else {
                DownloadRowState::Paused
            },
            message: d.error.clone(),
        })
        .collect();
    ready(
        PopoverValue::Downloads {
            more: count(rest.len().saturating_sub(rows.len())),
            rows,
        },
        f,
    )
}

fn updates(f: &Facts, now_ms: i64) -> PopoverState {
    match &f.updates {
        Some(u) if u.count > 0 => ready(
            PopoverValue::Updates {
                count: u.count,
                summary: u.summary.clone(),
            },
            f,
        ),
        Some(u) => match u.last_checked_ms {
            Some(at) => calm("Models are up to date", &format!("Checked {}", ago(now_ms, at)), open(Page::Models, "Open Models")),
            None => empty("Know when models have updates", "Not checked yet.", open(Page::Models, "Open Models")),
        },
        None => empty("Know when models have updates", "Not checked yet.", open(Page::Models, "Open Models")),
    }
}

fn recent_errors(f: &Facts, now_ms: i64) -> PopoverState {
    let recent: Vec<_> = f.errors.iter().filter(|e| now_ms - e.at_ms <= ERROR_WINDOW_MS).collect();
    if recent.is_empty() && !f.events_read {
        return empty("See what failed", "Not checked yet.", open(Page::Requests, "Open Requests"));
    }
    if recent.is_empty() {
        return calm("No errors", "In the last day", open(Page::Requests, "Open Requests"));
    }
    let rows: Vec<ErrorRow> = recent
        .iter()
        .take(ERROR_ROWS)
        .map(|e| ErrorRow {
            id: format!("e{}", e.id),
            message: e.message.clone(),
            at_ms: e.at_ms,
        })
        .collect();
    ready(
        PopoverValue::RecentErrors {
            more: count(recent.len().saturating_sub(rows.len())),
            rows,
        },
        f,
    )
}

/// What a widget shows before the first snapshot (connecting) or while the server never answered.
fn unknown(kind: PopoverWidget, f: &Facts) -> PopoverState {
    let title = match kind {
        PopoverWidget::Server if f.failed() => return server(f),
        PopoverWidget::Server => "See whether the server is serving",
        PopoverWidget::Activity => "See what the server is doing",
        PopoverWidget::Memory => "See memory in use",
        PopoverWidget::LoadedModels => "See which models are loaded",
        PopoverWidget::Downloads => "See downloads as they run",
        PopoverWidget::Updates => "Know when models have updates",
        PopoverWidget::RecentErrors => "See what failed",
    };
    if f.failed() {
        empty(title, "The server is not running.", None)
    } else {
        empty(title, "Connecting to the server.", None)
    }
}

/// One widget's state. `now_ms` is the clock (milliseconds since the epoch).
pub fn state(kind: PopoverWidget, f: &Facts, now_ms: i64) -> PopoverState {
    if !f.known() {
        return unknown(kind, f);
    }
    match kind {
        PopoverWidget::Server => server(f),
        PopoverWidget::Activity => activity(f),
        PopoverWidget::Memory => memory(f),
        PopoverWidget::LoadedModels => loaded_models(f),
        PopoverWidget::Downloads => downloads(f),
        PopoverWidget::Updates => updates(f, now_ms),
        PopoverWidget::RecentErrors => recent_errors(f, now_ms),
    }
}

/// The surface notice: the one thing that needs the person before any widget does. A lost server first, then apps waiting to pair.
fn notice(f: &Facts) -> Option<SurfaceNotice<WidgetActionId>> {
    if !f.connected() && f.failed() {
        let (title, detail) = if f.known() {
            ("The server is not answering", "Showing the last readings. Pitwall keeps trying.")
        } else {
            ("The server is not running", "Open Pitwall to start or repair it.")
        };
        return Some(SurfaceNotice {
            tone: Tone::Danger,
            title: title.into(),
            detail: detail.into(),
            action: open(Page::Dashboard, "Repair server"),
        });
    }
    if f.pairings > 0 {
        let title = if f.pairings == 1 {
            "An app wants to pair".to_string()
        } else {
            format!("{} apps want to pair", f.pairings)
        };
        return Some(SurfaceNotice {
            tone: Tone::Info,
            title,
            detail: "Review in Pitwall.".into(),
            action: open(Page::Dashboard, "Review"),
        });
    }
    None
}

/// All values of the popup for `layout`.
pub fn popover_values(layout: &Layout<PopoverWidget>, f: &Facts, now_ms: i64) -> PopoverValues {
    SurfaceValues {
        notice: notice(f),
        placements: layout
            .placements
            .iter()
            .map(|p| PlacementState {
                id: p.id.clone(),
                state: state(p.widget, f, now_ms),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
