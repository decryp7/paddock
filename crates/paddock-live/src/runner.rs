//! Keeping a source connected.

use crate::frame::Link;
use crate::hub::{Hub, LiveState};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Backoff, BackoffPolicy, Clock, Retry};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// The hub as a [`Source`] sees it during one connection.
pub struct Publisher<'a, S: LiveState> {
    hub: &'a Hub<S>,
    clock: &'a Arc<dyn Clock>,
    seeded_at: Mutex<Option<Duration>>,
}

impl<S: LiveState> Publisher<'_, S> {
    /// The connection is up and `state` is its current picture: replaces the hub's state and sets the link to connected.
    /// Call it before the first [`publish`](Self::publish) of every connection.
    pub fn seed(&self, state: S) {
        self.hub.reset_with(state, Link::Connected);
        *self
            .seeded_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(self.clock.monotonic());
    }

    /// One change from the connection.
    pub fn publish(&self, delta: S::Delta) {
        self.hub.publish(delta);
    }

    /// Whether this connection has seeded.
    pub fn is_seeded(&self) -> bool {
        self.seeded_at().is_some()
    }

    fn seeded_at(&self) -> Option<Duration> {
        *self
            .seeded_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Where a feed's state comes from: an agent, a server stream, a companion. One call is one connection.
pub trait Source<S: LiveState>: Send {
    /// Connects, seeds the publisher, then publishes changes until the connection ends. `Ok` is a clean end (the peer
    /// closed), `Err` carries a short reason for the user, never content. The call should return soon after `cancel` fires.
    fn session(&mut self, feed: &Publisher<'_, S>, cancel: &CancelToken) -> Result<(), String>;
}

/// How [`run`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    /// The token was cancelled; the link is [`Link::Stopped`].
    Cancelled,
    /// The back-off policy gave up; the link is [`Link::Down`] with the last reason.
    GaveUp,
}

/// Connects `source`, and whenever the connection ends reconnects after the back-off's delay (waited on `clock`, so a test
/// drives it with a `FakeClock`). The hub's link says what is happening: connecting, connected (set by the seed),
/// reconnecting with the attempt and the delay, down. A connection that lasted resets the delay (`policy.reset_after`).
pub fn run<S: LiveState>(
    hub: &Hub<S>,
    source: &mut dyn Source<S>,
    clock: &Arc<dyn Clock>,
    policy: BackoffPolicy,
    cancel: &CancelToken,
) -> RunEnd {
    let mut backoff = Backoff::new(policy);
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            hub.set_link(Link::Stopped);
            return RunEnd::Cancelled;
        }
        let publisher = Publisher {
            hub,
            clock,
            seeded_at: Mutex::new(None),
        };
        let result = source.session(&publisher, cancel);
        if cancel.is_cancelled() {
            hub.set_link(Link::Stopped);
            return RunEnd::Cancelled;
        }
        let reason = match result {
            Ok(()) => "The connection was closed".to_string(),
            Err(reason) => reason,
        };
        let now = clock.monotonic();
        if let Some(since) = publisher.seeded_at() {
            // Healthy from the seed on: lasting `reset_after` starts the delays over.
            backoff.healthy(since);
            if now.saturating_sub(since) >= policy.reset_after {
                attempt = 0;
            }
        }
        match backoff.failed(now) {
            Retry::GiveUp => {
                hub.set_link(Link::Down { reason });
                return RunEnd::GaveUp;
            }
            Retry::After(delay) => {
                attempt += 1;
                hub.set_link(Link::Reconnecting {
                    attempt,
                    retry_in_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    reason,
                });
                if clock.wait(delay, cancel).is_err() {
                    hub.set_link(Link::Stopped);
                    return RunEnd::Cancelled;
                }
            }
        }
    }
}

/// Pitwall's reconnect delays (1 s doubling to 10 s, never giving up): the policy a UI link should use unless the app has
/// reasons.
pub fn default_policy() -> BackoffPolicy {
    BackoffPolicy::retry_forever(
        Duration::from_secs(1),
        Duration::from_secs(10),
        Duration::from_secs(30),
    )
}
