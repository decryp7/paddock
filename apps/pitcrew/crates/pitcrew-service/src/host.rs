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
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    // Shared for read and write only, never for delete or rename: nothing can remove or replace the lock file under the engine that
    // holds it (round 6 P2-9). Windows' default share mode would allow both.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3);
    }
    let file = options.open(&path).map_err(|source| Error::Io {
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
        Host::start_with_token(data_dir, socket_dir, mode, token_is_elevated(), on_fatal)
    }

    /// [`Host::start_with`] with the token's elevation given (the test seam of the refusal).
    fn start_with_token(
        data_dir: PathBuf,
        socket_dir: PathBuf,
        mode: ipc::ServeMode,
        elevated: bool,
        on_fatal: impl FnOnce(String) + Send + 'static,
    ) -> Result<Host, Error> {
        if mode == ipc::ServeMode::User {
            user_engine_token_allowed(elevated).map_err(|reason| Error::Os {
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
        // From here the engine owns its folder: a failure is written to its own log. Before the lock (an elevated refusal, a second
        // engine) it touches nothing, not even the log of the engine that holds the folder.
        // The lock stays in `instance_lock` until the host takes it, so a failure is logged while it is still held.
        let mut instance_lock = instance_lock;
        let result = Host::start_locked(data_dir.clone(), socket_dir, mode, on_fatal, &mut instance_lock);
        if let Err(e) = &result {
            log_engine_failure(&data_dir, mode, instance_lock.is_some(), &e.to_string());
        }
        result
    }

    fn start_locked(
        data_dir: PathBuf,
        socket_dir: PathBuf,
        mode: ipc::ServeMode,
        on_fatal: impl FnOnce(String) + Send + 'static,
        instance_lock: &mut Option<std::fs::File>,
    ) -> Result<Host, Error> {
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
            _instance_lock: instance_lock.take(),
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
        RootOutcome::Locked(swept) => {
            let mut note = format!("the data folder {} is protected (administrators and SYSTEM only)", data_dir.display());
            for line in swept.lines() {
                note.push('\n');
                note.push_str(&line);
            }
            note
        }
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
    let socket_dir = socket_dir_for(mode, &data_dir, crate::paths::socket_dir());
    let (fatal_tx, fatal_rx) = std::sync::mpsc::channel::<String>();
    let own_dir = data_dir.clone();
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
    if let Some(reason) = &fatal {
        // While the host still holds the engine's lock (stopping it releases the lock).
        log_engine_failure(&own_dir, mode, true, reason);
    }
    host.stop();
    match fatal {
        Some(reason) => Err(Error::Os {
            what: "Serving the Pitcrew app",
            reason,
        }),
        None => Ok(()),
    }
}

/// The folder of the engine's socket (Unix) or of its recorded pipe name (Windows): the per-user engine keeps both in **its own**
/// folder (the app reads `pipe.name` there), never in the machine's; the SYSTEM service uses the OS's socket folder when it has one.
/// Pure.
pub fn socket_dir_for(mode: ipc::ServeMode, data_dir: &std::path::Path, os_socket: Option<PathBuf>) -> PathBuf {
    match mode {
        ipc::ServeMode::User => data_dir.to_path_buf(),
        ipc::ServeMode::System => os_socket.unwrap_or_else(|| data_dir.to_path_buf()),
    }
}

/// Writes why the engine could not start or stopped to its own `logs/service.log` (ER-27): a user's engine has no console, so this is
/// where the cause can be found from a diagnostic bundle. A log that cannot be written is the only thing left unsaid.
pub fn log_engine_failure(data_dir: &std::path::Path, mode: ipc::ServeMode, lock_held: bool, cause: &str) {
    // The SYSTEM service's supervisor writes its own log; only the per-user engine, which owns its folder, writes here, and only while
    // it holds the folder's lock (without it the log belongs to the engine that does).
    if mode != ipc::ServeMode::User || !lock_held {
        return;
    }
    let who = if mode == ipc::ServeMode::User {
        "the engine for your own runners"
    } else {
        "the service"
    };
    let mut log = crate::logfile::LogWriter::open(&data_dir.join("logs"), "service.log", pitcrew_core::logs::RotationPolicy::default());
    log.supervisor(crate::cleanup::now(), &format!("{who} stopped: {cause}"));
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
        start_user(&dir)
            .expect("the folder is free again once the first engine stopped")
            .stop();
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

#[cfg(test)]
mod location_tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-loc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// Round 6 P1-A: on Windows the user's engine took `paths::socket_dir()`, which is `%ProgramData%\Pitcrew`, and wrote `pipe.name`
    /// there; the app reads it from the engine's own folder.
    #[test]
    fn the_user_engine_keeps_its_socket_and_pipe_name_in_its_own_folder_and_the_service_uses_the_os_one() {
        let own = PathBuf::from("/users/me/engine");
        let machine = PathBuf::from("/programdata/pitcrew");
        assert_eq!(socket_dir_for(ipc::ServeMode::User, &own, Some(machine.clone())), own);
        assert_eq!(socket_dir_for(ipc::ServeMode::User, &own, None), own);
        assert_eq!(
            socket_dir_for(ipc::ServeMode::System, &machine, Some(PathBuf::from("/run/pitcrew"))),
            PathBuf::from("/run/pitcrew")
        );
        assert_eq!(socket_dir_for(ipc::ServeMode::System, &machine, None), machine);
    }

    /// Every other test started the engine with one folder for both; with two, the engine's files stay in its data folder.
    #[test]
    fn an_engine_started_with_different_data_and_socket_folders_keeps_its_lock_and_log_in_the_data_folder() {
        let (data, sock) = (scratch("data"), scratch("sock"));
        let host = Host::start_with(data.clone(), sock.clone(), ipc::ServeMode::User, |_| {}).expect("starts");
        assert!(data.join(ENGINE_LOCK_FILE).exists(), "the lock is in the engine's folder");
        assert!(
            !sock.join(ENGINE_LOCK_FILE).exists(),
            "nothing of the engine's folder goes to the socket folder"
        );
        host.stop();
        let _ = (std::fs::remove_dir_all(&data), std::fs::remove_dir_all(&sock));
    }

    /// Round 7: an elevated refusal happens before the engine owns its folder: nothing is written, not a log, not a lock.
    #[test]
    fn an_elevated_refusal_writes_nothing_and_the_service_mode_never_writes_this_log() {
        let data = scratch("elevated");
        let e = Host::start_with_token(data.clone(), data.clone(), ipc::ServeMode::User, true, |_| {})
            .err()
            .expect("refused");
        assert!(e.to_string().contains("must not run elevated"), "{e}");
        assert!(!data.exists(), "an elevated refusal touched {}", data.display());
        log_engine_failure(&data, ipc::ServeMode::System, true, "a cause");
        assert!(!data.exists(), "the service mode writes no engine log");
    }

    /// Round 7: a second engine, refused at the lock, writes nothing to the first's log either.
    #[test]
    fn a_second_engine_refused_at_the_lock_leaves_the_first_ones_log_alone() {
        let data = scratch("second");
        let first = Host::start_with(data.clone(), data.clone(), ipc::ServeMode::User, |_| {}).expect("starts");
        let log = data.join("logs").join("service.log");
        let before = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(Host::start_with(data.clone(), data.clone(), ipc::ServeMode::User, |_| {}).is_err());
        assert_eq!(before, std::fs::read_to_string(&log).unwrap_or_default());
        first.stop();
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Round 6 P1-A (ER-27): a failure that ends the engine is in its own service log.
    #[test]
    fn the_cause_of_an_engine_stopping_is_written_to_its_own_service_log() {
        let data = scratch("fail");
        log_engine_failure(&data, ipc::ServeMode::User, false, "written without the lock");
        assert!(!data.exists(), "an engine that does not hold the lock writes nothing");
        log_engine_failure(&data, ipc::ServeMode::User, true, "the pipe could not be created (access denied)");
        let text = std::fs::read_to_string(data.join("logs").join("service.log")).unwrap();
        assert!(
            text.contains("engine for your own runners stopped") && text.contains("access denied"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&data);
    }
}
