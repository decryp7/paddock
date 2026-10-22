//! The live feed: one task keeps a connection to `GET /mgmt/v1/live`, forwards every update to the UI as the Tauri event
//! `pitwall://live`, keeps the tray model current and redraws the tray. It reconnects with a capped back-off and says
//! "server not running" in the tray in between. No polling; nothing is logged but connection state (never content).

use crate::server_client::{FrameStream, ServerClient};
use pitwall_core::mgmt::{CommandError, LiveUpdate, Overview};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// The Tauri event the UI listens to (`tauri-backend.ts`).
pub const EVENT: &str = "pitwall://live";
/// The delays between reconnect attempts: quick at first, then every 10 s.
const RECONNECT: [u64; 5] = [1, 2, 4, 8, 10];
/// Longest wait for the stream to open or for the seed snapshot.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait before reconnect attempt number `attempt` (0 is the first retry).
pub fn reconnect_delay(attempt: usize) -> Duration {
    Duration::from_secs(RECONNECT[attempt.min(RECONNECT.len() - 1)])
}

/// What the UI and tray receive from the pump.
pub trait LiveSink: Send {
    /// A full snapshot, after every (re)connect.
    fn seed(&mut self, overview: &Overview);
    /// An update the server pushed: the raw JSON (forwarded to the UI) and, when its kind is known, the typed value.
    fn update(&mut self, raw: &Value, parsed: Option<&LiveUpdate>);
    /// The connection ended or could not be made.
    fn disconnected(&mut self);
}

/// Reads frames until the stream ends or fails. Frames that are not JSON are skipped (the feed never sends them, and
/// one bad frame must not end it); frames of an unknown kind still reach the UI.
pub async fn pump(stream: &mut dyn FrameStream, sink: &mut dyn LiveSink) -> Result<(), CommandError> {
    while let Some(frame) = stream.next_frame().await? {
        let Ok(raw) = serde_json::from_str::<Value>(&frame) else { continue };
        let parsed = serde_json::from_value::<LiveUpdate>(raw.clone()).ok();
        sink.update(&raw, parsed.as_ref());
    }
    Ok(())
}

/// One connection: seeds from `overview`, opens the stream, pumps it. `connected` says whether the stream opened.
pub async fn session(client: &dyn ServerClient, sink: &mut dyn LiveSink, connected: &mut bool) -> Result<(), CommandError> {
    let overview = client.call("overview", Value::Null, OPEN_TIMEOUT).await?;
    if let Ok(o) = serde_json::from_value::<Overview>(overview) {
        sink.seed(&o);
    }
    let mut stream = client.open_stream("/mgmt/v1/live", None, OPEN_TIMEOUT).await?;
    *connected = true;
    pump(stream.as_mut(), sink).await
}

/// Runs forever: connect, pump, reconnect with back-off.
pub async fn run(client: Arc<dyn ServerClient>, sink: &mut dyn LiveSink) {
    let mut attempt = 0usize;
    loop {
        let mut connected = false;
        let result = session(client.as_ref(), sink, &mut connected).await;
        if connected {
            attempt = 0;
        }
        sink.disconnected();
        if let Err(e) = result {
            // Connection state only; never content.
            eprintln!("Pitwall: live feed ended ({:?}); reconnecting", e.code);
        }
        tokio::time::sleep(reconnect_delay(attempt)).await;
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::{FakeServer, Reply};
    use serde_json::json;

    #[derive(Default)]
    struct Collect {
        updates: Vec<(Value, bool)>,
        seeded: usize,
        disconnects: usize,
    }

    impl LiveSink for Collect {
        fn seed(&mut self, _: &Overview) {
            self.seeded += 1;
        }
        fn update(&mut self, raw: &Value, parsed: Option<&LiveUpdate>) {
            self.updates.push((raw.clone(), parsed.is_some()));
        }
        fn disconnected(&mut self) {
            self.disconnects += 1;
        }
    }

    #[test]
    fn reconnects_start_quickly_and_settle_at_ten_seconds() {
        let secs: Vec<u64> = (0..8).map(|i| reconnect_delay(i).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 10, 10, 10, 10]);
    }

    #[tokio::test]
    async fn frames_are_forwarded_and_bad_ones_skipped() {
        let f = FakeServer::new();
        f.stream(Ok(vec![
            json!({"kind": "throughput", "throughput": {"promptTps": 0.0, "genTps": 5.0, "ttftP50Ms": 0.0, "requestsPerMin": 0.0, "activeRequests": 0}}).to_string(),
            "not json".into(),
            json!({"kind": "somethingNew", "x": 1}).to_string(),
        ]));
        let mut s = f.open_stream("/mgmt/v1/live", None, Duration::from_secs(1)).await.unwrap();
        let mut sink = Collect::default();
        pump(s.as_mut(), &mut sink).await.unwrap();
        assert_eq!(sink.updates.len(), 2);
        assert!(sink.updates[0].1, "a known update parses");
        assert!(!sink.updates[1].1, "an unknown kind still reaches the UI, untyped");
    }

    #[tokio::test]
    async fn a_session_without_a_server_fails_with_the_unavailable_error() {
        let f = FakeServer::new();
        f.reply("overview", Reply::Err(crate::server_client::unavailable()));
        let mut sink = Collect::default();
        let mut connected = false;
        let e = session(&f, &mut sink, &mut connected).await.unwrap_err();
        assert_eq!(e.code, pitwall_core::mgmt::ErrorCode::ServerUnavailable);
        assert!(!connected);
    }

    #[tokio::test]
    async fn the_loop_reports_each_disconnect_and_comes_back() {
        let f = Arc::new(FakeServer::new());
        f.reply("overview", Reply::Err(crate::server_client::unavailable()));
        let mut sink = Collect::default();
        // The loop never returns; give it one retry's worth of time (1 s) and look at what it did.
        let client: Arc<dyn ServerClient> = f.clone();
        let _ = tokio::time::timeout(Duration::from_millis(1500), run(client, &mut sink)).await;
        assert!(sink.disconnects >= 2, "{}", sink.disconnects);
        assert!(f.calls().len() >= 2, "it retried");
    }

    #[tokio::test]
    async fn a_session_marks_itself_connected_once_the_stream_is_open() {
        let f = FakeServer::new();
        f.stream(Ok(vec![]));
        let mut sink = Collect::default();
        let mut connected = false;
        session(&f, &mut sink, &mut connected).await.unwrap();
        assert!(connected);
    }
}
