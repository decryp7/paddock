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
        std::fs::create_dir_all(&data_dir).map_err(|source| Error::Io {
            path: data_dir.display().to_string(),
            source,
        })?;
        let (tx, rx) = std::sync::mpsc::channel();
        let supervisor = Supervisor::new(os_env(data_dir.clone()), tx.clone());
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
            if let Err(e) = ipc::serve(ipc_tx, data_dir).await {
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

/// `run`: supervise in the foreground until Ctrl+C / SIGTERM (launchd stops agents with SIGTERM).
pub fn run_foreground(data_dir: PathBuf) -> Result<(), Error> {
    let (fatal_tx, fatal_rx) = std::sync::mpsc::channel::<String>();
    let host = Host::start(data_dir, move |e| {
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
