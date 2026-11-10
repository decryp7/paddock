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

/// In a test binary: when set, a process that runs as root acts as the unprivileged user the guardian is meant for, so the
/// Linux CI containers (which run as root) exercise the guardian's acting code under coverage instead of only the root
/// refusal. Tests set it on the child processes they start (never on their own process); it does not exist outside tests.
#[cfg(test)]
pub(crate) const ACT_AS_USER_ENV: &str = "PADDOCK_PROC_TEST_ACT_AS_USER";

/// Whether the root policy applies to this process: root binaries do not use the guardian (ADR 0076 amendment 2026-11-09).
fn runs_as_root() -> bool {
    #[cfg(test)]
    if std::env::var_os(ACT_AS_USER_ENV).is_some() || tests::acting_as_user_here() {
        return false;
    }
    crate::sys::effective_uid() == 0
}

/// The owner's identity (`<pid> <start µs>`), which the guardian checks against its parent before it takes any line.
const OWNER_ENV: &str = "PADDOCK_PROC_GUARDIAN_OWNER";
/// The file in a guarded folder that names its owner (`<pid> <start µs>`): the guardian removes only a folder that has it,
/// owned by the same user and naming the owner it serves.
pub(crate) const MARKER: &str = ".paddock-proc-guardian";
/// How often, and how many times, the guardian looks whether an ended group has emptied before it removes the folder.
const EMPTY_LOOK: Duration = Duration::from_millis(50);
const EMPTY_LOOKS: u32 = 200;
/// The longest line the guardian takes (a folder of 4096 bytes is 8192 hex digits).
const MAX_LINE: usize = 16 << 10;

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
/// A test binary's guardian has no log unless the test names one ([`LOG_ENV`] or [`set_log`]): tests never write into the
/// user's folders (ER-14).
pub fn enable_in_test_binary(entry: &str) {
    let _ = HOOK.set(vec![entry.into(), "--exact".into()]);
}

/// Whether this is a test binary (it called [`enable_in_test_binary`]).
fn in_test_binary() -> bool {
    HOOK.get().is_some_and(|args| !args.is_empty())
}

/// Where the guardian writes its log lines (call before the first guarded child; the environment's [`LOG_ENV`] wins). The
/// folder must belong to this user and be writable by nobody else, or nothing is logged.
pub fn set_log(path: &Path) {
    let _ = LOG.set(path.to_path_buf());
}

/// The guardian's log: [`LOG_ENV`], else [`set_log`], else the user's log folder (`~/Library/Logs` on macOS,
/// `$XDG_STATE_HOME` or `~/.local/state` elsewhere). Never a shared folder such as `/tmp` (the guardian checks the folder
/// when it opens the file); never from the environment for root.
fn log_path() -> Option<PathBuf> {
    let env = (!runs_as_root())
        .then(|| std::env::var_os(LOG_ENV).map(PathBuf::from))
        .flatten();
    env.or_else(|| LOG.get().cloned()).or_else(|| {
        if in_test_binary() {
            return None;
        }
        default_log_path(
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            cfg!(target_os = "macos"),
        )
    })
}

/// The user's own log folder: `~/Library/Logs` on macOS, `$XDG_STATE_HOME` or `~/.local/state` elsewhere.
fn default_log_path(
    home: Option<PathBuf>,
    xdg_state: Option<PathBuf>,
    macos: bool,
) -> Option<PathBuf> {
    let dir = if macos {
        home.map(|h| h.join("Library/Logs"))
    } else {
        xdg_state.or_else(|| home.map(|h| h.join(".local/state")))
    };
    dir.map(|d| d.join("paddock-proc-guardian.log"))
}

/// Opens the log for appending: only in a folder this user owns that nobody else can write to, never through a link
/// (`O_NOFOLLOW`), created with mode 0600. `None` when any of that does not hold.
pub(crate) fn open_log(path: &Path) -> Option<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let dir = std::fs::metadata(path.parent()?).ok()?;
    if !dir.is_dir() || dir.uid() != crate::sys::effective_uid() || dir.mode() & 0o022 != 0 {
        return None;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()
        .filter(|f| {
            f.metadata()
                .is_ok_and(|m| m.is_file() && m.uid() == crate::sys::effective_uid())
        })
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

/// `<pid> <start µs>`.
fn identity_text(id: ProcessId) -> String {
    format!("{} {}", id.pid(), id.started().as_micros())
}

fn parse_identity(s: &str) -> Option<ProcessId> {
    let (pid, start) = s.trim().split_once(' ')?;
    Some(ProcessId::new(
        pid.parse().ok()?,
        crate::StartTime::from_micros(start.parse().ok()?),
    ))
}

/// Starts the guardian: this executable with [`GUARDIAN_ENV`] and this process's identity, in a group of its own.
fn start(args: &[OsString], me: ProcessId) -> std::io::Result<Running> {
    let exe = std::env::current_exe()?;
    let (from_owner, to) = std::io::pipe()?;
    let (keeper_input, keeper_hold) = std::io::pipe()?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env(GUARDIAN_ENV, "1")
        .env(OWNER_ENV, identity_text(me))
        .env_remove(LOG_ENV)
        .stdin(from_owner)
        .stdout(keeper_hold)
        .stderr(Stdio::null())
        .process_group(0);
    let log = log_path();
    if let Some(l) = &log {
        cmd.env(LOG_ENV, l);
    }
    #[cfg(test)]
    if tests::acting_as_user_here() {
        // The guardian is a process of its own: it acts as the user too.
        cmd.env(ACT_AS_USER_ENV, "1");
    }
    let child = cmd.spawn()?;
    log::info!(
        "Started the orphan guardian (pid {}) for this process's supervised children; its log is {}",
        child.id(),
        log_text(log.as_deref())
    );
    Ok(Running {
        child,
        to,
        keeper_input,
    })
}

/// How the log reads in a message.
fn log_text(log: Option<&Path>) -> String {
    log.map_or_else(|| "off".into(), |l| l.display().to_string())
}

/// Sends one line as a single write, so a line the guardian takes is whole (a line cut short by the owner's death has no
/// newline, and the guardian drops it).
fn send(to: &mut std::io::PipeWriter, line: &str) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    to.write_all(&buf)
}

/// A guarded child, as its owner holds it. Dropping it unregisters the child (its owner ended it) and ends its keeper;
/// drop it after ending the child's tree.
pub(crate) struct Registration {
    group: u32,
    pub(crate) keeper: Child,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(r) = lock().as_mut() {
            crate::cleanup::logged(
                "unregistering a child from the orphan guardian",
                send(&mut r.to, &format!("del {}", self.group)),
            );
        }
        // Ended through its handle: it is this process's own unreaped child.
        crate::cleanup::logged("ending a guard's keeper", self.keeper.kill());
        crate::cleanup::logged("reaping a guard's keeper", self.keeper.wait());
    }
}

/// Writes the marker that lets the guardian remove `folder` for `me`; the canonical folder, or `None` (then the folder is
/// not removed by the guardian; the reason is logged).
fn mark(folder: &Path, me: ProcessId) -> Option<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let canonical = std::fs::canonicalize(folder).ok()?;
    let written = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(canonical.join(MARKER))
        .and_then(|mut f| f.write_all(identity_text(me).as_bytes()));
    match written {
        Ok(()) => Some(canonical),
        Err(e) => {
            log::warn!(
                "{} cannot be marked for the orphan guardian ({e}): it is not removed if this process is killed",
                folder.display()
            );
            None
        }
    }
}

/// Registers the child leading process group `group` (just started and not reaped: the group is still its own), with
/// `folder` to remove after the group if this process dies first. `None` when this executable has no guardian (it does
/// not call [`run_if_requested`], or it runs as root) or the guard could not be set up; the reason is logged (once when
/// the guard is off).
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
    if runs_as_root() {
        return off(
            "this process runs as root, and root binaries do not use the guardian (ADR 0076 amendment 2026-11-09)",
        );
    }
    let Some(cat) = cat() else {
        return off("there is no /bin/cat for the keepers");
    };
    let Some(me) = ProcessId::of(std::process::id()) else {
        return off("this process's start time cannot be read");
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
        match start(args, me) {
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
    let folder = folder.and_then(|f| mark(f, me));
    let line = add_line(group, id, folder.as_deref());
    if let Err(e) = send(&mut r.to, &line) {
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

/// `add <group> <keeper pid> <keeper start µs> <hex length> <folder as hex | ->`.
pub(crate) fn add_line(group: u32, keeper: ProcessId, folder: Option<&Path>) -> String {
    let (len, hex) = folder.map_or((0, "-".to_string()), |f| {
        let h = hex(f.as_os_str().as_bytes());
        (h.len(), h)
    });
    format!("add {group} {} {len} {hex}", identity_text(keeper))
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

/// Parses one line from the owner ([`add_line`], `del <group>`); `None` for anything else, a folder whose hex does not
/// have the length the line declares (a line cut short) included.
pub(crate) fn parse(line: &str) -> Option<Line> {
    let w: Vec<&str> = line.split(' ').collect();
    match w[..] {
        ["add", g, k, s, len, f] => {
            let len: usize = len.parse().ok()?;
            let folder = match (len, f) {
                (0, "-") => None,
                (n, f) if n > 0 && f.len() == n => {
                    Some(PathBuf::from(OsString::from_vec(unhex(f)?)))
                }
                _ => return None,
            };
            Some(Line::Add(Guarded {
                group: g.parse().ok()?,
                keeper: ProcessId::new(
                    k.parse().ok()?,
                    crate::StartTime::from_micros(s.parse().ok()?),
                ),
                folder,
            }))
        }
        ["del", g] => Some(Line::Del(g.parse().ok()?)),
        _ => None,
    }
}

/// What the guardian does to a registered group once its owner is gone (the OS behind a port, so the logic is tested
/// without real processes).
pub(crate) trait Ends {
    /// Ends the group if its keeper still holds it; false when it does not (then nothing is signalled).
    fn end_group(&mut self, g: &Guarded) -> bool;
    /// Whether the group still has a member that runs (a zombie waiting to be reaped is not one).
    fn group_exists(&mut self, group: u32) -> bool;
    /// Removes the folder if it is one the owner marked ([`removable`]); the reason when it is not removed.
    fn remove(&mut self, folder: &Path) -> Result<(), String>;
    /// Waits before looking at an ended group again.
    fn pause(&mut self);
    /// Writes one log line.
    fn log(&mut self, line: &str);
}

/// Reads the owner's lines until end of file, then ends what is still registered: the guardian's whole life. Only whole
/// lines count: a last line without its newline (the owner died while writing it) is dropped. An ended group is looked
/// at up to `looks` times for its last member to go before its folder is removed.
pub(crate) fn guard(input: impl BufRead, ends: &mut dyn Ends, looks: u32) {
    let mut input = input;
    let mut guarded: Vec<Guarded> = Vec::new();
    let mut buf = Vec::new();
    'lines: loop {
        buf.clear();
        match std::io::Read::take(&mut input, MAX_LINE as u64).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if buf.last() != Some(&b'\n') && buf.len() == MAX_LINE {
            // Longer than any line the owner sends: skipped up to its newline; the lines after it still count.
            ends.log(&format!("skipped a line longer than {MAX_LINE} bytes"));
            loop {
                buf.clear();
                match std::io::Read::take(&mut input, MAX_LINE as u64).read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break 'lines,
                    Ok(_) if buf.last() == Some(&b'\n') => continue 'lines,
                    Ok(_) => {}
                }
            }
        }
        if buf.last() != Some(&b'\n') {
            ends.log(&format!(
                "dropped a line without its end ({} bytes)",
                buf.len()
            ));
            break;
        }
        buf.pop();
        let line = String::from_utf8_lossy(&buf);
        match parse(&line) {
            Some(Line::Add(g)) => {
                guarded.retain(|x| x.group != g.group);
                guarded.push(g);
            }
            Some(Line::Del(group)) => guarded.retain(|x| x.group != group),
            None => ends.log(&format!("ignored a line it does not take: {line:?}")),
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
                    "ended group {} ({state}); left {}: {e}",
                    g.group,
                    f.display()
                )),
            },
            None => ends.log(&format!("ended group {} ({state})", g.group)),
        }
    }
}

/// Whether the guardian of `owner` may remove `folder`: an absolute, canonical path (no link on the way), a real folder
/// (not a link), holding the owner's [`MARKER`]: a regular file of this user that names `owner`. The marker is the
/// capability: only the owner wrote it, so a forged line cannot name a folder of the user's that is not the owner's.
pub(crate) fn removable(
    folder: &Path,
    owner: ProcessId,
    home: Option<&Path>,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if let Some(why) = never_removed(folder, home) {
        return Err(format!("refused: {why}"));
    }
    if std::fs::canonicalize(folder).ok().as_deref() != Some(folder) {
        return Err("refused: not a canonical path to an existing folder".into());
    }
    if !std::fs::symlink_metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Err("refused: not a folder".into());
    }
    let marker = folder.join(MARKER);
    let ok = std::fs::symlink_metadata(&marker)
        .is_ok_and(|m| m.is_file() && m.uid() == crate::sys::effective_uid())
        && std::fs::read_to_string(&marker).is_ok_and(|t| parse_identity(&t) == Some(owner));
    if ok {
        Ok(())
    } else {
        Err("refused: it has no marker of this owner".into())
    }
}

/// Folders the guardian never removes, marker or not: the home folder and its ancestors, and any path of two components or
/// fewer (`/`, `/Users`, `/home/u`, `/tmp/x`): a guarded folder is a working folder deep inside some base.
pub(crate) fn never_removed(folder: &Path, home: Option<&Path>) -> Option<&'static str> {
    let depth = folder
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    if depth <= 2 {
        return Some("a path of two components or fewer");
    }
    if home.is_some_and(|h| h.starts_with(folder)) {
        return Some("the home folder or one of its ancestors");
    }
    None
}

/// Whether the process that started the guardian is the owner it names: its parent, alive by identity (rule 53), running
/// this same executable. A guardian started by anyone else (an environment and a standard input are easy to forge)
/// does nothing.
pub(crate) fn check_owner(
    owner: ProcessId,
    parent: u32,
    owner_alive: bool,
    owner_exe: Option<&Path>,
    my_exe: Option<&Path>,
) -> Result<(), String> {
    if parent != owner.pid() {
        return Err(format!(
            "its parent (pid {parent}) is not the owner it names ({owner})"
        ));
    }
    if !owner_alive {
        return Err(format!("the owner it names ({owner}) is not running"));
    }
    match (owner_exe, my_exe) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err("its parent runs another executable".into()),
    }
}

/// Linux: whether `owner` holds the pipe this process reads (its inode among the owner's descriptors).
#[cfg(target_os = "linux")]
fn owner_holds_pipe(owner: u32, inode: u64) -> bool {
    let want = format!("pipe:[{inode}]");
    std::fs::read_dir(format!("/proc/{owner}/fd")).is_ok_and(|d| {
        d.flatten()
            .any(|e| std::fs::read_link(e.path()).is_ok_and(|l| l.as_os_str() == want.as_str()))
    })
}

/// The guardian's log: opened on its first line, so a guardian with nothing to say leaves no file behind.
enum LogFile {
    Closed(Option<PathBuf>),
    Open(Option<std::fs::File>),
}

impl LogFile {
    fn file(&mut self) -> Option<&mut std::fs::File> {
        if let LogFile::Closed(path) = self {
            *self = LogFile::Open(path.as_deref().and_then(open_log));
        }
        match self {
            LogFile::Open(f) => f.as_mut(),
            LogFile::Closed(_) => None,
        }
    }
}

/// The real ends: signals through [`crate::tree`] (rule 53), the file system and the log file.
struct OsEnds {
    log: LogFile,
    owner: ProcessId,
    cat: Option<PathBuf>,
}

impl Ends for OsEnds {
    fn end_group(&mut self, g: &Guarded) -> bool {
        // The keeper must be a keeper: the program the owner starts for it.
        self.cat.is_some()
            && crate::sys::exe_of(g.keeper.pid()) == self.cat
            && crate::tree::end_pinned_group(g.group, g.keeper)
    }
    fn group_exists(&mut self, group: u32) -> bool {
        crate::tree::group_exists(group)
    }
    fn remove(&mut self, folder: &Path) -> Result<(), String> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let home = home
            .as_deref()
            .map(|h| std::fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()));
        removable(folder, self.owner, home.as_deref())?;
        std::fs::remove_dir_all(folder).map_err(|e| e.to_string())
    }
    fn pause(&mut self) {
        // pace: how often an ended group is looked at; EMPTY_LOOKS bounds the wait
        std::thread::sleep(EMPTY_LOOK);
    }
    fn log(&mut self, line: &str) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Some(f) = self.log.file() {
            let text = format!(
                "{secs} paddock-proc guardian (pid {}) of owner {}: {line}\n",
                std::process::id(),
                self.owner
            );
            crate::cleanup::logged("writing the guardian's log", f.write_all(text.as_bytes()));
        }
    }
}

/// The guardian process: checks who started it, serves the owner on `input` and returns the exit code.
fn serve(input: impl BufRead) -> i32 {
    if runs_as_root() {
        return 3;
    }
    let log = LogFile::Closed(std::env::var_os(LOG_ENV).map(PathBuf::from));
    let Some(owner) = std::env::var(OWNER_ENV)
        .ok()
        .and_then(|s| parse_identity(&s))
    else {
        let mut ends = OsEnds {
            log,
            owner: ProcessId::new(0, crate::StartTime::from_micros(0)),
            cat: None,
        };
        ends.log(&format!("refused to serve: {OWNER_ENV} names no owner"));
        return 2;
    };
    let mut ends = OsEnds {
        log,
        owner,
        cat: cat().and_then(|c| std::fs::canonicalize(c).ok()),
    };
    let me = std::env::current_exe()
        .ok()
        .and_then(|e| std::fs::canonicalize(e).ok());
    let mut checked = check_owner(
        owner,
        crate::sys::parent_pid(),
        owner.is_alive(),
        crate::sys::exe_of(owner.pid()).as_deref(),
        me.as_deref(),
    );
    let pipe = crate::sys::stdin_pipe();
    if checked.is_ok() && pipe.is_none() {
        checked = Err("its standard input is not a pipe".into());
    }
    #[cfg(target_os = "linux")]
    if checked.is_ok() && !pipe.is_some_and(|ino| owner_holds_pipe(owner.pid(), ino)) {
        checked = Err("the owner does not hold the pipe it reads".into());
    }
    if let Err(why) = checked {
        ends.log(&format!("refused to serve: {why}"));
        return 2;
    }
    guard(input, &mut ends, EMPTY_LOOKS);
    // The keepers' pipe closes with this process, so the keepers of groups that were not ended end too.
    0
}

#[cfg(test)]
mod tests;
