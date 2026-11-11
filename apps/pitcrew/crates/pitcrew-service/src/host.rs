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
    /// The per-user engine's exclusive lock on `engine.lock` (ER-38 follow-up, round 5 P1-1): held for the process, released by the OS.
    _instance_lock: Option<std::fs::File>,
}

/// The file whose exclusive lock makes a per-user engine the only one on its folder.
pub const ENGINE_LOCK_FILE: &str = "engine.lock";

/// Takes the per-user engine's exclusive lock in `data_dir` (made if missing).
///
/// # Errors
/// The folder or file cannot be used, or another engine holds the lock (the message says so and what to do).
fn take_instance_lock(data_dir: &std::path::Path) -> Result<std::fs::File, Error> {
    std::fs::create_dir_all(data_dir).map_err(|source| Error::Io {
        path: data_dir.display().to_string(),
        source,
    })?;
    let path = data_dir.join(ENGINE_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::Os {
            what: "Starting the engine for your own runners",
            reason: format!(
                "another engine for your runners is already running on {} (it holds {ENGINE_LOCK_FILE}); a second one would supervise the same runners twice. Quit the other one first",
                data_dir.display()
            ),
        }),
        Err(std::fs::TryLockError::Error(source)) => Err(Error::Io {
            path: path.display().to_string(),
            source,
        }),
    }
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
        Host::start_with(data_dir, socket_dir, ipc::ServeMode::System, on_fatal)
    }

    /// [`Host::start_at`] for a given [`ipc::ServeMode`]: the SYSTEM service, or the per-user engine (never from an elevated token).
    pub fn start_with(
        data_dir: PathBuf,
        socket_dir: PathBuf,
        mode: ipc::ServeMode,
        on_fatal: impl FnOnce(String) + Send + 'static,
    ) -> Result<Host, Error> {
        if mode == ipc::ServeMode::User {
            user_engine_token_allowed(token_is_elevated()).map_err(|reason| Error::Os {
                what: "Starting the engine for your own runners",
                reason,
            })?;
        }
        // Before the supervisor, the pipe or `pipe.name`: a second engine must not touch anything of the first.
        let instance_lock = if mode == ipc::ServeMode::User {
            Some(take_instance_lock(&data_dir)?)
        } else {
            None
        };
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
            if let Err(e) = ipc::serve(ipc_tx, socket_dir, mode).await {
                on_fatal(e);
                let _ = stop_tx.send(Msg::Shutdown);
            }
        });
        Ok(Host {
            tx,
            supervisor: Some(handle),
            runtime: Some(runtime),
            _instance_lock: instance_lock,
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
    let list = |d: &str| -> std::io::Result<Vec<String>> {
        std::fs::read_dir(d)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect()
    };
    let outcome = ensure_protected_root(
        &mut machine,
        &list,
        data_dir,
        crate::protected::TRUSTED_FILES,
        crate::protected::TRUSTED_DIRS,
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

/// Whether the per-user engine may run with this token. Pure.
///
/// # Errors
/// An elevated token: the engine for a user's own runners is the user's, not an administrator's.
pub fn user_engine_token_allowed(elevated: bool) -> Result<(), String> {
    if elevated {
        Err("the engine for your own runners must not run elevated (as administrator): it takes requests from programs of your own account, and \
             an elevated engine would run what they name with administrator rights. Start it again from a normal (not \"Run as administrator\") \
             window or app"
            .into())
    } else {
        Ok(())
    }
}

/// Whether this process has an elevated token (Windows; elsewhere no).
#[cfg(windows)]
fn token_is_elevated() -> bool {
    crate::platform::win_session::is_elevated()
}

#[cfg(not(windows))]
fn token_is_elevated() -> bool {
    false
}

/// Where `run` keeps its data and whose engine it is (see [`run_foreground`]).
#[cfg(windows)]
fn engine_location(machine_dir: PathBuf) -> Result<(PathBuf, ipc::ServeMode), Error> {
    if crate::platform::win_session::is_local_system() {
        return Ok((machine_dir, ipc::ServeMode::System));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let dir = pitcrew_core::service_paths::user_engine_dir(&env).ok_or(Error::Os {
        what: "Finding the per-user engine's folder",
        reason: "LOCALAPPDATA is not set".into(),
    })?;
    Ok((dir, ipc::ServeMode::User))
}

#[cfg(not(windows))]
#[expect(clippy::unnecessary_wraps, reason = "the Windows version can fail")]
fn engine_location(machine_dir: PathBuf) -> Result<(PathBuf, ipc::ServeMode), Error> {
    Ok((machine_dir, ipc::ServeMode::System))
}

/// `run`: supervise in the foreground until Ctrl+C / SIGTERM (launchd stops agents with SIGTERM).
pub fn run_foreground(data_dir: PathBuf) -> Result<(), Error> {
    // On Windows, `run` as an ordinary user is the **per-user engine** (ADR 0004, second amendment): its data folder is the
    // user's own and its pipe admits only that user. `run` as SYSTEM (the service's own entry is `service`) keeps the machine's.
    let (data_dir, mode) = engine_location(data_dir)?;
    let socket_dir = crate::paths::socket_dir().unwrap_or_else(|| data_dir.clone());
    let (fatal_tx, fatal_rx) = std::sync::mpsc::channel::<String>();
    let host = Host::start_with(data_dir, socket_dir, mode, move |e| {
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

#[cfg(test)]
mod elevation_tests {
    use super::*;

    #[test]
    fn the_engine_for_a_users_own_runners_refuses_an_elevated_token_and_says_why() {
        assert!(user_engine_token_allowed(false).is_ok());
        let e = user_engine_token_allowed(true).unwrap_err();
        assert!(e.contains("must not run elevated") && e.contains("normal"), "{e}");
    }
}

#[cfg(test)]
mod single_instance_tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-single-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn start_user(dir: &std::path::Path) -> Result<Host, Error> {
        Host::start_with(dir.to_path_buf(), dir.to_path_buf(), ipc::ServeMode::User, |_| {})
    }

    #[test]
    fn a_second_user_engine_on_one_folder_does_not_start_and_says_who_holds_it() {
        let dir = scratch("two");
        let first = start_user(&dir).expect("the first engine starts");
        let second = start_user(&dir).err().expect("a second engine on the same folder must be refused");
        let text = second.to_string();
        assert!(text.contains("already running"), "{text}");
        first.stop();
        // The lock goes with the first engine: a new one starts.
        start_user(&dir).expect("the folder is free again once the first engine stopped").stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_second_engine_leaves_the_first_ones_pipe_name_alone() {
        let dir = scratch("name");
        let first = start_user(&dir).expect("the first engine starts");
        let before = std::fs::read_to_string(dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE)).ok();
        assert!(start_user(&dir).is_err());
        let after = std::fs::read_to_string(dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE)).ok();
        assert_eq!(before, after, "the refused engine must not overwrite pipe.name");
        first.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
