//! Pitwall's knowledge of the server's Offline mode, for the app's own network path (its update checks, through the shared
//! updater): every status, overview, settings answer and live `server` frame that passes through the client tells
//! [`OfflineKnown`] what the server said. The server is the source of truth for Offline mode; the app keeps the last value.

use crate::server_client::ServerClient;
use pitwall_core::mgmt::CommandError;
use serde_json::Value;

/// The last `network.offline` the server reported (to this app, by any status or overview read). Offline mode blocks every
/// network path, so when the server cannot be asked this is the answer; a value never seen is "unknown".
#[derive(Default)]
pub struct OfflineKnown(std::sync::atomic::AtomicU8);

impl OfflineKnown {
    const ONLINE: u8 = 1;
    const OFFLINE: u8 = 2;

    /// Remembers what the server said.
    pub fn set(&self, offline: bool) {
        self.0.store(
            if offline { Self::OFFLINE } else { Self::ONLINE },
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    /// Whether app updates are blocked: Offline mode is on, or was never confirmed off (fails closed). Last seen online
    /// allows an update while the server is down, since an update may be what repairs a broken service. The shared
    /// updater asks this before every check and download (`UpdatesBuilder::offline_mode`).
    pub fn blocks_updates(&self) -> bool {
        self.get() != Some(false)
    }

    /// The last value seen, `None` when the server never answered.
    pub fn get(&self) -> Option<bool> {
        match self.0.load(std::sync::atomic::Ordering::SeqCst) {
            Self::ONLINE => Some(false),
            Self::OFFLINE => Some(true),
            _ => None,
        }
    }

    /// Learns from a server reply: `overview` (`server.offline`) and `update_check_status` (`offline`).
    pub fn observe(&self, method: &str, reply: &Value) {
        let flag = match method {
            "update_check_status" => reply.get("offline"),
            "overview" => reply.get("server").and_then(|s| s.get("offline")),
            "settings" | "save_settings" => reply.get("network").and_then(|n| n.get("offline")),
            _ => None,
        };
        if let Some(b) = flag.and_then(Value::as_bool) {
            self.set(b);
        }
    }

    /// Learns from one frame of the live feed: a `server` update carries `server.offline`. Frames that are not JSON or
    /// do not carry it say nothing.
    pub fn observe_frame(&self, frame: &str) {
        if let Ok(v) = serde_json::from_str::<Value>(frame) {
            if v.get("kind").and_then(Value::as_str) == Some("server") {
                self.observe("overview", &json_server(&v));
            }
        }
    }
}

/// A `server` frame as an overview-shaped value, so one reader serves both.
fn json_server(frame: &Value) -> Value {
    serde_json::json!({ "server": frame.get("server").cloned().unwrap_or(Value::Null) })
}

/// A live stream that tells [`OfflineKnown`] about every frame it passes on.
struct ObservedFrames {
    inner: Box<dyn crate::server_client::FrameStream>,
    known: std::sync::Arc<OfflineKnown>,
}

impl crate::server_client::FrameStream for ObservedFrames {
    fn next_frame(&mut self) -> crate::server_client::BoxFuture<'_, Result<Option<String>, CommandError>> {
        Box::pin(async move {
            let frame = self.inner.next_frame().await?;
            if let Some(f) = &frame {
                self.known.observe_frame(f);
            }
            Ok(frame)
        })
    }
}

/// A client that tells [`OfflineKnown`] about every status, overview or settings answer, and every live `server` frame, that
/// passes through.
pub struct Observing {
    inner: std::sync::Arc<dyn ServerClient>,
    known: std::sync::Arc<OfflineKnown>,
}

impl Observing {
    /// Wraps `inner`.
    pub fn new(inner: std::sync::Arc<dyn ServerClient>, known: std::sync::Arc<OfflineKnown>) -> Observing {
        Observing { inner, known }
    }
}

impl ServerClient for Observing {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        timeout: Option<std::time::Duration>,
    ) -> crate::server_client::BoxFuture<'a, Result<Value, CommandError>> {
        Box::pin(async move {
            let reply = self.inner.call(method, args, timeout).await?;
            self.known.observe(method, &reply);
            Ok(reply)
        })
    }

    fn open_stream<'a>(
        &'a self,
        path: &'a str,
        body: Option<Value>,
        open_timeout: std::time::Duration,
    ) -> crate::server_client::BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
        Box::pin(async move {
            let inner = self.inner.open_stream(path, body, open_timeout).await?;
            Ok(Box::new(ObservedFrames {
                inner,
                known: self.known.clone(),
            }) as Box<dyn crate::server_client::FrameStream>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::forward;
    use crate::server_client::fake::{FakeServer, Reply};
    use serde_json::json;

    fn status(offline: Value) -> Reply {
        Reply::Ok(json!({ "offline": offline }))
    }

    /// Offline mode blocks updates, and so does a value never seen (fail closed); last seen online allows them.
    #[test]
    fn updates_are_blocked_unless_offline_mode_was_last_seen_off() {
        let known = OfflineKnown::default();
        assert!(known.blocks_updates(), "never known: blocked");
        known.set(true);
        assert!(known.blocks_updates());
        known.set(false);
        assert!(!known.blocks_updates());
    }

    #[tokio::test]
    async fn a_settings_read_or_save_updates_the_last_known_value() {
        for method in ["settings", "save_settings"] {
            let inner = FakeServer::new();
            inner.reply(method, Reply::Ok(json!({ "network": { "offline": true, "lanEnabled": false } })));
            let known = std::sync::Arc::new(OfflineKnown::default());
            let client = Observing::new(std::sync::Arc::new(inner), known.clone());
            forward(&client, method, Value::Null).await.unwrap();
            assert_eq!(known.get(), Some(true), "{method}");
        }
    }

    #[tokio::test]
    async fn a_live_server_frame_updates_the_last_known_value_and_other_frames_do_not() {
        let inner = FakeServer::new();
        inner.stream(Ok(vec![
            r#"{"kind":"memory","sample":{}}"#.into(),
            "not json".into(),
            r#"{"kind":"server","server":{"offline":true}}"#.into(),
        ]));
        let known = std::sync::Arc::new(OfflineKnown::default());
        let client = Observing::new(std::sync::Arc::new(inner), known.clone());
        let mut stream = client
            .open_stream("/mgmt/v1/live", None, std::time::Duration::from_secs(1))
            .await
            .unwrap();
        assert!(stream.next_frame().await.unwrap().is_some());
        assert_eq!(known.get(), None, "a frame without the flag says nothing");
        assert!(stream.next_frame().await.unwrap().is_some());
        assert_eq!(known.get(), None, "a frame that is not JSON is passed on and ignored");
        assert!(stream.next_frame().await.unwrap().is_some());
        assert_eq!(known.get(), Some(true));
    }

    #[tokio::test]
    async fn every_overview_or_status_read_updates_the_last_known_value() {
        let inner = FakeServer::new();
        inner.reply("overview", Reply::Ok(json!({ "server": { "offline": true } })));
        inner.reply("update_check_status", status(json!(false)));
        let known = std::sync::Arc::new(OfflineKnown::default());
        let client = Observing::new(std::sync::Arc::new(inner), known.clone());
        assert_eq!(known.get(), None);
        forward(&client, "overview", Value::Null).await.unwrap();
        assert_eq!(known.get(), Some(true));
        forward(&client, "update_check_status", Value::Null).await.unwrap();
        assert_eq!(known.get(), Some(false));
        forward(&client, "presets", Value::Null).await.unwrap();
        assert_eq!(known.get(), Some(false), "other methods say nothing");
    }
}
