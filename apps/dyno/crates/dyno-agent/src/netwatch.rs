//! Network change events (WP-35): every twenty seconds a cheap look at the interfaces (and, at most once a minute, the
//! Wi-Fi network joined) through [`dyno_core::ports::NetworkLinks`]; what changed since the previous look is put on the
//! history timeline. No polling of the wider network stack, no traffic, observe-only. A refused read is skipped quietly
//! (the next look tries again); the first look only sets the baseline.

use crate::agent::{lock, now, Shared};
use dyno_core::netchange::{NetChange, NetWatcher};
use dyno_core::protocol::{TimelineEvent, TimelineEventKind};
use std::sync::Mutex;
use std::time::Duration;

/// How often the interfaces are looked at.
pub const LOOK_EVERY: Duration = Duration::from_secs(20);
/// The first look waits this long (the sampler and the OS settle first).
const FIRST_LOOK_AFTER: Duration = Duration::from_secs(5);
/// Most events written per look (an interface flapping must not flood the timeline).
const MAX_PER_LOOK: usize = 6;

/// The watcher and its memory of the last look.
pub struct NetWatchCentre {
    watcher: Mutex<NetWatcher>,
}

impl Default for NetWatchCentre {
    fn default() -> Self {
        NetWatchCentre::new()
    }
}

impl NetWatchCentre {
    /// A watcher with no baseline.
    pub fn new() -> NetWatchCentre {
        NetWatchCentre {
            watcher: Mutex::new(NetWatcher::new()),
        }
    }

    /// One look; returns the changes it recorded.
    pub fn look(&self, shared: &Shared) -> Vec<NetChange> {
        let state = match lock(&shared.ports.links).links() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let changes = lock(&self.watcher).observe(state);
        let at = now();
        let core = lock(&shared.core);
        for c in changes.iter().take(MAX_PER_LOOK) {
            if let Err(e) = core.store.put_event(&TimelineEvent {
                at,
                kind: TimelineEventKind::Network,
                label: c.label(),
            }) {
                log::error!("dyno-agent: could not record a network change: {e}");
            }
        }
        changes
    }
}

/// The thread body: look now and then until the agent stops.
pub fn run(shared: &Shared) {
    let wait = |d: Duration| {
        let mut waited = Duration::ZERO;
        while waited < d && !shared.stopped() {
            std::thread::sleep(Duration::from_millis(250));
            waited += Duration::from_millis(250);
        }
    };
    wait(FIRST_LOOK_AFTER);
    while !shared.stopped() {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.netwatch.look(shared))).is_err() {
            log::error!("dyno-agent: a look at the network failed unexpectedly");
        }
        wait(LOOK_EVERY);
    }
}
