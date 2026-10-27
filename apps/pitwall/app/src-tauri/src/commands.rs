//! The Tauri commands: one per management method (names and argument shapes are pinned by `tauri-backend.ts`), plus
//! the app's own (service, autostart, updates). Commands are thin: they validate every argument, forward to the
//! [`ServerClient`] with a per-method timeout and return the server's JSON. They never touch HTTP or the file system.
//!
//! Errors are `pitwall_core::mgmt::CommandError`. When the server is not running every command answers
//! `ServerUnavailable` with the next step in the message.

use crate::server_client::ServerClient;
use pitwall_core::config::UiSettings;
use pitwall_core::limits::AppPermissions;
use pitwall_core::memory::Preset;
use pitwall_core::mgmt::{CommandError, ErrorCode, HistoryRange, PlaygroundRequest, PresetDraft, UpdateSchedule, UsageRange};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;

/// The client the commands share.
pub type SharedClient = Arc<dyn ServerClient>;

/// Longest text argument accepted (ids, names, repository paths, search queries).
pub const MAX_TEXT: usize = 1024;
/// Most chat messages in one request, and most bytes of their text together.
pub const MAX_CHAT_MESSAGES: usize = 512;
/// See [`MAX_CHAT_MESSAGES`].
pub const MAX_CHAT_BYTES: usize = 8 * 1024 * 1024;

/// How long a method may take before the app answers for the server. Short for reads; long for work the server does
/// itself (loading a big model, a benchmark); the UI shows an inline error with Retry when one fires.
pub fn timeout_for(method: &str) -> Duration {
    Duration::from_secs(match method {
        "load_model" | "run_benchmark" => 600,
        "stop_serving" => 30,
        "delete_model" | "install_build" | "remove_build" | "select_build" | "rollback_build" => 120,
        "search_models" | "repo_files" | "download_model" | "release_memory" | "unload_model" => 60,
        _ => 15,
    })
}

/// Checks one argument before it is forwarded. The server validates again; this keeps hostile or buggy webview input
/// from being relayed at all.
pub trait Validate {
    /// `Err` names the field.
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        let _ = field;
        Ok(())
    }
}

fn bad(field: &str, why: &str) -> CommandError {
    CommandError::new(ErrorCode::BadRequest, format!("{field}: {why}"))
}

impl Validate for String {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        if self.len() > MAX_TEXT {
            return Err(bad(field, "too long"));
        }
        if self.chars().any(char::is_control) {
            return Err(bad(field, "must not contain control characters"));
        }
        Ok(())
    }
}

impl Validate for Option<String> {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        self.as_ref().map_or(Ok(()), |s| s.validate(field))
    }
}

impl Validate for bool {}
impl Validate for u32 {}
impl Validate for Option<u32> {}
impl Validate for Preset {}
impl Validate for HistoryRange {}
impl Validate for UsageRange {}
impl Validate for UpdateSchedule {}
impl Validate for UiSettings {}

impl Validate for AppPermissions {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        if self.models.as_ref().is_some_and(|m| m.len() > 4096) {
            return Err(bad(field, "too many models"));
        }
        self.limits.validate().map_err(|why| bad(field, &why))?;
        Ok(())
    }
}

impl Validate for PresetDraft {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        self.alias.validate(field)?;
        self.target.validate(field)?;
        // The server re-validates (and refuses over its own limit); this only stops absurd sizes being relayed.
        if self
            .system_prompt
            .as_ref()
            .is_some_and(|p| p.len() > 4 * pitwall_core::presets::MAX_SYSTEM_PROMPT_BYTES)
        {
            return Err(bad(field, "the system prompt is too long"));
        }
        Ok(())
    }
}

impl Validate for pitwall_core::cache::ClearRequest {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        use pitwall_core::cache::ClearRequest;
        match self {
            ClearRequest::App { app_id: s } | ClearRequest::Model { model_id: s } => s.validate(field),
            _ => Ok(()),
        }
    }
}

impl Validate for PlaygroundRequest {
    fn validate(&self, field: &str) -> Result<(), CommandError> {
        if self.model_id.len() > MAX_TEXT {
            return Err(bad(field, "model id too long"));
        }
        if self.messages.is_empty() {
            return Err(bad(field, "needs at least one message"));
        }
        if self.messages.len() > MAX_CHAT_MESSAGES {
            return Err(bad(field, "too many messages"));
        }
        if self.messages.iter().map(|m| m.content.len()).sum::<usize>() > MAX_CHAT_BYTES {
            return Err(bad(field, "the conversation is too large"));
        }
        if !(0.0..=4.0).contains(&self.temperature) || self.max_tokens == 0 || self.max_tokens > 1_000_000 {
            return Err(bad(field, "temperature must be 0 to 4 and max tokens 1 to 1000000"));
        }
        Ok(())
    }
}

/// Forwards one method.
pub async fn forward(client: &dyn ServerClient, method: &str, args: Value) -> Result<Value, CommandError> {
    let timeout = timeout_for(method);
    // The transport has its own timeout; this one also covers a client that never returns.
    match tokio::time::timeout(timeout + Duration::from_secs(2), client.call(method, args, timeout)).await {
        Ok(r) => r,
        Err(_) => Err(crate::server_client::timed_out(timeout)),
    }
}

macro_rules! rpc {
    ($(#[$doc:meta])* $name:ident => $method:literal $(, $arg:ident : $ty:ty = $wire:literal)* $(,)?) => {
        $(#[$doc])*
        #[tauri::command]
        pub async fn $name(client: State<'_, SharedClient> $(, $arg: $ty)*) -> Result<Value, CommandError> {
            $( Validate::validate(&$arg, $wire)?; )*
            forward(client.as_ref(), $method, json!({ $( $wire: $arg ),* })).await
        }
    };
}

rpc!(
    /// The dashboard snapshot.
    overview => "overview");
rpc!(
    /// Metrics history.
    history => "history", range: HistoryRange = "range");
rpc!(
    /// Per-app usage over a range (one app, or all).
    usage_series => "usage_series", range: UsageRange = "range", app_id: Option<String> = "appId");
rpc!(
    /// One app in detail: usage, limits and its latest requests.
    app_detail => "app_detail", app_id: String = "appId");
rpc!(
    /// The state of the model update checks (schedule, last check, next check, offline).
    update_check_status => "update_check_status");
rpc!(
    /// The model updates on offer (newer revisions, better quantisations), not dismissed.
    update_notices => "update_notices");
rpc!(
    /// Asks Hugging Face now (repository ids of downloaded models only; refused in offline mode).
    check_updates_now => "check_updates_now");
rpc!(
    /// Turns the scheduled update check on or off and sets how often it runs.
    set_update_check => "set_update_check", enabled: bool = "enabled", schedule: UpdateSchedule = "schedule");
rpc!(
    /// Hides an update notice until the offer changes, or snoozes it for some days.
    dismiss_update => "dismiss_update", id: String = "id", snooze_days: Option<u32> = "snoozeDays");
rpc!(
    /// Model presets and aliases with where each stands now.
    presets => "presets");
rpc!(
    /// Creates a preset.
    create_preset => "create_preset", preset: PresetDraft = "preset");
rpc!(
    /// Replaces (or renames) a preset.
    update_preset => "update_preset", alias: String = "alias", preset: PresetDraft = "preset");
rpc!(
    /// Deletes a preset.
    delete_preset => "delete_preset", alias: String = "alias");
rpc!(
    /// The editor's live check of a draft: problems and what its target resolves to now.
    resolve_preset => "resolve_preset", preset: PresetDraft = "preset", editing: Option<String> = "editing");
rpc!(
    /// Recent events.
    events => "events", limit: u32 = "limit");
rpc!(
    /// Pauses or resumes serving.
    set_serving => "set_serving", paused: bool = "paused");
rpc!(
    /// Unloads every model that may be unloaded.
    release_memory => "release_memory");
rpc!(
    /// What the caches hold (ADR 0019).
    cache_info => "cache_info");
rpc!(
    /// Clears cached answers, embeddings and saved prompt prefixes in a scope.
    clear_cache => "clear_cache", scope: pitwall_core::cache::ClearRequest = "scope");
rpc!(
    /// Applies a memory preset.
    apply_preset => "apply_preset", preset: Preset = "preset");
rpc!(
    /// Cancels a queued or running job.
    cancel_job => "cancel_job", id: String = "id");
rpc!(
    /// Models on disk.
    local_models => "local_models");
rpc!(
    /// Searches Hugging Face.
    search_models => "search_models", query: String = "query");
rpc!(
    /// Lists a repository's files.
    repo_files => "repo_files", repo: String = "repo");
rpc!(
    /// Starts a model download.
    download_model => "download_model", repo: String = "repo", file: String = "file", projector: Option<String> = "projector");
rpc!(
    /// Downloads in progress.
    downloads => "downloads");
rpc!(
    /// Cancels a download.
    cancel_download => "cancel_download", id: String = "id");
rpc!(
    /// Deletes a model from disk.
    delete_model => "delete_model", id: String = "id");
rpc!(
    /// Loads a model now.
    load_model => "load_model", id: String = "id");
rpc!(
    /// Unloads a model.
    unload_model => "unload_model", id: String = "id");
rpc!(
    /// Pins or unpins a model in memory.
    pin_model => "pin_model", id: String = "id", pinned: bool = "pinned");
rpc!(
    /// Sets a model's idle release time.
    set_model_idle => "set_model_idle", id: String = "id", idle_secs: Option<u32> = "idleSecs");
rpc!(
    /// Sets a model's parallel slots (`None`: the default); applies on its next load.
    set_model_slots => "set_model_slots", id: String = "id", slots: Option<u32> = "slots");
rpc!(
    /// How much context and concurrency fit for a model.
    model_fit => "model_fit", id: String = "id", context: Option<u32> = "context", slots: Option<u32> = "slots");
rpc!(
    /// Disk use of models and engines.
    disk_usage => "disk_usage");
rpc!(
    /// Engines and their builds.
    engines => "engines");
rpc!(
    /// Installs an engine build.
    install_build => "install_build", build_id: String = "buildId");
rpc!(
    /// Tries the automatic llama.cpp install again.
    retry_engine_setup => "retry_engine_setup");
rpc!(
    /// Removes an installed build.
    remove_build => "remove_build", build_id: String = "buildId", tag: String = "tag");
rpc!(
    /// Chooses the build an engine uses.
    select_build => "select_build", choice: String = "choice");
rpc!(
    /// Goes back to the previous build.
    rollback_build => "rollback_build");
rpc!(
    /// The engine pins, what roll back goes to, and why not when it cannot.
    engine_pins => "engine_pins");
rpc!(
    /// Pins one build for all models (`None`: unpins).
    set_global_pin => "set_global_pin", build_id: Option<String> = "buildId");
rpc!(
    /// Pins one build for one model (`None`: unpins).
    set_model_pin => "set_model_pin", model_id: String = "modelId", build_id: Option<String> = "buildId");
rpc!(
    /// What each installed build supports.
    build_capabilities => "build_capabilities");
rpc!(
    /// Engine packs.
    packs => "packs");
rpc!(
    /// Removes something an older version left on disk (a llamafile download), on the user's request.
    remove_leftover => "remove_leftover", id: String = "id");
rpc!(
    /// Registered apps.
    apps => "apps");
rpc!(
    /// Registers an app.
    create_app => "create_app", name: String = "name");
rpc!(
    /// Removes an app.
    delete_app => "delete_app", id: String = "id");
rpc!(
    /// Makes a key for an app (shown once).
    create_key => "create_key", app_id: String = "appId");
rpc!(
    /// Revokes a key.
    revoke_key => "revoke_key", app_id: String = "appId", key_id: String = "keyId");
rpc!(
    /// Changes what an app may do.
    set_permissions => "set_permissions", app_id: String = "appId", permissions: AppPermissions = "permissions");
rpc!(
    /// Approves a pairing request.
    approve_pairing => "approve_pairing", id: String = "id", permissions: AppPermissions = "permissions");
rpc!(
    /// Denies a pairing request.
    deny_pairing => "deny_pairing", id: String = "id");
rpc!(
    /// Pending pairing requests.
    pairings => "pairings");
rpc!(
    /// Recent requests.
    requests => "requests", limit: u32 = "limit");
rpc!(
    /// One request's content.
    request_content => "request_content", id: String = "id");
rpc!(
    /// Past benchmark results.
    benchmarks => "benchmarks");
rpc!(
    /// Runs a benchmark.
    run_benchmark => "run_benchmark", model_id: String = "modelId");
rpc!(
    /// The settings.
    settings => "settings");
rpc!(
    /// Saves the settings.
    save_settings => "save_settings", settings: UiSettings = "settings");

/// The playground streams that are running, so they can be cancelled.
#[derive(Default)]
pub struct Streams {
    next: AtomicU64,
    tasks: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl Streams {
    fn register(&self, task: impl FnOnce(String) -> tauri::async_runtime::JoinHandle<()>) -> String {
        let id = format!("chat-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1);
        // The table stays locked from spawning to inserting: a task that ends at once blocks in `finish` until its
        // handle is in, so `finish` always removes it and no finished stream is left behind.
        let mut tasks = self.tasks.lock().unwrap_or_else(|p| p.into_inner());
        let handle = task(id.clone());
        tasks.insert(id.clone(), handle);
        id
    }

    fn finish(&self, id: &str) {
        self.tasks.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }

    /// Stops a stream; an unknown or finished id is fine.
    pub fn cancel(&self, id: &str) {
        if let Some(h) = self.tasks.lock().unwrap_or_else(|p| p.into_inner()).remove(id) {
            h.abort();
        }
    }

    /// How many streams are running.
    pub fn running(&self) -> usize {
        self.tasks.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// Where a stream's events go (the UI's channel, or a test's collector).
pub type EventSink = Arc<dyn Fn(Value) + Send + Sync>;

/// Opens the playground stream and forwards its events to `sink` until it ends; returns the stream's id. An error
/// opening the stream is returned (not sent); an error mid-stream is sent as `{kind: "error", error}` and ends it.
pub async fn start_chat(
    client: SharedClient,
    streams: Arc<Streams>,
    request: PlaygroundRequest,
    sink: EventSink,
) -> Result<String, CommandError> {
    request.validate("request")?;
    let body = serde_json::to_value(&request).map_err(|e| CommandError::new(ErrorCode::BadRequest, e.to_string()))?;
    let mut stream = client.open_stream("/mgmt/v1/chat", Some(body), Duration::from_secs(30)).await?;
    let s2 = streams.clone();
    Ok(streams.register(move |id| {
        tauri::async_runtime::spawn(async move {
            loop {
                match stream.next_frame().await {
                    Ok(Some(frame)) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&frame) {
                            sink(v);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        sink(json!({ "kind": "error", "error": e }));
                        break;
                    }
                }
            }
            s2.finish(&id);
        })
    }))
}

/// Streams a playground chat into `onEvent`; returns the stream id for `chat_cancel`.
#[tauri::command]
pub async fn chat(
    client: State<'_, SharedClient>,
    streams: State<'_, Arc<Streams>>,
    request: PlaygroundRequest,
    on_event: Channel<Value>,
) -> Result<String, CommandError> {
    let sink: EventSink = Arc::new(move |v| {
        let _ = on_event.send(v);
    });
    start_chat(client.inner().clone(), streams.inner().clone(), request, sink).await
}

/// Cancels a playground stream.
#[tauri::command]
pub async fn chat_cancel(streams: State<'_, Arc<Streams>>, id: String) -> Result<(), CommandError> {
    cancel_chat(streams.inner().clone(), id).await
}

/// [`chat_cancel`] without Tauri state. It is async and takes the stream table on a blocking thread, so a cancel is
/// never queued behind a chat on the main thread or a busy async worker.
pub async fn cancel_chat(streams: Arc<Streams>, id: String) -> Result<(), CommandError> {
    id.validate("id")?;
    tauri::async_runtime::spawn_blocking(move || streams.cancel(&id))
        .await
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("cancelling the chat stopped unexpectedly: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::{FakeServer, Reply};
    use pitwall_core::mgmt::PlaygroundMessage;

    fn request() -> PlaygroundRequest {
        PlaygroundRequest {
            model_id: "m".into(),
            messages: vec![PlaygroundMessage {
                role: pitwall_core::chat::Role::User,
                content: "hi".into(),
            }],
            temperature: 0.7,
            max_tokens: 256,
        }
    }

    #[test]
    fn long_or_hostile_text_is_refused_before_it_is_forwarded() {
        assert!("ok/model.gguf".to_string().validate("id").is_ok());
        assert_eq!("a".repeat(MAX_TEXT + 1).validate("id").unwrap_err().code, ErrorCode::BadRequest);
        let e = "a\u{0}b".to_string().validate("name").unwrap_err();
        assert!(e.message.starts_with("name:"), "{}", e.message);
        assert!(Some("x".to_string()).validate("p").is_ok());
        assert!(Option::<String>::None.validate("p").is_ok());
        assert!(Some("a\nb".to_string()).validate("p").is_err());
    }

    #[test]
    fn a_chat_request_is_bounded() {
        assert!(request().validate("r").is_ok());
        let mut r = request();
        r.messages.clear();
        assert!(r.validate("r").is_err());
        let mut r = request();
        r.temperature = 9.0;
        assert!(r.validate("r").is_err());
        let mut r = request();
        r.max_tokens = 0;
        assert!(r.validate("r").is_err());
        let mut r = request();
        r.messages = (0..=MAX_CHAT_MESSAGES).map(|_| r.messages[0].clone()).collect();
        assert!(r.validate("r").is_err());
        let mut r = request();
        r.messages[0].content = "x".repeat(MAX_CHAT_BYTES + 1);
        assert!(r.validate("r").is_err());
        let mut r = request();
        r.model_id = "m".repeat(MAX_TEXT + 1);
        assert!(r.validate("r").is_err());
        assert!(AppPermissions::default().validate("p").is_ok());
        assert!(pitwall_core::cache::ClearRequest::All.validate("scope").is_ok());
        assert!(pitwall_core::cache::ClearRequest::App { app_id: "debrief".into() }
            .validate("scope")
            .is_ok());
        assert!(pitwall_core::cache::ClearRequest::Model {
            model_id: "m".repeat(MAX_TEXT + 1)
        }
        .validate("scope")
        .is_err());
        let draft = |alias: &str| PresetDraft {
            alias: alias.into(),
            target: "m".into(),
            context: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            system_prompt: None,
            cache_responses: false,
            keep_warm: false,
        };
        assert!(draft("debrief-explain").validate("preset").is_ok());
        assert!(draft(&"a".repeat(MAX_TEXT + 1)).validate("preset").is_err());
        let mut d = draft("ok");
        d.system_prompt = Some("x".repeat(4 * pitwall_core::presets::MAX_SYSTEM_PROMPT_BYTES + 1));
        assert!(d.validate("preset").is_err());
        assert!(UsageRange::Week.validate("range").is_ok());
    }

    #[test]
    fn nonsense_limits_are_refused_before_they_are_forwarded() {
        let mut p = AppPermissions::default();
        p.limits.max_memory_percent = Some(150);
        let e = p.validate("permissions").unwrap_err();
        assert!(
            e.message.starts_with("permissions:") && e.message.contains("memory share"),
            "{}",
            e.message
        );
        let mut p = AppPermissions::default();
        p.limits.requests_per_day = Some(0);
        assert!(p.validate("permissions").is_err());
    }

    #[test]
    fn every_method_has_a_timeout_and_slow_ones_get_more() {
        assert_eq!(timeout_for("overview"), Duration::from_secs(15));
        assert!(timeout_for("load_model") >= Duration::from_secs(300));
        assert!(timeout_for("search_models") > timeout_for("overview"));
        assert!(timeout_for("anything_else") > Duration::ZERO);
    }

    #[tokio::test]
    async fn forward_passes_arguments_and_results_through() {
        let f = FakeServer::new();
        f.reply("set_serving", Reply::Ok(Value::Null));
        forward(&f, "set_serving", json!({"paused": true})).await.unwrap();
        assert_eq!(f.calls(), vec![("set_serving".to_string(), json!({"paused": true}))]);
    }

    #[tokio::test]
    async fn forward_reports_a_missing_server_with_the_next_step() {
        let f = FakeServer::new();
        f.reply("overview", Reply::Err(crate::server_client::unavailable()));
        let e = forward(&f, "overview", Value::Null).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("Repair service"));
    }

    #[tokio::test(start_paused = true)]
    async fn forward_gives_up_when_a_client_never_answers() {
        struct Never;
        impl ServerClient for Never {
            fn call<'a>(&'a self, _: &'a str, _: Value, _: Duration) -> crate::server_client::BoxFuture<'a, Result<Value, CommandError>> {
                Box::pin(std::future::pending())
            }
            fn open_stream<'a>(
                &'a self,
                _: &'a str,
                _: Option<Value>,
                _: Duration,
            ) -> crate::server_client::BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
                Box::pin(std::future::pending())
            }
        }
        let e = forward(&Never, "overview", Value::Null).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("Retry"), "{}", e.message);
    }

    fn collector() -> (EventSink, Arc<Mutex<Vec<Value>>>) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        (Arc::new(move |v| g.lock().unwrap().push(v)), got)
    }

    /// Waits for `cond`; the 60 s limit only guards against a hang, it is never the thing a test depends on.
    async fn wait_until(mut cond: impl FnMut() -> bool) {
        let limit = std::time::Instant::now() + Duration::from_secs(60);
        while !cond() {
            assert!(std::time::Instant::now() < limit, "condition not reached (hang guard)");
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn a_task_that_ends_before_it_is_registered_is_not_left_in_the_table() {
        let streams = Arc::new(Streams::default());
        for _ in 0..500 {
            let s = streams.clone();
            streams.register(move |id| tauri::async_runtime::spawn(async move { s.finish(&id) }));
        }
        let limit = std::time::Instant::now() + Duration::from_secs(60);
        while streams.running() != 0 {
            assert!(std::time::Instant::now() < limit, "stale entries: {}", streams.running());
            std::thread::yield_now();
        }
    }

    #[tokio::test]
    async fn a_chat_streams_events_to_the_sink_and_cleans_up() {
        let f = Arc::new(FakeServer::new());
        f.stream(Ok(vec![
            json!({"kind": "delta", "text": "Hel"}).to_string(),
            "junk".into(),
            json!({"kind": "delta", "text": "lo"}).to_string(),
        ]));
        let streams = Arc::new(Streams::default());
        let (sink, got) = collector();
        let client: SharedClient = f;
        let id = start_chat(client, streams.clone(), request(), sink).await.unwrap();
        assert_eq!(id, "chat-1");
        wait_until(|| got.lock().unwrap().len() == 2).await;
        wait_until(|| streams.running() == 0).await;
        assert_eq!(got.lock().unwrap()[0]["text"], "Hel");
    }

    #[tokio::test]
    async fn opening_a_chat_on_a_missing_server_fails_with_the_next_step() {
        let f: SharedClient = Arc::new(FakeServer::new());
        let (sink, got) = collector();
        let e = start_chat(f, Arc::new(Streams::default()), request(), sink).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(got.lock().unwrap().is_empty());
        // Invalid requests never reach the server.
        let mut r = request();
        r.max_tokens = 0;
        let f: SharedClient = Arc::new(FakeServer::new());
        let (sink, _) = collector();
        assert_eq!(
            start_chat(f, Arc::new(Streams::default()), r, sink).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[tokio::test]
    async fn a_stream_that_breaks_reports_an_error_event_and_a_cancel_of_an_unknown_id_is_fine() {
        struct Breaks;
        impl crate::server_client::FrameStream for Breaks {
            fn next_frame(&mut self) -> crate::server_client::BoxFuture<'_, Result<Option<String>, CommandError>> {
                Box::pin(async { Err(crate::server_client::unavailable()) })
            }
        }
        struct Client;
        impl ServerClient for Client {
            fn call<'a>(&'a self, _: &'a str, _: Value, _: Duration) -> crate::server_client::BoxFuture<'a, Result<Value, CommandError>> {
                Box::pin(async { Ok(Value::Null) })
            }
            fn open_stream<'a>(
                &'a self,
                _: &'a str,
                _: Option<Value>,
                _: Duration,
            ) -> crate::server_client::BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
                Box::pin(async { Ok(Box::new(Breaks) as Box<dyn crate::server_client::FrameStream>) })
            }
        }
        let streams = Arc::new(Streams::default());
        let (sink, got) = collector();
        start_chat(Arc::new(Client), streams.clone(), request(), sink).await.unwrap();
        wait_until(|| !got.lock().unwrap().is_empty()).await;
        let ev = got.lock().unwrap()[0].clone();
        assert_eq!(ev["kind"], "error");
        assert_eq!(ev["error"]["code"], "serverUnavailable");
        streams.cancel("chat-99");
    }

    #[tokio::test]
    async fn cancelling_stops_a_running_stream() {
        struct Slow;
        impl crate::server_client::FrameStream for Slow {
            fn next_frame(&mut self) -> crate::server_client::BoxFuture<'_, Result<Option<String>, CommandError>> {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Ok(None)
                })
            }
        }
        struct Client;
        impl ServerClient for Client {
            fn call<'a>(&'a self, _: &'a str, _: Value, _: Duration) -> crate::server_client::BoxFuture<'a, Result<Value, CommandError>> {
                Box::pin(async { Ok(Value::Null) })
            }
            fn open_stream<'a>(
                &'a self,
                _: &'a str,
                _: Option<Value>,
                _: Duration,
            ) -> crate::server_client::BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
                Box::pin(async { Ok(Box::new(Slow) as Box<dyn crate::server_client::FrameStream>) })
            }
        }
        let streams = Arc::new(Streams::default());
        let (sink, _) = collector();
        let id = start_chat(Arc::new(Client), streams.clone(), request(), sink).await.unwrap();
        assert_eq!(streams.running(), 1);
        streams.cancel(&id);
        assert_eq!(streams.running(), 0);
    }

    #[tokio::test]
    async fn a_chat_can_be_cancelled_while_its_stream_hogs_a_runtime_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Hog(Arc<AtomicBool>);
        impl crate::server_client::FrameStream for Hog {
            fn next_frame(&mut self) -> crate::server_client::BoxFuture<'_, Result<Option<String>, CommandError>> {
                let release = self.0.clone();
                // Blocks its runtime thread (a chat busy in a long synchronous step) until the test lets go.
                Box::pin(async move {
                    while !release.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(None)
                })
            }
        }
        struct Client(Arc<AtomicBool>);
        impl ServerClient for Client {
            fn call<'a>(&'a self, _: &'a str, _: Value, _: Duration) -> crate::server_client::BoxFuture<'a, Result<Value, CommandError>> {
                Box::pin(async { Ok(Value::Null) })
            }
            fn open_stream<'a>(
                &'a self,
                _: &'a str,
                _: Option<Value>,
                _: Duration,
            ) -> crate::server_client::BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
                let r = self.0.clone();
                Box::pin(async move { Ok(Box::new(Hog(r)) as Box<dyn crate::server_client::FrameStream>) })
            }
        }
        let release = Arc::new(AtomicBool::new(false));
        let streams = Arc::new(Streams::default());
        let (sink, _) = collector();
        let id = start_chat(Arc::new(Client(release.clone())), streams.clone(), request(), sink)
            .await
            .unwrap();
        assert_eq!(streams.running(), 1);
        let done = tokio::time::timeout(Duration::from_secs(5), cancel_chat(streams.clone(), id)).await;
        release.store(true, Ordering::SeqCst);
        assert!(done.expect("the cancel was not answered while the chat hogged a thread").is_ok());
        assert_eq!(streams.running(), 0);
        assert!(cancel_chat(streams.clone(), "a\u{0}b".into()).await.is_err(), "ids are validated");
        assert!(cancel_chat(streams, "chat-99".into()).await.is_ok(), "an unknown id is fine");
    }
}
