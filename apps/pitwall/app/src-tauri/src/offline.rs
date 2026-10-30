//! Pitwall's knowledge of the server's Offline mode, for the app's own network path (its update checks, through the shared
//! updater): every status, overview, settings answer and live `server` frame that passes through the client tells
//! [`OfflineKnown`] what the server said. The server is the source of truth for Offline mode; the app keeps the last value.

use crate::server_client::ServerClient;
use pitwall_core::mgmt::CommandError;
use serde_json::Value;

/// The last `network.offline` the server reported (to this app, by any status or overview read). Offline mode blocks every
/// network path, so when the server cannot be asked this is the answer; a value never seen is "unknown". The last
/// confirmed value is kept in the app's data folder (`offline-known.json`), so after a restart with the server not
/// answering the app still knows it; on a first run it is unknown (and blocks). When Offline mode turns on, the
/// watchers run (Pitwall cancels an update check or download in flight).
#[derive(Default)]
pub struct OfflineKnown {
    value: std::sync::atomic::AtomicU8,
    file: Option<std::path::PathBuf>,
    watchers: std::sync::Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl OfflineKnown {
    const ONLINE: u8 = 1;
    const OFFLINE: u8 = 2;

    /// Kept in `file` (read now: a missing or damaged file is unknown).
    pub fn at(file: std::path::PathBuf) -> OfflineKnown {
        let value = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("offline").and_then(Value::as_bool))
            .map_or(0, |b| if b { Self::OFFLINE } else { Self::ONLINE });
        OfflineKnown {
            value: std::sync::atomic::AtomicU8::new(value),
            file: Some(file),
            watchers: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Runs `f` each time Offline mode turns on (from off or unknown).
    pub fn watch(&self, f: impl Fn() + Send + Sync + 'static) {
        self.watchers.lock().unwrap_or_else(|p| p.into_inner()).push(Box::new(f));
    }

    /// Remembers what the server said (and keeps it in the file when it changed).
    pub fn set(&self, offline: bool) {
        let new = if offline { Self::OFFLINE } else { Self::ONLINE };
        let old = self.value.swap(new, std::sync::atomic::Ordering::SeqCst);
        if old == new {
            return;
        }
        if let Some(file) = &self.file {
            // Atomic: a temp file in the same folder, then a rename. A failure is logged; the value in memory stands.
            let tmp = file.with_extension("json.tmp");
            let text = serde_json::json!({ "offline": offline }).to_string();
            if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file)) {
                eprintln!("Pitwall: the last known Offline mode could not be kept ({e})");
            }
        }
        if offline {
            for f in self.watchers.lock().unwrap_or_else(|p| p.into_inner()).iter() {
                f();
            }
        }
    }

    /// What the shared updater asks before every check and download (`UpdatesBuilder::offline_mode`): on, off, or unknown
    /// (never confirmed; blocks, and says so). Last seen off allows an update while the server is down, since an update
    /// may be what repairs a broken service.
    pub fn state(&self) -> paddock_updater::OfflineState {
        match self.get() {
            Some(true) => paddock_updater::OfflineState::On,
            Some(false) => paddock_updater::OfflineState::Off,
            None => paddock_updater::OfflineState::Unknown,
        }
    }

    /// The last value seen, `None` when the server never answered.
    pub fn get(&self) -> Option<bool> {
        match self.value.load(std::sync::atomic::Ordering::SeqCst) {
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

    /// Offline mode blocks updates, and so does a value never seen (fail closed, said as unknown); last seen off allows them.
    #[test]
    fn the_updater_sees_on_off_or_unknown() {
        use paddock_updater::OfflineState;
        let known = OfflineKnown::default();
        assert_eq!(known.state(), OfflineState::Unknown, "never known");
        known.set(true);
        assert_eq!(known.state(), OfflineState::On);
        known.set(false);
        assert_eq!(known.state(), OfflineState::Off);
    }

    /// The last confirmed value survives a restart (the file in the data folder); a missing or damaged file is unknown.
    #[test]
    fn the_last_confirmed_value_is_kept_across_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("offline-known.json");
        assert_eq!(OfflineKnown::at(file.clone()).get(), None, "first run: unknown");
        OfflineKnown::at(file.clone()).set(false);
        assert_eq!(OfflineKnown::at(file.clone()).get(), Some(false));
        OfflineKnown::at(file.clone()).set(true);
        assert_eq!(OfflineKnown::at(file.clone()).get(), Some(true));
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(OfflineKnown::at(file).get(), None, "damaged: unknown");
    }

    /// The watchers run when Offline mode turns on, once per change, never when it turns off or stays the same.
    #[test]
    fn the_watchers_run_when_offline_mode_turns_on() {
        let known = OfflineKnown::default();
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = n.clone();
        known.watch(move || {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        known.set(false);
        known.set(true);
        known.set(true);
        known.set(false);
        known.set(true);
        assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 2);
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
