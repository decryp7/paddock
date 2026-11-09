//! The Unix guardian and keepers (see [`super`]).

use super::{GUARDIAN_ENV, LOG_ENV};
use crate::ProcessId;
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// The owner's pid, for the guardian's log lines.
const OWNER_ENV: &str = "PADDOCK_PROC_GUARDIAN_OWNER";
/// How often, and how many times, the guardian looks whether an ended group has emptied before it removes the folder.
const EMPTY_LOOK: Duration = Duration::from_millis(50);
const EMPTY_LOOKS: u32 = 200;

/// How this executable runs as the guardian: unset until [`run_if_requested`] (or, in a test binary,
/// [`enable_in_test_binary`]) was called; then the arguments to run it with.
static HOOK: OnceLock<Vec<OsString>> = OnceLock::new();
static LOG: OnceLock<PathBuf> = OnceLock::new();

/// Call first thing in `main`. In the guardian process this serves the owner and never returns; elsewhere it returns at
/// once and lets this executable start a guardian for its supervised children.
pub fn run_if_requested() {
    if std::env::var_os(GUARDIAN_ENV).is_some_and(|v| v == "1") {
        std::process::exit(serve(std::io::stdin().lock()));
    }
    // A second call (or one after `enable_in_test_binary`) keeps the first.
    let _ = HOOK.set(Vec::new());
}

/// For a test binary (whose `main` is the test harness): `entry` is the full path of a test function of this binary that
/// calls [`run_if_requested`] (for example `guardian_entry`, or `launch::tests::guardian_entry`); the guardian is this
/// binary run on that test alone.
pub fn enable_in_test_binary(entry: &str) {
    let _ = HOOK.set(vec![entry.into(), "--exact".into()]);
}

/// Where the guardian writes its log lines (call before the first guarded child; the environment's [`LOG_ENV`] wins).
pub fn set_log(path: &Path) {
    let _ = LOG.set(path.to_path_buf());
}

fn log_path() -> PathBuf {
    std::env::var_os(LOG_ENV)
        .map(PathBuf::from)
        .or_else(|| LOG.get().cloned())
        .unwrap_or_else(|| std::env::temp_dir().join("paddock-proc-guardian.log"))
}

/// The keeper program.
fn cat() -> Option<&'static Path> {
    ["/bin/cat", "/usr/bin/cat"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.exists())
}

/// The running guardian, as the owner holds it.
struct Running {
    /// The guardian process (this process's child).
    child: Child,
    /// The registrations go here; this process ending closes it, which is the guardian's end of file.
    to: std::io::PipeWriter,
    /// Each keeper's standard input: a pipe whose write end only the guardian holds.
    keeper_input: std::io::PipeReader,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Running>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Starts the guardian: this executable with [`GUARDIAN_ENV`], in a group of its own.
fn start(args: &[OsString]) -> std::io::Result<Running> {
    let exe = std::env::current_exe()?;
    let (from_owner, to) = std::io::pipe()?;
    let (keeper_input, keeper_hold) = std::io::pipe()?;
    let child = Command::new(exe)
        .args(args)
        .env(GUARDIAN_ENV, "1")
        .env(LOG_ENV, log_path())
        .env(OWNER_ENV, std::process::id().to_string())
        .stdin(from_owner)
        .stdout(keeper_hold)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    log::info!(
        "Started the orphan guardian (pid {}) for this process's supervised children; its log is {}",
        child.id(),
        log_path().display()
    );
    Ok(Running {
        child,
        to,
        keeper_input,
    })
}

/// A guarded child, as its owner holds it. Dropping it unregisters the child (its owner ended it) and ends its keeper;
/// drop it after ending the child's tree.
pub(crate) struct Registration {
    group: u32,
    keeper: Child,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(r) = lock().as_mut() {
            crate::cleanup::logged(
                "unregistering a child from the orphan guardian",
                writeln!(r.to, "del {}", self.group),
            );
        }
        // Ended through its handle: it is this process's own unreaped child.
        crate::cleanup::logged("ending a guard's keeper", self.keeper.kill());
        crate::cleanup::logged("reaping a guard's keeper", self.keeper.wait());
    }
}

/// Registers the child leading process group `group` (just started and not reaped: the group is still its own), with
/// `folder` to remove after the group if this process dies first. `None` when this executable has no guardian (it does
/// not call [`run_if_requested`]) or the guard could not be set up; the reason is logged (once when the guard is off).
pub(crate) fn register(group: u32, folder: Option<&Path>) -> Option<Registration> {
    static OFF: OnceLock<()> = OnceLock::new();
    let off = |why: &str| {
        OFF.get_or_init(|| {
            log::warn!(
                "The orphan guard is off: {why}; if this process is killed, its supervised children's trees are left running"
            );
        });
        None
    };
    let Some(args) = HOOK.get() else {
        return off(
            "this executable does not call paddock_proc::guardian::run_if_requested() in main",
        );
    };
    let Some(cat) = cat() else {
        return off("there is no /bin/cat for the keepers");
    };
    let pgid = i32::try_from(group).ok()?;
    let mut guard = lock();
    // A guardian that ended (killed by someone) is started again; its registrations ended with it.
    if let Some(r) = guard.as_mut()
        && !matches!(r.child.try_wait(), Ok(None))
    {
        log::warn!(
            "The orphan guardian (pid {}) had ended; a new one is started",
            r.child.id()
        );
        *guard = None;
    }
    if guard.is_none() {
        match start(args) {
            Ok(r) => *guard = Some(r),
            Err(e) => return off(&format!("the guardian could not be started ({e})")),
        }
    }
    let r = guard.as_mut()?;
    let started = r.keeper_input.try_clone().and_then(|input| {
        Command::new(cat)
            .stdin(input)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(pgid)
            .spawn()
    });
    let keeper = match started {
        Ok(k) => k,
        Err(e) => {
            log::warn!(
                "The keeper of child group {group} could not be started ({e}): the child is not guarded"
            );
            return None;
        }
    };
    let end = |mut keeper: Child| {
        crate::cleanup::logged("ending an unregistered keeper", keeper.kill());
        crate::cleanup::logged("reaping an unregistered keeper", keeper.wait());
    };
    let Some(id) = ProcessId::of(keeper.id()) else {
        log::warn!(
            "The keeper of child group {group} has no readable start time: the child is not guarded"
        );
        end(keeper);
        return None;
    };
    let folder = folder.map_or_else(|| "-".to_string(), |f| hex(f.as_os_str().as_bytes()));
    if let Err(e) = writeln!(
        r.to,
        "add {group} {} {} {folder}",
        id.pid(),
        id.started().as_micros()
    ) {
        log::warn!(
            "Child group {group} could not be registered with the orphan guardian ({e}): the child is not guarded, and a new \
             guardian is started for the next one"
        );
        *guard = None;
        end(keeper);
        return None;
    }
    Some(Registration { group, keeper })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// One registered group, as the guardian keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Guarded {
    pub group: u32,
    pub keeper: ProcessId,
    pub folder: Option<PathBuf>,
}

/// One line from the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    Add(Guarded),
    Del(u32),
}

/// Parses one line from the owner (`add <group> <keeper pid> <keeper start µs> <folder as hex | ->`, `del <group>`);
/// `None` for anything else.
pub(crate) fn parse(line: &str) -> Option<Line> {
    let w: Vec<&str> = line.split_whitespace().collect();
    match w[..] {
        ["add", g, k, s, f] => Some(Line::Add(Guarded {
            group: g.parse().ok()?,
            keeper: ProcessId::new(
                k.parse().ok()?,
                crate::StartTime::from_micros(s.parse().ok()?),
            ),
            folder: if f == "-" {
                None
            } else {
                Some(PathBuf::from(OsString::from_vec(unhex(f)?)))
            },
        })),
        ["del", g] => Some(Line::Del(g.parse().ok()?)),
        _ => None,
    }
}

/// What the guardian does to a registered group once its owner is gone (the OS behind a port, so the logic is tested
/// without real processes).
pub(crate) trait Ends {
    /// Ends the group if its keeper still holds it; false when it does not (then nothing is signalled).
    fn end_group(&mut self, g: &Guarded) -> bool;
    /// Whether the group still has a member.
    fn group_exists(&mut self, group: u32) -> bool;
    /// Removes the folder (one that is already gone is no error).
    fn remove(&mut self, folder: &Path) -> std::io::Result<()>;
    /// Waits before looking at an ended group again.
    fn pause(&mut self);
    /// Writes one log line.
    fn log(&mut self, line: &str);
}

/// Reads the owner's lines until end of file, then ends what is still registered: the guardian's whole life. An ended
/// group is looked at up to `looks` times for its last member to go before its folder is removed.
pub(crate) fn guard(input: impl BufRead, ends: &mut dyn Ends, looks: u32) {
    let mut guarded: Vec<Guarded> = Vec::new();
    for line in input.lines() {
        let Ok(line) = line else { break };
        match parse(&line) {
            Some(Line::Add(g)) => {
                guarded.retain(|x| x.group != g.group);
                guarded.push(g);
            }
            Some(Line::Del(group)) => guarded.retain(|x| x.group != group),
            None => ends.log(&format!("ignored a line it does not know: {line:?}")),
        }
    }
    if guarded.is_empty() {
        return;
    }
    ends.log(&format!(
        "its owner is gone with {} child group(s) still registered",
        guarded.len()
    ));
    for g in &guarded {
        if !ends.end_group(g) {
            ends.log(&format!(
                "left group {}: its keeper ({}) no longer holds it",
                g.group, g.keeper
            ));
            continue;
        }
        let mut n = 0;
        while ends.group_exists(g.group) && n < looks {
            ends.pause();
            n += 1;
        }
        let state = if n < looks {
            "emptied"
        } else {
            "still had members"
        };
        match &g.folder {
            Some(f) => match ends.remove(f) {
                Ok(()) => ends.log(&format!(
                    "ended group {} ({state}) and removed {}",
                    g.group,
                    f.display()
                )),
                Err(e) => ends.log(&format!(
                    "ended group {} ({state}); {} could not be removed: {e}",
                    g.group,
                    f.display()
                )),
            },
            None => ends.log(&format!("ended group {} ({state})", g.group)),
        }
    }
}

/// The real ends: signals through [`crate::tree`] (rule 53), the file system and the log file.
struct OsEnds {
    log: Option<std::fs::File>,
    owner: String,
}

impl Ends for OsEnds {
    fn end_group(&mut self, g: &Guarded) -> bool {
        crate::tree::end_pinned_group(g.group, g.keeper)
    }
    fn group_exists(&mut self, group: u32) -> bool {
        crate::tree::group_exists(group)
    }
    fn remove(&mut self, folder: &Path) -> std::io::Result<()> {
        match std::fs::remove_dir_all(folder) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
    fn pause(&mut self) {
        // pace: how often an ended group is looked at; EMPTY_LOOKS bounds the wait
        std::thread::sleep(EMPTY_LOOK);
    }
    fn log(&mut self, line: &str) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Some(f) = self.log.as_mut() {
            crate::cleanup::logged(
                "writing the guardian's log",
                writeln!(
                    f,
                    "{secs} paddock-proc guardian (pid {}) of owner pid {}: {line}",
                    std::process::id(),
                    self.owner
                ),
            );
        }
    }
}

/// The guardian process: serves the owner on `input` and returns the exit code.
fn serve(input: impl BufRead) -> i32 {
    let log = std::env::var_os(LOG_ENV).and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    let owner = std::env::var(OWNER_ENV).unwrap_or_else(|_| "?".into());
    guard(input, &mut OsEnds { log, owner }, EMPTY_LOOKS);
    // The keepers' pipe closes with this process, so the keepers of groups that were not ended end too.
    0
}

#[cfg(test)]
mod tests;
