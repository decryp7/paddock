//! Spotter's services for the shell's service health (ADR 0010, rule 19): what the status bar chip lists, with a Repair
//! that never depends on the broken service.
//!
//! - `sp.scheduler` "the watch scheduler": supervised here; when its task dies the chip says so and Restart starts it
//!   again.
//! - `sp.sources` "your sources": the connection. Down when every watch that was checked could not be reached (network,
//!   VPN, shares); Check now tries again.
//! - `sp.renderer` "the page browser": down when a watch needs JavaScript pages and no browser is available; Install
//!   downloads the pinned Chromium (a job, with Cancel).
//!
//! The decisions are pure functions over the engine's view; the supervisors only report them.

use crate::state::Shared;
use paddock_health::{Cause, Health, RepairAction, ServiceSpec};
use spotter_core::state::WatchStatus;
use spotter_engine::view::WatchSummary;
use spotter_render::RendererKind;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::Notify;

/// The watch scheduler.
pub const SCHEDULER: &str = "sp.scheduler";
/// The watches' places (the connection).
pub const SOURCES: &str = "sp.sources";
/// The browser for JavaScript pages.
pub const RENDERER: &str = "sp.renderer";

/// Why the sources are down, or `None` when at least one checked watch worked (a single failing watch is that
/// watch's own state, shown on it, not an outage).
pub fn sources_problem(watches: &[WatchSummary]) -> Option<Cause> {
    let checked: Vec<&WatchSummary> = watches
        .iter()
        .filter(|w| !w.paused && !matches!(w.status, WatchStatus::Pending))
        .collect();
    let first = checked.first()?;
    let all_unreachable = checked.iter().all(|w| matches!(w.status, WatchStatus::Unreachable(_)));
    if !all_unreachable {
        return None;
    }
    let detail = match &first.status {
        WatchStatus::Unreachable(why) => why.clone(),
        _ => String::new(),
    };
    let count = checked.len();
    Some(
        Cause::new(if count == 1 {
            "Spotter cannot reach the place it watches.".to_string()
        } else {
            format!("Spotter cannot reach any of the {count} places it watches.")
        })
        .hint("Check the network connection (or the VPN and the shares), then check again.")
        .detail(detail)
        .code("sp.sources.unreachable"),
    )
}

/// Why the page browser is down: a watch needs JavaScript pages and no browser is available.
pub fn renderer_problem(needed_by: usize, kind: RendererKind) -> Option<Cause> {
    if needed_by == 0 || kind != RendererKind::None {
        return None;
    }
    Some(
        Cause::new(if needed_by == 1 {
            "A watch needs a browser to read its page, and none is available.".to_string()
        } else {
            format!("{needed_by} watches need a browser to read their pages, and none is available.")
        })
        .hint("Install the browser Spotter uses for JavaScript pages (a download of about 150 MB).")
        .code("sp.renderer.missing"),
    )
}

/// What the services' repairs reach: the app, once it is set up (the specs are built before the app exists).
pub struct Hub<R: Runtime> {
    app: Arc<OnceLock<AppHandle<R>>>,
    last_needed: Arc<Mutex<Option<usize>>>,
    restart: Arc<Notify>,
}

impl<R: Runtime> Clone for Hub<R> {
    fn clone(&self) -> Self {
        Hub {
            app: self.app.clone(),
            last_needed: self.last_needed.clone(),
            restart: self.restart.clone(),
        }
    }
}

impl<R: Runtime> Default for Hub<R> {
    fn default() -> Self {
        Hub {
            app: Arc::new(OnceLock::new()),
            last_needed: Arc::new(Mutex::new(None)),
            restart: Arc::new(Notify::new()),
        }
    }
}

impl<R: Runtime> Hub<R> {
    /// Connects the repairs to the running app.
    pub fn attach(&self, app: &AppHandle<R>) {
        let _ = self.app.set(app.clone());
    }

    /// The scheduler's restart signal.
    pub fn restart_signal(&self) -> Arc<Notify> {
        self.restart.clone()
    }

    fn state(&self) -> Result<Shared, Cause> {
        self.app
            .get()
            .and_then(|a| a.try_state::<Shared>().map(|s| s.inner().clone()))
            .ok_or_else(|| Cause::new("Spotter is still starting.").hint("Try again in a moment."))
    }

    /// The services, in the order the status bar lists them. Each repair runs on the health crate's thread.
    pub fn services(&self) -> Vec<ServiceSpec> {
        let restart = self.restart.clone();
        let check = self.clone();
        let install = self.clone();
        vec![
            ServiceSpec::new(SCHEDULER, "the watch scheduler").repair(move || {
                restart.notify_one();
                Ok(())
            }),
            ServiceSpec::new(SOURCES, "your sources").repair(move || {
                let state = check.state()?;
                let engine = state.engine.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = engine.check_now(None).await;
                });
                Ok(())
            }),
            ServiceSpec::new(RENDERER, "the page browser").repair(move || {
                let state = install.state()?;
                let app = install.app.get().cloned().ok_or_else(|| Cause::new("Spotter is still starting."))?;
                let registry = crate::jobs::registry(&app).ok_or_else(|| Cause::new("The shell's jobs are not set up."))?;
                let op = crate::jobs::begin(&registry, crate::jobs::Kind::InstallRenderer, "Installing the browser", None);
                let result = tauri::async_runtime::block_on(spotter_render::install::install(
                    &state.data_dir,
                    &state.downloader,
                    op.ctx(),
                    &op.job,
                ));
                op.end(result.is_ok(), &result.as_ref().err().map(ToString::to_string).unwrap_or_default());
                result.map(drop).map_err(|e| {
                    Cause::new("The browser could not be installed.")
                        .hint("Check the connection and try again.")
                        .detail(e.to_string())
                        .code("sp.renderer.install")
                })
            }),
        ]
    }

    /// Reports the connection from the watches the engine just pushed.
    pub fn report_sources(&self, watches: &[WatchSummary]) {
        let Some(health) = self.health() else { return };
        match sources_problem(watches) {
            Some(cause) => {
                health.down(SOURCES, cause, Some(RepairAction::Retry));
            }
            None => {
                if health.is_in_outage(SOURCES) {
                    health.ok(SOURCES);
                }
            }
        }
    }

    /// Looks at the page browser when the number of watches that need it changed (the `--version` lookup is slow,
    /// so it runs off the async threads and never on every push).
    pub fn report_renderer(&self, watches: &[WatchSummary]) {
        let Some(app) = self.app.get().cloned() else { return };
        let Some(state) = app.try_state::<Shared>().map(|s| s.inner().clone()) else {
            return;
        };
        let needed = watches
            .iter()
            .filter(|w| !w.paused)
            .filter(|w| {
                state
                    .engine
                    .get_watch(&w.id)
                    .is_ok_and(|d| d.spec.source.values.get("rendered").is_some_and(|v| v == "true"))
            })
            .count();
        {
            let mut last = self.last_needed.lock().unwrap_or_else(PoisonError::into_inner);
            if *last == Some(needed) {
                return;
            }
            *last = Some(needed);
        }
        let hub = self.clone();
        tauri::async_runtime::spawn_blocking(move || hub.look_at_renderer(needed));
    }

    /// Reads the renderer status and reports it (also after an install).
    pub fn look_at_renderer(&self, needed: usize) {
        let (Some(app), Some(health)) = (self.app.get(), self.health()) else {
            return;
        };
        let Some(state) = app.try_state::<Shared>() else { return };
        let kind = spotter_render::renderer_status(&state.data_dir).kind;
        match renderer_problem(needed, kind) {
            Some(cause) => {
                health.down(RENDERER, cause, Some(RepairAction::Install));
            }
            None => {
                if health.is_in_outage(RENDERER) {
                    health.ok(RENDERER);
                }
            }
        }
    }

    fn health(&self) -> Option<Arc<Health>> {
        self.app.get()?.try_state::<Arc<Health>>().map(|h| h.inner().clone())
    }

    /// Runs the scheduler under supervision: when its task dies (a panic), the chip says so and Restart runs it again;
    /// a normal end (the app is stopping) ends the supervisor.
    pub async fn supervise<F, Fut>(&self, mut run: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        loop {
            let ended = tauri::async_runtime::spawn(run()).await;
            let Err(e) = ended else { return };
            if let Some(health) = self.health() {
                health.down(
                    SCHEDULER,
                    Cause::new("Spotter stopped checking your watches.")
                        .hint("Restart the scheduler; your watches and their history are safe.")
                        .detail(e.to_string())
                        .code("sp.scheduler.stopped"),
                    Some(RepairAction::Restart),
                );
            }
            self.restart.notified().await;
            if let Some(health) = self.health() {
                health.ok(SCHEDULER);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_engine::view::ReleaseView;

    fn watch(id: &str, status: WatchStatus, paused: bool) -> WatchSummary {
        WatchSummary {
            id: id.into(),
            name: id.into(),
            source_kind: "http".into(),
            source_label: "HTTP".into(),
            source_description: String::new(),
            status,
            paused,
            checking: false,
            latest: None::<ReleaseView>,
            acknowledged: None,
            installed: None,
            unseen: false,
            newer_than_installed: None,
            last_check_ms: None,
            next_check_ms: None,
            failures: 0,
            schedule_text: String::new(),
            product_count: 0,
        }
    }

    #[test]
    fn the_sources_are_down_only_when_every_checked_watch_is_unreachable() {
        let off = || WatchStatus::Unreachable("dns error".into());
        assert!(sources_problem(&[]).is_none(), "no watches, nothing to reach");
        assert!(
            sources_problem(&[watch("a", WatchStatus::Pending, false)]).is_none(),
            "not checked yet"
        );
        let all = sources_problem(&[
            watch("a", off(), false),
            watch("b", off(), false),
            watch("p", WatchStatus::Ok, true),
        ])
        .unwrap();
        assert!(all.message.contains("any of the 2 places"), "{}", all.message);
        assert_eq!(all.detail.as_deref(), Some("dns error"));
        assert!(all.hint.is_some(), "an outage always says what to do next");
        assert!(
            sources_problem(&[watch("a", off(), false), watch("b", WatchStatus::Ok, false)]).is_none(),
            "one watch working"
        );
        assert!(sources_problem(&[watch("a", off(), false), watch("b", WatchStatus::Failed("x".into()), false)]).is_none());
    }

    #[test]
    fn the_browser_is_down_only_when_a_watch_needs_it_and_none_exists() {
        assert!(renderer_problem(0, RendererKind::None).is_none());
        assert!(renderer_problem(2, RendererKind::ChromiumInstalled).is_none());
        assert!(renderer_problem(2, RendererKind::OsWebview).is_none());
        let c = renderer_problem(2, RendererKind::None).unwrap();
        assert!(c.message.starts_with("2 watches"), "{}", c.message);
        assert!(renderer_problem(1, RendererKind::None).unwrap().message.starts_with("A watch"));
    }

    #[test]
    fn every_service_has_a_repair() {
        let hub: Hub<tauri::test::MockRuntime> = Hub::default();
        let ids: Vec<&str> = [SCHEDULER, SOURCES, RENDERER].to_vec();
        let services = hub.services();
        assert_eq!(services.iter().map(ServiceSpec::id).collect::<Vec<_>>(), ids);
    }

    #[tokio::test]
    async fn a_dead_scheduler_is_restarted_by_the_repair_and_a_normal_end_stops_the_supervisor() {
        let hub: Hub<tauri::test::MockRuntime> = Hub::default();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = runs.clone();
        let restart = hub.restart_signal();
        let supervisor = hub.clone();
        let task = tokio::spawn(async move {
            supervisor
                .supervise(move || {
                    let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    async move {
                        assert!(n != 0, "the first run panics");
                    }
                })
                .await;
        });
        while runs.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            tokio::task::yield_now().await;
        }
        restart.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            runs.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "restarted once, then ended normally"
        );
    }
}
