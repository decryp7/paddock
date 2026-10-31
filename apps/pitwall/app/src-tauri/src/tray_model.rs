//! What the tray / menu bar shows, as a pure model fed by the live feed: loaded models, memory, queue, tokens per
//! second and whether serving is paused. [`TrayModel::view`] turns it into the icon state, the tooltip and the menu
//! entries; `tray.rs` only draws them. Nothing here touches Tauri.

use pitwall_core::mgmt::{LiveUpdate, Overview, ServingState};

/// The menu actions the tray offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Show or create the window.
    Open,
    /// Pause or resume serving.
    ToggleServing,
    /// Unload every model that may be unloaded.
    ReleaseMemory,
    /// Quit the app; what happens to serving follows the setting "When you quit Pitwall".
    Quit,
    /// Quit the app and stop serving (models unloaded, engines ended, paused until Pitwall is opened again).
    QuitAndStop,
}

impl TrayAction {
    /// The menu item id.
    pub const fn id(self) -> &'static str {
        match self {
            TrayAction::Open => "open",
            TrayAction::ToggleServing => "toggle_serving",
            TrayAction::ReleaseMemory => "release_memory",
            TrayAction::Quit => "quit",
            TrayAction::QuitAndStop => "quit_and_stop",
        }
    }

    /// The action of a menu item id.
    pub fn parse(id: &str) -> Option<TrayAction> {
        [
            TrayAction::Open,
            TrayAction::ToggleServing,
            TrayAction::ReleaseMemory,
            TrayAction::Quit,
            TrayAction::QuitAndStop,
        ]
        .into_iter()
        .find(|a| a.id() == id)
    }
}

/// The icon the tray shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// The server is up and idle.
    Idle,
    /// Requests are running or queued.
    Busy,
    /// Serving is paused on purpose.
    Paused,
    /// The server cannot be reached.
    Unavailable,
}

/// One line of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A disabled line of status text.
    Status(String),
    /// A separator.
    Separator,
    /// A clickable item.
    Item {
        /// What it does.
        action: TrayAction,
        /// Its label.
        label: String,
        /// Whether it can be chosen now.
        enabled: bool,
    },
}

/// Everything the tray draws; two equal views need no redraw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayView {
    /// The icon.
    pub state: TrayState,
    /// The tooltip.
    pub tooltip: String,
    /// The menu.
    pub entries: Vec<Entry>,
}

/// The facts the tray needs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrayModel {
    connected: bool,
    paused: bool,
    models: Vec<(String, u64)>,
    running: usize,
    waiting: usize,
    gen_tps: f64,
    pairings: usize,
}

/// `4.2 GB` / `512 MB` / `0 MB`.
pub fn bytes_text(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

impl TrayModel {
    /// A model of a server that has not been reached yet.
    pub fn new() -> TrayModel {
        TrayModel::default()
    }

    /// Marks the server reachable or not; an unreachable server shows nothing stale.
    pub fn set_connected(&mut self, connected: bool) {
        if !connected {
            *self = TrayModel::default();
        }
        self.connected = connected;
    }

    /// Starts from a full snapshot (after every (re)connect).
    pub fn seed(&mut self, o: &Overview) {
        self.connected = true;
        self.paused = o.server.serving != ServingState::Serving;
        self.models = o.models.iter().map(|m| (m.name.clone(), m.ram_bytes + m.vram_bytes)).collect();
        self.running = o.queue.running.len();
        self.waiting = o.queue.waiting.len();
        self.gen_tps = o.throughput.gen_tps;
        self.pairings = o.pairings.len();
    }

    /// Applies one pushed update.
    pub fn apply(&mut self, u: &LiveUpdate) {
        self.connected = true;
        match u {
            LiveUpdate::Models { models } => self.models = models.iter().map(|m| (m.name.clone(), m.ram_bytes + m.vram_bytes)).collect(),
            LiveUpdate::Queue { queue } => {
                self.running = queue.running.len();
                self.waiting = queue.waiting.len();
            }
            LiveUpdate::Throughput { throughput } => self.gen_tps = throughput.gen_tps,
            LiveUpdate::Server { server } => self.paused = server.serving != ServingState::Serving,
            LiveUpdate::Pairing { pairings } => self.pairings = pairings.len(),
            LiveUpdate::Memory { .. }
            | LiveUpdate::Event { .. }
            | LiveUpdate::Download { .. }
            | LiveUpdate::EngineSetup { .. }
            | LiveUpdate::Issues { .. } => {}
        }
    }

    /// Whether serving is paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The icon state.
    pub fn state(&self) -> TrayState {
        if !self.connected {
            TrayState::Unavailable
        } else if self.paused {
            TrayState::Paused
        } else if self.running + self.waiting > 0 {
            TrayState::Busy
        } else {
            TrayState::Idle
        }
    }

    fn memory(&self) -> u64 {
        self.models.iter().map(|(_, b)| *b).sum()
    }

    /// The compact status line: `2 models · 9.4 GB · queue 1 · 38 tok/s`.
    pub fn summary(&self) -> String {
        if !self.connected {
            return "The server is not running".into();
        }
        let mut parts = vec![match self.models.len() {
            0 => "No model loaded".to_string(),
            1 => "1 model".to_string(),
            n => format!("{n} models"),
        }];
        if !self.models.is_empty() {
            parts.push(bytes_text(self.memory()));
        }
        let queued = self.running + self.waiting;
        if queued > 0 {
            parts.push(format!("queue {queued}"));
        }
        if self.gen_tps >= 0.5 {
            parts.push(format!("{:.0} tok/s", self.gen_tps));
        }
        parts.join(" · ")
    }

    /// What to draw.
    pub fn view(&self) -> TrayView {
        let state = self.state();
        let tooltip = match state {
            TrayState::Unavailable => "Pitwall: the server is not running".to_string(),
            TrayState::Paused => format!("Pitwall (paused): {}", self.summary()),
            _ => format!("Pitwall: {}", self.summary()),
        };
        let mut entries = vec![Entry::Status(match state {
            TrayState::Unavailable => "Server not running".to_string(),
            TrayState::Paused => "Serving paused".to_string(),
            TrayState::Busy => "Working".to_string(),
            TrayState::Idle => "Serving".to_string(),
        })];
        if self.connected {
            entries.push(Entry::Status(self.summary()));
            for (name, bytes) in self.models.iter().take(5) {
                entries.push(Entry::Status(format!("  {name}  {}", bytes_text(*bytes))));
            }
            if self.models.len() > 5 {
                entries.push(Entry::Status(format!("  and {} more", self.models.len() - 5)));
            }
            if self.pairings > 0 {
                entries.push(Entry::Status(format!(
                    "{} app{} waiting to pair",
                    self.pairings,
                    if self.pairings == 1 { "" } else { "s" }
                )));
            }
        }
        entries.push(Entry::Separator);
        entries.push(Entry::Item {
            action: TrayAction::Open,
            label: "Open Pitwall".into(),
            enabled: true,
        });
        entries.push(Entry::Item {
            action: TrayAction::ToggleServing,
            label: if self.paused { "Resume serving" } else { "Pause serving" }.into(),
            enabled: self.connected,
        });
        entries.push(Entry::Item {
            action: TrayAction::ReleaseMemory,
            label: "Release memory".into(),
            enabled: self.connected && !self.models.is_empty(),
        });
        entries.push(Entry::Separator);
        entries.push(Entry::Item {
            action: TrayAction::Quit,
            label: "Quit Pitwall".into(),
            enabled: true,
        });
        entries.push(Entry::Item {
            action: TrayAction::QuitAndStop,
            label: "Quit and stop serving".into(),
            enabled: true,
        });
        TrayView { state, tooltip, entries }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_core::mgmt::{Depth, LoadedModel, QueueSnapshot, ServerInfo, Throughput};
    use serde_json::json;

    fn model(name: &str, ram: u64, vram: u64) -> LoadedModel {
        serde_json::from_value(json!({
            "modelId": name, "name": name, "engine": "llama.cpp", "build": "b1", "state": "ready",
            "ramBytes": ram, "vramBytes": vram, "contextTokens": 4096, "pinned": false, "activeRequests": 0,
            "loadedAt": 0, "lastUsedAt": 0, "idleReleaseAt": null, "coldStartMs": 0, "capabilities": []
        }))
        .unwrap()
    }

    fn server(paused: bool) -> ServerInfo {
        serde_json::from_value(json!({
            "version": "0.1.0", "apiVersion": 1, "pid": 1, "port": 11435, "startedAt": 0,
            "serving": if paused { "paused" } else { "serving" },
            "lan": {"enabled": false, "url": null, "certFingerprint": null}
        }))
        .unwrap()
    }

    const GB: u64 = 1024 * 1024 * 1024;

    fn sample() -> pitwall_core::mgmt::MemorySample {
        serde_json::from_value(json!({
            "at": 0, "ramTotal": 16 * GB, "ramAvailable": 8 * GB, "commitUsed": null, "commitLimit": null, "swapUsed": 0,
            "pressure": "normal", "cpuPercent": 3.0, "gpus": [], "pitwallRss": GB, "foregroundBusy": false
        }))
        .unwrap()
    }

    #[test]
    fn a_server_that_was_never_reached_is_unavailable() {
        let m = TrayModel::new();
        assert_eq!(m.state(), TrayState::Unavailable);
        let v = m.view();
        assert_eq!(v.tooltip, "Pitwall: the server is not running");
        // Open and Quit still work; everything that needs the server is disabled.
        let item = |a: TrayAction| {
            v.entries.iter().find_map(|e| match e {
                Entry::Item { action, enabled, .. } if *action == a => Some(*enabled),
                _ => None,
            })
        };
        assert_eq!(item(TrayAction::Open), Some(true));
        assert_eq!(item(TrayAction::Quit), Some(true));
        assert_eq!(item(TrayAction::ToggleServing), Some(false));
        assert_eq!(item(TrayAction::ReleaseMemory), Some(false));
    }

    #[test]
    fn live_updates_drive_models_memory_queue_and_speed() {
        let mut m = TrayModel::new();
        m.apply(&LiveUpdate::Models {
            models: vec![model("Qwen 7B", 4 * GB, 2 * GB), model("Gemma 2B", GB, 0)],
        });
        assert_eq!(m.state(), TrayState::Idle);
        assert_eq!(m.summary(), "2 models · 7.0 GB");
        m.apply(&LiveUpdate::Queue {
            queue: QueueSnapshot {
                running: vec![],
                waiting: vec![],
                depth: Depth {
                    interactive: 0,
                    background: 0,
                    batch: 0,
                },
            },
        });
        m.apply(&LiveUpdate::Throughput {
            throughput: Throughput {
                gen_tps: 38.4,
                ..Throughput::default()
            },
        });
        assert_eq!(m.summary(), "2 models · 7.0 GB · 38 tok/s");
        assert!(m.view().tooltip.starts_with("Pitwall: 2 models"));
        assert!(m.view().entries.contains(&Entry::Status("  Qwen 7B  6.0 GB".into())));
        // Memory samples and events do not change what the tray shows.
        let before = m.view();
        m.apply(&LiveUpdate::Memory { sample: sample() });
        assert_eq!(m.view(), before);
    }

    #[test]
    fn pausing_changes_the_icon_the_tooltip_and_the_menu_label() {
        let mut m = TrayModel::new();
        m.apply(&LiveUpdate::Server { server: server(true) });
        assert_eq!(m.state(), TrayState::Paused);
        assert!(m.paused());
        let v = m.view();
        assert!(v.tooltip.starts_with("Pitwall (paused)"));
        assert!(v
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Item { label, .. } if label == "Resume serving")));
        m.apply(&LiveUpdate::Server { server: server(false) });
        assert!(m
            .view()
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Item { label, .. } if label == "Pause serving")));
    }

    #[test]
    fn a_lost_connection_clears_everything_stale() {
        let mut m = TrayModel::new();
        m.apply(&LiveUpdate::Models {
            models: vec![model("A", GB, 0)],
        });
        assert_eq!(m.state(), TrayState::Idle);
        m.set_connected(false);
        assert_eq!(m, TrayModel::default());
        assert_eq!(m.state(), TrayState::Unavailable);
        assert_eq!(m.summary(), "The server is not running");
    }

    #[test]
    fn jobs_in_the_queue_make_the_icon_busy() {
        let mut m = TrayModel::new();
        let job: pitwall_core::mgmt::Job = serde_json::from_value(json!({
            "id": "1", "appId": "debrief", "modelId": "m", "priority": "interactive", "kind": "chat",
            "state": "running", "submittedAt": 0, "startedAt": 0, "progress": null, "tokens": 0
        }))
        .unwrap();
        m.apply(&LiveUpdate::Queue {
            queue: QueueSnapshot {
                running: vec![job],
                waiting: vec![],
                depth: Depth {
                    interactive: 0,
                    background: 0,
                    batch: 0,
                },
            },
        });
        assert_eq!(m.state(), TrayState::Busy);
        assert!(m.summary().contains("queue 1"));
    }

    #[test]
    fn menu_ids_round_trip() {
        for a in [
            TrayAction::Open,
            TrayAction::ToggleServing,
            TrayAction::ReleaseMemory,
            TrayAction::Quit,
        ] {
            assert_eq!(TrayAction::parse(a.id()), Some(a));
        }
        assert_eq!(TrayAction::parse("nope"), None);
    }

    #[test]
    fn byte_sizes_read_naturally_and_long_model_lists_are_cut() {
        assert_eq!(bytes_text(0), "0 MB");
        assert_eq!(bytes_text(512 * 1024 * 1024), "512 MB");
        assert_eq!(bytes_text(GB + GB / 2), "1.5 GB");
        let mut m = TrayModel::new();
        m.apply(&LiveUpdate::Models {
            models: (0..8).map(|i| model(&format!("m{i}"), GB, 0)).collect(),
        });
        assert!(m.view().entries.contains(&Entry::Status("  and 3 more".into())));
        m.apply(&LiveUpdate::Models {
            models: vec![model("one", GB, 0)],
        });
        assert_eq!(m.summary(), "1 model · 1.0 GB");
        m.apply(&LiveUpdate::Models { models: vec![] });
        assert_eq!(m.summary(), "No model loaded");
    }

    #[test]
    fn a_snapshot_seeds_the_model() {
        let overview = json!({
            "server": server(true), "memory": sample(), "models": [model("A", GB, 0)], "engines": [],
            "queue": {"running": [], "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}},
            "throughput": {"promptTps": 0.0, "genTps": 12.0, "ttftP50Ms": 0.0, "requestsPerMin": 0.0, "activeRequests": 0},
            "apps": [], "preset": "balanced", "pairings": [], "issues": []
        });
        let o: Overview = serde_json::from_value(overview).unwrap();
        let mut m = TrayModel::new();
        m.seed(&o);
        assert_eq!(m.summary(), "1 model · 1.0 GB · 12 tok/s");
        assert_eq!(m.state(), TrayState::Paused);
    }
}
