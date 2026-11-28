//! A unit of work's processes in one value (ADR 0119 §1, ER-51 "start, hold, watch, end"): a [`Scope`] prepares, starts
//! and runs, and when it ends (normally, by `?`, or by a panic that unwinds) it ends everything it started. The in-process
//! form of ADR 0112's job fixture, built on what this crate has:
//!
//! - **Prepare** ([`Scope::new`]): one folder of its own under a folder the caller names, an [`own::Unit`](crate::own::Unit)
//!   that records what it starts (and a marker variable of its own its children carry, [`MARKER_PREFIX`]), and the means that
//!   end everything later: on Unix the orphan guardian ([`crate::guardian`], ADR 0076), made to serve before the first child
//!   exists, and a process group per child registered with it, so an owner ended by SIGKILL or a crash still has its
//!   children's groups ended and the folder removed; on Windows one kill-on-close job ([`own::KillOnClose`](crate::own)) that
//!   every child joins first (its own job nests under it), whose handle closes with the owner. A child gets the folder as its
//!   working folder and `TMPDIR`/`TMP`/`TEMP` unless its command names its own.
//! - **Execute**: [`Scope::start`] gives a [`Held`] (the child by its handle and identity; [`Owned`]: wait with a bound,
//!   result, end), [`Scope::run`] runs to completion under [`Limits`] ([`crate::run_bounded`]). [`Scope::cancel`] (or a
//!   [`Cancel`] handed to another thread) ends every wait in the scope with [`Waited::Cancelled`] and the tree of a run in
//!   progress; nothing starts after it.
//! - **Clean-up** ([`Drop`], or [`Scope::finish`] for the [`Summary`]): asks each live child to stop, waits up to the
//!   scope's grace ([`GRACE`], measured), then ends what is left by identity, tree and group included, from one look at the
//!   process table for all children (ER-20); on Unix reads the table once more ([`own::members`](crate::own::members)) for
//!   members that left their group (found by the marker or the folder) and ends them; on Windows ends the job and waits for it
//!   to empty; removes the folder unless it was kept ([`Scope::keep_folder`]); logs one line per member: pid, start time,
//!   program (its file name, never its arguments), how it ended and whether it was guarded against a crash (ER-27).
//!
//! | How the scope ends | What ends its processes |
//! | --- | --- |
//! | normally, an error, `?` | [`Drop`] (or [`Scope::finish`]) |
//! | a panic that unwinds | [`Drop`] during unwinding |
//! | `panic = "abort"`, SIGKILL, a crash | Unix: the orphan guardian; Windows: the job handle closes |
//! | a cancel | the cancel ends the waits; the end ends the processes |
//!
//! Waits ([`Held::wait`], `Scope`'s [`Owned::wait`]) take the [`Owned`] trait's bound (ADR 0112): they end at the child's
//! exit, a cancel or the bound. Judging a start on progress is [`crate::StartWatch`]'s, on the child's identity
//! ([`Held::process`]); ADR 0119 §1 says so since the review of WP-1.
//!
//! Limits: on Unix a program that leaves its process group and drops the marker from its environment (`setsid` plus
//! `env -i`) is outside every net (Windows jobs have no such escape). The guardian is off for root (ADR 0076 amendment
//! 2026-11-09) and in a binary that does not call [`crate::guardian::run_if_requested`]; it says so once in the log, and each
//! [`Report`] says whether its member was guarded. On Unix a guarded child is started inside a group the guardian already
//! holds (keeper first): with `posix_spawn` it is in the group from its first instruction, and on the fork path it holds the
//! owner's close-on-exec registration pipe until its `exec`, so the guardian cannot see the owner gone before the child has
//! joined the group ([`crate::guardian`]). On Windows a child joins the scope's job only after
//! `CreateProcess` returned (no `CREATE_SUSPENDED` start yet): an owner killed in that moment, or a grandchild started in it,
//! is outside the job. A run ([`Scope::run`]) is ended by its bound, a cancel and the end, but it does not join the Windows
//! job yet (ADR 0119 WP-14 moves runs onto the scope's own start).

use crate::identity::{ProcessId, StartTime};
use crate::own::{Owned, Unit, Waited};
#[cfg(unix)]
use crate::own::{Why, members};
use crate::run::{Limits, Outcome, Output, RunError, program_name, run_bounded_noting};
use crate::supervised::{Registry, Supervised};
#[cfg(unix)]
use crate::tree::kill_tree_with;
use crate::tree::{self, OnceRead, kill_tree};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long the end of a scope waits for the children it asked to stop before it ends them (ADR 0119; a limit with its
/// measured cause, ER-12): [`crate::FINISH_GRACE`], the crate's one grace. Measured 2026-11-24 on the Mac mini at a 1-minute
/// load of 27 to 35 (docs/benchmarks.md, "2026-11-24, scope grace: from the stop request to the exit"): a Rust program, `sh`
/// and node with the default SIGTERM action exit within 45 ms at the worst (p99 at most 21 ms, 200 asks each); a shell script
/// that traps SIGTERM to clean up exits when its current foreground command ends, 1.02 s at the worst for one-second steps
/// (50 asks). The grace is twice that worst kind; a child that ignores the request costs one grace per scope end, not one per
/// child (all are asked at once).
pub const GRACE: Duration = crate::session::FINISH_GRACE;

/// The start of the variable every child of a scope carries, `PADDOCK_SCOPE_<owner pid>_<start µs>_<sequence>=1`: one
/// variable per scope, so a scope started inside another scope's child keeps the outer marker too. The end finds a member
/// that left its process group by it ([`Why::Marker`], Unix).
pub const MARKER_PREFIX: &str = "PADDOCK_SCOPE_";

/// The pace of the waits for a child's exit (a cancel wakes a wait at once).
const TICK: Duration = Duration::from_millis(5);

/// The longest part of a folder name taken from a scope's name.
const NAME_MAX: usize = 40;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// How one member of a scope ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HowEnded {
    /// It had ended by itself (with this status, when known) before anything ended it.
    Exited(Option<ExitStatus>),
    /// It was asked to stop and exited within the grace.
    Asked,
    /// It was ended, its tree and group with it (after the grace, at once by [`Held::end`], or by a run's bound or cancel).
    Ended,
    /// The scope did not start it, but it belonged to the scope (it carried the marker, or named the folder as an orphan)
    /// after it had left its group; it was ended by identity.
    Escaped,
    /// It was still there after the scope ended it (the OS refused).
    Remains,
    /// A run whose program could not be started.
    NotStarted,
}

impl std::fmt::Display for HowEnded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HowEnded::Exited(Some(s)) => write!(f, "had exited ({s})"),
            HowEnded::Exited(None) => write!(f, "had exited"),
            HowEnded::Asked => write!(f, "exited when asked"),
            HowEnded::Ended => write!(f, "was ended"),
            HowEnded::Escaped => write!(f, "had left its group and was ended"),
            HowEnded::Remains => write!(f, "could not be ended"),
            HowEnded::NotStarted => write!(f, "never started"),
        }
    }
}

/// One member of a scope in its [`Summary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Its identity; `None` when its start time could not be read (it was then ended through its handle and group only).
    pub id: Option<ProcessId>,
    /// Its pid (0 for a run that never started).
    pub pid: u32,
    /// The program's file name (never its arguments); empty for an escaped member.
    pub program: String,
    /// How it ended.
    pub how: HowEnded,
    /// Whether it would have been ended had this process died (registered with the orphan guardian on Unix, in a
    /// kill-on-close job on Windows). False for a run, an escaped member, and a child started while the guard was off.
    pub guarded: bool,
}

/// What the end of a scope did: one report per member, in the order they started, escaped members last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// The scope's name.
    pub scope: String,
    /// One per member.
    pub reports: Vec<Report>,
}

/// One member as the scope holds it.
struct Slot {
    pid: u32,
    id: Option<ProcessId>,
    program: String,
    /// A started child until it is ended; `None` for a run (the run holds its own child).
    child: Option<Supervised>,
    /// Set once it has ended.
    how: Option<HowEnded>,
    guarded: bool,
}

/// What a scope shares with its [`Held`] children and its [`Cancel`] handles.
struct Inner {
    name: String,
    cancelled: Mutex<bool>,
    changed: Condvar,
    slots: Mutex<Vec<Slot>>,
    unit: Mutex<Unit>,
    registry: Registry,
    #[cfg(windows)]
    job: Option<crate::own::KillOnClose>,
}

impl Inner {
    fn is_cancelled(&self) -> bool {
        *lock(&self.cancelled)
    }

    fn set_cancelled(&self) {
        *lock(&self.cancelled) = true;
        self.changed.notify_all();
    }

    fn cancel(&self) {
        self.set_cancelled();
        // A run in progress is a wait too: its tree ends, so the run returns (with how its program ended).
        let runs: Vec<ProcessId> = lock(&self.slots)
            .iter()
            .filter(|s| s.child.is_none() && s.how.is_none())
            .filter_map(|s| s.id)
            .collect();
        for id in runs {
            kill_tree(id);
        }
    }

    /// Sleeps up to `d`, or until a cancel.
    fn pause(&self, d: Duration) {
        let guard = lock(&self.cancelled);
        if !*guard {
            drop(self.changed.wait_timeout(guard, d));
        }
    }

    fn slot<T>(&self, index: usize, f: impl FnOnce(&mut Slot) -> T) -> Option<T> {
        lock(&self.slots).get_mut(index).map(f)
    }
}

/// Waits until `gone` holds, `bound` passes, or `inner` is cancelled; `gone` is asked first.
fn wait_on(inner: &Inner, bound: Duration, mut gone: impl FnMut() -> bool) -> Waited {
    let began = Instant::now();
    loop {
        if gone() {
            return Waited::Gone;
        }
        if inner.is_cancelled() {
            return Waited::Cancelled;
        }
        let left = bound.saturating_sub(began.elapsed());
        if left.is_zero() {
            return Waited::Bound;
        }
        inner.pause(left.min(TICK));
    }
}

/// Whether `cmd` sets `var` itself (Windows compares names without case, as its environment does).
fn sets(cmd: &Command, var: &str) -> bool {
    cmd.get_envs().any(|(k, _)| {
        if cfg!(windows) {
            k.to_string_lossy().eq_ignore_ascii_case(var)
        } else {
            k == var
        }
    })
}

/// Ends the waits of a scope from another thread (a tool's Ctrl+C handler): the same as [`Scope::cancel`]. Cheap to clone.
#[derive(Clone)]
pub struct Cancel(Arc<Inner>);

impl Cancel {
    /// See [`Scope::cancel`].
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

/// A unit of work's processes (see the module).
pub struct Scope {
    inner: Arc<Inner>,
    folder: PathBuf,
    keep: bool,
    grace: Duration,
    marker: String,
    summary: Option<Summary>,
    /// The end has run ([`Scope::finish`] takes the summary, so it is not the flag).
    ended: bool,
}

impl Scope {
    /// Prepares a scope named `name`: makes its folder inside `under` (made if missing; the scope's folder itself is new),
    /// its unit, and the means that end its children if this process dies (Unix: the orphan guardian, serving before this
    /// returns; Windows: the scope's job).
    ///
    /// # Errors
    ///
    /// When the folder cannot be made.
    pub fn new(name: &str, under: &Path) -> std::io::Result<Scope> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let me = std::process::id();
        let began = crate::own::now();
        let marker = format!(
            "{MARKER_PREFIX}{me}_{}_{}",
            began.as_micros(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .take(NAME_MAX)
            .collect();
        std::fs::create_dir_all(under)?;
        let folder = under.join(format!("scope-{safe}-{}", &marker[MARKER_PREFIX.len()..]));
        std::fs::create_dir(&folder)?;
        // Linux reads a start time from a boot time in whole seconds: one second of slack, as the Reaper takes.
        let since = StartTime::from_micros(began.as_micros().saturating_sub(1_000_000));
        let mut unit = Unit::new(since, me);
        unit.set_marker(&marker, "1");
        unit.add_folder(&folder.to_string_lossy());
        // The guardian serves before the first child exists (review P1-4): an owner killed from here on leaves no unguarded
        // child. Off is logged once by the guardian; each report says whether its member was guarded.
        #[cfg(unix)]
        crate::guardian::ensure();
        #[cfg(windows)]
        let job = match crate::own::KillOnClose::new() {
            Ok(j) => Some(j),
            Err(e) => {
                log::warn!(
                    "Scope {name}: its kill-on-close job could not be made ({e}); each child keeps a job of its own, which \
                     ends it when this process ends"
                );
                None
            }
        };
        log::debug!("Scope {name}: prepared in {}", folder.display());
        Ok(Scope {
            inner: Arc::new(Inner {
                name: name.to_owned(),
                cancelled: Mutex::new(false),
                changed: Condvar::new(),
                slots: Mutex::new(Vec::new()),
                unit: Mutex::new(unit),
                registry: Registry::new(),
                #[cfg(windows)]
                job,
            }),
            folder,
            keep: false,
            grace: GRACE,
            marker,
            summary: None,
            ended: false,
        })
    }

    /// Keeps the folder when the scope ends (it holds outputs the caller reads afterwards). Set before the first start: a
    /// kept folder is not handed to the orphan guardian, so it stays after a crash too.
    pub fn keep_folder(mut self) -> Scope {
        self.keep = true;
        self
    }

    /// Another grace than [`GRACE`] for the end of this scope.
    pub fn with_grace(mut self, grace: Duration) -> Scope {
        self.grace = grace;
        self
    }

    /// The scope's own folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The `(name, value)` every child carries in its environment.
    pub fn marker(&self) -> (&str, &str) {
        (&self.marker, "1")
    }

    /// A handle that cancels this scope from another thread.
    pub fn canceller(&self) -> Cancel {
        Cancel(self.inner.clone())
    }

    /// Ends every wait in the scope with [`Waited::Cancelled`] and the tree of a run in progress; nothing starts after it.
    /// The started children run on until the scope ends ([`Drop`] or [`Scope::finish`]).
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Whether the scope was cancelled (or has ended).
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// The members that have not ended yet, by identity (started children and runs in progress).
    pub fn running(&self) -> Vec<ProcessId> {
        lock(&self.inner.slots)
            .iter_mut()
            .filter_map(|s| {
                let live = s.how.is_none() && s.child.as_mut().is_none_or(Supervised::is_running);
                if live { s.id } else { None }
            })
            .collect()
    }

    /// Gives `cmd` the scope's folder and marker (see the module).
    fn prepare(&self, cmd: &mut Command) {
        if cmd.get_current_dir().is_none() {
            cmd.current_dir(&self.folder);
        }
        for var in ["TMPDIR", "TMP", "TEMP"] {
            if !sets(cmd, var) {
                cmd.env(var, &self.folder);
            }
        }
        cmd.env(&self.marker, "1");
    }

    fn refused(&self) -> std::io::Error {
        std::io::Error::other(format!(
            "scope {} was cancelled or has ended: nothing more starts in it",
            self.inner.name
        ))
    }

    /// Starts `cmd` as a child of the scope, held until it or the scope ends it (see the module).
    ///
    /// # Errors
    ///
    /// When the scope was cancelled or the program cannot be started.
    pub fn start(&self, mut cmd: Command) -> std::io::Result<Held> {
        if self.inner.is_cancelled() {
            return Err(self.refused());
        }
        self.prepare(&mut cmd);
        let program = program_name(&cmd);
        let folder = (!self.keep).then_some(self.folder.as_path());
        #[cfg(windows)]
        let child = Supervised::spawn_scoped(
            &self.inner.registry,
            cmd,
            folder,
            self.inner.job.as_ref().map(crate::own::KillOnClose::job),
        )?;
        #[cfg(not(windows))]
        let child = Supervised::spawn_scoped(&self.inner.registry, cmd, folder)?;
        let pid = child.id().unwrap_or_default();
        let id = child.process();
        let guarded = child.guarded();
        let leader = child.group_leader();
        if !guarded {
            log::warn!(
                "Scope {}: {program} (pid {pid}) is not guarded: if this process dies, it runs on",
                self.inner.name
            );
        }
        if let Some(id) = id {
            let mut unit = lock(&self.inner.unit);
            unit.add_root(id);
            // The child's process group (Unix: led by its guard's keeper, or by the child) is the unit's while its leader is the
            // process recorded.
            if let (true, Some(leader)) = (cfg!(unix), leader) {
                unit.add_group(leader);
            }
        }
        let mut slots = lock(&self.inner.slots);
        slots.push(Slot {
            pid,
            id,
            program,
            child: Some(child),
            how: None,
            guarded,
        });
        Ok(Held {
            inner: self.inner.clone(),
            index: slots.len() - 1,
            id,
            ended: None,
        })
    }

    /// Runs `cmd` to completion under `limits` as a member of the scope ([`crate::run_bounded`]: stdin closed, output
    /// captured, the tree ended at the bound). A cancel ends its tree; the output then shows how the program ended.
    ///
    /// # Errors
    ///
    /// As [`crate::run_bounded`], and [`RunError::Spawn`] when the scope was cancelled.
    pub fn run(&self, mut cmd: Command, limits: Limits) -> Result<Output, RunError> {
        let program = program_name(&cmd);
        if self.inner.is_cancelled() {
            return Err(RunError::Spawn(program, self.refused()));
        }
        self.prepare(&mut cmd);
        let index = {
            let mut slots = lock(&self.inner.slots);
            slots.push(Slot {
                pid: 0,
                id: None,
                program,
                child: None,
                how: None,
                guarded: false,
            });
            slots.len() - 1
        };
        let inner = &self.inner;
        let noted = |id: Option<ProcessId>| {
            inner.slot(index, |s| {
                s.id = id;
                s.pid = id.map_or(0, ProcessId::pid);
            });
            if let Some(id) = id {
                lock(&inner.unit).add_root(id);
                // A cancel that came between the start and this note found no identity to end: it is ended here.
                if inner.is_cancelled() {
                    kill_tree(id);
                }
            }
        };
        let result = run_bounded_noting(cmd, limits, &noted);
        let how = match &result {
            Ok(out) => match out.outcome {
                Outcome::Exited(_) if self.inner.is_cancelled() => HowEnded::Ended,
                Outcome::Exited(s) => HowEnded::Exited(Some(s)),
                Outcome::TimedOut | Outcome::OutputCap => HowEnded::Ended,
            },
            Err(RunError::Wait(..)) => HowEnded::Exited(None),
            Err(_) => HowEnded::NotStarted,
        };
        inner.slot(index, |s| s.how = Some(how));
        result
    }

    /// Ends the scope now and gives its [`Summary`] (what [`Drop`] does, with the summary kept).
    pub fn finish(mut self) -> Summary {
        self.end_all();
        self.summary.take().unwrap_or_default()
    }

    /// The clean-up (see the module); runs once.
    fn end_all(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        let began = Instant::now();
        // Waits in other threads end (a wait whose child is gone still says so first), and nothing starts any more.
        self.inner.set_cancelled();
        let reports = self.end_children(began);
        // A scope that started nothing has nothing that could have escaped: no table read.
        #[cfg(unix)]
        let reports = if reports.is_empty() {
            reports
        } else {
            self.end_escaped(reports)
        };
        // Windows: the job ends whatever joined it (helpers that left the tree included; a job has no escape, so no marker
        // look is needed there), and the folder goes only once the job is empty.
        #[cfg(windows)]
        self.end_job(began);
        self.inner.registry.end_all();
        if !self.keep {
            crate::cleanup::logged(
                "removing a scope's folder",
                std::fs::remove_dir_all(&self.folder),
            );
        }
        for r in &reports {
            let started = r.id.map_or_else(
                || "unknown".to_owned(),
                |id| id.started().as_micros().to_string(),
            );
            let program = if r.program.is_empty() {
                "?"
            } else {
                &r.program
            };
            log::info!(
                "Scope {}: {program} (pid {}, started {started} µs{}) {}",
                self.inner.name,
                r.pid,
                if r.guarded { "" } else { ", unguarded" },
                r.how
            );
        }
        log::info!(
            "Scope {}: ended with {} member(s) in {} ms{}",
            self.inner.name,
            reports.len(),
            began.elapsed().as_millis(),
            if self.keep {
                "; its folder was kept"
            } else {
                ""
            }
        );
        self.summary = Some(Summary {
            scope: self.inner.name.clone(),
            reports,
        });
    }

    /// Asks every live child to stop at once, waits up to the grace (the members lock is taken per look, never across the
    /// wait), then ends each one's tree and group from one look at the table (also of a child that had ended by itself:
    /// helpers it left behind). One report per member.
    fn end_children(&self, began: Instant) -> Vec<Report> {
        let mut asked = Vec::new();
        for (i, slot) in lock(&self.inner.slots).iter_mut().enumerate() {
            let (Some(child), None) = (slot.child.as_mut(), slot.how) else {
                continue;
            };
            if let Some(status) = child.exit_status() {
                slot.how = Some(HowEnded::Exited(Some(status)));
            } else if slot.id.is_some_and(tree::ask) {
                asked.push(i);
            }
        }
        let deadline = began.checked_add(self.grace);
        let waiting = || {
            lock(&self.inner.slots)
                .iter_mut()
                .enumerate()
                .any(|(i, s)| {
                    asked.contains(&i)
                        && s.child.as_mut().is_some_and(|c| c.exit_status().is_none())
                })
        };
        while !asked.is_empty() && waiting() && deadline.is_some_and(|d| Instant::now() < d) {
            // The end has set the cancel flag, so a cancel-woken pause would not wait: a plain tick, bounded by the grace.
            std::thread::sleep(TICK);
        }
        let mut slots = lock(&self.inner.slots);
        let any_child = slots.iter().any(|s| s.child.is_some());
        let os = any_child.then(OnceRead::now);
        for (i, slot) in slots.iter_mut().enumerate() {
            let (Some(mut child), Some(os)) = (slot.child.take(), os.as_ref()) else {
                continue;
            };
            if slot.how.is_none() {
                let status = child.exit_status();
                slot.how = Some(match status {
                    Some(_) if asked.contains(&i) => HowEnded::Asked,
                    Some(s) => HowEnded::Exited(Some(s)),
                    None => HowEnded::Ended,
                });
            }
            child.kill_on(os);
        }
        slots
            .iter()
            .map(|s| Report {
                id: s.id,
                pid: s.pid,
                program: s.program.clone(),
                how: s.how.unwrap_or(HowEnded::Exited(None)),
                guarded: s.guarded,
            })
            .collect()
    }

    /// One read of the process table: members that are still there after [`Scope::end_children`] (they left their group:
    /// found by the marker, or as an orphan naming the folder) are ended by identity and reported. A SIGKILL is not seen at
    /// once, so their end is waited for, all together, up to the grace before one is called [`HowEnded::Remains`].
    #[cfg(unix)]
    fn end_escaped(&self, mut reports: Vec<Report>) -> Vec<Report> {
        let unit = lock(&self.inner.unit).clone();
        let found = members(&unit.look(), &unit);
        if found.is_empty() {
            return reports;
        }
        let os = OnceRead::now();
        for m in &found {
            kill_tree_with(&os, m.id);
        }
        let deadline = Instant::now().checked_add(self.grace);
        while !found.iter().all(|m| crate::own::has_ended(m.id))
            && deadline.is_some_and(|d| Instant::now() < d)
        {
            // The end has set the cancel flag, so a cancel-woken pause would not wait: a plain tick, bounded by the grace.
            std::thread::sleep(TICK);
        }
        for m in found {
            let ended = crate::own::has_ended(m.id);
            match reports.iter_mut().find(|r| r.id == Some(m.id)) {
                // A child the end could not end through its handle.
                Some(r) if m.why == Why::Root => {
                    if !ended {
                        r.how = HowEnded::Remains;
                    }
                }
                _ => reports.push(Report {
                    id: Some(m.id),
                    pid: m.id.pid(),
                    program: String::new(),
                    how: if ended {
                        HowEnded::Escaped
                    } else {
                        HowEnded::Remains
                    },
                    guarded: false,
                }),
            }
        }
        reports
    }

    /// Windows: ends every process in the scope's job and waits, up to the grace, for the job to empty before the folder is
    /// removed (a process ended through the job lets go of its files only when it has exited).
    #[cfg(windows)]
    fn end_job(&self, began: Instant) {
        let Some(job) = self.inner.job.as_ref() else {
            return;
        };
        job.terminate();
        let deadline = Instant::now().checked_add(self.grace);
        while job.job().pids().is_some_and(|p| !p.is_empty())
            && deadline.is_some_and(|d| Instant::now() < d)
        {
            // The end has set the cancel flag, so a cancel-woken pause would not wait: a plain tick, bounded by the grace.
            std::thread::sleep(TICK);
        }
        if let Some(left) = job.job().pids().filter(|p| !p.is_empty()) {
            log::warn!(
                "Scope {}: {} process(es) still in its job {} ms after the end: {left:?}",
                self.inner.name,
                left.len(),
                began.elapsed().as_millis()
            );
        }
    }
}

impl Owned for Scope {
    type Ended = Summary;

    /// Waits until every member has ended, `bound` passes, or a cancel.
    fn wait(&mut self, bound: Duration) -> Waited {
        let inner = self.inner.clone();
        wait_on(&inner, bound, || self.running().is_empty())
    }

    fn result(&self) -> Option<&Summary> {
        self.summary.as_ref()
    }

    fn end(&mut self) -> &Summary {
        self.end_all();
        self.summary.get_or_insert_with(Summary::default)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.end_all();
    }
}

/// A child of a [`Scope`], held by its handle and identity until it or the scope ends it (see the module). Dropping it does
/// nothing: the scope holds the child.
pub struct Held {
    inner: Arc<Inner>,
    index: usize,
    id: Option<ProcessId>,
    ended: Option<HowEnded>,
}

impl Held {
    /// The child by identity; `None` when its start time could not be read.
    pub fn process(&self) -> Option<ProcessId> {
        self.id
    }

    fn child<T>(&self, f: impl FnOnce(&mut Supervised) -> Option<T>) -> Option<T> {
        self.inner
            .slot(self.index, |s| s.child.as_mut().and_then(f))
            .flatten()
    }

    /// Whether the child would be ended if this process died (see [`Report::guarded`]).
    pub fn guarded(&self) -> bool {
        self.inner.slot(self.index, |s| s.guarded).unwrap_or(false)
    }

    /// The exit status once the child has ended by itself; `None` while it runs or after it was ended.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.child(Supervised::exit_status)
    }

    /// The child's piped standard input, once.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child(Supervised::take_stdin)
    }

    /// The child's piped standard output, once.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child(Supervised::take_stdout)
    }

    /// The child's piped standard error, once.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child(Supervised::take_stderr)
    }

    fn gone(&self) -> bool {
        self.inner
            .slot(self.index, |s| {
                s.child.as_mut().is_none_or(|c| c.exit_status().is_some())
            })
            .unwrap_or(true)
    }
}

impl Owned for Held {
    type Ended = HowEnded;

    /// Waits until the child has ended, `bound` passes, or the scope is cancelled (or ends).
    fn wait(&mut self, bound: Duration) -> Waited {
        let inner = self.inner.clone();
        wait_on(&inner, bound, || self.gone())
    }

    fn result(&self) -> Option<&HowEnded> {
        self.ended.as_ref()
    }

    /// Ends the child now, its tree and group with it (no grace: the end of the scope asks first). Ending twice reports the
    /// first ending again.
    fn end(&mut self) -> &HowEnded {
        let how = match self.ended {
            Some(how) => how,
            None => self
                .inner
                .slot(self.index, |s| {
                    if let Some(how) = s.how {
                        return how;
                    }
                    let Some(mut child) = s.child.take() else {
                        return HowEnded::Exited(None);
                    };
                    let how = child
                        .exit_status()
                        .map_or(HowEnded::Ended, |st| HowEnded::Exited(Some(st)));
                    child.kill();
                    s.how = Some(how);
                    how
                })
                .unwrap_or(HowEnded::Exited(None)),
        };
        self.ended.insert(how)
    }
}

/// The owner tests run here, in the crate's own test binary, so the Linux CI containers (root) exercise the guardian through
/// the act-as-user path (review P2-5) instead of only its root refusal.
#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use crate::guardian::{ACT_AS_USER_ENV, LOG_ENV};
    use crate::testchild::{Mode, child};
    use paddock_testkit::wait::{HANG_GUARD, poll_until_within};
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    const ENTRY: &str = "guardian::unix::tests::guardian_entry";
    const OWNER_ENV: &str = "PADDOCK_SCOPE_TEST_OWNER_UNDER";
    const REFUSE_OWNER_ENV: &str = "PADDOCK_SCOPE_TEST_REFUSE_OWNER_UNDER";

    /// Not a test: the owner of a scope with one child that has a grandchild in its group; names both and the folder, then
    /// blocks until it is killed (or its standard input ends).
    #[test]
    fn scope_owner_process() {
        let Some(under) = std::env::var_os(OWNER_ENV).map(PathBuf::from) else {
            return;
        };
        crate::guardian::enable_in_test_binary(ENTRY);
        let scope = Scope::new("owner", &under).unwrap();
        let mut cmd = child(Mode::Tree);
        cmd.stdout(Stdio::piped());
        let mut held = scope.start(cmd).unwrap();
        // The grandchild writes into the child's output too, so the owner reads on until the grandchild's test harness has
        // named its test (libtest writes `test <name> ... ` without a newline, hence bytes, not lines). A grandchild that had
        // not written it yet when the owner was killed met a closed pipe, ended by itself with the write error, and was
        // SIGKILLed by the guardian while it wrote its coverage profile at its exit: a corrupt .profraw that failed the
        // crates job's coverage merge (CI run 9011).
        let mut out = held.take_stdout().unwrap();
        let mut text = String::new();
        let mut buf = [0u8; 4096];
        let grandchild = loop {
            let pid = text
                .find("CHILD:pid ")
                .and_then(|i| text[i + 10..].split_once('\n'))
                .map(|(pid, _)| pid.trim().to_owned());
            if let Some(pid) = pid
                && text.matches("testchild::helper_main").count() >= 2
            {
                break pid;
            }
            let n = out.read(&mut buf).unwrap();
            assert!(
                n > 0,
                "the child's output ended before its grandchild started: {text}"
            );
            text.push_str(&String::from_utf8_lossy(&buf[..n]));
        };
        println!("child {}", held.process().unwrap().pid());
        println!("grandchild {grandchild}");
        println!("guarded {}", held.guarded());
        println!("folder {}", scope.folder().display());
        println!("ready");
        let _ = std::io::stdin().read_line(&mut String::new());
        std::process::exit(0);
    }

    /// The owner is SIGKILLed through its handle, so none of its own clean-up runs: the guardian ends the child's group and
    /// removes the folder.
    #[test]
    fn an_owner_ended_by_sigkill_leaves_no_process_and_no_folder() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("guardian.log");
        let mut owner = crate::command(std::env::current_exe().unwrap())
            .args([
                "scope::unix_tests::scope_owner_process",
                "--exact",
                "--nocapture",
            ])
            .env(ACT_AS_USER_ENV, "1")
            .env(OWNER_ENV, dir.path())
            .env(LOG_ENV, &log)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // conformance: test-steps — every failure path below ends the owner (and what it started) by identity before it fails
            .spawn()
            .unwrap();
        let owner_id = ProcessId::of(owner.id()).unwrap();
        let mut out = BufReader::new(owner.stdout.take().unwrap());
        let (mut ids, mut folder, mut guarded) = (Vec::new(), None, None);
        let mut line = String::new();
        while line.trim() != "ready" {
            line.clear();
            if out.read_line(&mut line).unwrap() == 0 {
                kill_tree(owner_id);
                panic!("the owner ended before its children were ready");
            }
            match line.trim().split_once(' ') {
                Some(("child" | "grandchild", pid)) => {
                    ids.push(
                        crate::recorded::descendant_of(owner_id, pid.parse().unwrap())
                            .expect_running("a child of the owner"),
                    );
                }
                Some(("folder", f)) => folder = Some(PathBuf::from(f)),
                Some(("guarded", g)) => guarded = Some(g == "true"),
                _ => {}
            }
        }
        let folder = folder.expect("the owner named its folder");
        owner.kill().unwrap();
        owner.wait().unwrap();
        let gone = poll_until_within(HANG_GUARD, || {
            ids.iter().all(|id| crate::own::has_ended(*id)) && !folder.exists()
        });
        if !gone {
            let left: Vec<u32> = ids
                .iter()
                .filter(|id| !crate::own::has_ended(**id))
                .map(|id| id.pid())
                .collect();
            for id in &ids {
                kill_tree(*id);
            }
            panic!(
                "the killed owner's scope left processes {left:?} or its folder (there: {}); guardian log:\n{}",
                folder.exists(),
                std::fs::read_to_string(&log).unwrap_or_default()
            );
        }
        assert_eq!(guarded, Some(true), "the owner's child was guarded");
    }

    /// Not a test: an owner whose guardian refuses; starts two children and says how many guardians it started and whether
    /// each child was guarded.
    #[test]
    fn refused_owner_process() {
        let Some(under) = std::env::var_os(REFUSE_OWNER_ENV).map(PathBuf::from) else {
            return;
        };
        crate::guardian::enable_in_test_binary(ENTRY);
        let scope = Scope::new("refused", &under).unwrap();
        let a = scope.start(child(Mode::Sleep)).unwrap().guarded();
        let b = scope.start(child(Mode::Sleep)).unwrap().guarded();
        let summary = scope.finish();
        println!(
            "guardians {} guarded {a} {b} reports {}",
            crate::guardian::guardians_started(),
            summary.reports.iter().filter(|r| !r.guarded).count()
        );
    }

    /// A guardian that refused is not started again for every child (review P2-1), and each unguarded start says so.
    #[test]
    fn a_refused_guardian_is_not_retried_and_the_starts_are_marked_unguarded() {
        let dir = tempfile::tempdir().unwrap();
        let out = crate::run_bounded(
            {
                let mut c = crate::command(std::env::current_exe().unwrap());
                c.args([
                    "scope::unix_tests::refused_owner_process",
                    "--exact",
                    "--nocapture",
                ])
                .env(ACT_AS_USER_ENV, "1")
                .env(crate::guardian::REFUSE_ENV, "1")
                .env(REFUSE_OWNER_ENV, dir.path());
                c
            },
            crate::Limits::new(HANG_GUARD),
        )
        .unwrap();
        let text = out.stdout_text();
        assert!(
            text.contains("guardians 1 guarded false false reports 2"),
            "{text}"
        );
    }

    /// A guardian that ended without refusing (killed, crashed: any exit code but 2) is started again at the next start (review
    /// round 2, P2-A): the prepare and each of the two starts try once, three guardians in all.
    #[test]
    fn a_guardian_that_crashed_is_started_again_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let out = crate::run_bounded(
            {
                let mut c = crate::command(std::env::current_exe().unwrap());
                c.args([
                    "scope::unix_tests::refused_owner_process",
                    "--exact",
                    "--nocapture",
                ])
                .env(ACT_AS_USER_ENV, "1")
                .env(crate::guardian::CRASH_ENV, "1")
                .env(REFUSE_OWNER_ENV, dir.path());
                c
            },
            crate::Limits::new(HANG_GUARD),
        )
        .unwrap();
        let text = out.stdout_text();
        assert!(
            text.contains("guardians 3 guarded false false reports 2"),
            "{text}"
        );
    }

    /// The whole-table reads one scope end can make, by the path that makes them (`crate::table_reads` counts each). The end
    /// is a fixed set of looks, shared by all members (ER-20); which of the later ones run depends on the state the kill leaves
    /// (a member still listed when the marker look runs), so the budget is a bound over all paths, not one number.
    /// The look every end makes in `Scope::end_children`: `OnceRead::now` = one snapshot and one read of the names.
    const READS_SNAPSHOT_AND_NAMES: u64 = 2;
    /// The marker look of `Scope::end_escaped`: `own::look` = a snapshot and its own read of environments and command lines.
    const READS_LOOK: u64 = 2;
    /// The second `OnceRead::now` of `end_escaped`, only when that look still lists a member (a kill is not seen at once).
    const READS_AFTER_A_KILL: u64 = 2;
    /// `tree::end_unreaped_group`'s snapshot, only for a member with no readable start time, once for the whole end.
    const READS_UNREAPED_GROUP: u64 = 1;
    const READS_AT_LEAST: u64 = READS_SNAPSHOT_AND_NAMES + READS_LOOK;
    const READS_AT_MOST: u64 = READS_AT_LEAST + READS_AFTER_A_KILL + READS_UNREAPED_GROUP;

    /// The invariant (review P2-3, ER-20): the cost of an end is the same bounded set of looks whatever the number of members,
    /// so each member count stays inside `READS_AT_LEAST..=READS_AT_MOST`. A cost that grows by a read per member leaves
    /// the range as soon as there are more members than the range is wide.
    fn assert_fixed_table_reads(costs: &[(usize, u64)]) {
        for &(members, reads) in costs {
            assert!(
                (READS_AT_LEAST..=READS_AT_MOST).contains(&reads),
                "{members} member(s) cost {reads} table reads, outside the fixed {READS_AT_LEAST}..={READS_AT_MOST} (costs: {costs:?})"
            );
        }
    }

    /// Positive control (ER-33): the check fails on a cost that grows with the number of members (one snapshot and one read of
    /// the names per member, as the single ends cost), and on a cost below the looks every end makes.
    #[test]
    #[should_panic(expected = "outside the fixed")]
    fn the_table_read_check_fails_when_the_cost_grows_with_the_members() {
        let per_member = |n: u64| READS_AT_LEAST + READS_SNAPSHOT_AND_NAMES * n;
        assert_fixed_table_reads(&[(1, per_member(1)), (5, per_member(5))]);
    }

    #[test]
    #[should_panic(expected = "outside the fixed")]
    fn the_table_read_check_fails_when_the_end_reads_nothing() {
        assert_fixed_table_reads(&[(1, 0), (5, 0)]);
    }

    /// N members cost a bounded number of whole-table reads at the end, not N (review P2-3, ER-20). The counter is
    /// process-wide: this needs one process per test (nextest's model; plain `cargo test` threads would count each other's reads).
    #[test]
    fn the_end_of_a_scope_reads_the_table_a_fixed_number_of_times_whatever_its_size() {
        let reads = |n: usize| {
            let dir = tempfile::tempdir().unwrap();
            let scope = Scope::new("budget", dir.path()).unwrap();
            let held: Vec<ProcessId> = (0..n)
                .map(|_| scope.start(child(Mode::Sleep)).unwrap().process().unwrap())
                .collect();
            let before = crate::table_reads();
            let summary = scope.finish();
            let after = crate::table_reads();
            assert_eq!(summary.reports.len(), n);
            assert!(
                held.iter()
                    .all(|id| poll_until_within(HANG_GUARD, || crate::own::has_ended(*id)))
            );
            (n, after - before)
        };
        assert_fixed_table_reads(&[reads(1), reads(5)]);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::testchild::{Mode, child};

    /// Every child joins the scope's job, not only the first (review P1-3: a job a process joins must be empty or already in
    /// its chain of jobs, so each child joins the scope's job before its own). The children's own jobs are leaked (never
    /// closed, never terminated), so only closing the scope's job, as Windows does when the owner dies, can end them.
    #[test]
    fn every_windows_child_ends_when_the_scopes_job_closes() {
        let dir = tempfile::tempdir().unwrap();
        let mut scope = Scope::new("job", dir.path()).unwrap();
        let ids: Vec<ProcessId> = (0..2)
            .map(|_| scope.start(child(Mode::Sleep)).unwrap().process().unwrap())
            .collect();
        let in_job = scope
            .inner
            .job
            .as_ref()
            .unwrap()
            .job()
            .pids()
            .unwrap_or_default();
        for id in &ids {
            assert!(
                in_job.contains(&id.pid()),
                "child {} is not in the scope's job: {in_job:?}",
                id.pid()
            );
        }
        for slot in lock(&scope.inner.slots).iter_mut() {
            std::mem::forget(slot.child.take());
        }
        let job = Arc::get_mut(&mut scope.inner)
            .expect("no other handle")
            .job
            .take();
        drop(job);
        let gone =
            paddock_testkit::wait::poll_until_within(paddock_testkit::wait::HANG_GUARD, || {
                ids.iter().all(|id| crate::own::has_ended(*id))
            });
        assert!(gone, "a child outlived the scope's job");
    }
}
