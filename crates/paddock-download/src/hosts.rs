//! Per-host connection caps and back-off, shared by every download of the process.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Connections one host gets at once unless [`HostCaps::with_cap`] says otherwise.
pub const DEFAULT_HOST_CAP: usize = 4;
/// The largest cap a host can be given.
pub const MAX_HOST_CAP: usize = 16;

#[derive(Default)]
struct Host {
    in_use: usize,
    peak: usize,
    not_before: Option<Instant>,
}

/// How many connections each host may have open across all downloads, and until when a host asked to be left alone (429/503
/// with `Retry-After`). One instance per app, shared by its [`Downloader`](crate::Downloader)s.
pub struct HostCaps {
    default: usize,
    caps: HashMap<String, usize>,
    hosts: Mutex<HashMap<String, Host>>,
    freed: Condvar,
}

impl HostCaps {
    /// Every host gets `default` connections (1..=[`MAX_HOST_CAP`]).
    pub fn new(default: usize) -> Self {
        HostCaps {
            default: default.clamp(1, MAX_HOST_CAP),
            caps: HashMap::new(),
            hosts: Mutex::new(HashMap::new()),
            freed: Condvar::new(),
        }
    }

    /// A cap for one host key (`host[:port]`, lower case, as [`Request::host`](crate::Request::host) gives it).
    pub fn with_cap(mut self, host: &str, cap: usize) -> Self {
        self.caps
            .insert(host.to_ascii_lowercase(), cap.clamp(1, MAX_HOST_CAP));
        self
    }

    pub fn cap(&self, host: &str) -> usize {
        self.caps.get(host).copied().unwrap_or(self.default)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Host>> {
        self.hosts.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Connections open to `host` right now.
    pub fn in_use(&self, host: &str) -> usize {
        self.lock().get(host).map_or(0, |h| h.in_use)
    }

    /// The most connections `host` had open at once.
    pub fn peak(&self, host: &str) -> usize {
        self.lock().get(host).map_or(0, |h| h.peak)
    }

    /// Asks every download to wait before the next request to `host` until `until` (later deadlines win).
    pub fn back_off(&self, host: &str, until: Instant) {
        let mut g = self.lock();
        let h = g.entry(host.to_string()).or_default();
        h.not_before = Some(h.not_before.map_or(until, |t| t.max(until)));
    }

    /// Waits for a free connection to `host` (and for its back-off to pass), checking `halt` at least every 10 ms. `None` when
    /// `halt` said stop. While waiting for a back-off `waiting` is called, so the caller can tell a deliberate wait from a stall.
    pub(crate) fn acquire(
        self: &Arc<Self>,
        host: &str,
        halt: &dyn Fn() -> bool,
        waiting: &dyn Fn(),
    ) -> Option<Permit> {
        let cap = self.cap(host);
        let mut g = self.lock();
        loop {
            if halt() {
                return None;
            }
            let now = Instant::now();
            let h = g.entry(host.to_string()).or_default();
            let wait = match h.not_before {
                Some(t) if t > now => {
                    waiting();
                    (t - now).min(Duration::from_millis(10))
                }
                _ if h.in_use < cap => {
                    h.in_use += 1;
                    h.peak = h.peak.max(h.in_use);
                    return Some(Permit {
                        caps: self.clone(),
                        host: host.to_string(),
                    });
                }
                _ => Duration::from_millis(10),
            };
            g = self
                .freed
                .wait_timeout(g, wait)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

impl Default for HostCaps {
    fn default() -> Self {
        HostCaps::new(DEFAULT_HOST_CAP)
    }
}

/// One open connection to a host; dropping it frees the slot.
pub(crate) struct Permit {
    caps: Arc<HostCaps>,
    host: String,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(h) = self.caps.lock().get_mut(&self.host) {
            h.in_use = h.in_use.saturating_sub(1);
        }
        self.caps.freed.notify_all();
    }
}
