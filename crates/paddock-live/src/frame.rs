//! What goes over the wire.

use serde::{Deserialize, Serialize};

/// The state of the connection to whatever feeds the hub (the agent, a server, a companion), for the window to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Link {
    /// Trying to connect, the first time.
    Connecting,
    /// Connected and seeded: the state is live.
    Connected,
    /// The connection ended; the next attempt is in `retry_in_ms`. The state shown is the last one known.
    #[serde(rename_all = "camelCase")]
    Reconnecting {
        /// Which attempt is next (1 for the first retry).
        attempt: u32,
        /// How long until it, in milliseconds.
        retry_in_ms: u64,
        /// Why the connection ended, a short sentence without content (never user data).
        reason: String,
    },
    /// Gave up (the back-off policy allows it): the user must act.
    Down {
        /// Why, a short sentence.
        reason: String,
    },
    /// The feed was stopped on purpose (the app is closing or the user stopped it).
    Stopped,
}

/// One message to the windows. Serialised with a `kind` tag in camelCase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[serde(bound(
    serialize = "S: Serialize, D: Serialize",
    deserialize = "S: serde::de::DeserializeOwned, D: serde::de::DeserializeOwned"
))]
pub enum Frame<S, D> {
    /// The whole state as of `seq`; replaces whatever the window has.
    Snapshot {
        /// The sequence number of the state.
        seq: u64,
        /// The state.
        state: S,
        /// The link now.
        link: Link,
    },
    /// Changes `from..=seq` (one number, or several merged into one by [`LiveState::merge`](crate::LiveState::merge)). A
    /// window applies it when `from` is the next number it expects.
    Delta {
        /// The first sequence number the delta stands for.
        from: u64,
        /// The last one.
        seq: u64,
        /// The change.
        delta: D,
    },
    /// The link changed; the state did not.
    Link {
        /// The link now.
        link: Link,
    },
}
