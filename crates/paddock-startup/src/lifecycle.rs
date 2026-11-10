//! The run lifecycle: the running marker, unclean-shutdown detection and the restore offer.
//!
//! Taken from Debrief's `lifecycle.rs` (marker, crash detection, instances, sessions), on the shared pieces: the marker
//! is a [`paddock_env::VersionedFile`], a panic's evidence is [`paddock_log::CrashMarker`], the session is
//! [`SessionStore`].
//!
//! - **Running marker.** [`Lifecycle::begin`] writes `running.json` (`pid`, `startedAt`, `safeMode`) and
//!   [`Lifecycle::end`] removes it on a clean exit. A marker found at the next start is a crash when its process is gone:
//!   no process with that PID, or one whose start time differs (the PID was reused). A marker whose process still runs
//!   (the previous instance is still exiting after "Restart") is taken over without a crash report, and `end` removes the
//!   marker only while it is still this process's, so the old process cannot erase the new one's. A marker without a start
//!   time counts as running while its PID is alive. An unreadable marker counts as a crash.
//! - **Crash marker.** A panic left `crash-<process>.json` in the data folder ([`paddock_log`]'s hook). It makes the start
//!   a crash even when the process ended some other way, and says which thread and where, and whether it was in safe
//!   mode. It is cleared when the user decides or the run ends cleanly.
//! - **Instances.** The single (main) instance keeps its marker and session in the data folder. A separate process
//!   (`--new-instance`, and always safe mode) uses `instances/<pid>/`, so it never rotates the main instance's session or
//!   reports a crash of it; its folder is removed on a clean exit and pruned at every start when the process is gone.
//! - **The offer.** After a crash [`Lifecycle::report`] says what the notice may offer: Restore (a session exists),
//!   Safe mode (not when the crash already was in safe mode: extensions and plugins are then not the cause) and Start
//!   fresh. The decision methods record the answer, so the notice does not return.

use crate::safe_mode::Mode;
use crate::session::{Autosaver, SessionError, SessionInfo, SessionStore};
use paddock_env::VersionedFile;
use paddock_log::CrashMarker;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// File name of the running marker.
pub const MARKER_FILE: &str = "running.json";
/// Folder of the separate instances' state, one `<pid>` folder each.
pub const INSTANCES_DIR: &str = "instances";
/// Folder of the session files inside an instance's state folder.
pub const SESSION_DIR: &str = "session";
/// Format version of the running marker.
pub const MARKER_VERSION: u32 = 1;
/// At most this many instance folders are inspected per start (bounded work).
pub const MAX_INSTANCE_DIRS: usize = 256;
/// Start times are whole seconds; allow for rounding between reads.
const START_TOLERANCE_SECS: u64 = 1;

/// What a look at the process table found for one PID. Three-valued because "could not look" is not "gone" (rule 53).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// No process has this PID.
    Gone,
    /// A process has it, started at this time (seconds since the Unix epoch).
    Started(u64),
    /// The process table could not be read: nothing is known.
    Unknown,
}

impl Look {
    /// The start time when a process was seen.
    pub fn started(self) -> Option<u64> {
        match self {
            Look::Started(t) => Some(t),
            Look::Gone | Look::Unknown => None,
        }
    }
}

#[cfg(test)]
impl From<Option<u64>> for Look {
    /// A table that was read: a start time, or nobody.
    fn from(seen: Option<u64>) -> Look {
        seen.map_or(Look::Gone, Look::Started)
    }
}

/// Looks up the process with this PID in the process table.
pub type Probe<'a> = &'a dyn Fn(u32) -> Look;

/// The OS's process table (the production [`Probe`]). A process sysinfo could not read (start time 0 or status Unknown: macOS
/// lists another user's root process so) is `Unknown`, and one it does not list is `Gone` only when `paddock_proc` confirms
/// it with an OS answer that tells "no such process" from "refused".
#[cfg(feature = "os-probe")]
pub fn os_probe(pid: u32) -> Look {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let id = Pid::from_u32(pid);
    let Ok(mut sys) = System::new()
        .inspect_err(|why| log::warn!("the OS process table is unavailable (sysinfo): {why}; the start time of pid {id} is unknown"))
    else {
        return Look::Unknown;
    };
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        false,
        ProcessRefreshKind::nothing(),
    );
    look_from(
        sys.process(id).map(|p| (p.start_time(), p.status())),
        || paddock_proc::presence(pid),
    )
}

/// [`os_probe`] over what sysinfo listed (start time, status) and the confirmation asked when it listed nothing.
#[cfg(feature = "os-probe")]
fn look_from(
    seen: Option<(u64, sysinfo::ProcessStatus)>,
    confirm_absent: impl FnOnce() -> paddock_proc::Presence,
) -> Look {
    match seen {
        Some((0, _) | (_, sysinfo::ProcessStatus::Unknown(_))) => Look::Unknown,
        Some((started, _)) => Look::Started(started),
        None => match confirm_absent() {
            paddock_proc::Presence::Gone => Look::Gone,
            paddock_proc::Presence::Running | paddock_proc::Presence::Unknown => Look::Unknown,
        },
    }
}

/// Which state folder a process uses (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceScope {
    /// The single instance: the data folder itself.
    Main,
    /// `--new-instance` or safe mode: `instances/<pid>/`.
    Separate,
}

impl InstanceScope {
    /// The scope for the process that holds (or wants) the single-instance lock.
    pub fn of(single_instance: bool) -> InstanceScope {
        if single_instance {
            InstanceScope::Main
        } else {
            InstanceScope::Separate
        }
    }
}

/// The running process recorded in the marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// Its process id.
    pub pid: u32,
    /// Its start time; `None` when the OS did not tell.
    pub started_at: Option<u64>,
}

/// The marker file's content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarkerBody {
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    safe_mode: bool,
    #[serde(default)]
    app_version: String,
}

impl MarkerBody {
    fn marker(&self) -> Marker {
        Marker {
            pid: self.pid,
            started_at: self.started_at,
        }
    }
}

/// What is on disk where the marker lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerState {
    /// No marker.
    Missing,
    /// Present but not a marker (truncated write, garbage, from the future).
    Unreadable,
    /// A marker.
    Found(Marker),
}

/// How the run that left a marker ended, as far as this start can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviousRun {
    /// No marker: it shut down cleanly (or never ran).
    Clean,
    /// A marker whose process is gone (or an unreadable marker), or a crash marker.
    Crashed,
    /// The marker's process is still alive (another instance, or one still exiting).
    StillRunning,
    /// The process table could not be read, so whether the marker's process runs is not known: neither crashed nor running.
    Unknown,
}

/// Whether the marker's process is alive: the PID exists and, when the marker recorded it, with the same start time
/// (else the PID was reused).
fn alive(m: &Marker, probe: Probe<'_>) -> Liveness {
    match (probe(m.pid), m.started_at) {
        (Look::Gone, _) => Liveness::Dead,
        (Look::Unknown, _) => Liveness::Unknown,
        // A start time of 0 is no start time (the process could not be read), not a pid that was reused.
        (Look::Started(0), _) => Liveness::Unknown,
        (Look::Started(_), None) => Liveness::Alive,
        (Look::Started(now), Some(then)) if now.abs_diff(then) <= START_TOLERANCE_SECS => {
            Liveness::Alive
        }
        (Look::Started(_), Some(_)) => Liveness::Dead,
    }
}

/// Whether a marker's process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Liveness {
    Alive,
    Dead,
    Unknown,
}

/// Decides how the previous run ended. A marker with this process's own PID is necessarily from a dead process. Pure: the
/// OS is reached through `probe`.
pub fn previous_run(marker: MarkerState, own_pid: u32, probe: Probe<'_>) -> PreviousRun {
    match marker {
        MarkerState::Missing => PreviousRun::Clean,
        MarkerState::Unreadable => PreviousRun::Crashed,
        MarkerState::Found(m) if m.pid != own_pid => match alive(&m, probe) {
            Liveness::Alive => PreviousRun::StillRunning,
            Liveness::Dead => PreviousRun::Crashed,
            Liveness::Unknown => PreviousRun::Unknown,
        },
        MarkerState::Found(_) => PreviousRun::Crashed,
    }
}

fn marker_file(dir: &Path) -> VersionedFile<MarkerBody> {
    VersionedFile::new(dir.join(MARKER_FILE), MARKER_VERSION).keep_corrupt(2)
}

fn read_marker(file: &VersionedFile<MarkerBody>) -> (MarkerState, bool) {
    if !file.path().exists() {
        return (MarkerState::Missing, false);
    }
    let loaded = file.load();
    if loaded.problem.is_some() || loaded.value.pid == 0 {
        // A file from the future or a folder in its place is no marker we can keep: it is replaced below.
        return (MarkerState::Unreadable, false);
    }
    (
        MarkerState::Found(loaded.value.marker()),
        loaded.value.safe_mode,
    )
}

/// Removes the folders of separate instances seen to be gone, and any left with this process's PID, which can only be
/// stale. An unreadable process table keeps every folder. Folders whose name is not a PID are left alone. Returns how many were removed. Never fails.
pub fn prune_instances(data: &Path, own_pid: u32, probe: Probe<'_>) -> usize {
    let Ok(entries) = std::fs::read_dir(data.join(INSTANCES_DIR)) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten().take(MAX_INSTANCE_DIRS) {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        let live = pid != own_pid
            && match read_marker(&marker_file(&path)).0 {
                // Only a process seen gone loses its folder: an unreadable table keeps it (rule 53).
                MarkerState::Found(m) if m.pid == pid => alive(&m, probe) != Liveness::Dead,
                // Being created right now, or damaged: decide by the PID alone.
                _ => probe(pid) != Look::Gone,
            };
        if !live && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// What the panic that ended the last run left behind (no panic message: it may name data).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashInfo {
    /// When (RFC 3339, UTC).
    pub time: String,
    /// The panicking thread.
    pub thread: String,
    /// `file:line` of the panic.
    pub location: String,
    /// The crash happened in safe mode.
    pub in_safe_mode: bool,
}

/// What the notice may offer.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offers {
    /// Reopen the session of the run that ended badly.
    pub restore: bool,
    /// Start again with extensions, plugins and the last session off.
    pub safe_mode: bool,
    /// Forget the session and start clean.
    pub start_fresh: bool,
}

/// The offer after a run that ended badly. Pure.
pub fn offers(
    session: &SessionInfo,
    crash: Option<&CrashInfo>,
    safe_mode_available: bool,
) -> Offers {
    Offers {
        restore: session.available,
        // A crash in safe mode was not caused by what safe mode switches off.
        safe_mode: safe_mode_available && !crash.is_some_and(|c| c.in_safe_mode),
        start_fresh: true,
    }
}

/// The previous run ended badly and the user has not decided yet.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UncleanShutdown {
    /// The panic, when one was recorded.
    pub crash: Option<CrashInfo>,
    /// The session that could be restored.
    pub session: SessionInfo,
    /// What the notice offers.
    pub offers: Offers,
}

/// How this run started: the answer to the UI's `recovery_status`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartReport {
    /// This run is in safe mode (the title bar shows it).
    pub safe_mode: bool,
    /// Set after an unclean shutdown until the user decides.
    pub unclean: Option<UncleanShutdown>,
}

/// What a restore gives back.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlan {
    /// The windows of the session, in label order (the app reopens the ones it knows).
    pub windows: Vec<String>,
    /// The asking window's own UI state, if the session had one.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub ui: Option<Value>,
}

/// The user's answer to the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Reopen the session.
    Restore,
    /// Restart in safe mode.
    SafeMode,
    /// Forget the session.
    StartFresh,
}

/// How to begin a run.
#[derive(Debug, Clone, Copy)]
pub struct Start<'a> {
    /// The app's data folder ([`paddock_env::AppDirs::data`]).
    pub data_dir: &'a Path,
    /// The process name the log's panic hook used for its crash marker.
    pub process: &'a str,
    /// Normal or safe. Safe mode always uses a separate instance folder.
    pub mode: Mode,
    /// Whether this process holds the single-instance lock.
    pub scope: InstanceScope,
    /// The app's version, recorded in the marker.
    pub app_version: &'a str,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How this run started and what to do about the last one's end. Managed as the app's state.
pub struct Lifecycle {
    mode: Mode,
    scope: InstanceScope,
    dir: PathBuf,
    crash_marker: PathBuf,
    own: Marker,
    file: VersionedFile<MarkerBody>,
    previous: PreviousRun,
    crash: Option<CrashInfo>,
    store: Arc<SessionStore>,
    saver: Autosaver,
    decision: Mutex<Option<Decision>>,
}

impl Lifecycle {
    /// Begins a run in this process: the real PID and process table.
    #[cfg(feature = "os-probe")]
    pub fn begin(start: Start<'_>) -> Lifecycle {
        Lifecycle::begin_with(start, std::process::id(), &os_probe)
    }

    /// Begins a run for a given PID and process table (tests).
    ///
    /// Reads how the previous run ended, writes this run's marker, prunes dead instances and rotates the session. Never
    /// fails: what cannot be done (a read-only folder) is logged and the run goes on without that protection.
    pub fn begin_with(start: Start<'_>, own_pid: u32, probe: Probe<'_>) -> Lifecycle {
        let own = Marker {
            pid: own_pid,
            started_at: probe(own_pid).started(),
        };
        let scope = if start.mode.is_safe() {
            InstanceScope::Separate
        } else {
            start.scope
        };
        let crash_marker = CrashMarker::path_for(start.data_dir, start.process);
        // Only the main instance reads (and later clears) the app's crash marker: a separate instance, safe mode among
        // them, must not report or erase the main instance's crash. It is read before pruning because the crash may have
        // been a separate instance's, whose folder is about to be removed.
        let main = scope == InstanceScope::Main;
        let crash = main
            .then(|| CrashMarker::read(&crash_marker))
            .flatten()
            .map(|m| CrashInfo {
                in_safe_mode: read_marker(&marker_file(&instance_dir(start.data_dir, m.pid))).1,
                time: m.time,
                thread: m.thread,
                location: m.location,
            });
        let crash_file_exists = main && crash_marker.exists();
        let pruned = prune_instances(start.data_dir, own_pid, probe);
        if pruned > 0 {
            log::info!(Folders = pruned as u64; "Removed the state of instances that are no longer running");
        }
        let dir = match scope {
            InstanceScope::Main => start.data_dir.to_path_buf(),
            InstanceScope::Separate => instance_dir(start.data_dir, own_pid),
        };
        let file = marker_file(&dir);
        let (state, _) = read_marker(&file);
        let mut previous = previous_run(state, own_pid, probe);
        if previous == PreviousRun::Clean && (crash.is_some() || crash_file_exists) {
            previous = PreviousRun::Crashed;
        }
        match previous {
            PreviousRun::Clean => {}
            PreviousRun::Crashed => log::warn!("The previous run of the app did not end cleanly"),
            PreviousRun::Unknown => log::warn!(
                "Whether the previous run of the app ended cleanly is not known: the process table could not be read"
            ),
            PreviousRun::StillRunning => {
                log::info!("Another process still holds the running marker; taking it over");
            }
        }
        if state == MarkerState::Unreadable {
            // Damaged or from a newer version: gone, so this run can write its own.
            let _ = std::fs::remove_file(file.path());
            let _ = file.load();
        }
        let body = MarkerBody {
            pid: own.pid,
            started_at: own.started_at,
            safe_mode: start.mode.is_safe(),
            app_version: start.app_version.to_string(),
        };
        if let Err(e) = file.save(&body) {
            log::warn!("Could not record that the app is running: {e}");
        }
        let store = Arc::new(SessionStore::new(&dir.join(SESSION_DIR)));
        store.rotate();
        let saver = Autosaver::start(Arc::clone(&store));
        Lifecycle {
            mode: start.mode,
            scope,
            dir,
            crash_marker,
            own,
            file,
            previous,
            crash,
            store,
            saver,
            decision: Mutex::new(None),
        }
    }

    /// Whether this run is in safe mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The state folder of this instance.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// How the previous run ended.
    pub fn previous_run(&self) -> PreviousRun {
        self.previous
    }

    /// The session files (the previous run's, for the app's own "reopen last session" setting).
    pub fn session(&self) -> &SessionStore {
        &self.store
    }

    /// How this run started and what the notice offers. `safe_mode_available`: the app can restart in safe mode.
    pub fn report(&self, safe_mode_available: bool) -> StartReport {
        let unclean = (self.previous == PreviousRun::Crashed
            && !self.mode.is_safe()
            && lock(&self.decision).is_none())
        .then(|| {
            let session = self.store.previous_info();
            UncleanShutdown {
                offers: offers(&session, self.crash.as_ref(), safe_mode_available),
                crash: self.crash.clone(),
                session,
            }
        });
        StartReport {
            safe_mode: self.mode.is_safe(),
            unclean,
        }
    }

    /// The user chose Restore: the previous session, with window `label`'s own state. The notice does not return.
    pub fn restore(&self, label: &str) -> RestorePlan {
        self.decided(Decision::Restore);
        let doc = self.store.previous();
        RestorePlan {
            ui: doc.windows.get(label).cloned(),
            windows: doc.windows.keys().cloned().collect(),
        }
    }

    /// The user chose Start fresh: the previous session is forgotten for good.
    pub fn start_fresh(&self) {
        self.store.discard_previous();
        self.decided(Decision::StartFresh);
    }

    /// The user chose Safe mode and the app is restarting in it: the notice does not return.
    pub fn chose_safe_mode(&self) {
        self.decided(Decision::SafeMode);
    }

    /// The answer so far, if the user gave one.
    pub fn decision(&self) -> Option<Decision> {
        *lock(&self.decision)
    }

    fn decided(&self, d: Decision) {
        *lock(&self.decision) = Some(d);
        if self.scope == InstanceScope::Main {
            let _ = CrashMarker::clear(&self.crash_marker);
        }
    }

    /// Keeps window `label`'s UI state in the session (written on the autosave thread). A state that cannot be kept is
    /// refused with the reason; the in-memory checkpoint is unaffected.
    pub fn checkpoint(&self, label: &str, ui: Value) -> Result<(), SessionError> {
        self.saver.submit(label, ui)
    }

    /// A window was closed by the user: it is not restored after a crash.
    pub fn forget_window(&self, label: &str) {
        self.saver.forget(label);
    }

    /// Waits until every checkpoint so far is on disk.
    pub fn flush(&self) {
        self.saver.flush();
    }

    /// Clean shutdown: the last checkpoint is written, then the next start will not report a crash. A separate instance
    /// removes its whole folder; the main instance removes the marker (and the crash marker) unless a newer process has
    /// taken the marker over.
    pub fn end(&self) {
        self.saver.stop();
        match self.scope {
            InstanceScope::Separate => {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
            InstanceScope::Main => {
                if read_marker(&self.file).0 == MarkerState::Found(self.own) {
                    let _ = std::fs::remove_file(self.file.path());
                    let _ = CrashMarker::clear(&self.crash_marker);
                }
            }
        }
    }
}

fn instance_dir(data: &Path, pid: u32) -> PathBuf {
    data.join(INSTANCES_DIR).join(pid.to_string())
}

#[cfg(test)]
mod tests;
