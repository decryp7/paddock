//! A supervised child: its whole process tree is ended on drop, on [`Supervised::terminate`] and when it exceeds a
//! memory cap, so nothing leaks. A [`Registry`] knows every supervised tree, so [`end_all`] can end them at shutdown or
//! in a panic hook even when the owners never get to drop them.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::identity::ProcessId;
use crate::tree::{self, Terminated, kill_child_tree, kill_tree};
use crate::usage;
use crate::watch::{MemoryEvent, MemoryListener, MemoryWatch, Verdict};
use std::collections::BTreeSet;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The roots of the trees that are being supervised, by identity (a root that ended and was reaped has a pid the OS may
/// hand on; [`Registry::end_all`] leaves such a pid alone). Cheap to clone; clones share the list.
#[derive(Debug, Clone, Default)]
pub struct Registry(Arc<Mutex<BTreeSet<ProcessId>>>);

impl Registry {
    /// An empty registry.
    pub fn new() -> Registry {
        Registry::default()
    }

    fn add(&self, id: ProcessId) {
        lock(&self.0).insert(id);
    }

    fn remove(&self, id: ProcessId) {
        lock(&self.0).remove(&id);
    }

    /// The pids of the roots of the trees still supervised.
    pub fn pids(&self) -> Vec<u32> {
        lock(&self.0).iter().map(|id| id.pid()).collect()
    }

    /// Ends every registered tree now and forgets them; returns how many were registered. The owning [`Supervised`]
    /// values still reap their children when they are dropped or waited for.
    pub fn end_all(&self) -> usize {
        let roots = std::mem::take(&mut *lock(&self.0));
        for root in &roots {
            kill_tree(*root);
        }
        roots.len()
    }
}

/// The registry [`Supervised::spawn`] uses.
pub fn global_registry() -> &'static Registry {
    static GLOBAL: OnceLock<Registry> = OnceLock::new();
    GLOBAL.get_or_init(Registry::new)
}

/// Ends every tree supervised through [`Supervised::spawn`]: call it from the app's exit and panic paths.
pub fn end_all() -> usize {
    global_registry().end_all()
}

/// How memory is measured for a cap: bytes used by the tree rooted at the pid. Defaults to real memory
/// ([`usage::tree_memory`]); tests and hosts with a better figure pass their own.
pub type Sampler = Arc<dyn Fn(u32) -> u64 + Send + Sync>;

/// A memory cap for a supervised child: the whole tree's memory (real memory per OS, rule 26: PSS on Linux so shared pages count once, the physical footprint on macOS) may not
/// exceed `limit`; it is warned at 80% and ended over the cap (rule 26: a 17 GB idle browser).
#[derive(Clone)]
pub struct MemoryCap {
    /// Bytes.
    pub limit: u64,
    /// Names the child in events: "the browser", `plugin "x"`.
    pub label: String,
    /// Hears the warning, the stop and the peak at the end.
    pub listener: Option<MemoryListener>,
    /// How often the memory is looked at.
    pub every: Duration,
    /// Another way to measure; `None` uses [`usage::tree_memory`].
    pub sampler: Option<Sampler>,
    /// Windows: also give the child's job object a hard per-process commit limit of `limit` (`JOB_OBJECT_LIMIT_PROCESS_MEMORY`),
    /// so an allocation past it fails in the child at once and a polling watcher cannot be outrun. Ignored elsewhere.
    pub hard_commit: bool,
}

impl MemoryCap {
    /// A cap of `limit` bytes, looked at every 200 ms.
    pub fn new(limit: u64, label: impl Into<String>) -> MemoryCap {
        MemoryCap {
            limit,
            label: label.into(),
            listener: None,
            every: Duration::from_millis(200),
            sampler: None,
            hard_commit: false,
        }
    }

    /// Also enforce `limit` as a hard commit limit of the job object (Windows; see [`MemoryCap::hard_commit`]).
    #[must_use]
    pub fn hard_commit(mut self) -> MemoryCap {
        self.hard_commit = true;
        self
    }

    /// Tells `listener` what the watcher sees.
    pub fn listener(mut self, listener: MemoryListener) -> MemoryCap {
        self.listener = Some(listener);
        self
    }
}

impl std::fmt::Debug for MemoryCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryCap")
            .field("limit", &self.limit)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// What the memory watcher shares with its [`Supervised`].
#[derive(Default)]
struct Shared {
    /// The cap the tree was ended over, once it was (0 = not).
    over: AtomicU64,
    /// The most memory the watcher saw.
    peak: AtomicU64,
}

/// The sampler of a cap without one of its own: the tree's memory, walked by identity from the root (ADR 0033). On Windows a
/// child in a job is measured through the job's member list, which needs no scan of the machine and follows no parent pid.
#[cfg(windows)]
fn default_sampler(root: ProcessId, job: Option<Arc<crate::job::Job>>) -> Sampler {
    if let Some(job) = job {
        return Arc::new(move |_| match job.pids() {
            Some(pids) => usage::tree_memory_of(&usage::OsSource::default(), &pids),
            None => usage::tree_memory_from(&usage::OsSource::default(), root),
        });
    }
    Arc::new(move |_| usage::tree_memory_from(&usage::OsSource::default(), root))
}

#[cfg(not(windows))]
fn default_sampler(root: ProcessId) -> Sampler {
    Arc::new(move |_| usage::tree_memory_from(&usage::OsSource::default(), root))
}

struct Watcher {
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    fn start(
        id: ProcessId,
        cap: MemoryCap,
        shared: Arc<Shared>,
        #[cfg(windows)] job: Option<Arc<crate::job::Job>>,
    ) -> Watcher {
        let pid = id.pid();
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("paddock-proc-memory".into())
            .spawn(move || {
                #[cfg(windows)]
                let fallback = default_sampler(id, job.clone());
                #[cfg(not(windows))]
                let fallback = default_sampler(id);
                let sample = cap.sampler.clone().unwrap_or(fallback);
                let tell = |e: MemoryEvent| {
                    if let Some(l) = &cap.listener {
                        l.tell(&cap.label, e);
                    }
                };
                let mut watch = MemoryWatch::new(cap.limit);
                let mut last = 0;
                loop {
                    // pace: how often the memory is looked at; it bounds nothing
                    match stopped.recv_timeout(cap.every) {
                        Err(RecvTimeoutError::Timeout) => {}
                        // Told to stop, or the Supervised is gone.
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                            return tell(MemoryEvent::Ended(watch.ended(last)));
                        }
                    }
                    last = sample(pid);
                    let verdict = watch.observe(last);
                    shared.peak.store(watch.peak(), Ordering::Relaxed);
                    match verdict {
                        Verdict::Fine => {}
                        Verdict::Warn(u) => tell(MemoryEvent::Warning(u)),
                        Verdict::Stop(u) => {
                            shared.over.store(cap.limit.max(1), Ordering::Relaxed);
                            // Windows: through the job first (by handle); then by identity, since the owner may have
                            // reaped the child meanwhile.
                            #[cfg(windows)]
                            if let Some(j) = &job {
                                j.terminate();
                            }
                            kill_tree(id);
                            return tell(MemoryEvent::Stopped(u));
                        }
                    }
                }
            })
            .map_err(|e| log::error!("the memory watcher thread could not be started, so the cap is not enforced: {e}"))
            .ok();
        Watcher { stop, thread }
    }

    fn finish(mut self) {
        // silent: the watcher thread has already ended when its receiver is gone (it stopped the child itself)
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            crate::cleanup::join("the memory watcher thread", t);
        }
    }
}

/// A child that is killed with its whole tree when dropped.
pub struct Supervised {
    child: Option<Child>,
    /// Windows: the kill-on-close job the child runs in (`None` when Windows refused one; the tree kill still applies).
    #[cfg(windows)]
    job: Option<Arc<crate::job::Job>>,
    /// The child's pid, kept after it was reaped for the registry and the log.
    pid: u32,
    /// The root by identity; `None` only when the OS would not say when the child started.
    process: Option<ProcessId>,
    registry: Registry,
    watcher: Option<Watcher>,
    shared: Arc<Shared>,
}

impl Supervised {
    /// Starts `cmd` in its own process group (Unix), registered in the [`global_registry`].
    pub fn spawn(cmd: Command) -> std::io::Result<Supervised> {
        Supervised::spawn_in(global_registry(), cmd, None)
    }

    /// Like [`Supervised::spawn`] with a memory cap.
    pub fn spawn_capped(cmd: Command, cap: MemoryCap) -> std::io::Result<Supervised> {
        Supervised::spawn_in(global_registry(), cmd, Some(cap))
    }

    /// Starts `cmd` registered in `registry`, optionally under a memory cap.
    pub fn spawn_in(
        registry: &Registry,
        cmd: Command,
        cap: Option<MemoryCap>,
    ) -> std::io::Result<Supervised> {
        Supervised::spawn_identified(registry, cmd, cap, ProcessId::of)
    }

    /// [`Supervised::spawn_in`] with the way the child's identity is read (tests: a child whose start time is unknown).
    fn spawn_identified(
        registry: &Registry,
        mut cmd: Command,
        cap: Option<MemoryCap>,
        identify: fn(u32) -> Option<ProcessId>,
    ) -> std::io::Result<Supervised> {
        crate::sys::own_group(&mut cmd);
        let child = cmd.spawn()?;
        let pid = child.id();
        // Not reaped yet, so the pid is still this child's.
        let process = identify(pid);
        #[cfg(windows)]
        let job = {
            let hard = cap.as_ref().filter(|c| c.hard_commit).map(|c| c.limit);
            match crate::job::Job::assign(&child, hard) {
                Ok(job) => Some(Arc::new(job)),
                // A hard limit that cannot be set must not become a child without one: the launch fails.
                Err(e) if hard.is_some() => {
                    let mut child = child;
                    crate::cleanup::logged("killing the child whose hard limit failed", child.kill());
                    crate::cleanup::logged("reaping the child whose hard limit failed", child.wait());
                    log::error!(
                        "The hard memory limit of child {pid} could not be set in its job object, so it was not started: {e}"
                    );
                    return Err(std::io::Error::new(
                        e.kind(),
                        format!("cannot set the hard memory limit in the child's job object: {e}"),
                    ));
                }
                Err(_) => None,
            }
        };
        let shared = Arc::new(Shared::default());
        if process.is_none() {
            log::warn!(
                "The start time of child {pid} could not be read: it is ended through its handle and its process group only, \
                 without the registry, the tree walk or a memory cap"
            );
        }
        let watcher = process.and_then(|id| {
            registry.add(id);
            #[cfg(windows)]
            let w = cap.map(|c| Watcher::start(id, c, shared.clone(), job.clone()));
            #[cfg(not(windows))]
            let w = cap.map(|c| Watcher::start(id, c, shared.clone()));
            w
        });
        Ok(Supervised {
            child: Some(child),
            #[cfg(windows)]
            job,
            pid,
            process,
            registry: registry.clone(),
            watcher,
            shared,
        })
    }

    /// Lowers the child's priority (Unix; Windows lowered it at the start, see [`crate::lower_priority`]). Done through the
    /// held child right after the spawn, so no fork window is needed. Only a child that has not ended is touched (an ended one
    /// may be reaped and its pid reused, rule 53). False when it is gone or the OS refused.
    pub fn lower_priority(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)) && crate::sys::lower_child(c))
    }

    /// Whether the child runs in a kill-on-close job object (Windows; always false elsewhere, where the process group
    /// plays that part).
    pub fn in_job(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// The process id while the child has not been killed or waited for.
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// The child by identity while it has not been killed or waited for.
    pub fn process(&self) -> Option<ProcessId> {
        self.child.as_ref().and(self.process)
    }

    /// The child's piped standard input, once.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// The child's piped standard output, once.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }

    /// The child's piped standard error, once.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }

    /// Whether the process still runs.
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// The most memory the watcher has seen (0 without a cap).
    pub fn memory_peak(&self) -> u64 {
        self.shared.peak.load(Ordering::Relaxed)
    }

    /// The cap the tree was ended over, if it was.
    pub fn stopped_over_memory(&self) -> Option<u64> {
        match self.shared.over.load(Ordering::Relaxed) {
            0 => None,
            cap => Some(cap),
        }
    }

    /// Waits for the child to end (a memory stop or [`Supervised::kill`] from elsewhere ends it too), reaps it and
    /// forgets the tree. `None` if it was already reaped.
    pub fn wait(&mut self) -> Option<ExitStatus> {
        let mut c = self.child.take()?;
        let status = c.wait().ok();
        self.release();
        status
    }

    /// Asks the process to quit (SIGTERM; Windows has no polite equivalent without a window), waits up to `grace` for it,
    /// then ends the tree. For browsers, where an abrupt kill during start-up can leave a copy of the app behind on
    /// macOS. Idempotent.
    pub fn terminate(&mut self, grace: Duration) -> Terminated {
        if !self.is_running() {
            self.kill();
            return Terminated::Gone;
        }
        if self.process.is_some_and(tree::ask) {
            let end = Instant::now().checked_add(grace);
            while self.is_running() && end.is_some_and(|e| Instant::now() < e) {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        let how = if self.is_running() {
            Terminated::Killed
        } else {
            Terminated::Gracefully
        };
        self.kill();
        how
    }

    /// Ends the whole process tree now and reaps it. Idempotent.
    pub fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            // Windows: the job's members are the tree (each identified through a handle that checks it is in the job), and
            // the job ends them all at once, by handle, with no pid to go stale.
            #[cfg(windows)]
            let members = self.job.as_ref().and_then(|job| {
                let members = job.members();
                job.terminate();
                members
            });
            #[cfg(not(windows))]
            let members = None;
            // Not reaped yet (`try_wait` reaps one that has ended): its pid and group are still its own. On Windows the
            // `Child`'s handle pins the pid until it is dropped, reaped or not.
            let unreaped = matches!(c.try_wait(), Ok(None)) || cfg!(windows);
            match self.process {
                Some(id) if unreaped => {
                    kill_child_tree(id, members);
                }
                Some(id) => {
                    kill_tree(id);
                }
                None if unreaped => {
                    // No start time, but an unreaped child is ours by definition (ADR 0033 decision 4): its group too.
                    log::warn!(
                        "Ending child {} without its identity: through its handle and its process group only",
                        self.pid
                    );
                    #[cfg(unix)]
                    crate::sys::signal_group(self.pid, crate::sys::SIGKILL);
                }
                None => {}
            }
            crate::cleanup::logged("killing the child", c.kill());
            crate::cleanup::logged("reaping the child", c.wait());
        }
        self.release();
    }

    fn release(&mut self) {
        // Windows: closing the job ends whatever of the tree is left (helpers that left it included).
        #[cfg(windows)]
        drop(self.job.take());
        if let Some(id) = self.process {
            self.registry.remove(id);
        }
        if let Some(w) = self.watcher.take() {
            w.finish();
        }
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testchild::{Mode, child, lines};
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;
    use std::sync::Mutex;

    fn piped(mode: Mode) -> Command {
        let mut c = child(mode);
        c.stdout(Stdio::piped());
        c
    }

    fn pid_line(r: &mut impl BufRead) -> u32 {
        let mut l = String::new();
        loop {
            l.clear();
            assert!(
                r.read_line(&mut l).unwrap() > 0,
                "the child ended before telling its grandchild"
            );
            if let Some(p) = lines(&l).first().and_then(|p| p.strip_prefix("pid ")) {
                return p.trim().parse().unwrap();
            }
        }
    }

    #[test]
    fn a_windows_child_runs_in_a_kill_on_close_job() {
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::Sleep), None).unwrap();
        assert_eq!(s.in_job(), cfg!(windows));
        s.kill();
        assert!(!s.in_job(), "the job closed with the tree");
    }

    /// The child starts a helper and exits: the helper is an orphan no tree walk finds, but the job still holds it.
    #[cfg(windows)]
    #[test]
    fn closing_the_job_ends_an_orphaned_helper() {
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::Leave), None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let grand = pid_line(&mut r);
        assert!(s.wait().is_some());
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(
            crate::testchild::is_gone(grand),
            "the orphaned helper leaked"
        );
    }

    #[test]
    fn drop_ends_the_child_and_its_grandchild() {
        let reg = Registry::new();
        let mut s = Supervised::spawn_in(&reg, piped(Mode::Tree), None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let grand = pid_line(&mut r);
        let pid = s.id().unwrap();
        assert!(s.is_running() && crate::is_alive(grand));
        assert_eq!(reg.pids(), vec![pid]);
        drop(s);
        // The pipe is shared with the grandchild: EOF means every holder is dead.
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(crate::testchild::is_gone(pid), "the child leaked");
        assert!(crate::testchild::is_gone(grand), "the grandchild leaked");
        assert!(reg.pids().is_empty(), "a dropped child leaves the registry");
    }

    /// macOS P1 (ADR 0033): a setuid child (`/usr/bin/top`, owned by root) runs under another effective user, which
    /// `proc_pidinfo` refuses; its identity comes from the sysctl, so it is registered, capped and ended with its group.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_setuid_child_has_an_identity_and_is_supervised() {
        let mut cmd = crate::command("/usr/bin/top");
        cmd.args(["-l", "0", "-s", "60"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let reg = Registry::new();
        let mut s = Supervised::spawn_in(&reg, cmd, Some(MemoryCap::new(u64::MAX, "top"))).unwrap();
        let id = s.process().expect("a setuid child has an identity");
        assert_eq!(reg.pids(), vec![id.pid()]);
        assert!(s.watcher.is_some(), "the memory cap watches it");
        s.kill();
        assert!(
            crate::testchild::is_gone(id.pid()),
            "the setuid child was ended"
        );
        assert!(reg.pids().is_empty());
    }

    /// The child exits and is reaped (`is_running` reaps), leaving a helper in its process group: `kill` then finds the root
    /// gone with no new holder of its pid, so the group is still the root's and is signalled.
    #[cfg(unix)]
    #[test]
    fn a_reaped_childs_helper_is_ended_through_its_group() {
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::Leave), None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let grand = pid_line(&mut r);
        // conformance: hang-guard — the child exits at once; the bound only keeps a broken run from hanging
        let end = Instant::now() + Duration::from_secs(60);
        while s.is_running() {
            assert!(Instant::now() < end, "the child did not exit");
            std::thread::yield_now();
        }
        assert!(crate::is_alive(grand), "the helper outlived the child");
        s.kill();
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(
            crate::testchild::is_gone(grand),
            "the helper in the group was ended"
        );
    }

    /// A child whose start time could not be read is still ours while unreaped: it is ended with its process group, and is
    /// in no registry and under no cap.
    #[cfg(unix)]
    #[test]
    fn a_child_without_an_identity_is_ended_through_its_group_and_its_handle() {
        let reg = Registry::new();
        let cap = MemoryCap::new(u64::MAX, "x");
        let mut s =
            Supervised::spawn_identified(&reg, piped(Mode::Tree), Some(cap), |_| None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let grand = pid_line(&mut r);
        assert!(s.process().is_none() && reg.pids().is_empty() && s.watcher.is_none());
        s.kill();
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(
            crate::testchild::is_gone(grand),
            "the grandchild in its group was ended"
        );
        // Reaped and without an identity: nothing more is signalled.
        let mut gone =
            Supervised::spawn_identified(&reg, child(Mode::Exit3), None, |_| None).unwrap();
        // conformance: hang-guard — the child exits at once; the bound only keeps a broken run from hanging
        let end = Instant::now() + Duration::from_secs(60);
        while gone.is_running() {
            assert!(Instant::now() < end, "the child did not exit");
            std::thread::yield_now();
        }
        gone.kill();
        assert_eq!(gone.wait(), None);
    }

    #[test]
    fn end_all_ends_every_registered_tree() {
        let reg = Registry::new();
        let mut a = Supervised::spawn_in(&reg, piped(Mode::Tree), None).unwrap();
        let mut b = Supervised::spawn_in(&reg, piped(Mode::Sleep), None).unwrap();
        let mut r = BufReader::new(a.take_stdout().unwrap());
        let grand = pid_line(&mut r);
        assert_eq!(reg.pids().len(), 2);
        assert_eq!(reg.end_all(), 2);
        assert!(reg.pids().is_empty());
        assert!(a.wait().is_some_and(|s| !s.success()));
        assert!(b.wait().is_some_and(|s| !s.success()));
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        assert!(crate::testchild::is_gone(grand));
        assert_eq!(reg.end_all(), 0);
    }

    #[test]
    fn the_global_registry_is_used_by_spawn() {
        let mut s = Supervised::spawn(child(Mode::Sleep)).unwrap();
        let pid = s.id().unwrap();
        assert!(global_registry().pids().contains(&pid));
        s.kill();
        assert!(!global_registry().pids().contains(&pid));
    }

    #[test]
    fn kill_and_wait_are_idempotent() {
        let mut s = Supervised::spawn_in(&Registry::new(), child(Mode::Sleep), None).unwrap();
        assert!(s.is_running());
        s.kill();
        s.kill();
        assert!(!s.is_running());
        assert_eq!(s.id(), None);
        assert_eq!(s.wait(), None);
        assert!(s.take_stdout().is_none() && s.take_stdin().is_none() && s.take_stderr().is_none());
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_registered_child() {
        let reg = Registry::new();
        assert!(
            Supervised::spawn_in(
                &reg,
                crate::command("/nonexistent/paddock-no-such-program"),
                None
            )
            .is_err()
        );
        assert!(reg.pids().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn terminate_ends_a_polite_process_without_waiting_for_the_grace_period() {
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::Polite), None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let mut l = String::new();
        while lines(&l) != ["ready"] {
            l.clear();
            assert!(r.read_line(&mut l).unwrap() > 0);
        }
        // It stops by itself on the polite request: not killed after the grace period.
        assert_eq!(
            s.terminate(Duration::from_secs(100)),
            Terminated::Gracefully
        );
        assert!(!s.is_running());
        assert_eq!(s.terminate(Duration::from_secs(1)), Terminated::Gone);
    }

    #[cfg(unix)]
    #[test]
    fn terminate_kills_a_process_that_ignores_sigterm_after_the_grace_period() {
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::IgnoreTerm), None).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let mut l = String::new();
        while lines(&l) != ["ready"] {
            l.clear();
            assert!(r.read_line(&mut l).unwrap() > 0);
        }
        assert_eq!(s.terminate(Duration::ZERO), Terminated::Killed);
        assert!(!s.is_running());
    }

    fn capped(
        limit: u64,
        sampler: impl Fn(u32) -> u64 + Send + Sync + 'static,
    ) -> (MemoryCap, Arc<Mutex<Vec<MemoryEvent>>>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        let mut cap =
            MemoryCap::new(limit, "the engine").listener(MemoryListener::new(move |label, e| {
                assert_eq!(label, "the engine");
                h.lock().unwrap().push(e);
            }));
        cap.every = Duration::from_millis(1);
        cap.sampler = Some(Arc::new(sampler));
        (cap, heard)
    }

    #[test]
    fn a_child_over_its_memory_cap_is_ended_with_its_tree() {
        let (cap, heard) = capped(1000, |_| 5000);
        let mut s = Supervised::spawn_in(&Registry::new(), piped(Mode::Tree), Some(cap)).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        // The watcher may end the tree before the child tells its grandchild: EOF then is the signal.
        let mut rest = Vec::new();
        r.read_to_end(&mut rest).unwrap();
        let status = s.wait().expect("reaped");
        assert!(!status.success());
        assert_eq!(s.stopped_over_memory(), Some(1000));
        assert_eq!(s.memory_peak(), 5000);
        let events = heard.lock().unwrap().clone();
        assert!(
            matches!(events.last(), Some(MemoryEvent::Stopped(u)) if u.used == 5000 && u.limit == 1000),
            "{events:?}"
        );
        if let Some(g) = lines(&String::from_utf8_lossy(&rest))
            .iter()
            .find_map(|l| l.strip_prefix("pid "))
        {
            assert!(
                crate::testchild::is_gone(g.parse().unwrap()),
                "the grandchild survived the memory stop"
            );
        }
    }

    #[test]
    fn a_child_under_its_cap_is_warned_once_and_the_peak_is_reported_at_the_end() {
        // 850 of 1000: a warning; the sampler then stays there, so nothing more is said until the end.
        let samples = Arc::new(AtomicU64::new(0));
        let n = samples.clone();
        let (cap, heard) = capped(1000, move |_| {
            n.fetch_add(1, Ordering::SeqCst);
            850
        });
        let mut s = Supervised::spawn_in(&Registry::new(), child(Mode::Sleep), Some(cap)).unwrap();
        // Block until the watcher has sampled three times (it runs every millisecond), without sleeping.
        while samples.load(Ordering::SeqCst) < 3 {
            std::thread::yield_now();
        }
        s.kill();
        let events = heard.lock().unwrap().clone();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, MemoryEvent::Warning(_)))
                .count(),
            1,
            "{events:?}"
        );
        assert!(
            matches!(events.last(), Some(MemoryEvent::Ended(u)) if u.peak == 850),
            "{events:?}"
        );
        assert_eq!(s.stopped_over_memory(), None);
        assert!(s.memory_peak() >= 850);
    }

    #[test]
    fn the_real_sampler_reads_the_tree_memory() {
        let mut cap = MemoryCap::new(u64::MAX, "x");
        cap.every = Duration::from_millis(1);
        let s = Supervised::spawn_in(&Registry::new(), child(Mode::Sleep), Some(cap)).unwrap();
        // Bounded: a sampler that never reports fails the test instead of hanging it.
        let deadline = Instant::now() + Duration::from_secs(60);
        while s.memory_peak() == 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            s.memory_peak() > 0,
            "the real sampler never reported the tree's memory"
        );
        drop(s);
    }

    /// Windows: the job lists the child itself (the default sampler reads the job, not the machine).
    #[cfg(windows)]
    #[test]
    fn the_job_lists_the_child() {
        let s = Supervised::spawn_in(&Registry::new(), child(Mode::Sleep), None).unwrap();
        let pids = s
            .job
            .as_ref()
            .expect("a job")
            .pids()
            .expect("the job can be queried");
        assert!(pids.contains(&s.id().unwrap()), "{pids:?}");
    }

    #[test]
    fn a_capped_child_spawns_in_the_global_registry_and_the_cap_prints_without_its_hooks() {
        let cap = MemoryCap::new(u64::MAX, "the helper");
        let text = format!("{cap:?}");
        assert!(
            text.contains("MemoryCap") && text.contains("the helper"),
            "{text}"
        );
        let mut s = Supervised::spawn_capped(child(Mode::Sleep), cap).unwrap();
        let pid = s.id().unwrap();
        assert!(global_registry().pids().contains(&pid));
        assert_eq!(s.stopped_over_memory(), None);
        s.kill();
        assert!(!global_registry().pids().contains(&pid));
    }

    /// Regression (macOS): a child whose freed pages stay resident is not over a cap that lies between its footprint and its
    /// resident size.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_whose_freed_pages_stay_resident_is_not_stopped_under_a_cap_between_footprint_and_resident_size()
     {
        use std::sync::atomic::AtomicBool;
        let cap_bytes = (crate::testchild::FREED_BYTES / 2) as u64;
        // The child really holds the memory while it touches it: the watcher looks only once it said `ready`.
        let ready = Arc::new(AtomicBool::new(false));
        let looks = Arc::new(AtomicU64::new(0));
        let (r2, l2) = (ready.clone(), looks.clone());
        let (cap, heard) = capped(cap_bytes, move |pid| {
            if !r2.load(Ordering::SeqCst) {
                return 0;
            }
            l2.fetch_add(1, Ordering::SeqCst);
            usage::tree_memory(pid)
        });
        let mut cmd = piped(Mode::Freed);
        cmd.stdin(Stdio::null());
        let mut s = Supervised::spawn_in(&Registry::new(), cmd, Some(cap)).unwrap();
        let mut r = BufReader::new(s.take_stdout().unwrap());
        let mut l = String::new();
        loop {
            l.clear();
            assert!(
                r.read_line(&mut l).unwrap() > 0,
                "the child ended before it was ready"
            );
            if lines(&l).first().is_some_and(|p| *p == "ready") {
                break;
            }
        }
        let rss = usage::snapshot().get(&s.id().unwrap()).map_or(0, |e| e.rss);
        assert!(
            rss > cap_bytes,
            "precondition: the resident size {rss} is over the cap {cap_bytes}"
        );
        ready.store(true, Ordering::SeqCst);
        // Several looks after `ready`, counted instead of waited for.
        while looks.load(Ordering::SeqCst) < 5 && s.stopped_over_memory().is_none() {
            std::thread::yield_now();
        }
        assert_eq!(s.stopped_over_memory(), None, "{:?}", heard.lock().unwrap());
        assert!(s.is_running());
        assert!(s.memory_peak() < cap_bytes, "peak {}", s.memory_peak());
    }

    /// The child is lowered from the parent after the spawn (rule 52: bulk work lowered), read back from the OS.
    #[cfg(unix)]
    #[test]
    fn lower_priority_lowers_the_started_child_and_not_a_gone_one() {
        let mut s = Supervised::spawn(child(Mode::Sleep)).unwrap();
        let pid = s.id().unwrap();
        // SAFETY: getpriority takes two integers; the child is unreaped, so the pid is its own.
        let nice = |pid: u32| unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
        let own = crate::sys::own_niceness();
        assert_eq!(nice(pid), own, "starts at the parent's priority");
        assert!(s.lower_priority());
        // A parent already below the target (a niced test run) keeps its level.
        assert_eq!(nice(pid), own.max(crate::sys::LOWERED_NICE));
        s.kill();
        assert!(!s.lower_priority(), "nothing left to lower");
    }

    /// A child that ended is not touched: its pid may be reused once it is reaped (rule 53).
    #[cfg(unix)]
    #[test]
    fn an_ended_child_is_not_lowered() {
        let mut s = Supervised::spawn(child(Mode::Exit3)).unwrap();
        while s.is_running() {
            std::thread::yield_now();
        }
        assert!(!s.lower_priority());
    }

    /// A child already below the target keeps its level and counts as lowered; one above is lowered to it.
    #[cfg(unix)]
    #[test]
    fn a_child_is_lowered_to_the_level_and_never_raised() {
        // SAFETY: getpriority/setpriority take plain integers; the pid is a child we hold unreaped.
        let nice = |pid: u32| unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
        let mut s = Supervised::spawn(child(Mode::Sleep)).unwrap();
        let pid = s.id().unwrap();
        // The child starts at our level; make it lower than the target first. Under a niced runner (plock's `nice -n 19`) we
        // are already lower than 15 and an unprivileged process cannot raise a child's priority, so start from our own level
        // when that is lower.
        let level = nice(0).max(15);
        unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, level) };
        assert!(s.lower_priority(), "already below the target");
        assert_eq!(nice(pid), level, "never raised");
        s.kill();
    }
}

#[cfg(test)]
mod hard_commit_tests {
    use super::*;

    #[test]
    fn a_cap_is_soft_until_asked_to_be_a_hard_commit_limit() {
        assert!(!MemoryCap::new(1 << 20, "x").hard_commit);
        assert!(MemoryCap::new(1 << 20, "x").hard_commit().hard_commit);
    }
}
