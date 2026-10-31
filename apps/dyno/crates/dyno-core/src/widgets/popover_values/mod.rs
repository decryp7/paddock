//! What the tray popup's widgets show: one pure function per kind from the inputs the engine already has (family ADR 0035
//! §5). The agent builds the slow kinds (alarms, volumes, backups, issues, counters) from state it keeps; the app builds
//! the live kinds (CPU and memory, network, top apps) from the live push it subscribes to. Severity, thresholds, ordering
//! and staleness are decided here and nowhere else: the UI only draws what arrives.
//!
//! Nothing here reads the clock, the disk or the OS: time is an argument.

use super::dashboard::{Thresholds, ValueTone};
use super::popover::{PopoverKind, PopoverWidget, VolumeChoice};
use crate::alarms::{Alarm, AlarmState};
use crate::backup::{BackupReport, BackupState};
use crate::counters::{CounterReading, CounterUnit};
use crate::disk::volume::{Volume, VolumeLocation};
use crate::health::Severity;
use crate::issues::Issue;
use crate::protocol::LiveUpdate;
use crate::time::{Timestamp, DAY};
use paddock_widgets::{PlacementId, PlacementState, Progress, SurfaceNotice, SurfaceValues, Tone, WidgetAction, WidgetState};
use serde::{Deserialize, Serialize};

/// Alarm rows shown before "+N more".
pub const ALARM_ROWS: usize = 2;
/// Volume rows shown by one disk space widget before "+N more".
pub const VOLUME_ROWS: usize = 3;
/// Pinned counters shown.
pub const COUNTER_ROWS: usize = 4;
/// Busiest apps shown.
pub const APP_ROWS: usize = 3;
/// How far back "last issue" looks.
pub const ISSUE_DAYS: i64 = 7;
/// Samples kept for the "last minute" sparklines.
pub const MINUTE_SAMPLES: usize = 60;

/// CPU load turns amber and red here (percent).
pub const CPU_THRESHOLDS: Thresholds = Thresholds {
    warning: Some(75.0),
    critical: Some(90.0),
};
/// Memory use turns amber and red here (percent).
pub const MEMORY_THRESHOLDS: Thresholds = Thresholds {
    warning: Some(80.0),
    critical: Some(92.0),
};
/// A percent counter turns amber and red here.
pub const COUNTER_THRESHOLDS: Thresholds = Thresholds {
    warning: Some(75.0),
    critical: Some(90.0),
};
/// Used space of a volume turns amber and red here (percent).
pub const DISK_THRESHOLDS: Thresholds = Thresholds {
    warning: Some(85.0),
    critical: Some(95.0),
};

/// A page of the app a widget can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum Page {
    /// The Overview.
    Overview,
    /// Alarms.
    Alarms,
    /// The disk map.
    Disk,
    /// Network.
    Network,
    /// Issues.
    Issues,
    /// Backups.
    Backups,
    /// Performance counters.
    Counters,
    /// Processes.
    Processes,
}

/// What a widget's button does: a closed set, run by the app's `run_widget_action`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum WidgetActionId {
    /// Open a page in the main window.
    OpenPage(Page),
    /// Ask for this widget's value again.
    Retry(PlacementId),
    /// Open Settings at the popup widgets to choose another volume.
    ChooseVolume,
    /// Open the counters page to pin counters.
    PinCounters,
    /// Open the backups page to set up checks.
    SetUpBackupChecks,
    /// Start the backup check job.
    StartBackupCheck,
    /// Restart the agent (the app's supervisor, never through the agent).
    RestartAgent,
    /// Open Settings at Menu bar > Popup widgets.
    CustomizeWidgets,
    /// Open Settings.
    OpenSettings,
}

/// Where in Settings a widget's button leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum SettingsSection {
    /// Menu bar > Popup widgets.
    PopupWidgets,
    /// The top of Dyno's page.
    General,
}

/// What the main window is asked to open (`dyno://navigate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum Navigate {
    /// A page.
    Page(Page),
    /// A section of Settings.
    Settings(SettingsSection),
}

/// What the app does for a widget's button: the one place the closed set of actions becomes behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Open the main window at a page or a Settings section.
    Navigate(Navigate),
    /// Ask for the popup's slow values again.
    Refresh,
    /// Start the backup check job.
    StartBackupCheck,
    /// Restart the agent through the app's supervisor.
    RestartAgent,
}

impl WidgetActionId {
    /// What running this action does.
    pub fn effect(&self) -> Effect {
        match self {
            WidgetActionId::OpenPage(p) => Effect::Navigate(Navigate::Page(*p)),
            WidgetActionId::ChooseVolume | WidgetActionId::CustomizeWidgets => {
                Effect::Navigate(Navigate::Settings(SettingsSection::PopupWidgets))
            }
            WidgetActionId::OpenSettings => Effect::Navigate(Navigate::Settings(SettingsSection::General)),
            WidgetActionId::PinCounters => Effect::Navigate(Navigate::Page(Page::Counters)),
            WidgetActionId::SetUpBackupChecks => Effect::Navigate(Navigate::Page(Page::Backups)),
            WidgetActionId::Retry(_) => Effect::Refresh,
            WidgetActionId::StartBackupCheck => Effect::StartBackupCheck,
            WidgetActionId::RestartAgent => Effect::RestartAgent,
        }
    }
}

/// The agent's popup widget value types.
type State = WidgetState<PopoverValue, WidgetActionId>;
/// A widget's state by placement.
pub type PopoverState = PlacementState<PopoverValue, WidgetActionId>;
/// All of the popup's values, with the surface notice.
pub type PopoverSurface = SurfaceValues<PopoverValue, WidgetActionId>;

/// What the popup's one notice is about. The UI reads this, never the notice's text or action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum PopoverNoticeKind {
    /// The agent is not running: nothing is being measured.
    AgentStopped,
}

/// Everything the popup shows: the notice and its kind, and each placement's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PopoverSurfaceValues {
    /// What the notice is about; none when there is no notice.
    pub notice_kind: Option<PopoverNoticeKind>,
    /// The notice and the placements.
    #[serde(flatten)]
    pub surface: PopoverSurface,
}

impl std::ops::Deref for PopoverSurfaceValues {
    type Target = PopoverSurface;
    fn deref(&self) -> &PopoverSurface {
        &self.surface
    }
}

/// How serious an alarm or issue is (always drawn with its word).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Worth knowing.
    Info,
    /// Should be fixed.
    Warning,
    /// Fix it now.
    Critical,
}

impl From<Severity> for Level {
    fn from(s: Severity) -> Level {
        match s {
            Severity::Info => Level::Info,
            Severity::Warning => Level::Warning,
            Severity::Critical => Level::Critical,
        }
    }
}

/// One alarm row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AlarmRow {
    /// The alarm's id.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub id: u64,
    /// One line.
    pub title: String,
    /// How bad.
    pub level: Level,
    /// When the condition first appeared, ms since the epoch.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub since: i64,
    /// Whether someone acknowledged it.
    pub acknowledged: bool,
}

/// One volume row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct VolumeRow {
    /// The volume's id (its mount point).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Used space, percent; none when the volume cannot say.
    pub used_percent: Option<f32>,
    /// Free bytes; none when the volume cannot say.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number | null"))]
    pub free_bytes: Option<u64>,
    /// Capacity in bytes; none when the volume cannot say.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number | null"))]
    pub total_bytes: Option<u64>,
    /// Against the disk thresholds.
    pub tone: ValueTone,
    /// False for a network volume that did not answer ("Not answering").
    pub reachable: bool,
}

/// One pinned counter row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CounterRow {
    /// The counter's path.
    pub path: String,
    /// Its name: the part after the last backslash.
    pub name: String,
    /// The reading, or none ("Unavailable").
    pub value: Option<f64>,
    /// What the value measures.
    pub unit: CounterUnit,
    /// 0 to 100 for a percent counter, else none.
    pub percent: Option<f32>,
    /// Against [`COUNTER_THRESHOLDS`] for a percent counter; `ok` otherwise.
    pub tone: ValueTone,
}

/// One busy app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppRow {
    /// The app's friendly name.
    pub name: String,
    /// Its CPU, percent of the machine; none when it could not be read.
    pub cpu_percent: Option<f32>,
}

/// How the newest backup tool looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BackupValue {
    /// The tool's name ("Time Machine").
    pub name: String,
    /// Its verdict.
    pub state: BackupVerdict,
    /// When the last backup finished.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number | null"))]
    pub last_backup: Option<i64>,
    /// One plain sentence.
    pub summary: String,
    /// How many tools are set up.
    pub tools: u32,
}

/// A backup tool's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum BackupVerdict {
    /// The last backup is recent.
    Healthy,
    /// The last backup is older than the limit.
    Stale,
    /// Set up, but no backup yet.
    NoBackupYet,
}

/// The newest issue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct IssueValue {
    /// One line.
    pub title: String,
    /// The application or device it concerns; empty when none applies.
    pub app: String,
    /// The kind's name ("App crash").
    pub kind: String,
    /// How serious.
    pub level: Level,
    /// When it happened, ms since the epoch.
    #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
    pub at: i64,
    /// How many days back the widget looks ([`ISSUE_DAYS`]); the UI says "last N days" from it.
    pub days: u32,
}

/// What one popup widget shows, by kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "ts"), derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PopoverValue {
    /// The open alarms, most severe then newest.
    ActiveAlarms {
        /// The first rows.
        rows: Vec<AlarmRow>,
        /// How many more there are.
        more: u32,
    },
    /// Volumes and their space.
    DiskSpace {
        /// The first rows.
        rows: Vec<VolumeRow>,
        /// How many more there are.
        more: u32,
    },
    /// CPU load and memory use.
    CpuMemory {
        /// CPU load, 0 to 100.
        cpu_percent: f32,
        /// Against the CPU thresholds.
        cpu_tone: ValueTone,
        /// Memory in use, bytes.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        memory_used: u64,
        /// Memory installed, bytes.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        memory_total: u64,
        /// Memory in use, percent.
        memory_percent: f32,
        /// Against the memory thresholds.
        memory_tone: ValueTone,
        /// CPU load over the last minute, oldest first.
        cpu_minute: Vec<f32>,
    },
    /// Network traffic.
    Network {
        /// Download, bytes per second.
        rx_bps: f64,
        /// Upload, bytes per second.
        tx_bps: f64,
        /// Download over the last minute, oldest first.
        rx_minute: Vec<f32>,
        /// Upload over the last minute, oldest first.
        tx_minute: Vec<f32>,
    },
    /// The backup check.
    BackupHealth(BackupValue),
    /// The newest issue.
    LastIssue(IssueValue),
    /// The pinned counters.
    PinnedCounters {
        /// The first rows.
        rows: Vec<CounterRow>,
        /// How many more are pinned.
        more: u32,
    },
    /// The busiest apps.
    TopApps {
        /// The rows.
        rows: Vec<AppRow>,
    },
}

/// What one source reports: not yet (the widget is loading), an error, or its data.
#[derive(Debug, Clone, PartialEq)]
pub enum Source<T> {
    /// No answer yet: the widget has no state and the UI shows it loading.
    NotYet,
    /// The source failed; the string is the cause in plain words.
    Failed(String),
    /// The data.
    Ready(T),
}

impl<T> Source<T> {
    /// The same source over a borrow of its data.
    pub fn as_ref(&self) -> Source<&T> {
        match self {
            Source::NotYet => Source::NotYet,
            Source::Failed(c) => Source::Failed(c.clone()),
            Source::Ready(v) => Source::Ready(v),
        }
    }
}

impl<T> Source<Vec<T>> {
    /// The same source over a borrowed slice.
    pub fn as_slice(&self) -> Source<&[T]> {
        match self {
            Source::NotYet => Source::NotYet,
            Source::Failed(c) => Source::Failed(c.clone()),
            Source::Ready(v) => Source::Ready(v.as_slice()),
        }
    }
}

/// The last minute of live samples, oldest first, for the sparklines. The app pushes every live tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Minute {
    cpu: std::collections::VecDeque<f32>,
    rx: std::collections::VecDeque<f32>,
    tx: std::collections::VecDeque<f32>,
}

impl Minute {
    /// Adds one live sample, dropping what is more than [`MINUTE_SAMPLES`] old.
    pub fn push(&mut self, update: &LiveUpdate) {
        let (rx, tx) = update.system.network_totals();
        for (q, v) in [
            (&mut self.cpu, update.system.cpu.total),
            (&mut self.rx, rx as f32),
            (&mut self.tx, tx as f32),
        ] {
            q.push_back(if v.is_finite() { v.max(0.0) } else { 0.0 });
            while q.len() > MINUTE_SAMPLES {
                q.pop_front();
            }
        }
    }

    /// Samples held.
    pub fn len(&self) -> usize {
        self.cpu.len()
    }

    /// Whether no sample was pushed.
    pub fn is_empty(&self) -> bool {
        self.cpu.is_empty()
    }
}

/// The agent's volumes, backup report, alarms, newest issue and pinned counters, and the app's live push.
pub struct Inputs<'a> {
    /// Now, ms since the epoch.
    pub now: Timestamp,
    /// The live push (the app).
    pub live: Source<&'a LiveUpdate>,
    /// The last minute of live samples (the app).
    pub minute: &'a Minute,
    /// The alarms the agent keeps.
    pub alarms: Source<&'a [Alarm]>,
    /// The mounted volumes (cached by the agent).
    pub volumes: Source<&'a [Volume]>,
    /// The backup check's last report.
    pub backup: Source<BackupInput<'a>>,
    /// The issues of the last [`ISSUE_DAYS`] days, newest first, and how far a reading of the logs has come.
    pub issues: Source<IssuesInput<'a>>,
    /// The pinned counters' latest readings and how many are pinned.
    pub counters: Source<CountersInput<'a>>,
}

/// The backup report as the backup widget needs it.
#[derive(Debug, Clone)]
pub struct BackupInput<'a> {
    /// The last report.
    pub report: &'a BackupReport,
    /// How far the check job has come, while one runs (its own phase and count; nothing is invented when there is none).
    pub job: Option<JobProgress>,
}

/// A running job's phase, as its own progress reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobProgress {
    /// The phase's label ("Time Machine").
    pub phase: String,
    /// What has been done so far in this phase.
    pub done: u64,
    /// What the count counts ("items").
    pub unit: String,
}

/// The issue index as the last-issue widget needs it.
#[derive(Debug, Clone, Copy)]
pub struct IssuesInput<'a> {
    /// The issues of the last week, any order.
    pub recent: &'a [Issue],
    /// Events read so far while the logs are being read (the issue job is running), else none.
    pub reading: Option<u64>,
    /// When the logs were last read; the value is as old as that.
    pub scanned_at: Option<Timestamp>,
}

/// The pinned counters as the widget needs them.
#[derive(Debug, Clone, Copy)]
pub struct CountersInput<'a> {
    /// How many counters are pinned.
    pub pinned: usize,
    /// The latest readings of the first pinned ones.
    pub readings: &'a [CounterReading],
    /// When the newest of them was read.
    pub read_at: Option<Timestamp>,
}

impl<'a> Inputs<'a> {
    /// Inputs where nothing has reported yet.
    pub fn empty(now: Timestamp, minute: &'a Minute) -> Inputs<'a> {
        Inputs {
            now,
            live: Source::NotYet,
            minute,
            alarms: Source::NotYet,
            volumes: Source::NotYet,
            backup: Source::NotYet,
            issues: Source::NotYet,
            counters: Source::NotYet,
        }
    }
}

/// Whether the app builds a kind from its live push (true) or asks the agent for it (`PopoverValues`).
pub fn is_live_push(kind: PopoverKind) -> bool {
    matches!(kind, PopoverKind::CpuMemory | PopoverKind::Network | PopoverKind::TopApps)
}

fn action(action: WidgetActionId, label: &str) -> Option<WidgetAction<WidgetActionId>> {
    Some(WidgetAction {
        action,
        label: label.into(),
    })
}

fn ready(value: PopoverValue, now: Timestamp) -> Option<State> {
    Some(WidgetState::Ready {
        value,
        as_of: now.millis(),
        stale_since: None,
    })
}

fn error(title: &str, cause: &str, id: &PlacementId) -> Option<State> {
    Some(WidgetState::Error {
        title: title.into(),
        cause: (!cause.is_empty()).then(|| cause.to_string()),
        next: action(WidgetActionId::Retry(id.clone()), "Retry"),
    })
}

/// The state of one popup widget, or `None` while its source has not answered (the UI shows it loading).
pub fn state(widget: &PopoverWidget, id: &PlacementId, inputs: &Inputs<'_>) -> Option<State> {
    match widget {
        PopoverWidget::ActiveAlarms => alarms(id, inputs),
        PopoverWidget::DiskSpace { volume } => disk_space(volume, id, inputs),
        PopoverWidget::CpuMemory => cpu_memory(inputs),
        PopoverWidget::Network => network(inputs),
        PopoverWidget::BackupHealth => backup(inputs),
        PopoverWidget::LastIssue => last_issue(id, inputs),
        PopoverWidget::PinnedCounters => counters(id, inputs),
        PopoverWidget::TopApps => top_apps(inputs),
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn alarms(id: &PlacementId, inputs: &Inputs<'_>) -> Option<State> {
    let list = match inputs.alarms.as_ref() {
        Source::NotYet => return None,
        Source::Failed(_) => return error("Couldn't read alarms", "The alarm store didn't answer.", id),
        Source::Ready(l) => l,
    };
    let mut open: Vec<&Alarm> = list
        .iter()
        .filter(|a| matches!(a.state, AlarmState::Active | AlarmState::Acknowledged) && !a.is_shelved(inputs.now))
        .collect();
    if open.is_empty() {
        return Some(WidgetState::Empty {
            title: "No active alarms".into(),
            calm: true,
            detail: "Nothing needs your attention.".into(),
            next: action(WidgetActionId::OpenPage(Page::Alarms), "Open alarms"),
        });
    }
    // Most severe first, then newest (last seen), then the lowest id so the order never flickers.
    open.sort_by(|a, b| {
        Level::from(b.severity)
            .cmp(&Level::from(a.severity))
            .then(b.last_seen.cmp(&a.last_seen))
            .then(a.id.cmp(&b.id))
    });
    let more = count(open.len().saturating_sub(ALARM_ROWS));
    let rows = open
        .into_iter()
        .take(ALARM_ROWS)
        .map(|a| AlarmRow {
            id: a.id,
            title: a.title.clone(),
            level: a.severity.into(),
            since: a.first_seen.millis(),
            acknowledged: a.state == AlarmState::Acknowledged,
        })
        .collect();
    ready(PopoverValue::ActiveAlarms { rows, more }, inputs.now)
}

/// The volume Windows and macOS call the system one: `C:` or `/`; failing that the first internal volume.
pub fn system_volume(volumes: &[Volume]) -> Option<&Volume> {
    let by_mount = |v: &&Volume| {
        let m = v.mount.trim_end_matches(['\\', '/']);
        m.is_empty() || m.eq_ignore_ascii_case("C:")
    };
    volumes
        .iter()
        .find(by_mount)
        .or_else(|| volumes.iter().find(|v| v.location == VolumeLocation::Internal))
}

fn volume_row(v: &Volume) -> VolumeRow {
    let used_percent = match (v.capacity, v.used()) {
        (Some(total), Some(used)) if total > 0 => Some((used as f64 / total as f64 * 100.0) as f32),
        _ => None,
    };
    VolumeRow {
        id: v.id.clone(),
        name: v.name.clone(),
        used_percent,
        free_bytes: v.free,
        total_bytes: v.capacity,
        tone: DISK_THRESHOLDS.tone(used_percent.map(f64::from)),
        reachable: v.reachable,
    }
}

fn location_rank(l: &VolumeLocation) -> u8 {
    match l {
        VolumeLocation::Internal => 0,
        VolumeLocation::External { .. } => 1,
        VolumeLocation::Network { .. } => 2,
    }
}

fn disk_space(choice: &VolumeChoice, id: &PlacementId, inputs: &Inputs<'_>) -> Option<State> {
    let volumes = match inputs.volumes.as_ref() {
        Source::NotYet => return None,
        Source::Failed(_) => return error("Couldn't read volumes", "The volume list didn't answer.", id),
        Source::Ready(v) => v,
    };
    let chosen: Vec<&Volume> = match choice {
        VolumeChoice::All => {
            let mut all: Vec<&Volume> = volumes.iter().collect();
            // The system volume first, then internal, external and network; a stable order inside each.
            let system = system_volume(volumes).map(|v| v.id.clone());
            all.sort_by_key(|v| (system.as_deref() != Some(v.id.as_str()), location_rank(&v.location)));
            all
        }
        VolumeChoice::System => system_volume(volumes).into_iter().collect(),
        VolumeChoice::Mount(want) => volumes.iter().filter(|v| &v.id == want).collect(),
    };
    if chosen.is_empty() {
        let (title, detail) = match choice {
            VolumeChoice::Mount(want) => (
                format!("See {}'s space when it's connected", short_name(want)),
                "The disk isn't connected now.",
            ),
            _ => (
                "See your disk space when a volume is available".to_string(),
                "No volume was reported.",
            ),
        };
        return Some(WidgetState::Empty {
            title,
            calm: false,
            detail: detail.into(),
            next: action(WidgetActionId::ChooseVolume, "Choose another volume"),
        });
    }
    let more = count(chosen.len().saturating_sub(VOLUME_ROWS));
    let rows = chosen.into_iter().take(VOLUME_ROWS).map(volume_row).collect();
    ready(PopoverValue::DiskSpace { rows, more }, inputs.now)
}

/// The last part of a mount point or drive: "Backup" for "/Volumes/Backup".
fn short_name(id: &str) -> String {
    let t = id.trim_end_matches(['\\', '/']);
    t.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()).unwrap_or(id).to_string()
}

fn cpu_memory(inputs: &Inputs<'_>) -> Option<State> {
    let Source::Ready(live) = inputs.live.as_ref() else { return None };
    let s = &live.system;
    let memory_percent = s.memory_percent();
    ready(
        PopoverValue::CpuMemory {
            cpu_percent: s.cpu.total,
            cpu_tone: CPU_THRESHOLDS.tone(Some(f64::from(s.cpu.total))),
            memory_used: s.memory.used,
            memory_total: s.memory.total,
            memory_percent,
            memory_tone: MEMORY_THRESHOLDS.tone(Some(f64::from(memory_percent))),
            cpu_minute: inputs.minute.cpu.iter().copied().collect(),
        },
        inputs.now,
    )
}

fn network(inputs: &Inputs<'_>) -> Option<State> {
    let Source::Ready(live) = inputs.live.as_ref() else { return None };
    if live.system.network.is_empty() {
        return Some(WidgetState::Empty {
            title: "See traffic when online".into(),
            calm: false,
            detail: "Offline now.".into(),
            next: action(WidgetActionId::OpenPage(Page::Network), "Open network"),
        });
    }
    let (rx_bps, tx_bps) = live.system.network_totals();
    ready(
        PopoverValue::Network {
            rx_bps,
            tx_bps,
            rx_minute: inputs.minute.rx.iter().copied().collect(),
            tx_minute: inputs.minute.tx.iter().copied().collect(),
        },
        inputs.now,
    )
}

fn top_apps(inputs: &Inputs<'_>) -> Option<State> {
    let Source::Ready(live) = inputs.live.as_ref() else { return None };
    let rows = live
        .top_cpu
        .iter()
        .take(APP_ROWS)
        .map(|g| AppRow {
            name: g.name.clone(),
            cpu_percent: g.cpu.copied(),
        })
        .collect();
    ready(PopoverValue::TopApps { rows }, inputs.now)
}

fn backup(inputs: &Inputs<'_>) -> Option<State> {
    let (report, job) = match inputs.backup.as_ref() {
        Source::NotYet => return None,
        Source::Failed(cause) => {
            return Some(WidgetState::Error {
                title: "Couldn't check".into(),
                cause: Some(cause),
                next: action(WidgetActionId::StartBackupCheck, "Retry"),
            })
        }
        Source::Ready(i) => (i.report, i.job.clone()),
    };
    let as_of = report.checked_at.unwrap_or(inputs.now);
    let configured: Vec<_> = report.statuses.iter().filter(|s| s.state != BackupState::NotConfigured).collect();
    // The verdict of the tool that most needs attention: stale, then no backup yet, then healthy.
    let rank = |s: BackupState| match s {
        BackupState::Stale => 3,
        BackupState::NoBackupYet => 2,
        BackupState::Healthy => 1,
        _ => 0,
    };
    let worst = configured
        .iter()
        .filter(|s| s.state != BackupState::CannotCheck)
        .max_by_key(|s| rank(s.state));
    let last = worst.map(|w| PopoverValue::BackupHealth(backup_value(w, configured.len())));
    if report.checking {
        return match job {
            // Real progress only: the running job's own phase and count (ER-18). Without one the last value stays, or the
            // widget is still loading; no count is made up.
            Some(j) => Some(WidgetState::Working {
                // Always short: one line in the half-width frame; the body is only the count. The tool's name shows in the
                // widget's normal state.
                title: "Checking backup".into(),
                phase: String::new(),
                progress: Progress::Count {
                    done: j.done,
                    unit: j.unit,
                },
                last,
                cancel: None,
            }),
            None => last.and_then(|v| ready(v, as_of)),
        };
    }
    if configured.is_empty() {
        return Some(WidgetState::Empty {
            title: "Know when backups ran".into(),
            calm: false,
            detail: "No backup set up.".into(),
            next: action(WidgetActionId::SetUpBackupChecks, "Set up checks"),
        });
    }
    match last {
        Some(v) => ready(v, as_of),
        None => Some(WidgetState::Error {
            title: "Couldn't check".into(),
            cause: configured.first().map(|s| s.summary.clone()),
            next: action(WidgetActionId::StartBackupCheck, "Retry"),
        }),
    }
}

fn backup_value(s: &crate::backup::BackupStatus, tools: usize) -> BackupValue {
    BackupValue {
        name: s.name.clone(),
        state: match s.state {
            BackupState::Stale => BackupVerdict::Stale,
            BackupState::NoBackupYet => BackupVerdict::NoBackupYet,
            _ => BackupVerdict::Healthy,
        },
        last_backup: s.last_backup.map(Timestamp::millis),
        summary: s.summary.clone(),
        tools: count(tools),
    }
}

fn last_issue(id: &PlacementId, inputs: &Inputs<'_>) -> Option<State> {
    let issues = match inputs.issues.as_ref() {
        Source::NotYet => return None,
        Source::Failed(_) => return error("Couldn't read issues", "The issue index is unavailable.", id),
        Source::Ready(i) => i,
    };
    let newest = issues
        .recent
        .iter()
        .filter(|i| inputs.now.since(i.at) <= ISSUE_DAYS * DAY)
        .max_by(|a, b| a.at.cmp(&b.at).then_with(|| b.id.cmp(&a.id)));
    let last = newest.map(|i| {
        PopoverValue::LastIssue(IssueValue {
            title: i.title.clone(),
            app: i.app.clone(),
            kind: i.kind.label().into(),
            level: i.severity.into(),
            at: i.at.millis(),
            days: ISSUE_DAYS as u32,
        })
    });
    if let Some(done) = issues.reading {
        return Some(WidgetState::Working {
            title: "Last issue".into(),
            phase: "Reading system logs · Indexing".into(),
            progress: Progress::Count {
                done,
                unit: "events".into(),
            },
            last,
            cancel: None,
        });
    }
    match last {
        Some(value) => ready(value, issues.scanned_at.unwrap_or(inputs.now)),
        None => Some(WidgetState::Empty {
            title: "No issues in the last 7 days".into(),
            calm: true,
            detail: "Dyno reads the system logs for crashes and hangs.".into(),
            next: action(WidgetActionId::OpenPage(Page::Issues), "Open issues"),
        }),
    }
}

fn counters(id: &PlacementId, inputs: &Inputs<'_>) -> Option<State> {
    let c = match inputs.counters.as_ref() {
        Source::NotYet => return None,
        Source::Failed(_) => return error("Couldn't read counters", "The counters didn't answer.", id),
        Source::Ready(c) => c,
    };
    if c.pinned == 0 {
        return Some(WidgetState::Empty {
            title: "Watch your own counters here".into(),
            calm: false,
            detail: format!("Pin counters in Dyno; the first {COUNTER_ROWS} show here."),
            next: action(WidgetActionId::PinCounters, "Pin counters"),
        });
    }
    let rows: Vec<CounterRow> = c
        .readings
        .iter()
        .take(COUNTER_ROWS)
        .map(|r| CounterRow {
            // The percent and the tone are the reading's own (worked out once, in `CounterReading::new`).
            percent: r.percent,
            tone: r.tone,
            path: r.path.clone(),
            name: r.path.rsplit('\\').next().filter(|s| !s.is_empty()).unwrap_or(&r.path).to_string(),
            value: r.value.value().copied().filter(|v| v.is_finite()),
            unit: r.unit,
        })
        .collect();
    if !rows.is_empty() && rows.iter().all(|r| r.value.is_none()) {
        let n = c.pinned.min(COUNTER_ROWS.max(rows.len()));
        let cause = if n == 1 {
            "The pinned counter didn't answer.".to_string()
        } else {
            format!("None of the {n} pinned counters answered.")
        };
        return error("Couldn't read counters", &cause, id);
    }
    ready(
        PopoverValue::PinnedCounters {
            rows,
            more: count(c.pinned.saturating_sub(COUNTER_ROWS)),
        },
        c.read_at.unwrap_or(inputs.now),
    )
}

/// Marks every ready value as stale since the time it was read: the source stopped refreshing it (the agent stopped).
/// Working, empty and error states stay as they are.
pub fn mark_stale(values: &mut [PopoverState]) {
    for p in values {
        if let WidgetState::Ready { as_of, stale_since, .. } = &mut p.state {
            stale_since.get_or_insert(*as_of);
        }
    }
}

/// The notice for the whole surface when the app lost the agent: "Agent stopped", with Restart agent. `paused_at` is
/// the time of the last reading, `zone` the person's.
pub fn agent_stopped(paused_at: Option<Timestamp>, zone: &dyn crate::time::LocalZone) -> SurfaceNotice<WidgetActionId> {
    let detail = match paused_at {
        Some(t) => {
            let m = crate::time::local_minute(t, zone);
            format!("Readings paused at {:02}:{:02}", m / 60, m % 60)
        }
        None => "No readings yet".to_string(),
    };
    SurfaceNotice {
        tone: Tone::Danger,
        title: "Agent stopped".into(),
        detail,
        action: action(WidgetActionId::RestartAgent, "Restart agent"),
    }
}

/// The popup's values from states the caller collected: the live kinds from the app's push, the slow ones from the
/// agent's last answer, in the layout's order. A placement with no state yet is left out (the UI shows it loading).
pub fn surface_values(
    notice: Option<(PopoverNoticeKind, SurfaceNotice<WidgetActionId>)>,
    placements: Vec<PopoverState>,
) -> PopoverSurfaceValues {
    let (notice_kind, notice) = match notice {
        Some((kind, n)) => (Some(kind), Some(n)),
        None => (None, None),
    };
    PopoverSurfaceValues {
        notice_kind,
        surface: SurfaceValues { notice, placements },
    }
}

#[cfg(test)]
mod tests;
