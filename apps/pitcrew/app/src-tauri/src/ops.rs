//! The commands for workflow insights, container engine hygiene, the Actions cache and the hand-off to Debrief. Each
//! is a thin edge: parse the webview's arguments, ask the service (or read the Gitea API with the owner's read token,
//! cached and bounded), off the UI thread.

use crate::commands::{ask, blocking, name, unexpected, SharedClient};
use crate::dto::CommandError;
use crate::events::Latest;
use crate::handoff::{self, DebriefStatus, Handoff};
use crate::{credentials, gitea};
use pitcrew_core::hygiene::{CacheLimits, EngineReport, PrunePreview, PruneResult, PruneSelection};
use pitcrew_core::insights::{self, Insights};
use pitcrew_core::protocol::{CacheInfo, CachePlan, Request, Response};
use pitcrew_core::time::Timestamp;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

/// Most days of history an insights request covers.
pub const MAX_INSIGHT_DAYS: u32 = 90;

/// `InsightsReply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsReply {
    /// The numbers and the advice.
    pub insights: Insights,
    /// Whether a Gitea read token is set for at least one runner's server.
    pub has_api_token: bool,
    /// Finished jobs whose outcome the server has not been asked about yet (each refresh asks for more).
    pub pending_outcomes: u32,
}

/// `fleet_insights`: the jobs of every runner over the last `days`, joined with what the Gitea API says (cached,
/// bounded lookups), aggregated by the core.
#[tauri::command]
pub async fn fleet_insights<R: Runtime>(
    app: AppHandle<R>,
    client: State<'_, SharedClient>,
    days: u32,
    utc_offset_minutes: i32,
) -> Result<InsightsReply, CommandError> {
    let days = days.clamp(1, MAX_INSIGHT_DAYS);
    let c = client.inner().clone();
    let cache = app
        .path()
        .app_data_dir()
        .map(|d| d.join("job-outcomes.json"))
        .map_err(|e| CommandError::internal(e.to_string()))?;
    let known = app.state::<Latest>().status().map(|s| s.runners).unwrap_or_default();
    blocking(move || {
        let mut jobs = vec![];
        let mut has_token = false;
        let mut pending = 0u32;
        let mut now = Timestamp(0);
        for r in &known {
            let Response::History { summary, .. } = ask(
                c.as_ref(),
                Request::History {
                    runner: r.name.clone(),
                    days,
                    utc_offset_minutes,
                },
            )?
            else {
                continue;
            };
            now = now.max(summary.to);
            let token = r.server_url.as_deref().and_then(|s| credentials::get(credentials::API_SERVICE, s));
            has_token |= token.is_some();
            let details = gitea::outcomes(&cache, &summary.recent_jobs, token.as_deref());
            pending += u32::try_from(
                summary
                    .recent_jobs
                    .iter()
                    .filter(|j| token.is_some() && j.info.job_id.is_some_and(|id| !details.contains_key(&id)))
                    .count(),
            )
            .unwrap_or(u32::MAX);
            jobs.extend(insights::from_finished(r.name.as_str(), &summary.recent_jobs, &details));
        }
        Ok(InsightsReply {
            insights: insights::compute(&jobs, now, days, utc_offset_minutes),
            has_api_token: has_token,
            pending_outcomes: pending,
        })
    })
    .await
}

/// `engine_report`.
#[tauri::command]
pub async fn engine_report(client: State<'_, SharedClient>) -> Result<EngineReport, CommandError> {
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), Request::EngineReport)? {
        Response::EngineReport { report } => Ok(report),
        other => Err(unexpected(other)),
    })
    .await
}

/// `engine_prune_preview`: a dry run.
#[tauri::command]
pub async fn engine_prune_preview(client: State<'_, SharedClient>, selection: PruneSelection) -> Result<PrunePreview, CommandError> {
    selection.validate().map_err(CommandError::bad)?;
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), Request::EnginePrunePreview { selection })? {
        Response::EnginePreview { preview } => Ok(preview),
        other => Err(unexpected(other)),
    })
    .await
}

/// `engine_prune`: removes what the selection names (the UI asks for confirmation after the preview).
#[tauri::command]
pub async fn engine_prune<R: Runtime>(
    app: AppHandle<R>,
    client: State<'_, SharedClient>,
    work: State<'_, crate::jobs::Work>,
    selection: PruneSelection,
) -> Result<u64, CommandError> {
    selection.validate().map_err(CommandError::bad)?;
    let c = client.inner().clone();
    let jobs = crate::jobs::registry(&app)?;
    work.start(
        &jobs,
        "pitcrew.engine-prune",
        "Pruning the container engine",
        None,
        move || -> Result<PruneResult, CommandError> {
            match ask(c.as_ref(), Request::EnginePrune { selection })? {
                Response::EnginePruned { result } => Ok(result),
                other => Err(unexpected(other)),
            }
        },
    )
}

/// `cache_report`.
#[tauri::command]
pub async fn cache_report(client: State<'_, SharedClient>, runner: String) -> Result<CacheInfo, CommandError> {
    let req = Request::CacheReport { runner: name(&runner)? };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::CacheReport { cache } => Ok(cache),
        other => Err(unexpected(other)),
    })
    .await
}

/// `cache_preview`: a dry run of trimming the cache.
#[tauri::command]
pub async fn cache_preview(client: State<'_, SharedClient>, runner: String, limits: CacheLimits) -> Result<CachePlan, CommandError> {
    limits.validate().map_err(CommandError::bad)?;
    let req = Request::CachePreview {
        runner: name(&runner)?,
        limits,
    };
    let c = client.inner().clone();
    blocking(move || match ask(c.as_ref(), req)? {
        Response::CachePreview { plan } => Ok(plan),
        other => Err(unexpected(other)),
    })
    .await
}

/// `CacheCleaned` for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleanedReply {
    /// What was deleted (or tried).
    pub plan: CachePlan,
    /// Entries that could not be deleted, with the reason.
    pub failures: Vec<(String, String)>,
}

/// `cache_clean`: trims the cache.
#[tauri::command]
pub async fn cache_clean<R: Runtime>(
    app: AppHandle<R>,
    client: State<'_, SharedClient>,
    work: State<'_, crate::jobs::Work>,
    runner: String,
    limits: CacheLimits,
) -> Result<u64, CommandError> {
    limits.validate().map_err(CommandError::bad)?;
    let n = name(&runner)?;
    let req = Request::CacheClean { runner: n.clone(), limits };
    let c = client.inner().clone();
    let jobs = crate::jobs::registry(&app)?;
    work.start(
        &jobs,
        "pitcrew.cache-clean",
        "Trimming the Actions cache",
        Some(n.as_str()),
        move || match ask(c.as_ref(), req)? {
            Response::CacheCleaned { plan, failures } => Ok(CacheCleanedReply { plan, failures }),
            other => Err(unexpected(other)),
        },
    )
}

/// `debrief_status`.
#[tauri::command]
pub async fn debrief_status() -> Result<DebriefStatus, CommandError> {
    blocking(|| Ok(handoff::status())).await
}

/// `open_in_debrief`: the runner's log (or one job's part of it, when `started_at` and `finished_at` are given) goes to
/// Debrief.
#[tauri::command]
pub async fn open_in_debrief<R: Runtime>(
    app: AppHandle<R>,
    runner: String,
    label: String,
    started_at: Option<u64>,
    finished_at: Option<u64>,
) -> Result<Handoff, CommandError> {
    let n = name(&runner)?;
    let dir = app
        .state::<Latest>()
        .status()
        .and_then(|s| s.runners.into_iter().find(|r| r.name == n))
        .map(|r| r.dir)
        .ok_or_else(|| CommandError::bad(format!("unknown runner {runner}")))?;
    let out = app
        .path()
        .app_data_dir()
        .map(|d| d.join("debrief-handoff"))
        .map_err(|e| CommandError::internal(e.to_string()))?;
    let window = match (started_at, finished_at) {
        (Some(a), Some(b)) if b >= a => Some((Timestamp(a), Timestamp(b))),
        (None, None) => None,
        _ => return Err(CommandError::bad("a job needs both its start and its end")),
    };
    blocking(move || {
        let log = std::path::Path::new(&dir).join("logs").join(pitcrew_core::layout::LOG_FILE);
        let path = std::env::var("PATH").ok();
        let env = |k: &str| std::env::var(k).ok();
        let found = handoff::find(path.as_deref(), &env);
        let program = match &found {
            Some(p) => handoff::Lookup::Found(p),
            None if handoff::legacy_only(path.as_deref(), &env) => handoff::Lookup::NeedsUpdate,
            None => handoff::Lookup::Missing,
        };
        handoff::hand_off(&log, n.as_str(), &label, window, &out, program, &handoff::start).map_err(CommandError::internal)
    })
    .await
}

fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, CommandError> {
    app.path().app_data_dir().map_err(|e| CommandError::internal(e.to_string()))
}

/// `notification_channels_get`: the notification channels (none by default).
#[tauri::command]
pub async fn notification_channels_get<R: Runtime>(app: AppHandle<R>) -> Result<notify_channels::Settings, CommandError> {
    let dir = data_dir(&app)?;
    blocking(move || Ok(crate::channels::load(&dir))).await
}

/// `notification_channels_save`: validates and saves the channels. Secrets are set separately and never travel in the settings.
#[tauri::command]
pub async fn notification_channels_save<R: Runtime>(app: AppHandle<R>, settings: notify_channels::Settings) -> Result<(), CommandError> {
    let dir = data_dir(&app)?;
    blocking(move || crate::channels::save(&dir, &settings).map_err(CommandError::bad)).await
}

/// `channel_secret_set`: stores a channel's secret in the OS keychain.
#[tauri::command]
pub async fn channel_secret_set(key: String, value: String) -> Result<(), CommandError> {
    blocking(move || crate::channels::set_secret(&key, &value).map_err(CommandError::bad)).await
}

/// `channel_secret_clear`.
#[tauri::command]
pub async fn channel_secret_clear(key: String) -> Result<(), CommandError> {
    blocking(move || crate::channels::clear_secret(&key).map_err(CommandError::bad)).await
}

/// `channel_secret_present`: whether a secret is stored (never its value).
#[tauri::command]
pub async fn channel_secret_present(key: String) -> Result<bool, CommandError> {
    blocking(move || Ok(crate::channels::has_secret(&key))).await
}

/// `channel_test`: sends a test message through one saved channel, whatever its event list says, and reports how it went.
#[tauri::command]
pub async fn channel_test<R: Runtime>(app: AppHandle<R>, id: String) -> Result<String, CommandError> {
    let dir = data_dir(&app)?;
    blocking(move || {
        let mut settings = crate::channels::load(&dir);
        settings.channels.retain(|c| c.id == id);
        let Some(c) = settings.channels.first_mut() else {
            return Err(CommandError::bad("save the channel first, then test it"));
        };
        c.enabled = true;
        c.events = vec!["test".into()];
        c.min_severity = notify_channels::Severity::Info;
        let msg = notify_channels::Message {
            title: "Pitcrew test message".into(),
            body: "If you can read this, the channel works.".into(),
            severity: notify_channels::Severity::Info,
            kind: "test".into(),
            source: "pitcrew".into(),
        };
        match notify_channels::Dispatcher::new(settings)
            .dispatch(&msg, &crate::channels::Keychain)
            .into_iter()
            .next()
        {
            Some((_, Ok(()))) => Ok("Sent. Check the destination.".to_string()),
            Some((_, Err(e))) => Err(CommandError::bad(format!("The test failed: {}", crate::channels::describe(&e)))),
            None => Err(CommandError::internal("nothing was sent")),
        }
    })
    .await
}
