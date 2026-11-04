//! Stopping every session's copy of an installed app (ADR 0053 section 3): the installer, updater and uninstaller of a
//! per-machine app must replace files that programs in other users' sessions hold open. [`stop_all`] works over a port
//! ([`Machine`]) so the whole decision runs over a fake in unit tests.
//!
//! **By handle, with `SeDebugPrivilege`, is the way** (measured on Windows 11 in the S0 spike): Restart Manager lists other
//! sessions' processes but `RmShutdown(RmForceShutdown)` returns `ERROR_FAIL_NOACTION_REBOOT` and ends nothing, not even
//! the same session's processes, whenever another session holds a file; and `SeDebugPrivilege` is needed even to open
//! another user's process for `PROCESS_QUERY_LIMITED_INFORMATION`. So, from one look at the process table, for each
//! process whose image name is listed: the privilege is enabled, a handle is opened and the image path and start time
//! are read from it; a program that does not run from exactly the install folder (the helper's `Dyno Helper` folder
//! starts with `Dyno`) is left alone and logged; otherwise a second handle opened for termination must be the same process
//! (rule 53), it is terminated and waited for within [`Bounds::end_each`], and the privilege is dropped again at once.
//! Restart Manager is only the check afterwards: one listing over the full path of every listed program names anything
//! that still uses them, in any session.
//!
//! The report names every process acted on (image, identity, session, outcome) so the installer's `setup.log` can show the
//! whole flow (ER-27). The caller is never ended: its identity is excluded and its own image is left out of the registration.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ProcessId;

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{HeldProcess, WindowsMachine, stop_all_here};

/// The exit code of `paddock-setup stop-all` when something listed still runs from the folder.
pub const EXIT_STILL_RUNNING: i32 = 11;

/// How long each step may take. These are an installer's bounds, not a UI's (ER-19 is about the app's own jobs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Waiting for one terminated process to exit.
    pub end_each: Duration,
    /// All the fallback's waits together.
    pub end_total: Duration,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            end_each: Duration::from_secs(5),
            end_total: Duration::from_secs(20),
        }
    }
}

/// What to stop.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The install folder (canonical).
    pub dir: &'a Path,
    /// File names of the programs in `dir` (case does not matter).
    pub images: &'a [String],
    /// The calling process, which is never ended.
    pub own: ProcessId,
    /// The calling process's own program file, left out of the Restart Manager registration.
    pub own_image: Option<&'a Path>,
}

/// Why a step failed. Every variant carries the Windows error code so the log names the cause.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StopError {
    /// A Restart Manager call failed.
    #[error("Restart Manager {call} failed with error {code}")]
    RestartManager {
        /// The API call.
        call: &'static str,
        /// Its Windows error code.
        code: u32,
    },
    /// The process table could not be read.
    #[error("the process table could not be read (error {0})")]
    Table(u32),
    /// `SeDebugPrivilege` could not be enabled.
    #[error("SeDebugPrivilege could not be enabled (error {0}); the policy may have removed it")]
    Privilege(u32),
    /// `TerminateProcess` failed.
    #[error("TerminateProcess failed (error {0})")]
    Terminate(u32),
}

/// Why a process could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFailure {
    /// There is no such process any more.
    Gone,
    /// The system refused, with this error code.
    Denied(u32),
}

/// One process Restart Manager lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RmProcess {
    /// Its identity (`RM_UNIQUE_PROCESS`: pid plus start time).
    pub id: ProcessId,
    /// Its session, when known.
    pub session: Option<u32>,
    /// Its image file name, or the application name Restart Manager gives.
    pub name: String,
}

/// One process of the process table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Its pid (the table has no start time; the handle gives it).
    pub pid: u32,
    /// Its image file name.
    pub image: String,
}

/// A process held open by handle.
pub trait Held {
    /// The identity read from the handle itself.
    fn id(&self) -> ProcessId;
    /// The full image path read from the handle, when it could be read.
    fn image(&self) -> Option<&Path>;
    /// The session, when known.
    fn session(&self) -> Option<u32>;
}

/// What `stop_all` needs from the machine: a port, so the decisions are tested over a fake.
pub trait Machine {
    /// A process held open by handle.
    type Held: Held;
    /// Names every process in any session that uses one of `files` (a new Restart Manager session): the check.
    fn rm_list(&mut self, files: &[PathBuf]) -> Result<Vec<RmProcess>, StopError>;
    /// One look at the process table.
    fn processes(&mut self) -> Result<Vec<Candidate>, StopError>;
    /// Enables `SeDebugPrivilege` in this process's token.
    fn enable_privilege(&mut self) -> Result<(), StopError>;
    /// Disables it again.
    fn drop_privilege(&mut self);
    /// Opens `pid` to read its image and start time (`for_end` false) or to end it (`for_end` true).
    fn open(&mut self, pid: u32, for_end: bool) -> Result<Self::Held, OpenFailure>;
    /// Terminates the held process.
    fn terminate(&mut self, held: &Self::Held) -> Result<(), StopError>;
    /// Waits on the held process's handle for it to exit; true when it did within `bound`.
    fn wait_exit(&mut self, held: &Self::Held, bound: Duration) -> bool;
}

/// What came of one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It is gone.
    Ended,
    /// It was left alone, for this reason.
    Skipped(String),
    /// It was not ended, for this reason; it may still run.
    Failed(String),
}

/// One process acted on, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Its image file name.
    pub image: String,
    /// Its identity, when known.
    pub id: Option<ProcessId>,
    /// Its session, when known.
    pub session: Option<u32>,
    /// The outcome.
    pub outcome: Outcome,
}

/// What `stop_all` found and did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopReport {
    /// Every process acted on or deliberately left alone, in order.
    pub actions: Vec<Action>,
    /// Image names of listed programs that still run from the folder.
    pub still_running: Vec<String>,
    /// Why the final Restart Manager check failed, when it did (the handle results then stand alone).
    pub restart_manager_error: Option<StopError>,
    /// Why `SeDebugPrivilege` could not be enabled, when it could not.
    pub privilege_error: Option<StopError>,
}

impl StopReport {
    /// 0 when nothing listed runs from the folder, [`EXIT_STILL_RUNNING`] otherwise.
    pub fn exit_code(&self) -> i32 {
        if self.still_running.is_empty() { 0 } else { EXIT_STILL_RUNNING }
    }
}

fn parts(path: &Path) -> Vec<String> {
    let s = path.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.split(['\\', '/']).filter(|p| !p.is_empty()).map(str::to_lowercase).collect()
}

/// Whether `file` is directly inside `dir`, compared by path components and without case, never by string prefix:
/// `C:\Program Files\Dyno Helper\x.exe` is not inside `C:\Program Files\Dyno`.
pub fn is_directly_in(file: &Path, dir: &Path) -> bool {
    let (f, d) = (parts(file), parts(dir));
    !d.is_empty() && f.len() == d.len() + 1 && f[..d.len()] == d[..]
}

fn join_win(dir: &Path, image: &str) -> PathBuf {
    let d = dir.to_string_lossy();
    PathBuf::from(format!("{}\\{image}", d.trim_end_matches(['\\', '/'])))
}

fn note(report: &mut StopReport, action: Action) {
    let who = format!(
        "{} {}session {}",
        action.image,
        action.id.map(|i| format!("{i}, ")).unwrap_or_default(),
        action.session.map_or_else(|| "?".to_owned(), |s| s.to_string())
    );
    match &action.outcome {
        Outcome::Ended => log::info!("stop-all: ended {who}"),
        Outcome::Skipped(why) => log::info!("stop-all: left {who} alone: {why}"),
        Outcome::Failed(why) => log::warn!("stop-all: could not end {who}: {why}"),
    }
    report.actions.push(action);
}

fn remember(report: &mut StopReport, image: &str) {
    if !report.still_running.iter().any(|i| i.eq_ignore_ascii_case(image)) {
        report.still_running.push(image.to_owned());
    }
}

/// Stops every listed program that runs from `req.dir`, in any session. See the module docs.
pub fn stop_all<M: Machine>(machine: &mut M, req: &Request<'_>, bounds: &Bounds) -> StopReport {
    let started = Instant::now();
    let mut report = StopReport::default();
    let wanted: Vec<String> = req.images.iter().map(|i| i.to_lowercase()).collect();
    let own_image = req.own_image.map(parts);
    let files: Vec<PathBuf> = req
        .images
        .iter()
        .map(|i| join_win(req.dir, i))
        .filter(|f| own_image.as_ref().is_none_or(|o| *o != parts(f)))
        .collect();
    log::info!("stop-all: {} listed programs; ending them by handle", files.len());

    let table = machine.processes();
    let candidates: Vec<Candidate> = match table {
        Ok(t) => t
            .into_iter()
            .filter(|c| c.pid != req.own.pid() && wanted.contains(&c.image.to_lowercase()))
            .collect(),
        Err(e) => {
            // Nothing can be ended without the table; the check below names what runs.
            log::warn!("stop-all: {e}");
            Vec::new()
        }
    };
    let mut spent = Duration::ZERO;
    let mut privilege_lost = false;
    for c in candidates {
        if privilege_lost {
            remember(&mut report, &c.image);
            continue;
        }
        if let Err(e) = machine.enable_privilege() {
            log::warn!("stop-all: {e}");
            report.privilege_error = Some(e);
            privilege_lost = true;
            remember(&mut report, &c.image);
            continue;
        }
        let action = end_by_handle(machine, req, bounds, &c, &mut spent);
        machine.drop_privilege();
        if matches!(action.outcome, Outcome::Failed(_)) {
            remember(&mut report, &c.image);
        }
        note(&mut report, action);
    }

    // The check: Restart Manager names whatever still uses the files, in any session.
    match machine.rm_list(&files) {
        Ok(rest) => {
            for p in rest.iter().filter(|p| p.id != req.own) {
                log::warn!("stop-all: {} (session {:?}, {}) still uses the install folder", p.name, p.session, p.id);
                remember(&mut report, &p.name);
            }
        }
        Err(e) => {
            log::warn!("stop-all: the final Restart Manager check failed: {e}");
            report.restart_manager_error = Some(e);
        }
    }
    log::info!(
        "stop-all: done in {:?}: {} acted on, {} still running",
        started.elapsed(),
        report.actions.len(),
        report.still_running.len()
    );
    report
}

fn end_by_handle<M: Machine>(
    machine: &mut M,
    req: &Request<'_>,
    bounds: &Bounds,
    c: &Candidate,
    spent: &mut Duration,
) -> Action {
    let mut action = Action { image: c.image.clone(), id: None, session: None, outcome: Outcome::Ended };
    let held = match machine.open(c.pid, false) {
        Ok(h) => h,
        Err(OpenFailure::Gone) => {
            action.outcome = Outcome::Skipped("it exited before it was opened".into());
            return action;
        }
        Err(OpenFailure::Denied(code)) => {
            action.outcome = Outcome::Failed(format!("it could not be opened (error {code})"));
            return action;
        }
    };
    action.id = Some(held.id());
    action.session = held.session();
    match held.image() {
        None => {
            action.outcome = Outcome::Failed("its image path could not be read".into());
            return action;
        }
        Some(image) if !is_directly_in(image, req.dir) => {
            action.outcome = Outcome::Skipped("it runs from outside the install folder".into());
            return action;
        }
        Some(_) => {}
    }
    let end = match machine.open(c.pid, true) {
        Ok(h) => h,
        Err(OpenFailure::Gone) => {
            action.outcome = Outcome::Skipped("it exited before it was opened to end".into());
            return action;
        }
        Err(OpenFailure::Denied(code)) => {
            action.outcome = Outcome::Failed(format!("it could not be opened to end (error {code})"));
            return action;
        }
    };
    if end.id() != held.id() {
        action.outcome = Outcome::Skipped("its identity changed between the two opens".into());
        return action;
    }
    if let Err(e) = machine.terminate(&end) {
        action.outcome = Outcome::Failed(e.to_string());
        return action;
    }
    let bound = bounds.end_each.min(bounds.end_total.saturating_sub(*spent));
    let t = Instant::now();
    let exited = machine.wait_exit(&end, bound);
    *spent += t.elapsed();
    if !exited {
        action.outcome = Outcome::Failed(format!("it did not exit within {bound:?} of the end"));
    }
    action
}

#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod windows_tests;
