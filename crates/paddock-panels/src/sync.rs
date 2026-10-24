//! UI-state forwarding between the windows of one host: who receives a message, how large it may be and how often a
//! window may send.
//!
//! The UI (`workbench/panels`, `UiSync`) keeps the protocol: ordering, echo prevention, the snapshot a new pop-out takes
//! from its host. Rust is the transport and its guard (the webview is not trusted): a message goes only to the other
//! windows of the sender's host, never to another host's windows; dialogs get display-scope messages only (theme and
//! formats), never the workspace's selection; a message over [`MAX_MESSAGE_BYTES`] is refused; and a window that sends
//! faster than [`RATE`] messages a second is cut off (a runaway observer must not flood every window).

use crate::wire::{PanelError, PanelErrorCode, SyncScope};
use paddock_commands::labels::{Label, resolve};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// The largest message (serialised JSON bytes): a selected record with huge fields should not flood every window.
pub const MAX_MESSAGE_BYTES: usize = 512 * 1024;
/// Messages a window may send per second, on average (the UI sends a pointer-following value at most every 50 ms).
pub const RATE: u32 = 200;
/// How many messages a window may send at once before the rate applies.
pub const BURST: u32 = 100;

/// The labels, among `labels`, that receive a message `scope` from `sender`: the windows of the sender's host (the host
/// itself, its pop-outs and, for [`SyncScope::Display`], its dialogs), without the sender. A webview outside the label
/// scheme, a tray popover and another host's windows receive nothing; a sender outside the scheme reaches nobody.
pub fn recipients<'a>(
    sender: &str,
    scope: SyncScope,
    labels: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    if matches!(Label::parse(sender), None | Some(Label::Tray { .. })) {
        return Vec::new();
    }
    let host = resolve(sender);
    let mut out: Vec<String> = labels
        .into_iter()
        .filter(|l| *l != sender)
        .filter(|l| match Label::parse(l) {
            Some(Label::Main | Label::Window(_)) => *l == host,
            Some(Label::Popout { host: h, .. }) => h == host,
            Some(Label::Dialog { host: h, .. }) => h == host && scope == SyncScope::Display,
            Some(Label::Tray { .. }) | None => false,
        })
        .map(str::to_string)
        .collect();
    out.sort();
    out
}

/// The message as it must be: a JSON object no larger than [`MAX_MESSAGE_BYTES`].
pub fn check_message(message: &Value) -> Result<(), PanelError> {
    if !message.is_object() {
        return Err(PanelError::new(
            PanelErrorCode::Invalid,
            "a UI state message is a JSON object",
        ));
    }
    let size = serde_json::to_vec(message).map_or(usize::MAX, |b| b.len());
    if size > MAX_MESSAGE_BYTES {
        return Err(PanelError::new(
            PanelErrorCode::TooLarge,
            format!(
                "the UI state is larger than {} KiB; it is not shared",
                MAX_MESSAGE_BYTES / 1024
            ),
        ));
    }
    Ok(())
}

/// A token bucket per sending window: [`BURST`] tokens, [`RATE`] new ones a second, time given by the caller (a test
/// needs no clock).
#[derive(Default)]
pub struct Limiter {
    buckets: Mutex<HashMap<String, (f64, u64)>>,
}

impl Limiter {
    /// Whether `sender` may send one more message at `now_ms` (milliseconds on any monotonic clock).
    pub fn allow(&self, sender: &str, now_ms: u64) -> bool {
        let mut map = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let (tokens, at) = map
            .entry(sender.to_string())
            .or_insert((f64::from(BURST), now_ms));
        let elapsed = now_ms.saturating_sub(*at) as f64 / 1000.0;
        *tokens = (*tokens + elapsed * f64::from(RATE)).min(f64::from(BURST));
        *at = (*at).max(now_ms);
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Forgets a window that is gone.
    pub fn forget(&self, sender: &str) {
        self.buckets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(sender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALL: [&str; 9] = [
        "main",
        "w-2",
        "p-main-1",
        "p-main-2",
        "p-w-2-3",
        "d-main-4",
        "d-w-2-5",
        "tray",
        "something-else",
    ];

    #[test]
    fn a_full_message_reaches_the_host_and_its_pop_outs_only() {
        assert_eq!(
            recipients("main", SyncScope::Full, ALL),
            ["p-main-1", "p-main-2"]
        );
        assert_eq!(
            recipients("p-main-1", SyncScope::Full, ALL),
            ["main", "p-main-2"]
        );
        assert_eq!(recipients("w-2", SyncScope::Full, ALL), ["p-w-2-3"]);
        assert_eq!(recipients("p-w-2-3", SyncScope::Full, ALL), ["w-2"]);
        assert_eq!(
            recipients("d-w-2-5", SyncScope::Full, ALL),
            ["p-w-2-3", "w-2"]
        );
    }

    #[test]
    fn a_display_message_also_reaches_the_dialogs_of_the_same_host() {
        assert_eq!(
            recipients("main", SyncScope::Display, ALL),
            ["d-main-4", "p-main-1", "p-main-2"]
        );
        assert_eq!(
            recipients("d-main-4", SyncScope::Display, ALL),
            ["main", "p-main-1", "p-main-2"]
        );
    }

    #[test]
    fn nothing_crosses_hosts_and_unknown_senders_reach_nobody() {
        for scope in [SyncScope::Full, SyncScope::Display] {
            let from_main = recipients("main", scope, ALL);
            assert!(
                from_main.iter().all(|l| !l.contains("w-2")),
                "{from_main:?}"
            );
            assert!(recipients("tray", scope, ALL).is_empty());
            assert!(recipients("something-else", scope, ALL).is_empty());
            assert!(recipients("", scope, ALL).is_empty());
        }
        assert!(recipients("main", SyncScope::Display, []).is_empty());
    }

    #[test]
    fn a_message_is_an_object_of_bounded_size() {
        assert!(check_message(&json!({ "t": "state", "key": "k", "value": 1 })).is_ok());
        for bad in [json!(1), json!("x"), json!([1]), json!(null)] {
            assert_eq!(
                check_message(&bad).unwrap_err().code,
                PanelErrorCode::Invalid
            );
        }
        let big = json!({ "t": "state", "value": "x".repeat(MAX_MESSAGE_BYTES) });
        let e = check_message(&big).unwrap_err();
        assert_eq!(e.code, PanelErrorCode::TooLarge);
        assert!(e.message.contains("512 KiB"), "{e}");
        let just = json!({ "v": "x".repeat(MAX_MESSAGE_BYTES - 10) });
        assert!(check_message(&just).is_ok());
    }

    #[test]
    fn a_window_may_burst_then_is_held_to_the_rate_and_recovers() {
        let l = Limiter::default();
        for i in 0..BURST {
            assert!(l.allow("main", 0), "{i}");
        }
        assert!(!l.allow("main", 0), "the burst is used up");
        assert!(l.allow("p-main-1", 0), "other windows have their own");
        for _ in 0..4 {
            assert!(l.allow("main", 20));
        }
        assert!(!l.allow("main", 20));
        for i in 0..BURST {
            assert!(l.allow("main", 100_000), "{i}");
        }
        assert!(!l.allow("main", 100_000));
        assert!(
            !l.allow("main", 5),
            "a clock that went backwards refills nothing"
        );
        l.forget("main");
        assert!(l.allow("main", 0));
    }
}
