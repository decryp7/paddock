//! The demo's own commands, behind its guard ([`crate::COMMANDS`]). Each is async and returns at once (rule 10); the
//! arguments are parsed into the engine's types at this edge.

use crate::engine::{self, DemoJob, DemoService, Engine};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};

/// A refusal as the shell's commands send theirs (`{ code, message, retryable }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl DemoError {
    fn not_set_up(part: &str) -> DemoError {
        DemoError {
            code: "notSetUp",
            message: format!("The demo's {part} is not set up."),
            retryable: false,
        }
    }
}

/// What the engine inspector shows (`DemoStatus` in the UI's `dialogs/demo-ipc.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoStatus {
    /// `running` or `stopped` (the engine service's health).
    pub engine: &'static str,
    pub uptime_seconds: u64,
    /// Jobs queued or running.
    pub queued: usize,
}

/// Starts a sample job and returns its id; its progress goes out on `paddock://jobs` like every job of the app.
#[tauri::command]
pub async fn demo_job_start<R: Runtime>(
    app: AppHandle<R>,
    engine: State<'_, Engine>,
    kind: DemoJob,
) -> Result<u64, DemoError> {
    let jobs = engine::registry(&app).ok_or_else(|| DemoError::not_set_up("job registry"))?;
    let (id, _running) = engine.start(&jobs, kind).map_err(|_| DemoError {
        code: "stopped",
        message: "The demo engine is shutting down.".into(),
        retryable: false,
    })?;
    Ok(id.0)
}

/// A service fails as its supervisor would report it; the shell shows the chip, the notification and the repair.
#[tauri::command]
pub async fn demo_break<R: Runtime>(
    app: AppHandle<R>,
    service: DemoService,
) -> Result<(), DemoError> {
    let health = app
        .try_state::<Arc<paddock_health::Health>>()
        .ok_or_else(|| DemoError::not_set_up("service health"))?;
    let (id, cause, repair) = engine::failure(service);
    health.down(id, cause, Some(repair));
    Ok(())
}

/// What the tray popover shows (it may call only commands open to the tray class): the services that need attention and
/// the job that leads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayState {
    /// Every service that is not serving, as the status bar's chip shows them.
    pub problems: Vec<paddock_health::ServiceHealth>,
    /// The first job still queued or running.
    pub job: Option<paddock_jobs::JobSnapshot>,
}

/// The tray popover's state; it asks again on each `paddock://service-health` and `paddock://jobs` event.
#[tauri::command]
pub async fn demo_tray_state<R: Runtime>(app: AppHandle<R>) -> Result<TrayState, DemoError> {
    let problems = app
        .try_state::<Arc<paddock_health::Health>>()
        .map(|h| {
            h.snapshot()
                .services
                .into_iter()
                .filter(|s| s.state.is_problem())
                .collect()
        })
        .unwrap_or_default();
    let job = engine::registry(&app)
        .and_then(|jobs| jobs.snapshots().into_iter().find(|s| active(&s.state)));
    Ok(TrayState { problems, job })
}

/// The tray's Repair: the shell's own repair of the service (the same as the chip's Repair, rule 19), on its thread.
#[tauri::command]
pub async fn demo_tray_repair<R: Runtime>(
    app: AppHandle<R>,
    service: DemoService,
) -> Result<(), DemoError> {
    let health = app
        .try_state::<Arc<paddock_health::Health>>()
        .ok_or_else(|| DemoError::not_set_up("service health"))?;
    let (id, _, _) = engine::failure(service);
    health.repair(id).map(drop).map_err(|e| DemoError {
        code: "refused",
        message: e.to_string(),
        retryable: false,
    })
}

fn active(state: &paddock_jobs::JobState) -> bool {
    matches!(
        state,
        paddock_jobs::JobState::Queued | paddock_jobs::JobState::Running { .. }
    )
}

/// Forgets the greetings (the greetings live in the window; the engine keeps nothing of them).
#[tauri::command]
pub async fn demo_reset() -> Result<(), DemoError> {
    Ok(())
}

/// The engine as the inspector sees it.
#[tauri::command]
pub async fn demo_status<R: Runtime>(
    app: AppHandle<R>,
    engine: State<'_, Engine>,
) -> Result<DemoStatus, DemoError> {
    let running = app
        .try_state::<Arc<paddock_health::Health>>()
        .is_none_or(|h| !h.is_in_outage(engine::ENGINE));
    let queued = engine::registry(&app).map_or(0, |jobs| {
        jobs.snapshots().iter().filter(|s| active(&s.state)).count()
    });
    Ok(DemoStatus {
        engine: if running { "running" } else { "stopped" },
        uptime_seconds: engine.uptime().as_secs(),
        queued,
    })
}
