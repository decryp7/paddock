//! The pure watchdog: every window's start-up deadline and, once it is ready, its liveness (ADR 0058, plan §11). The
//! caller passes the time, so every decision is a unit test with a fake clock; the Tauri adapter's ticker only applies
//! the [`Action`]s.

use crate::config::Config;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

/// Why the shell reloaded a window (told to the reloaded page once, with `window_ready`'s reply).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Recovery {
    /// The page did not report ready in time.
    Unresponsive,
    /// The page stopped answering after it was ready (missed its liveness pings) and the user chose Reload.
    Hung,
    /// The web content process terminated (crashed, killed, out of memory). Reported by WebKit on macOS and iOS.
    Terminated,
}

/// What the ticker must do for a window. The ticker applies them in the order given (sorted by window label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// The page missed its start-up deadline for the first time: reload it.
    Reload(String),
    /// The reloaded page missed the deadline too: ask the user (Reload / Restart in Safe Mode / Keep Waiting).
    AskStartup(String),
    /// A liveness ping to a ready, visible window (`paddock://ping` with this sequence number).
    Ping { label: String, seq: u64 },
    /// A ready window missed [`Config::missed_pings`] pings in a row: log it, let its dialog close by the OS close and ask
    /// the user (Reload / Keep Waiting).
    Unresponsive(String),
}

impl Action {
    /// The window the action is for.
    pub fn label(&self) -> &str {
        match self {
            Action::Reload(l) | Action::AskStartup(l) | Action::Unresponsive(l) => l,
            Action::Ping { label, .. } => label,
        }
    }
}

/// What a pong did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pong {
    /// The window answered as expected (or the answer was stale or for an unknown window: ignored).
    Alive,
    /// The window had been marked unresponsive and answers again.
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Loading: must report ready before `since + ready_timeout`.
    Waiting,
    /// Ready: answers pings while visible.
    Ready,
    /// The start-up question is open; nothing more until the user answers.
    AskingStartup,
    /// Missed too many pings; pings go on, so a page that resumes is noticed.
    Unresponsive,
}

/// The liveness of a ready window: pings sent and answered.
#[derive(Debug, Clone, Default)]
struct Liveness {
    /// The last ping sent.
    sent: u64,
    /// The last ping answered.
    answered: u64,
    /// Pings missed in a row.
    misses: u32,
    /// When the next ping is due (`None`: not pinged, e.g. hidden or not ready).
    next_at: Option<Instant>,
}

impl Liveness {
    /// Forgets outstanding pings: the next one is due one interval after `now`.
    fn restart(&mut self, now: Instant, config: &Config) {
        self.answered = self.sent;
        self.misses = 0;
        self.next_at = Some(now + config.ping_interval);
    }

    fn stop(&mut self) {
        self.answered = self.sent;
        self.misses = 0;
        self.next_at = None;
    }
}

#[derive(Debug, Clone)]
struct Entry {
    phase: Phase,
    since: Instant,
    /// Reloads the watchdog did for this window since it was last ready.
    reloads: u8,
    recovered: Option<Recovery>,
    visible: bool,
    live: Liveness,
    /// Keep Waiting answers since the window was last ready: each doubles the next wait (up to [`Config::max_doublings`]).
    asks: u8,
}

/// The per-window start-up and liveness state (pure: the caller passes the time).
#[derive(Debug)]
pub struct Watchdog {
    config: Config,
    windows: HashMap<String, Entry>,
    last_tick: Option<Instant>,
    /// Reloads after a terminated content process, per window, within [`Config::terminate_window`].
    terminations: HashMap<String, Vec<Instant>>,
    /// Boot errors logged per window since its page started loading.
    boot_errors: HashMap<String, u32>,
}

impl Default for Watchdog {
    fn default() -> Self {
        Watchdog::new(Config::default())
    }
}

impl Watchdog {
    /// A watchdog with `config` (checked by [`Config::checked`] when it comes from the app).
    pub fn new(config: Config) -> Self {
        Watchdog {
            config,
            windows: HashMap::new(),
            last_tick: None,
            terminations: HashMap::new(),
            boot_errors: HashMap::new(),
        }
    }

    /// The timings in use.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// A page started loading in window `label` (first load or any reload): its start-up clock starts.
    pub fn page_started(&mut self, label: &str, now: Instant) {
        let e = self.windows.entry(label.to_string()).or_insert(Entry {
            phase: Phase::Waiting,
            since: now,
            reloads: 0,
            recovered: None,
            visible: true,
            live: Liveness::default(),
            asks: 0,
        });
        e.phase = Phase::Waiting;
        e.since = now;
        e.live.stop();
        self.boot_errors.remove(label);
    }

    /// The page reported ready: returns (and forgets) why the shell reloaded it, if it did. Liveness pings start one
    /// interval after `now`. An unknown window (not watched) gets `None`.
    pub fn ready(&mut self, label: &str, now: Instant) -> Option<Recovery> {
        let e = self.windows.get_mut(label)?;
        e.phase = Phase::Ready;
        e.reloads = 0;
        e.asks = 0;
        if e.visible {
            e.live.restart(now, &self.config);
        }
        e.recovered.take()
    }

    /// The window's web content process terminated. Marks the window (its reloaded page hears why) and returns whether
    /// it may be reloaded now: at most [`Config::terminate_reloads`] within [`Config::terminate_window`], so a page that
    /// crashes on load does not loop (the start-up deadline then applies to the dead page as to any other).
    pub fn terminated(&mut self, label: &str, now: Instant) -> bool {
        self.page_started(label, now);
        if let Some(e) = self.windows.get_mut(label) {
            e.recovered = Some(Recovery::Terminated);
        }
        let window = self.config.terminate_window;
        let times = self.terminations.entry(label.to_string()).or_default();
        times.retain(|t| now.saturating_duration_since(*t) < window);
        if times.len() >= usize::from(self.config.terminate_reloads) {
            return false;
        }
        times.push(now);
        true
    }

    /// The user chose Keep Waiting: a page still starting gets another full timeout before it is asked about again; an
    /// unresponsive page gets [`Config::missed_pings`] more pings. Nothing for a window that recovered meanwhile.
    pub fn keep_waiting(&mut self, label: &str, now: Instant) {
        let config = self.config;
        if let Some(e) = self.windows.get_mut(label) {
            match e.phase {
                Phase::AskingStartup => {
                    e.phase = Phase::Waiting;
                    e.since = now;
                    e.asks = e.asks.saturating_add(1).min(config.max_doublings);
                }
                Phase::Unresponsive => {
                    e.phase = Phase::Ready;
                    e.live.restart(now, &config);
                    e.asks = e.asks.saturating_add(1).min(config.max_doublings);
                }
                Phase::Waiting | Phase::Ready => {}
            }
        }
    }

    /// The page answered ping `seq`. A stale or unknown answer changes nothing.
    pub fn pong(&mut self, label: &str, seq: u64, now: Instant) -> Pong {
        let config = self.config;
        let Some(e) = self.windows.get_mut(label) else {
            return Pong::Alive;
        };
        if seq <= e.live.answered || seq > e.live.sent {
            return Pong::Alive;
        }
        e.live.answered = seq;
        e.live.misses = 0;
        if e.phase == Phase::Unresponsive {
            e.phase = Phase::Ready;
            e.recovered = None;
            e.live.restart(now, &config);
            return Pong::Recovered;
        }
        Pong::Alive
    }

    /// Whether window `label` is visible (shown and not minimised). A hidden window is not pinged (its page may be
    /// throttled by the OS); pings start again one interval after it is shown.
    pub fn set_visible(&mut self, label: &str, visible: bool, now: Instant) {
        let config = self.config;
        let Some(e) = self.windows.get_mut(label) else {
            return;
        };
        if e.visible == visible {
            return;
        }
        e.visible = visible;
        // A window shown after its reload gets a fresh wait: it was never asked about while hidden.
        if visible && e.phase == Phase::Waiting && e.reloads > 0 {
            e.since = now;
        }
        if !matches!(e.phase, Phase::Ready | Phase::Unresponsive) {
            return;
        }
        if visible {
            e.live.restart(now, &config);
        } else {
            e.live.stop();
        }
    }

    /// Whether another boot error of window `label` may be logged (at most [`Config::max_boot_errors`] per page load).
    pub fn admit_boot_error(&mut self, label: &str) -> bool {
        let n = self.boot_errors.entry(label.to_string()).or_insert(0);
        if *n >= self.config.max_boot_errors {
            return false;
        }
        *n += 1;
        true
    }

    /// Forgets a closed window.
    pub fn closed(&mut self, label: &str) {
        self.windows.remove(label);
        self.terminations.remove(label);
        self.boot_errors.remove(label);
    }

    /// The windows the watchdog knows, in label order.
    pub fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.windows.keys().cloned().collect();
        labels.sort();
        labels
    }

    /// Whether window `label` reported ready (and has not started loading again since).
    pub fn is_ready(&self, label: &str) -> bool {
        self.windows
            .get(label)
            .is_some_and(|e| matches!(e.phase, Phase::Ready | Phase::Unresponsive))
    }

    /// Whether the start-up question about window `label` is open (it is not asked again until it is answered).
    pub fn is_asking(&self, label: &str) -> bool {
        self.windows
            .get(label)
            .is_some_and(|e| e.phase == Phase::AskingStartup)
    }

    /// Whether window `label` is marked unresponsive.
    pub fn is_unresponsive(&self, label: &str) -> bool {
        self.windows
            .get(label)
            .is_some_and(|e| e.phase == Phase::Unresponsive)
    }

    /// What is due at `now` (and records it), sorted by window label. A gap since the last tick longer than
    /// [`Config::pause_gap`] means the app process itself was paused (suspended, stopped by a CPU limiter): the start-up
    /// clocks and the pings restart instead of all firing at once on resume.
    pub fn due(&mut self, now: Instant) -> Vec<Action> {
        let paused = self
            .last_tick
            .is_some_and(|t| now.saturating_duration_since(t) > self.config.pause_gap);
        if paused {
            // The silent case that explains a "slow" start or a missed ping: the process itself was not running (ER-27).
            let gap = self
                .last_tick
                .map_or(0, |t| now.saturating_duration_since(t).as_millis());
            log::warn!(
                GapMs = u64::try_from(gap).unwrap_or(u64::MAX), LimitMs = self.config.pause_gap.as_millis() as u64;
                "process.paused: the app was not running between two watchdog ticks (suspended, stopped by a CPU limiter or the machine slept); start-up and liveness clocks restart"
            );
        }
        self.last_tick = Some(now);
        let config = self.config;
        let mut out = Vec::new();
        for (label, e) in &mut self.windows {
            match e.phase {
                Phase::AskingStartup => {}
                Phase::Waiting => {
                    if paused {
                        e.since = now;
                    } else if e.reloads == 0 {
                        if now.saturating_duration_since(e.since) >= config.ready_timeout {
                            e.reloads = 1;
                            e.since = now;
                            e.recovered = Some(Recovery::Unresponsive);
                            out.push(Action::Reload(label.clone()));
                        }
                    } else if e.visible
                        && now.saturating_duration_since(e.since) >= config.ask_wait(e.asks)
                    {
                        // Only a window the user can see is asked about (a hidden one starts at login, its page may be paused by the
                        // OS), and only after the longer wait; each Keep Waiting doubles it.
                        e.phase = Phase::AskingStartup;
                        out.push(Action::AskStartup(label.clone()));
                    }
                }
                Phase::Ready | Phase::Unresponsive => {
                    if !e.visible {
                        continue;
                    }
                    if paused {
                        e.live.restart(now, &config);
                        continue;
                    }
                    // No ping scheduled yet: one is due now.
                    if now < e.live.next_at.unwrap_or(now) {
                        continue;
                    }
                    if e.live.sent > e.live.answered {
                        e.live.misses = e.live.misses.saturating_add(1);
                    }
                    if e.phase == Phase::Ready
                        && e.live.misses
                            >= config.missed_pings.saturating_mul(1u32 << e.asks.min(16))
                    {
                        e.phase = Phase::Unresponsive;
                        e.recovered = Some(Recovery::Hung);
                        out.push(Action::Unresponsive(label.clone()));
                    }
                    e.live.sent += 1;
                    e.live.next_at = Some(now + config.ping_interval);
                    out.push(Action::Ping {
                        label: label.clone(),
                        seq: e.live.sent,
                    });
                }
            }
        }
        // Per window: the unresponsive mark before its ping (sort is stable).
        out.sort_by(|a, b| a.label().cmp(b.label()));
        out
    }
}

#[cfg(test)]
mod tests;
