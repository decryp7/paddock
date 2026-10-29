//! Pitwall updating itself with the Tauri updater (minisign-signed, opt-in). Wired but inert in unsigned builds.
//!
//! The public key comes from the build (CI secret `TAURI_UPDATER_PUBKEY`, read with `option_env!`) or from
//! `plugins.updater.pubkey` in `tauri.conf.json` (empty in the repository). A build without a key never checks and says
//! "Updates are not available in this build". Checks happen only when the user asks. After an install the app restarts;
//! the server restarts itself once it is idle and finds its own program replaced. (Adapted from Pitcrew's updater.)

use crate::commands::forward;
use crate::server_client::ServerClient;
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

pub use tauri_plugin_updater::Update;

/// A value kept for later that can be read any number of times (reading never removes it).
struct Slot<T>(Mutex<Option<T>>);

impl<T: Clone> Slot<T> {
    fn set(&self, v: Option<T>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = v;
    }
    fn get(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot(Mutex::new(None))
    }
}

/// The update found by the last check, kept for the install. An install that is cancelled or fails leaves it here, so
/// the next try needs no new check.
#[derive(Default)]
pub struct Pending(Slot<Update>);

/// Counts the bytes of a download from the plugin's chunk callbacks: `done` only grows, `total` is the largest size told.
#[derive(Debug, Default)]
pub struct ByteCounter {
    done: u64,
    total: Option<u64>,
}

impl ByteCounter {
    /// One chunk of `len` bytes arrived; `total` is the size the server announced, when it did. Returns `(done, total)`.
    pub fn chunk(&mut self, len: usize, total: Option<u64>) -> (u64, Option<u64>) {
        self.done = self.done.saturating_add(len as u64);
        self.total = match (self.total, total) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        (self.done, self.total.map(|t| t.max(self.done)))
    }
}

/// What an install reports while it works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallProgress {
    /// Bytes downloaded so far and the size when known.
    Downloading {
        /// Bytes so far.
        done: u64,
        /// The size, once known.
        total: Option<u64>,
    },
    /// The download is complete; the update is verified and installed.
    Installing,
}

/// The update public key given to this build. Cargo rebuilds when it changes (build.rs).
const BUILD_PUBKEY: Option<&str> = option_env!("TAURI_UPDATER_PUBKEY");

/// `AppUpdateStatus` in `api.types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    /// This build can update itself.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
    /// The running version.
    pub current_version: String,
}

/// An update found by a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Its version.
    pub version: String,
    /// Release notes.
    pub notes: Option<String>,
}

/// The plugin. Its own IPC commands are granted to no window: the UI goes through Pitwall's commands.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The key to verify updates with: the build's, else the configured one; `None` when neither looks like a Tauri updater
/// public key (base64 of a minisign public key file).
pub fn resolve_pubkey(build: Option<&str>, config: Option<&str>) -> Option<String> {
    let pick = |k: Option<&str>| k.map(str::trim).filter(|k| !k.is_empty()).map(str::to_string);
    let key = pick(build).or_else(|| pick(config))?;
    looks_like_public_key(&key).then_some(key)
}

/// A minisign public key file is two lines (~ 100 bytes); Tauri stores it base64-encoded. This is a sanity check against
/// pasting the wrong secret (e.g. the private key, which is much longer); the plugin verifies for real.
fn looks_like_public_key(key: &str) -> bool {
    (80..=400).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

fn key<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|c| c.get("pubkey"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    resolve_pubkey(BUILD_PUBKEY, configured.as_deref())
}

/// Whether this build can update itself.
pub fn status<R: Runtime>(app: &AppHandle<R>) -> AppUpdateStatus {
    status_for(key(app).is_some(), app.package_info().version.to_string())
}

fn status_for(has_key: bool, version: String) -> AppUpdateStatus {
    AppUpdateStatus {
        available: has_key,
        reason: (!has_key).then(|| "Updates are not available in this build.".to_string()),
        current_version: version,
    }
}

/// What an update check or install says when the server is in offline mode.
pub const OFFLINE_MESSAGE: &str =
    "Pitwall is in offline mode, so it does not look for or download updates. Turn offline mode off in Settings > Network to update.";
/// What it says when offline mode cannot be confirmed because the server does not answer and was never seen online.
pub const UNCONFIRMED_MESSAGE: &str = "Pitwall cannot confirm offline mode is off because the background service is not answering. Start or repair the service, then check again.";

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

/// Refuses while offline mode is on, or cannot be confirmed to be off. The server is the source of truth
/// (`update_check_status.offline`); when it cannot be asked (or does not say), the last value seen is used: offline refuses,
/// online allows (an app update may be what repairs a broken service), and never seen refuses with the next step.
pub async fn refuse_when_offline(client: &dyn ServerClient, known: &OfflineKnown) -> Result<(), CommandError> {
    if let Ok(status) = forward(client, "update_check_status", Value::Null).await {
        known.observe("update_check_status", &status);
    }
    match known.get() {
        Some(true) => Err(CommandError::new(ErrorCode::Offline, OFFLINE_MESSAGE)),
        Some(false) => Ok(()),
        None => Err(CommandError::new(ErrorCode::ServerUnavailable, UNCONFIRMED_MESSAGE)),
    }
}

/// Checks for a release (one HTTPS GET of latest.json; signatures are verified at install).
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<UpdateInfo>, CommandError> {
    let Some(k) = key(app) else {
        return Err(CommandError::new(
            ErrorCode::NotAvailable,
            "Updates are not available in this build.",
        ));
    };
    let updater = app
        .updater_builder()
        .pubkey(k)
        .build()
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the updater could not be set up: {e}")))?;
    let found = updater
        .check()
        .await
        .map_err(|e| CommandError::new(ErrorCode::Offline, format!("checking for updates failed: {e}")))?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
    });
    app.state::<Pending>().0.set(found);
    Ok(info)
}

/// Downloads, verifies and installs the update found by [`check`], then restarts the app (the server keeps serving).
/// `report` gets the byte progress of the download; `before_install` runs after it and before anything is installed. The pending update stays, so a cancelled or failed install can be
/// tried again.
pub async fn install<R: Runtime, F: std::future::Future<Output = Result<(), CommandError>>>(
    app: &AppHandle<R>,
    report: impl Fn(InstallProgress) + Send + Sync,
    before_install: impl FnOnce() -> F,
) -> Result<(), CommandError> {
    let Some(update) = app.state::<Pending>().0.get() else {
        return Err(CommandError::new(ErrorCode::BadRequest, "check for updates first"));
    };
    let mut bytes = ByteCounter::default();
    let downloaded = update
        .download(
            |len, total| {
                let (done, total) = bytes.chunk(len, total);
                report(InstallProgress::Downloading { done, total });
            },
            || {},
        )
        .await
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the update could not be downloaded: {e}")))?;
    // Offline mode may have been turned on during the download: nothing is installed then.
    before_install().await?;
    report(InstallProgress::Installing);
    update
        .install(downloaded)
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the update could not be installed: {e}")))?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape of a Tauri updater public key (base64 of "untrusted comment: ...\nRW..."), not a real key.
    const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDAwMDAwMDAwMDAwMDAwMDAKUldRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";

    use crate::server_client::fake::{FakeServer, Reply};
    use serde_json::json;

    fn status(offline: Value) -> Reply {
        Reply::Ok(json!({ "offline": offline }))
    }

    #[tokio::test]
    async fn offline_mode_refuses_an_update_check_or_install_with_the_offline_code() {
        let server = FakeServer::new();
        server.reply("update_check_status", status(json!(true)));
        let known = OfflineKnown::default();
        let e = refuse_when_offline(&server, &known).await.unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::Offline, OFFLINE_MESSAGE));
        assert_eq!(known.get(), Some(true), "what the server said is remembered");
    }

    #[tokio::test]
    async fn online_allows_an_update() {
        let server = FakeServer::new();
        server.reply("update_check_status", status(json!(false)));
        let known = OfflineKnown::default();
        assert!(refuse_when_offline(&server, &known).await.is_ok());
        assert_eq!(known.get(), Some(false));
    }

    #[tokio::test]
    async fn a_server_that_cannot_confirm_offline_mode_is_refused_unless_it_was_last_seen_online() {
        let down = FakeServer::new();
        down.reply("update_check_status", Reply::Err(crate::server_client::unavailable()));
        let known = OfflineKnown::default();
        let e = refuse_when_offline(&down, &known).await.unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::ServerUnavailable, UNCONFIRMED_MESSAGE),
            "never known: refused"
        );
        // An answer without the field tells nothing either.
        let empty = FakeServer::new();
        empty.reply("update_check_status", Reply::Ok(json!({})));
        assert_eq!(
            refuse_when_offline(&empty, &known).await.unwrap_err().code,
            ErrorCode::ServerUnavailable
        );
        // Last seen online: an update may be the repair for a broken service.
        known.set(false);
        assert!(refuse_when_offline(&down, &known).await.is_ok());
        assert!(refuse_when_offline(&empty, &known).await.is_ok());
        // Last seen offline: refused as offline.
        known.set(true);
        assert_eq!(refuse_when_offline(&down, &known).await.unwrap_err().code, ErrorCode::Offline);
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

    #[test]
    fn a_pending_update_survives_being_read_so_a_cancelled_install_can_be_tried_again() {
        let slot: Slot<String> = Slot::default();
        assert_eq!(slot.get(), None);
        slot.set(Some("1.2.0".into()));
        // An install reads it, is cancelled, and the next try finds it again.
        assert_eq!(slot.get().as_deref(), Some("1.2.0"));
        assert_eq!(slot.get().as_deref(), Some("1.2.0"));
        slot.set(None);
        assert_eq!(slot.get(), None, "a new check that finds nothing clears it");
    }

    #[test]
    fn download_progress_counts_bytes_and_never_goes_backwards() {
        let mut c = ByteCounter::default();
        assert_eq!(c.chunk(100, None), (100, None), "size unknown yet: a growing count");
        assert_eq!(c.chunk(50, Some(1000)), (150, Some(1000)));
        // A later smaller or missing size does not shrink the total; a total below the bytes seen is raised to them.
        assert_eq!(c.chunk(50, Some(10)), (200, Some(1000)));
        assert_eq!(c.chunk(10, None), (210, Some(1000)));
        let mut c = ByteCounter::default();
        assert_eq!(c.chunk(500, Some(100)), (500, Some(500)));
        assert!(c.chunk(usize::MAX, None).0 >= 500, "huge chunks saturate instead of wrapping");
    }

    #[test]
    fn the_build_key_wins_and_junk_is_refused() {
        assert_eq!(resolve_pubkey(Some(KEY), Some("other")).as_deref(), Some(KEY));
        assert_eq!(resolve_pubkey(Some("  "), Some(KEY)).as_deref(), Some(KEY));
        assert_eq!(resolve_pubkey(None, Some("")), None);
        assert_eq!(resolve_pubkey(Some("not a key!"), None), None);
        assert_eq!(resolve_pubkey(Some(&"A".repeat(2000)), None), None, "a private key is far longer");
    }

    #[test]
    fn a_build_without_a_key_says_so_and_the_wire_shape_is_camel_case() {
        let s = status_for(false, "0.1.0".into());
        assert!(!s.available);
        assert_eq!(s.reason.as_deref(), Some("Updates are not available in this build."));
        assert!(status_for(true, "0.1.0".into()).available);
        let v = serde_json::to_value(s).unwrap();
        assert_eq!(v["currentVersion"], "0.1.0");
    }
}
