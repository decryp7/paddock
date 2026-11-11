//! Running the service: the supervisor on its own thread and the IPC server on a small async runtime. Used by
//! `run` (console, macOS launch agent) and by the Windows service entry point.

use crate::error::Error;
use crate::ipc;
use crate::supervisor::{os_env, Msg, Supervisor};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;

/// A running service.
pub struct Host {
    tx: Sender<Msg>,
    supervisor: Option<JoinHandle<()>>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Host {
    /// Starts supervising the runners configured in `data_dir` and serving the app. `on_fatal` is called when the
    /// IPC server cannot run (e.g. another service owns the pipe).
    pub fn start(data_dir: PathBuf, on_fatal: impl FnOnce(String) + Send + 'static) -> Result<Host, Error> {
        let socket_dir = data_dir.clone();
        Host::start_at(data_dir, socket_dir, on_fatal)
    }

    /// [`Host::start`] with the Unix socket in `socket_dir` (Linux: the user's runtime folder).
    pub fn start_at(data_dir: PathBuf, socket_dir: PathBuf, on_fatal: impl FnOnce(String) + Send + 'static) -> Result<Host, Error> {
        let protected = protect_installed_folder(&data_dir)?;
        std::fs::create_dir_all(&data_dir).map_err(|source| Error::Io {
            path: data_dir.display().to_string(),
            source,
        })?;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut supervisor = Supervisor::new(os_env(data_dir.clone()), tx.clone());
        if let Some(note) = protected {
            supervisor.note(&note);
        }
        let handle = std::thread::Builder::new()
            .name("supervisor".into())
            .spawn(move || supervisor.run(rx))
            .map_err(|e| Error::Os {
                what: "Starting the supervisor",
                reason: e.to_string(),
            })?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("pitcrew-ipc")
            .build()
            .map_err(|e| Error::Os {
                what: "Starting the IPC runtime",
                reason: e.to_string(),
            })?;
        let ipc_tx = tx.clone();
        let stop_tx = tx.clone();
        runtime.spawn(async move {
            if let Err(e) = ipc::serve(ipc_tx, socket_dir).await {
                on_fatal(e);
                let _ = stop_tx.send(Msg::Shutdown);
            }
        });
        Ok(Host {
            tx,
            supervisor: Some(handle),
            runtime: Some(runtime),
        })
    }

    /// Sends a message to the supervisor (e.g. a log-on change).
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    /// Whether the supervisor has stopped by itself (fatal IPC error).
    pub fn finished(&self) -> bool {
        self.supervisor.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stops supervising (runners are asked to stop; running jobs finish) and waits for it.
    pub fn stop(mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(h) = self.supervisor.take() {
            let _ = h.join();
        }
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_timeout(Duration::from_secs(2));
        }
    }
}

/// The installed Windows service's data folder holds the configuration that names the programs the LocalSystem service starts,
/// so it is made (or locked) protected before anything is read from it ([`crate::protected`]). Returns a line for the service
/// log. Any other folder (tests, development, macOS, Linux) is left as it is: there the service runs as the user.
#[cfg(windows)]
fn protect_installed_folder(data_dir: &std::path::Path) -> Result<Option<String>, Error> {
    use crate::protected::{ensure_protected_root, RootOutcome};
    if ipc::pipe_name(data_dir) != ipc::PIPE_NAME {
        return Ok(None);
    }
    let mut machine = paddock_service::privileged::trust_real::RealTrust;
    let outcome = ensure_protected_root(
        &mut machine,
        data_dir,
        &[crate::paths::CONFIG_FILE, crate::paths::CONFIG_BACKUP, "state.json"],
        &["logs"],
    )
    .map_err(|reason| Error::Os {
        what: "Protecting the data folder",
        reason: format!(
            "{reason}. Run \"pitcrew-service install\" again as an administrator, or remove the folder if nothing in it is needed"
        ),
    })?;
    Ok(Some(match outcome {
        RootOutcome::Created => format!("the data folder {} was created protected (administrators and SYSTEM only)", data_dir.display()),
        RootOutcome::Locked => format!("the data folder {} is protected (administrators and SYSTEM only)", data_dir.display()),
        RootOutcome::MovedAside(aside, why) => format!(
            "the data folder {} could not be trusted ({why}); it was moved to {aside} and a protected one made. Nothing was deleted: copy back only what you trust",
            data_dir.display()
        ),
    }))
}

#[cfg(not(windows))]
#[expect(clippy::unnecessary_wraps, reason = "the Windows version can fail")]
fn protect_installed_folder(_: &std::path::Path) -> Result<Option<String>, Error> {
    Ok(None)
}

/// `run`: supervise in the foreground until Ctrl+C / SIGTERM (launchd stops agents with SIGTERM).
pub fn run_foreground(data_dir: PathBuf) -> Result<(), Error> {
    let socket_dir = crate::paths::socket_dir().unwrap_or_else(|| data_dir.clone());
    let (fatal_tx, fatal_rx) = std::sync::mpsc::channel::<String>();
    let host = Host::start_at(data_dir, socket_dir, move |e| {
        let _ = fatal_tx.send(e);
    })?;
    let signals = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Os {
            what: "Waiting for a stop signal",
            reason: e.to_string(),
        })?;
    signals.block_on(async {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        #[cfg(unix)]
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        loop {
            #[cfg(unix)]
            let term_fut = async {
                match term.as_mut() {
                    Some(t) => {
                        t.recv().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            };
            #[cfg(not(unix))]
            let term_fut = std::future::pending::<()>();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = term_fut => break,
                _ = tick.tick() => if host.finished() { break },
            }
        }
    });
    let fatal = fatal_rx.try_recv().ok();
    host.stop();
    match fatal {
        Some(reason) => Err(Error::Os {
            what: "Serving the Pitcrew app",
            reason,
        }),
        None => Ok(()),
    }
}
