//! Installing llama.cpp builds as tracked downloads, and installing one by itself (ADR 0004, ADR 0020): when no build is
//! installed Pitwall downloads the one suggested for this machine, checks it and self-tests it. Models loaded or
//! downloaded meanwhile wait ([`crate::engines::Engine::ready`]); a failure keeps its message and Retry starts again.

use crate::app::{lock, AppState};
use crate::egress::{CancelFlag, DownloadError};
use crate::engines::{BuildInstaller, EngineError, Setup};
use crate::paths::now_ms;
use pitwall_core::events::{EventKind, Severity};
use pitwall_core::mgmt::{Download, DownloadKind, DownloadState, EngineSetup, EngineSetupState, LiveUpdate};
use pitwall_core::runtimes::builds;
use std::sync::Arc;

/// The automatic install as the UI sees it.
pub fn dto(st: &AppState) -> EngineSetup {
    let (state, error) = match st.llama.setup() {
        Setup::Idle => (EngineSetupState::Idle, None),
        Setup::Installing => (EngineSetupState::Installing, None),
        Setup::Failed(m) => (EngineSetupState::Failed, Some(m)),
    };
    let download = (state == EngineSetupState::Installing)
        .then(|| {
            lock(&st.installs)
                .values()
                .filter(|d| d.id.starts_with("eng-"))
                .max_by_key(|d| d.started_at)
                .cloned()
        })
        .flatten();
    EngineSetup { state, error, download }
}

fn publish(st: &AppState, s: Setup) {
    st.llama.set_setup(s);
    st.push(&LiveUpdate::EngineSetup { setup: dto(st) });
}

/// Starts the automatic install of the suggested build unless one is running. A machine without a
/// pinned build ends in a `Failed` state with the next step. Returns at once; the work runs in the background.
pub fn auto_install(st: &Arc<AppState>, installer: Arc<dyn BuildInstaller>) {
    if st.llama.setup() == Setup::Installing {
        return;
    }
    let Some(id) = installer.suggested() else {
        let m = format!(
            "llama.cpp has no build for this machine ({}). Pick one on the llama.cpp page if you know one works.",
            st.llama.platform()
        );
        publish(st, Setup::Failed(m));
        return;
    };
    publish(st, Setup::Installing);
    st.event(
        EventKind::EngineStarted,
        Severity::Info,
        format!("Installing llama.cpp {} automatically", builds::label(&id)),
    );
    start_install(st, installer, &id, true);
}

/// Registers a tracked download for build `id` and installs it in the background. `automatic` installs also report
/// their outcome to the setup state that waiting models watch.
pub fn start_install(st: &Arc<AppState>, installer: Arc<dyn BuildInstaller>, id: &str, automatic: bool) -> Download {
    let platform = st.llama.platform();
    let build = st.llama.catalog().find(&platform, id).cloned();
    let (host, total) = build.as_ref().map_or((String::new(), 0), |b| {
        let host = b
            .assets
            .first()
            .and_then(|a| pitwall_core::privacy::DownloadUrl::parse(&a.url).ok())
            .map(|u| u.host().to_string())
            .unwrap_or_default();
        (host, b.download_bytes())
    });
    let dl_id = format!("eng-{}", lock(&st.installs).len());
    let cancel = CancelFlag::default();
    let dl = Download {
        id: dl_id.clone(),
        kind: DownloadKind::Engine,
        name: format!("llama.cpp {} ({})", builds::label(id), st.llama.catalog().tag),
        host,
        bytes_done: 0,
        bytes_total: total,
        speed_bps: 0,
        state: DownloadState::Downloading,
        error: None,
        started_at: now_ms(),
    };
    lock(&st.installs).insert(dl_id.clone(), dl.clone());
    lock(&st.install_cancel).insert(dl_id.clone(), cancel.clone());
    let (st2, bid) = (st.clone(), id.to_string());
    tokio::spawn(async move {
        let upd = |f: &dyn Fn(&mut Download)| {
            let mut g = lock(&st2.installs);
            if let Some(d) = g.get_mut(&dl_id) {
                f(d);
                st2.push(&LiveUpdate::Download { download: d.clone() });
            }
        };
        let t0 = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        let before = crate::engine_pins::current(&st2);
        let mut on_progress = |done: u64, total: u64| {
            if last.elapsed().as_millis() > 400 || done == total {
                last = std::time::Instant::now();
                let bps = (done as f64 / t0.elapsed().as_secs_f64().max(0.001)) as u64;
                let mut g = lock(&st2.installs);
                if let Some(d) = g.get_mut(&dl_id) {
                    d.bytes_done = done;
                    d.speed_bps = bps;
                    if done >= total {
                        d.state = DownloadState::SelfTest;
                    }
                    st2.push(&LiveUpdate::Download { download: d.clone() });
                }
            }
        };
        let res = installer.install(&bid, &cancel, &mut on_progress).await;
        match res {
            Ok(info) => {
                crate::engine_pins::record_change(&st2, before);
                upd(&|d| {
                    d.state = DownloadState::Done;
                    d.bytes_done = d.bytes_total;
                });
                st2.event(
                    EventKind::EngineStarted,
                    Severity::Info,
                    format!("Installed llama.cpp {} ({})", bid, info.tag),
                );
                if automatic {
                    publish(&st2, Setup::Idle);
                }
            }
            Err(e) => {
                let cancelled = matches!(e, EngineError::Download(DownloadError::Cancelled));
                let msg = e.to_string();
                upd(&|d| {
                    d.state = if cancelled {
                        DownloadState::Cancelled
                    } else {
                        DownloadState::Failed
                    };
                    d.error = Some(msg.clone());
                });
                st2.event(
                    EventKind::EngineFailed,
                    Severity::Error,
                    format!("Installing llama.cpp {bid} failed: {msg}"),
                );
                if automatic {
                    publish(&st2, Setup::Failed(format!("{msg} Press Retry to try again.")));
                }
            }
        }
        lock(&st2.install_cancel).remove(&dl_id);
    });
    dl
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{BoxFuture, BuildInfo};
    use crate::paths::Dirs;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Fake {
        fail: AtomicBool,
    }
    impl BuildInstaller for Fake {
        fn suggested(&self) -> Option<String> {
            Some("metal".into())
        }
        fn install<'a>(
            &'a self,
            id: &'a str,
            _: &'a CancelFlag,
            progress: &'a mut (dyn FnMut(u64, u64) + Send),
        ) -> BoxFuture<'a, Result<BuildInfo, EngineError>> {
            Box::pin(async move {
                progress(5, 10);
                tokio::task::yield_now().await;
                if self.fail.load(Ordering::SeqCst) {
                    return Err(EngineError::Install("the network is unreachable".into()));
                }
                progress(10, 10);
                Ok(BuildInfo {
                    id: id.into(),
                    tag: "b1".into(),
                    self_test: "passed".into(),
                    message: None,
                    devices: vec![],
                    size: 1,
                })
            })
        }
    }

    /// An engine that is always ready and never started: stands in for llama.cpp so nothing real is installed.
    struct Noop;
    impl crate::engines::Engine for Noop {
        fn kind(&self) -> pitwall_core::mgmt::EngineKind {
            pitwall_core::mgmt::EngineKind::LlamaCpp
        }
        fn availability(&self) -> pitwall_core::mgmt::Availability {
            pitwall_core::mgmt::Availability::Available
        }
        fn start<'a>(&'a self, _: &'a pitwall_core::llama::Launch) -> BoxFuture<'a, Result<crate::engines::Running, EngineError>> {
            Box::pin(async { Err(EngineError::Start("not a real engine".into())) })
        }
    }

    async fn state() -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let st = AppState::open(Dirs::at(d.path().to_path_buf()), Some(Arc::new(Noop))).await;
        (d, st)
    }

    async fn settled(st: &AppState) -> EngineSetup {
        for _ in 0..200 {
            let d = dto(st);
            if d.state != EngineSetupState::Installing {
                return d;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the install did not finish");
    }

    #[tokio::test]
    async fn the_suggested_build_is_installed_with_progress_and_the_setup_returns_to_idle() {
        let (_d, st) = state().await;
        let fake = Arc::new(Fake {
            fail: AtomicBool::new(false),
        });
        auto_install(&st, fake);
        assert_eq!(dto(&st).state, EngineSetupState::Installing);
        let done = settled(&st).await;
        assert_eq!(done.state, EngineSetupState::Idle);
        let dls = lock(&st.installs);
        let d = dls.values().next().unwrap();
        assert_eq!((d.kind, d.state), (DownloadKind::Engine, DownloadState::Done));
    }

    #[tokio::test]
    async fn a_failed_install_keeps_its_message_and_retry_starts_again() {
        let (_d, st) = state().await;
        let fake = Arc::new(Fake {
            fail: AtomicBool::new(true),
        });
        auto_install(&st, fake.clone());
        let failed = settled(&st).await;
        assert_eq!(failed.state, EngineSetupState::Failed);
        let m = failed.error.unwrap();
        assert!(m.contains("network is unreachable") && m.contains("Retry"), "{m}");
        fake.fail.store(false, Ordering::SeqCst);
        auto_install(&st, fake);
        assert_eq!(settled(&st).await.state, EngineSetupState::Idle);
    }
}
