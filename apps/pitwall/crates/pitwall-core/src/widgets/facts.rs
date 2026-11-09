//! The facts the popup's widgets read: an accumulator the live feed writes (a snapshot after every connect, then each pushed
//! update) and the app tops up with the few things the feed does not carry (downloads, events, model update notices). Plain
//! data and pure updates: time comes in as an argument, nothing here reads a clock, a file or the network.

use crate::events::Severity;
use crate::mgmt::{
    Download, DownloadState, EngineIssue, EngineSetup, EngineStatusTile, LiveUpdate, LoadedModel, MemorySample, Overview, PitwallEvent,
    ServerInfo, Throughput,
};
use std::collections::BTreeMap;

/// Error events kept (the newest); Recent errors lists the last day's of them.
pub const MAX_ERRORS: usize = 20;

/// One error event, as the widget needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorFact {
    /// The event's id.
    pub id: u64,
    /// When, in milliseconds since the epoch.
    pub at_ms: i64,
    /// One sentence (never request content).
    pub message: String,
}

/// What the model update checks found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdatesFact {
    /// Notices on offer.
    pub count: u32,
    /// The first notice's sentence.
    pub summary: Option<String>,
    /// When a check last completed, in milliseconds since the epoch.
    pub last_checked_ms: Option<i64>,
}

/// Everything the popup's widgets read, as far as it is known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    /// The live feed is connected now.
    pub(crate) connected: bool,
    /// When the feed first failed in the current outage (cleared when it connects): the time the facts went stale.
    pub(crate) failed_ms: Option<i64>,
    /// A snapshot was received at least once: the facts below are real, though they may be stale.
    pub(crate) known: bool,
    pub(crate) server: Option<ServerInfo>,
    pub(crate) memory: Option<MemorySample>,
    pub(crate) models: Vec<LoadedModel>,
    pub(crate) running: u32,
    pub(crate) waiting: u32,
    pub(crate) throughput: Throughput,
    pub(crate) engines: Vec<EngineStatusTile>,
    pub(crate) engine_setup: EngineSetup,
    pub(crate) issues: Vec<EngineIssue>,
    /// Downloads that are not finished or cancelled, by id.
    pub(crate) downloads: BTreeMap<String, Download>,
    /// Error events, newest first.
    pub(crate) errors: Vec<ErrorFact>,
    /// The downloads were read (after a connect): without it "no downloads" is not known.
    pub(crate) downloads_read: bool,
    /// The recent events were read: without it "no errors" is not known.
    pub(crate) events_read: bool,
    pub(crate) pairings: u32,
    pub(crate) updates: Option<UpdatesFact>,
    /// When any fact last changed: the time the widgets were read at.
    pub(crate) changed_ms: i64,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn ms(t: u64) -> i64 {
    i64::try_from(t).unwrap_or(i64::MAX)
}

impl Facts {
    /// Facts of a server that has not been reached yet.
    pub fn new() -> Facts {
        Facts::default()
    }

    /// Whether the feed is connected.
    pub fn connected(&self) -> bool {
        self.connected
    }

    /// Whether the feed failed since it last connected (or since the start).
    pub fn failed(&self) -> bool {
        self.failed_ms.is_some()
    }

    /// Whether the dashboard snapshot was received at least once.
    pub fn known(&self) -> bool {
        self.known
    }

    /// Starts from a full snapshot (after every (re)connect). What the feed does not carry stays as it was.
    pub fn seed(&mut self, o: &Overview, now_ms: i64) {
        self.connected = true;
        self.known = true;
        self.failed_ms = None;
        self.server = Some(o.server.clone());
        self.memory = Some(o.memory.clone());
        self.models = o.models.clone();
        self.running = count(o.queue.running.len());
        self.waiting = count(o.queue.waiting.len());
        self.throughput = o.throughput;
        self.engines = o.engines.clone();
        self.engine_setup = o.engine_setup.clone();
        self.issues = o.issues.clone();
        self.pairings = count(o.pairings.len());
        // A new connection: what the app reads after it (downloads, events) is unknown again until read again.
        self.downloads_read = false;
        self.events_read = false;
        self.changed_ms = now_ms;
    }

    /// Applies one pushed update.
    pub fn apply(&mut self, u: &LiveUpdate, now_ms: i64) {
        self.connected = true;
        self.failed_ms = None;
        match u {
            LiveUpdate::Memory { sample } => self.memory = Some(sample.clone()),
            LiveUpdate::Models { models } => self.models = models.clone(),
            LiveUpdate::Queue { queue } => {
                self.running = count(queue.running.len());
                self.waiting = count(queue.waiting.len());
            }
            LiveUpdate::Issues { issues } => self.issues = issues.clone(),
            LiveUpdate::Throughput { throughput } => self.throughput = *throughput,
            LiveUpdate::Event { event } => self.add_event(event),
            LiveUpdate::Download { download } => self.set_download(download.clone()),
            LiveUpdate::EngineSetup { setup } => self.engine_setup = setup.clone(),
            LiveUpdate::Pairing { pairings } => self.pairings = count(pairings.len()),
            LiveUpdate::Server { server } => self.server = Some(server.clone()),
        }
        self.changed_ms = now_ms;
    }

    /// The feed ended: the facts stay (shown stale, with the time they were last good) and the surface says so.
    pub fn set_disconnected(&mut self, now_ms: i64) {
        self.failed_ms.get_or_insert(now_ms);
        self.connected = false;
        self.changed_ms = now_ms;
    }

    /// Replaces the downloads that are not finished (the app reads them after a connect).
    pub fn set_downloads(&mut self, downloads: &[Download], now_ms: i64) {
        self.downloads.clear();
        for d in downloads {
            self.set_download(d.clone());
        }
        self.downloads_read = true;
        self.changed_ms = now_ms;
    }

    fn set_download(&mut self, d: Download) {
        if matches!(d.state, DownloadState::Done | DownloadState::Cancelled) {
            self.downloads.remove(&d.id);
        } else {
            self.downloads.insert(d.id.clone(), d);
        }
    }

    /// Takes the error events of `events` (the app reads the recent ones after a connect).
    pub fn seed_events(&mut self, events: &[PitwallEvent], now_ms: i64) {
        self.errors.clear();
        for e in events {
            self.add_event(e);
        }
        self.events_read = true;
        self.changed_ms = now_ms;
    }

    fn add_event(&mut self, e: &PitwallEvent) {
        if e.severity != Severity::Error || self.errors.iter().any(|x| x.id == e.id) {
            return;
        }
        self.errors.push(ErrorFact {
            id: e.id,
            at_ms: ms(e.at),
            message: e.message.clone(),
        });
        self.errors.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(b.id.cmp(&a.id)));
        self.errors.truncate(MAX_ERRORS);
    }

    /// Sets what the model update checks found.
    pub fn set_updates(&mut self, updates: UpdatesFact, now_ms: i64) {
        self.updates = Some(updates);
        self.changed_ms = now_ms;
    }

    /// Whether serving is paused or stopped (what Resume serving undoes).
    pub fn serving_off(&self) -> bool {
        self.server
            .as_ref()
            .is_some_and(|s| s.serving != crate::mgmt::ServingState::Serving)
    }
}
