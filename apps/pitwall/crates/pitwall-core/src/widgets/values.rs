//! What the popup's widgets show, and the actions they offer. Plain data: [`super::build`] computes it, the UI only draws it
//! (AGENTS.md "Where logic lives"). Tones, counts, states and the "nothing to do" decisions are made there.

use paddock_widgets::{PlacementState, SurfaceValues, WidgetState};
use serde::{Deserialize, Serialize};

/// Whether the server answers requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ServerState {
    /// Serving requests.
    Serving,
    /// New requests are refused on purpose.
    Paused,
    /// Quit with "stop serving": nothing is loaded and requests are refused until Pitwall is opened again.
    Stopped,
    /// The app cannot reach the server.
    Unreachable,
}

/// How the engines are doing, the most serious thing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum EngineHealth {
    /// Every engine is usable and none crashed.
    Ready,
    /// The engine is being installed.
    Installing,
    /// No engine build is installed, or the install failed.
    Missing,
    /// Engines of some models crashed and are being restarted.
    Restarting {
        /// How many models.
        count: u32,
    },
    /// Engines of some models kept crashing: no more automatic restarts.
    Failed {
        /// How many models.
        count: u32,
    },
}

/// A meter's tone, decided by the engine from the share in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum MeterTone {
    /// Plenty left.
    Neutral,
    /// Getting full.
    Warn,
    /// Nearly full.
    Bad,
}

/// The operating system's memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum MemoryLevel {
    /// Fine.
    Normal,
    /// Below the threshold or the OS warns.
    Elevated,
    /// The OS reports critical pressure.
    Critical,
}

/// Video memory of the first GPU that reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct VramReading {
    /// Bytes in use.
    #[ts(type = "number")]
    pub used: u64,
    /// Bytes in all.
    #[ts(type = "number")]
    pub total: u64,
    /// The share in use, 0 to 100.
    pub percent: u8,
    /// How busy the GPU is, 0 to 100, when it reports it.
    pub busy_percent: Option<u8>,
    /// The meter's tone.
    pub tone: MeterTone,
}

/// A loaded model's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelRowState {
    /// Loaded and idle.
    Ready,
    /// Answering requests.
    Busy,
    /// Loading.
    Loading,
    /// Giving back memory (its context or the whole model).
    Releasing,
    /// Its engine failed.
    Failed,
}

/// One model in Loaded models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    /// The model id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Memory it holds (RAM and video memory), in bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    /// What it is doing.
    pub state: ModelRowState,
    /// Kept loaded.
    pub pinned: bool,
    /// Requests it is answering.
    pub active: u32,
}

/// Where a download that is not running stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum DownloadRowState {
    /// Paused; it resumes where it stopped.
    Paused,
    /// Failed.
    Failed,
}

/// One download in Downloads that is not running (paused or failed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    /// The download id.
    pub id: String,
    /// What is downloaded.
    pub name: String,
    /// Paused or failed.
    pub state: DownloadRowState,
    /// Why it failed, when it did.
    pub message: Option<String>,
}

/// One error in Recent errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ErrorRow {
    /// A stable id (the event's).
    pub id: String,
    /// One sentence (never request content).
    pub message: String,
    /// When, in milliseconds since the epoch.
    #[ts(type = "number")]
    pub at_ms: i64,
}

/// How the local AI is doing overall, for the popup's header pill (the engine decides; the UI words nothing else).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PopoverStatus {
    /// Nothing known yet.
    Unknown,
    /// The server cannot be reached.
    Unreachable,
    /// Serving was stopped on quit.
    Stopped,
    /// Serving is paused.
    Paused,
    /// Engines have problems.
    Problems {
        /// How many models' engines crashed.
        count: u32,
    },
    /// Serving.
    Serving {
        /// The loopback port.
        port: u16,
    },
}

/// What the footer's Pause and Resume item does now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ServingToggle {
    /// Serving: the item pauses it.
    Pause,
    /// Paused or stopped: the item resumes it.
    Resume,
    /// The server cannot be reached: the item is off.
    Off,
}

/// The popup's header and footer menu, as the engine decides them: the status pill and which menu items work now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PopoverHeader {
    /// How the local AI is doing overall.
    pub status: PopoverStatus,
    /// What Pause serving or Resume serving does.
    pub serving: ServingToggle,
    /// Whether Release memory has anything to release.
    pub can_release_memory: bool,
}

/// A page of the main window a widget can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Page {
    /// The dashboard.
    Dashboard,
    /// Models, with the library and downloads.
    Models,
    /// Engines.
    Engines,
    /// Requests, with the errors.
    Requests,
}

/// What one widget shows. Rows are capped; `more` counts the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PopoverValue {
    /// The server.
    Server {
        /// Whether it answers.
        state: ServerState,
        /// The loopback port; zero when unknown.
        port: u16,
        /// The engines; none while nothing is known.
        engine: Option<EngineHealth>,
    },
    /// Chats and the queue.
    Activity {
        /// Requests running.
        running: u32,
        /// Requests waiting.
        waiting: u32,
        /// Generated tokens per second, rounded; zero while serving is paused.
        tokens_per_sec: u32,
    },
    /// Memory.
    Memory {
        /// RAM in use, in bytes.
        #[ts(type = "number")]
        used: u64,
        /// RAM in all, in bytes.
        #[ts(type = "number")]
        total: u64,
        /// The share in use, 0 to 100.
        percent: u8,
        /// The meter's tone.
        tone: MeterTone,
        /// The OS's pressure.
        pressure: MemoryLevel,
        /// Video memory, when a GPU reports it.
        vram: Option<VramReading>,
    },
    /// The loaded models.
    LoadedModels {
        /// All loaded models.
        total: u32,
        /// The busiest first, then the most recently used.
        rows: Vec<ModelRow>,
        /// Models not listed.
        more: u32,
        /// Memory all of them hold, in bytes.
        #[ts(type = "number")]
        bytes: u64,
    },
    /// Downloads that are not running (paused or failed); running ones are the working state.
    Downloads {
        /// The paused and failed downloads, failed first.
        rows: Vec<DownloadRow>,
        /// Not listed.
        more: u32,
    },
    /// Model updates on offer.
    Updates {
        /// How many.
        count: u32,
        /// The first one's sentence.
        summary: Option<String>,
    },
    /// The latest errors, newest first.
    RecentErrors {
        /// The listed errors.
        rows: Vec<ErrorRow>,
        /// Errors of the last day not listed.
        more: u32,
    },
}

/// What a widget can ask the app to do. A closed set: the app runs each through an existing command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum WidgetActionId {
    /// Show a page of the main window.
    OpenPage {
        /// Which.
        page: Page,
    },
    /// Cancel one download.
    CancelDownload {
        /// The download id.
        id: String,
    },
}

/// One widget's state in the popup.
pub type PopoverState = WidgetState<PopoverValue, WidgetActionId>;
/// All of the popup's widget values.
pub type PopoverValues = SurfaceValues<PopoverValue, WidgetActionId>;
/// A placement's state in the popup.
pub type PopoverPlacementState = PlacementState<PopoverValue, WidgetActionId>;
