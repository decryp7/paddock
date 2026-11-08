//! The crash model of service repair (ADR 0056 section 7): the real [`Manager`] over a simulated OS, explored **to a fixed
//! point** and not to a depth.
//!
//! A state is `(simulated OS, the record, the app's own settings)`. From every reachable state the explorer takes every
//! event of the alphabet (entry points, OS and user changes, faults of single OS calls) and, for an entry point, every
//! interruption of it: a crash before or after each OS effect, a cancel, a record that stops being writable. A crash is an
//! edge like any other, so a crash during recovery is a crash from the state a crash left. The search ends when no new
//! state appears, so it covers every depth.
//!
//! The key of a state abstracts what cannot change a decision: `attempts` saturate at the cap, `resume_pending` is a flag,
//! the clock is gone (the OS keeps "seconds until the job starts", bucketed), versions are four names. The run is
//! breadth-first and ordered, so it is deterministic (there is no randomness and so no seed to print); a violation prints
//! the shortest path, which `MODEL_REPLAY` replays with the OS calls of every step.
//!
//! Violations are either **bugs the model found** (listed in [`KNOWN_FAILING`] with where they point; the test fails when
//! one is not found any more, so a fix removes its line) or new ones (the test fails). No invariant is weakened for a
//! listed bug.

use paddock_service::backend::{
    Backend, Capabilities, DisabledMark, Inspection, Job, LastExit, LoginItemFacts, Mechanism,
    Registration, StartFacts, UnregisterError,
};
use paddock_service::plan::{Hold, MAX_ATTEMPTS, Plan};
use paddock_service::port::{AppServiceStatus, RegisterError};
use paddock_service::record::{Memory, Phase, RecordFile, Recorded, State};
use paddock_service::{Manager, RepairOutcome, ServiceError, StartWait, Started, Timings};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

const LABEL: &str = "net.model.agent";

/// The waits of a watched configuration, shrunk like the rest of the model's time: a fixed wait of 12 s (4 looks), the state
/// machine's backstop at 20 looks and the watch's own ceiling 5 looks before it, so the judged wait is as long as it is in
/// production relative to the fixed one but a run costs tens of looks, not hundreds.
fn watched_timings() -> Timings {
    Timings {
        start_wait: Duration::from_secs(12),
        judged_ceiling_polls: WATCH_BACKSTOP_LOOKS,
        ..Timings::default()
    }
}

/// The backstop of a judged wait in looks (see [`watched_timings`]).
const WATCH_BACKSTOP_LOOKS: u32 = 20;

/// Safety net: the space is finite, so this is never reached unless a bucket is missing from the key.
const STATES_CEILING: usize = 400_000;

// ---------------------------------------------------------------------------------------------------------------------
// The alphabet

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum Ver {
    /// Older than this app (a downgrade, or the other copy).
    V0,
    /// This app's version at the start.
    V1,
    /// The version an update brings.
    V2,
    /// A newer copy's.
    V3,
}

impl Ver {
    fn s(self) -> &'static str {
        match self {
            Ver::V0 => "1.0.0",
            Ver::V1 => "1.1.0",
            Ver::V2 => "1.2.0",
            Ver::V3 => "1.3.0",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Caps {
    Mac,
    Linux,
    NoAuto,
}

fn caps_of(c: Caps) -> Capabilities {
    match c {
        Caps::Mac => Capabilities {
            automatic: true,
            pinned_identity: true,
            login_items: true,
        },
        Caps::Linux => Capabilities {
            automatic: true,
            pinned_identity: false,
            login_items: false,
        },
        Caps::NoAuto => Capabilities {
            automatic: false,
            pinned_identity: false,
            login_items: false,
        },
    }
}

/// How the OS reacts to a registration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resp {
    /// The job comes up 2 s after it is registered.
    Fast,
    /// 50 s (inside the 120 s wait).
    Slow,
    /// 300 s (after the wait, still healthy).
    Late,
    /// Never.
    Never,
    /// Refused whatever is registered.
    RefusedAlways,
    /// The manager judges the wait with a [`StartWait`] (ADR 0056, amendment "the wait is judged"): the job comes up 30 s after
    /// it is registered, which is past the (shrunk, see [`watched_timings`]) fixed wait of 12 s; the judge waits for it.
    WatchedLate,
    /// Judged: the job never comes up and shows nothing but "not running" (launchd's spawn scheduled), which spends no patience,
    /// so only the ceiling of looks ends the wait, as a failure: nothing ever ran.
    WatchedNever,
    /// Judged: the job exits with code 3 and the OS restarts it in a loop, so the judge ends the wait early with the observation.
    Looping,
}

impl Resp {
    fn delay(self) -> Option<u32> {
        match self {
            Resp::Fast => Some(2),
            Resp::Slow => Some(50),
            Resp::Late => Some(300),
            Resp::WatchedLate => Some(30),
            Resp::Never | Resp::RefusedAlways | Resp::WatchedNever | Resp::Looping => None,
        }
    }

    /// Whether the manager under test has a start watch (and the backend can say what it sees of the starting job).
    fn watched(self) -> bool {
        matches!(self, Resp::WatchedLate | Resp::WatchedNever | Resp::Looping)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Reg {
    Absent,
    Registered,
    SwitchedOff,
    NeedsApproval,
    Unavailable,
}

/// The simulated OS, the record on disk and the app's own settings: one state of the graph.
#[derive(Clone, Debug)]
struct World {
    caps: Caps,
    resp: Resp,
    reg: Reg,
    /// The version whose copy registered it.
    owner: Ver,
    /// The registration points at another copy of the app.
    other_copy: bool,
    program_gone: bool,
    job: Job,
    /// The job is the old build's: refused until it is evicted.
    stale: bool,
    /// Seconds until a registered job comes up.
    start_in: Option<u32>,
    /// Seconds until an evicted job is dropped by the OS (a removal that times out).
    linger: u32,
    mark_unreadable: bool,
    /// This app's version.
    cur: Ver,
    wanted: Option<bool>,
    first_run_done: bool,
    location_ok: bool,
    /// The record cannot be written (a newer format, a read-only folder).
    ro: bool,
    /// The version whose process the running job is (an old build's job can outlive its registration).
    job_ver: Ver,
    /// Unattended registers of this version across starts that no answer has settled yet (saturates; reset by a settle or a version change).
    ghost: u8,
    /// The record file is there but cannot be used (damaged, unreadable): the app adopts a service that answers.
    rec_damaged: bool,
    /// One of the rare events happened on this path (see [`is_rare`]).
    rare_used: bool,
    /// Shared, so that cloning a world (several times an edge) copies no strings; a change goes through `Arc::make_mut`.
    rec: Arc<Recorded>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ev {
    // Entry points of the app (the Manager is called).
    Start,
    FirstRun,
    Install,
    Repair,
    Uninstall,
    Resume,
    Update,
    Downgrade,
    // The OS and the user (nothing of the app runs).
    Reboot,
    Wait,
    ToggleLoginItem,
    ToggleMark,
    JobCrash,
    JobUnknown,
    JobRefuse,
    StopRequested,
    SettingsOn,
    SettingsOff,
    SettingsDamaged,
    DamageRecord,
    NewerRecord,
    CopyNewer,
    CopyOlder,
    ProgramGone,
    Relocate,
}

const ALL_EVS: [Ev; 25] = [
    Ev::Start,
    Ev::FirstRun,
    Ev::Install,
    Ev::Repair,
    Ev::Uninstall,
    Ev::Resume,
    Ev::Update,
    Ev::Downgrade,
    Ev::Reboot,
    Ev::Wait,
    Ev::ToggleLoginItem,
    Ev::ToggleMark,
    Ev::JobCrash,
    Ev::JobUnknown,
    Ev::JobRefuse,
    Ev::StopRequested,
    Ev::SettingsOn,
    Ev::SettingsOff,
    Ev::SettingsDamaged,
    Ev::DamageRecord,
    Ev::NewerRecord,
    Ev::CopyNewer,
    Ev::CopyOlder,
    Ev::ProgramGone,
    Ev::Relocate,
];

/// Whether the long run is asked for (`MODEL_DEEP=1`): every event may repeat and faulted replaces are crashed too. The
/// default run keeps the whole alphabet but lets a path take at most one rare event, so it finishes in seconds; both end at a
/// fixed point and print their size.
fn depth_cap() -> u32 {
    std::env::var("MODEL_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(u32::MAX)
}

fn deep() -> bool {
    static DEEP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEEP.get_or_init(|| std::env::var("MODEL_DEEP").is_ok())
}

/// `MODEL_REAL_CRASHES=1`: every crash is a real panic unwound through the manager, as before the edges of crashes were taken
/// from a running run (the two give the same states and edges: the fingerprint of the states in the output says so).
fn real_crashes() -> bool {
    // Read once: it is asked on every edge, and an environment lookup is a system call on Windows (it also walks the whole
    // environment block, which on a CI runner is large).
    static REAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REAL.get_or_init(|| std::env::var_os("MODEL_REAL_CRASHES").is_some())
}

/// Panics that unwound through the manager (a crash edge taken by really panicking). Counted to show how many the
/// exploration still pays for: an unwind costs tens of microseconds on macOS and far more on Windows.
static REAL_PANICS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Events that matter in combination with the core alphabet but not with each other: a path takes at most one of them, so
/// each is tried from every core state without multiplying the state space by every combination.
fn is_rare(e: Ev) -> bool {
    !deep()
        && matches!(
            e,
            Ev::ToggleMark
                | Ev::JobUnknown
                | Ev::JobCrash
                | Ev::StopRequested
                | Ev::SettingsDamaged
                | Ev::DamageRecord
                | Ev::NewerRecord
                | Ev::CopyNewer
                | Ev::CopyOlder
                | Ev::ProgramGone
                | Ev::Relocate
                | Ev::Downgrade
        )
}

fn is_entry(e: Ev) -> bool {
    matches!(
        e,
        Ev::Start
            | Ev::FirstRun
            | Ev::Install
            | Ev::Repair
            | Ev::Uninstall
            | Ev::Resume
            | Ev::Update
            | Ev::Downgrade
    )
}

/// The app acts without the user asking.
fn unattended(e: Ev) -> bool {
    matches!(
        e,
        Ev::Start | Ev::FirstRun | Ev::Resume | Ev::Update | Ev::Downgrade
    )
}

/// A fault of the OS calls of one entry point (all of its calls of that kind).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    RegNeedsApproval,
    RegUnavailable,
    RegNotAsWritten,
    RegOther,
    RegNeedsElevation,
    RegDeclined,
    UnregFails,
    /// The old job outlives the wait for its removal.
    RemovalTimeout,
    /// A register or unregister call takes 130 s.
    SlowOs,
    /// Another copy holds the repair lock.
    LockHeld,
    /// The old job outlives its removal for good: it stays running (and answering) through a register (review P1-4).
    LingeringJob,
}

const FAULTS: [Fault; 11] = [
    Fault::RegNeedsApproval,
    Fault::RegUnavailable,
    Fault::RegNotAsWritten,
    Fault::RegOther,
    Fault::RegNeedsElevation,
    Fault::RegDeclined,
    Fault::UnregFails,
    Fault::RemovalTimeout,
    Fault::SlowOs,
    Fault::LockHeld,
    Fault::LingeringJob,
];

fn is_reg_fault(f: Fault) -> bool {
    matches!(
        f,
        Fault::RegNeedsApproval
            | Fault::RegUnavailable
            | Fault::RegNotAsWritten
            | Fault::RegOther
            | Fault::RegNeedsElevation
            | Fault::RegDeclined
    )
}

/// An interruption of one entry point at OS call number `k` (1-based; 0 is before the first).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Intr {
    None,
    CrashBefore(u32),
    CrashAfter(u32),
    /// The cancel (Quit) is set when call `k` is reached.
    Cancel(u32),
    /// From call `k` on the record cannot be written.
    WritesFail(u32),
}

struct Crash;

fn silence_crashes() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<Crash>().is_none() {
                previous(info);
            }
        }));
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// The simulated OS as a Backend

#[derive(Clone, Copy)]
struct Op {
    k: u32,
    label: &'static str,
}

/// The OS calls a trace holds, in order.
fn ops_of(trace: &[Line]) -> impl Iterator<Item = Op> + '_ {
    trace.iter().filter_map(|l| match l {
        Line::Call { k, label, .. } => Some(Op { k: *k, label }),
        _ => None,
    })
}

fn mutating(label: &str) -> bool {
    matches!(label, "register" | "unregister" | "evict")
}

/// One line of the trace of a run: an OS call with the state it met, or a crash. A plain value (no text) so that taking and
/// cloning a trace in the hot loop costs one allocation, not one per line; it is rendered only when a path is replayed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Line {
    Call {
        k: u32,
        label: &'static str,
        reg: Reg,
        owner: Ver,
        job: Job,
        stale: bool,
    },
    CrashBefore,
    CrashAfter,
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Line::Call {
                k,
                label,
                reg,
                owner,
                job,
                stale,
            } => write!(
                f,
                "  #{k} {label}: reg {reg:?} owner {owner:?} job {job:?} stale {stale}"
            ),
            Line::CrashBefore => f.write_str("  -- crash before the effect"),
            Line::CrashAfter => f.write_str("  -- crash after the effect"),
        }
    }
}

#[derive(Clone)]
struct Sim {
    w: World,
    fault: Fault,
    intr: Intr,
    tick: u32,
    trace: Vec<Line>,
    cancel: Arc<AtomicBool>,
    mem: Arc<Memory>,
    registers: u32,
    /// Register calls that were an attempt (not a state of the user or the OS: approval, unavailable, elevation, declined).
    counted: u32,
    prompts: u32,
    reg_while_off: u32,
    /// How often the manager has looked at the starting job in this run (what the OS's run count and the CPU time follow).
    looks: u32,
    /// Set when the run is to hand out the edges of the crashes at its mutating calls without panicking (see [`Capture`]).
    capture: Option<Arc<Mutex<Capture>>>,
}

/// What a crash at a mutating call leaves, taken at that moment of a run that goes on: a process that dies there stops
/// everything after it, so the state it leaves is the state of the run at that point. Unwinding a panic through the
/// manager to find out is the dearest thing the exploration does (some 40 us a crash on macOS, where libunwind asks dyld
/// about every frame, and far more on Windows), and there are hundreds of thousands of crashes.
struct Capture {
    rec: RecordFile,
    snaps: Vec<(Intr, Sim, Recorded, bool)>,
}

impl Capture {
    /// Takes the state at a crash `intr` of the run `s`; the line is what the trace of a real crash says.
    fn take(&mut self, intr: Intr, s: &Sim, line: Line) {
        let mut at = s.clone();
        at.intr = intr;
        at.capture = None;
        at.trace.push(line);
        let (rec, damaged) = self.rec.peek();
        self.snaps.push((intr, at, rec, damaged));
    }
}

fn lk(s: &Mutex<Sim>) -> std::sync::MutexGuard<'_, Sim> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Time passes in the OS: a registered job comes up, an evicted one is dropped.
fn advance(w: &mut World, secs: u32) {
    if w.linger > 0 {
        if secs >= w.linger {
            // The OS drops the old job; what is registered now starts as it would after a fresh registration.
            w.linger = 0;
            w.stale = false;
            w.start_in = None;
            if w.reg == Reg::Registered {
                if w.resp == Resp::RefusedAlways {
                    w.job = Job::Refused;
                    w.stale = true;
                } else {
                    w.job = Job::NotRunning;
                    w.start_in = w.resp.delay();
                }
            } else {
                w.job = Job::Absent;
            }
        } else {
            w.linger -= secs;
        }
    }
    if let Some(s) = w.start_in
        && w.job == Job::NotRunning
        && w.reg == Reg::Registered
        && !w.stale
    {
        if secs >= s {
            w.job = Job::Running;
            w.job_ver = w.owner;
            w.start_in = None;
        } else {
            w.start_in = Some(s - secs);
        }
    }
}

struct SimBackend {
    s: Arc<Mutex<Sim>>,
}

impl SimBackend {
    /// Counts the call, applies an interruption set for it and returns its number.
    fn enter(&self, label: &'static str) -> u32 {
        let mut s = lk(&self.s);
        s.tick += 1;
        let k = s.tick;
        let line = Line::Call {
            k,
            label,
            reg: s.w.reg,
            owner: s.w.owner,
            job: s.w.job,
            stale: s.w.stale,
        };
        s.trace.push(line);
        if let Some(c) = s.capture.clone()
            && mutating(label)
        {
            c.lock().unwrap_or_else(|e| e.into_inner()).take(
                Intr::CrashBefore(k),
                &s,
                Line::CrashBefore,
            );
        }
        match s.intr {
            Intr::Cancel(n) if n == k => s.cancel.store(true, Ordering::SeqCst),
            Intr::WritesFail(n) if n == k => s.mem.fail_writes(true),
            Intr::CrashBefore(n) if n == k => {
                s.trace.push(Line::CrashBefore);
                drop(s);
                panic_any(Crash);
            }
            _ => {}
        }
        k
    }

    fn leave(&self, k: u32) {
        let mut s = lk(&self.s);
        if let Some(c) = s.capture.clone()
            && ops_of(&s.trace).any(|o| o.k == k && mutating(o.label))
        {
            c.lock().unwrap_or_else(|e| e.into_inner()).take(
                Intr::CrashAfter(k),
                &s,
                Line::CrashAfter,
            );
        }
        if s.intr == Intr::CrashAfter(k) {
            s.trace.push(Line::CrashAfter);
            drop(s);
            panic_any(Crash);
        }
    }

    fn look(w: &World) -> Inspection {
        let mut i = Inspection::absent();
        i.registration = match w.reg {
            Reg::Absent => Registration::Absent,
            Reg::Registered => Registration::Registered,
            Reg::SwitchedOff => Registration::SwitchedOff,
            Reg::NeedsApproval => Registration::NeedsApproval,
            Reg::Unavailable => Registration::Unavailable("the app runs from a disk image".into()),
        };
        i.job = w.job;
        i.program_exists = Some(!w.program_gone);
        i.points_here = Some(!w.other_copy);
        // LaunchdBundled (SMAppService) cannot compare a definition; systemd and the SCM can.
        i.definition_current = (w.caps != Caps::Mac)
            .then(|| w.reg == Reg::Registered && w.owner == w.cur && !w.other_copy);
        if caps_of(w.caps).login_items {
            let (status, mark) = match w.reg {
                Reg::Registered => (AppServiceStatus::Enabled, DisabledMark::NotListed),
                Reg::SwitchedOff => (AppServiceStatus::RequiresApproval, DisabledMark::Disabled),
                Reg::NeedsApproval => (AppServiceStatus::RequiresApproval, DisabledMark::NotListed),
                Reg::Absent => (AppServiceStatus::NotRegistered, DisabledMark::NotListed),
                Reg::Unavailable => (AppServiceStatus::NotFound, DisabledMark::NotListed),
            };
            let mark = if w.mark_unreadable {
                DisabledMark::Unreadable
            } else {
                mark
            };
            i.login_item = Some(LoginItemFacts { status, mark });
        }
        i
    }
}

impl Backend for SimBackend {
    fn mechanism(&self) -> Mechanism {
        match lk(&self.s).w.caps {
            Caps::Mac => Mechanism::LaunchdBundled,
            Caps::Linux => Mechanism::SystemdUser,
            Caps::NoAuto => Mechanism::WindowsService,
        }
    }
    fn capabilities(&self) -> Capabilities {
        caps_of(lk(&self.s).w.caps)
    }
    fn inspect(&mut self) -> Inspection {
        let k = self.enter("inspect");
        let i = Self::look(&lk(&self.s).w);
        self.leave(k);
        i
    }
    fn register(&mut self) -> Result<Inspection, RegisterError> {
        let k = self.enter("register");
        let result = {
            let mut s = lk(&self.s);
            if s.fault == Fault::SlowOs {
                advance(&mut s.w, 130);
            }
            s.registers += 1;
            // A registration starts a new wait: the budget of looks counts from it.
            s.looks = 0;
            if !caps_of(s.w.caps).automatic {
                s.prompts += 1;
            }
            match (s.w.reg, s.fault) {
                (Reg::SwitchedOff, _) => {
                    s.reg_while_off += 1;
                    Err(RegisterError::NeedsApproval)
                }
                (Reg::NeedsApproval, _) | (_, Fault::RegNeedsApproval) => {
                    if s.w.caps == Caps::Mac {
                        // The item is registered by this copy and waits for the user's approval.
                        s.w.reg = Reg::NeedsApproval;
                        s.w.owner = s.w.cur;
                        s.w.other_copy = false;
                        s.w.program_gone = false;
                    }
                    Err(RegisterError::NeedsApproval)
                }
                (Reg::Unavailable, _) | (_, Fault::RegUnavailable) => {
                    Err(RegisterError::Unavailable("unavailable here".into()))
                }
                (_, Fault::RegOther) => {
                    s.counted += 1;
                    Err(RegisterError::Other("the tool failed".into()))
                }
                (_, Fault::RegNeedsElevation) => Err(RegisterError::NeedsElevation),
                (_, Fault::RegDeclined) => Err(RegisterError::Declined),
                (_, fault) => {
                    s.counted += 1;
                    let w = &mut s.w;
                    w.reg = Reg::Registered;
                    w.owner = w.cur;
                    w.other_copy = false;
                    w.program_gone = false;
                    let lingering_old = w.job != Job::Absent && w.stale;
                    if w.resp == Resp::RefusedAlways || lingering_old {
                        w.job = Job::Refused;
                        w.stale = true;
                        w.start_in = None;
                    } else if w.job != Job::Running {
                        w.job = Job::NotRunning;
                        w.start_in = w.resp.delay();
                    } else if !caps_of(w.caps).pinned_identity && w.job_ver != w.cur {
                        // Registering over a running old build (systemd, the Windows Run value): the unit is restarted so the
                        // new version runs, and the old agent is replaced by the new one.
                        w.job = Job::NotRunning;
                        w.job_ver = w.cur;
                        w.start_in = w.resp.delay();
                    }
                    let look = Self::look(&s.w);
                    if fault == Fault::RegNotAsWritten {
                        Err(RegisterError::NotAsWritten(look))
                    } else {
                        Ok(look)
                    }
                }
            }
        };
        self.leave(k);
        result
    }
    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        let k = self.enter("unregister");
        let result = {
            let mut s = lk(&self.s);
            if s.fault == Fault::SlowOs {
                advance(&mut s.w, 130);
            }
            if s.fault == Fault::UnregFails {
                Err(UnregisterError::Failed("the tool failed".into()))
            } else {
                if matches!(s.w.reg, Reg::Registered | Reg::NeedsApproval) {
                    s.w.reg = Reg::Absent;
                }
                Ok(Self::look(&s.w))
            }
        };
        self.leave(k);
        result
    }
    fn evict(&mut self) -> Result<(), String> {
        let k = self.enter("evict");
        {
            let mut s = lk(&self.s);
            if s.fault == Fault::LingeringJob {
                // The job stays as it is, running its old build.
            } else if s.fault == Fault::RemovalTimeout {
                if s.w.job != Job::Absent {
                    s.w.linger = 25;
                }
            } else if s.w.resp != Resp::RefusedAlways {
                s.w.linger = 0;
                s.w.job = Job::Absent;
                s.w.stale = false;
                s.w.start_in = None;
            }
        }
        self.leave(k);
        Ok(())
    }
    fn job_gone(&mut self) -> bool {
        lk(&self.s).w.job == Job::Absent
    }

    /// What launchd shows of the starting job, for a watched configuration (the others cannot be observed, like systemd).
    fn observe_start(&mut self) -> Option<StartFacts> {
        let mut s = lk(&self.s);
        if !s.w.resp.watched() {
            return None;
        }
        s.looks += 1;
        // A hard budget: if the ceiling ever stops ending the wait, the run stops here with the reason instead of hanging.
        assert!(
            s.looks <= 2 * WATCH_BACKSTOP_LOOKS,
            "a judged wait looked {} times since the last register, more than twice its backstop of {WATCH_BACKSTOP_LOOKS}: the ceiling no longer ends it",
            s.looks
        );
        let looks = s.looks;
        let w = &s.w;
        let running = w.job == Job::Running;
        Some(StartFacts {
            job: w.job,
            pid: running.then(|| {
                paddock_proc::ProcessId::new(
                    1000 + w.job_ver as u32,
                    paddock_proc::StartTime::from_micros(1),
                )
            }),
            runs: Some(if w.resp == Resp::Looping {
                1 + looks
            } else {
                1
            }),
            last_exit: (w.resp == Resp::Looping && !running).then_some(3),
            cpu: running.then(|| Duration::from_secs(u64::from(looks))),
            run_state: if running {
                paddock_proc::RunState::Running
            } else {
                paddock_proc::RunState::Unknown
            },
        })
    }

    /// The process of the job the OS runs: the old build's job that outlived a removal has its own, older identity, so the
    /// settle check can tell it from the job this registration started (ADR 0056 P1-4).
    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        let sim = lk(&self.s);
        let w = &sim.w;
        if w.job != Job::Running {
            return None;
        }
        let pid = 1000 + w.job_ver as u32;
        let started = if w.job_ver == w.owner { u64::MAX } else { 1 };
        Some(paddock_proc::ProcessId::new(
            pid,
            paddock_proc::StartTime::from_micros(started),
        ))
    }
    fn pause(&mut self, how_long: Duration) {
        advance(
            &mut lk(&self.s).w,
            u32::try_from(how_long.as_secs()).unwrap_or(u32::MAX).max(1),
        );
    }
    fn open_login_items(&mut self) -> bool {
        lk(&self.s).prompts += 1;
        true
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// One edge: an event applied to a world

/// A violated invariant: its kind (the name in [`KNOWN_FAILING`]) and the details of the run that showed it.
type Viol = (&'static str, String);

struct Edge {
    after: World,
    crashed: bool,
    trace: Vec<Line>,
    viols: Vec<Viol>,
}

fn normalize(w: &mut World) {
    let over =
        matches!(&w.rec.state, State::Attempting { attempts, .. } if *attempts > MAX_ATTEMPTS);
    let pending = matches!(w.rec.resume_pending, Some(n) if n != 1);
    if over || pending {
        let r = Arc::make_mut(&mut w.rec);
        if let State::Attempting { attempts, .. } = &mut r.state {
            *attempts = (*attempts).min(MAX_ATTEMPTS);
        }
        if r.resume_pending.is_some() {
            r.resume_pending = Some(1);
        }
    }
}

/// The clock every manager of the model reads: fixed, at a calendar time after the process start times the simulated jobs
/// report, never moving. The real one is a system call per reading, five to seven an edge, and on a QEMU Windows runner each
/// was 13 to 25 microseconds (CI run 7646's diagnostic), most of the 20 to 35 microseconds an edge cost there.
fn model_clock() -> Arc<dyn paddock_health::Clock> {
    static CLOCK: std::sync::OnceLock<Arc<dyn paddock_health::Clock>> = std::sync::OnceLock::new();
    CLOCK
        .get_or_init(|| Arc::new(paddock_health::FakeClock::at_wall_ms(1_700_000_000_000)))
        .clone()
}

fn build(sim: &Arc<Mutex<Sim>>, rec: RecordFile, timings: Option<Timings>) -> Manager {
    let version = lk(sim).w.cur.s();
    let (a, b, c, d) = (sim.clone(), sim.clone(), sim.clone(), sim.clone());
    let backend = SimBackend { s: sim.clone() };
    let mut m = Manager::new(
        "Model",
        Box::new(backend),
        std::path::Path::new("/nonexistent/model"),
        LABEL,
        version,
        move || lk(&a).w.job == Job::Running,
    )
    .with_record(rec)
    .with_clock(model_clock())
    .with_app_state(
        move || lk(&b).w.wanted,
        move || lk(&c).w.first_run_done,
        move || lk(&d).w.location_ok,
    );
    let watched = lk(sim).w.resp.watched();
    let timings = timings.or_else(|| watched.then(watched_timings));
    if let Some(t) = timings {
        m = m.with_timings(t);
    }
    if watched {
        let t = timings.unwrap_or_default();
        // The watch's own ceiling comes a few looks before the backstop, so its friendlier message is the one said.
        m = m.with_start_watch(Box::new(StartWait::with_ceiling(
            t.start_poll,
            t.judged_ceiling_polls.saturating_sub(5),
        )));
    }
    m
}

fn new_sim(w: &World, fault: Fault, intr: Intr, rec: &RecordFile) -> Arc<Mutex<Sim>> {
    let mem = rec
        .memory_handle()
        .expect("the model uses the memory store");
    let cancel = Arc::new(AtomicBool::new(false));
    if intr == Intr::Cancel(0) {
        cancel.store(true, Ordering::SeqCst);
    }
    mem.fail_writes(w.ro || intr == Intr::WritesFail(0));
    mem.set_damaged(w.rec_damaged);
    Arc::new(Mutex::new(Sim {
        w: w.clone(),
        fault,
        intr,
        tick: 0,
        // A run makes some ten calls; room for them at once instead of growing 4, 8, 16.
        trace: Vec::with_capacity(24),
        cancel,
        mem,
        registers: 0,
        counted: 0,
        prompts: 0,
        reg_while_off: 0,
        looks: 0,
        capture: None,
    }))
}

/// What the entry point returned, for the invariants.
#[derive(Debug)]
#[allow(dead_code)] // the payloads are read in failure output
enum Out {
    Started(Started),
    Install(Result<RepairOutcome, ServiceError>),
    Uninstall(Result<(), ServiceError>),
    Resume(Result<bool, ServiceError>),
}

fn job_after_enable(w: &mut World) {
    w.job = if w.resp == Resp::RefusedAlways {
        Job::Refused
    } else {
        Job::NotRunning
    };
    w.stale = w.resp == Resp::RefusedAlways;
    w.start_in = w.resp.delay();
}

/// An event of the OS or the user: nothing of the app runs.
fn environment(w: &mut World, ev: Ev) {
    match ev {
        Ev::Reboot => {
            w.linger = 0;
            if w.reg == Reg::Registered {
                if w.stale || w.resp == Resp::RefusedAlways {
                    w.job = Job::Refused;
                    w.stale = true;
                    w.start_in = None;
                } else {
                    w.job = Job::NotRunning;
                    w.start_in = w.resp.delay();
                }
            } else {
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
            }
        }
        Ev::Wait => {
            advance(w, 1000);
            if matches!(w.job, Job::Failed(_) | Job::Unknown) {
                if w.reg == Reg::Registered && !w.stale {
                    w.job = Job::Running;
                } else {
                    w.job = Job::NotRunning;
                }
            }
        }
        Ev::ToggleLoginItem => match w.reg {
            Reg::Registered => {
                w.reg = Reg::SwitchedOff;
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
            }
            Reg::SwitchedOff | Reg::NeedsApproval => {
                w.reg = Reg::Registered;
                job_after_enable(w);
            }
            Reg::Absent | Reg::Unavailable => {}
        },
        Ev::ToggleMark => {
            if w.caps == Caps::Mac {
                w.mark_unreadable = !w.mark_unreadable;
            }
        }
        Ev::JobCrash => {
            if w.job == Job::Running {
                w.job = Job::Failed(LastExit::Code(1));
            }
        }
        Ev::JobUnknown => {
            if matches!(w.job, Job::Running | Job::NotRunning) {
                w.job = Job::Unknown;
            }
        }
        Ev::JobRefuse => {
            if w.reg == Reg::Registered
                && matches!(w.job, Job::Running | Job::NotRunning | Job::Failed(_))
            {
                w.job = Job::Refused;
                w.stale = true;
                w.start_in = None;
            }
        }
        Ev::StopRequested => {
            if w.job != Job::Absent {
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
                w.linger = 0;
                Arc::make_mut(&mut w.rec).resume_pending = Some(1);
            }
        }
        Ev::SettingsOn => {
            // Off and on again is a user asking for the service again: the count starts over.
            if w.wanted == Some(false) {
                w.ghost = 0;
            }
            w.wanted = Some(true);
        }
        Ev::SettingsOff => w.wanted = Some(false),
        Ev::SettingsDamaged => w.wanted = None,
        Ev::DamageRecord => {
            w.rec = Arc::default();
            w.rec_damaged = true;
        }
        Ev::NewerRecord => {
            w.rec = Arc::default();
            w.rec_damaged = true;
            w.ro = true;
        }
        Ev::CopyNewer | Ev::CopyOlder => {
            let v = if ev == Ev::CopyNewer {
                Ver::V3
            } else {
                Ver::V0
            };
            w.reg = Reg::Registered;
            w.owner = v;
            w.other_copy = true;
            w.program_gone = false;
            w.job = Job::Running;
            w.job_ver = v;
            w.stale = false;
            w.start_in = None;
            let r = Arc::make_mut(&mut w.rec);
            r.registered_version = Some(v.s().to_string());
            r.state = State::Idle;
        }
        Ev::ProgramGone => {
            if w.reg == Reg::Registered {
                w.program_gone = true;
            }
        }
        Ev::Relocate => w.location_ok = !w.location_ok,
        _ => {}
    }
    normalize(w);
}

fn describe_panic(p: &(dyn std::any::Any + Send)) -> Option<String> {
    if p.downcast_ref::<Crash>().is_some() {
        return None;
    }
    Some(
        p.downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into()),
    )
}

/// Applies one event (with a fault and an interruption for an entry point) to `w0` and checks the edge invariants.
fn apply(w0: &World, ev: Ev, fault: Fault, intr: Intr) -> Edge {
    run(w0, ev, fault, intr, false).0
}

/// A run without an interruption, and with it the edges of the crashes at its mutating calls (see [`Capture`]).
fn apply_with_crashes(w0: &World, ev: Ev, fault: Fault) -> (Edge, Vec<(Intr, Edge)>) {
    run(w0, ev, fault, Intr::None, true)
}

fn run(
    w0: &World,
    ev: Ev,
    fault: Fault,
    intr: Intr,
    with_crashes: bool,
) -> (Edge, Vec<(Intr, Edge)>) {
    let mut w = w0.clone();
    if is_rare(ev) {
        w.rare_used = true;
    }
    if !is_entry(ev) {
        environment(&mut w, ev);
        let edge = Edge {
            after: w,
            crashed: false,
            trace: Vec::new(),
            viols: Vec::new(),
        };
        return (edge, Vec::new());
    }
    match ev {
        Ev::Update => {
            w.cur = Ver::V2;
            w.ghost = 0;
        }
        Ev::Downgrade => {
            w.cur = Ver::V1;
            w.ghost = 0;
        }
        Ev::FirstRun | Ev::Install => {
            // The switch was off and is on again: that is a user asking for the service again, so the count starts over (the
            // removal that followed the switch going off cleared the attempts in the record).
            if w.wanted == Some(false) {
                w.ghost = 0;
            }
            w.wanted = Some(true);
        }
        Ev::Uninstall => {
            // The user's switch off and on again tries again (the log of a hold says so), so the count starts over.
            w.wanted = Some(false);
            w.ghost = 0;
        }
        _ => {}
    }
    let entry = w;
    let rec = RecordFile::memory();
    rec.store(&entry.rec)
        .expect("the memory store takes the first write");
    let sim = new_sim(&entry, fault, intr, &rec);
    let capture = with_crashes.then(|| {
        Arc::new(Mutex::new(Capture {
            rec: rec.reopen(),
            snaps: Vec::new(),
        }))
    });
    lk(&sim).capture.clone_from(&capture);
    let other_holder = if fault == Fault::LockHeld {
        rec.lock()
    } else {
        None
    };
    let cancel = lk(&sim).cancel.clone();
    let no_wait = Timings::default().without_start_wait();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut m = build(
            &sim,
            rec.reopen(),
            if matches!(ev, Ev::FirstRun | Ev::Install) {
                Some(no_wait)
            } else {
                None
            },
        );
        match ev {
            Ev::Start | Ev::Update | Ev::Downgrade => Out::Started(m.on_start(&cancel)),
            Ev::FirstRun => Out::Install(m.install_unattended(&cancel)),
            Ev::Install | Ev::Repair => Out::Install(m.install(&cancel)),
            Ev::Uninstall => Out::Uninstall(m.uninstall()),
            Ev::Resume => Out::Resume(m.resume(&cancel, &mut |_| {})),
            _ => unreachable!("not an entry point"),
        }
    }));
    drop(other_holder);
    let mut viols: Vec<Viol> = Vec::new();
    let (crashed, out) = match result {
        Ok(o) => (false, Some(o)),
        Err(p) => {
            REAL_PANICS.fetch_add(1, Ordering::Relaxed);
            if let Some(msg) = describe_panic(p.as_ref()) {
                viols.push(("the code under test panicked", msg));
            }
            (true, None)
        }
    };
    let s = lk(&sim);
    let (recorded, damaged) = rec.peek();
    let edge = conclude(
        &entry,
        ev,
        fault,
        intr,
        &s,
        recorded,
        damaged,
        out.as_ref(),
        crashed,
        viols,
    );
    let crashes = capture.map_or_else(Vec::new, |c| {
        let snaps = std::mem::take(&mut c.lock().unwrap_or_else(|e| e.into_inner()).snaps);
        snaps
            .into_iter()
            .map(|(at, sim, recorded, damaged)| {
                let e = conclude(
                    &entry,
                    ev,
                    fault,
                    at,
                    &sim,
                    recorded,
                    damaged,
                    None,
                    true,
                    Vec::new(),
                );
                (at, e)
            })
            .collect()
    });
    (edge, crashes)
}

/// The edge a run left: the world after it, and the invariants of the step. `s` is the simulated OS as the run ended (or as it
/// stood at a crash), `recorded` and `damaged` what the record file then held.
#[allow(clippy::too_many_arguments)]
fn conclude(
    entry: &World,
    ev: Ev,
    fault: Fault,
    intr: Intr,
    s: &Sim,
    recorded: Recorded,
    damaged: bool,
    out: Option<&Out>,
    crashed: bool,
    mut viols: Vec<Viol>,
) -> Edge {
    let mut after = s.w.clone();
    after.rec = Arc::new(recorded);
    after.rec_damaged = damaged;
    if matches!(ev, Ev::FirstRun | Ev::Install) && matches!(out, Some(Out::Install(Ok(_)))) {
        // Dyno writes its first-run marker after a registration that did not fail.
        after.first_run_done = true;
    }
    normalize(&mut after);
    // The registers of this version across starts that no answer has settled (a free finish and a light resume are not attempts).
    if unattended(ev)
        && !after.rare_used
        && matches!(
            entry.resp,
            Resp::Never | Resp::RefusedAlways | Resp::WatchedNever | Resp::Looping
        )
    {
        let registers = s.counted;
        let free = free_finish(entry) + u32::from(ev == Ev::Resume);
        after.ghost =
            (u32::from(entry.ghost) + registers.saturating_sub(free)).min(MAX_ATTEMPTS + 1) as u8;
    }
    if after.reg == Reg::Registered && after.job == Job::Running && after.job_ver == after.cur {
        after.ghost = 0;
    }
    let trace = s.trace.clone();
    edge_checks(entry, &after, ev, fault, intr, s, out, crashed, &mut viols);
    Edge {
        after,
        crashed,
        trace,
        viols,
    }
}

/// The register a crash after the removal leaves is free (its attempt was never counted), by design (ADR 0056 section 1), and
/// so is the finish of a removal the user asked for.
fn free_finish(entry: &World) -> u32 {
    let nothing_to_remove =
        entry.reg == Reg::Absent && matches!(entry.job, Job::Absent | Job::Unknown);
    u32::from(match entry.rec.phase_of(entry.cur.s()) {
        Some(Phase::Registering) => true,
        // Paid for already: by the user's request that began it, or by the attempt counted when `Registering` was written.
        Some(Phase::RemovingOld { paid: true, .. }) => true,
        Some(Phase::RemovingOld { .. }) => nothing_to_remove,
        _ => false,
    })
}

#[allow(clippy::too_many_arguments)]
fn edge_checks(
    entry: &World,
    after: &World,
    ev: Ev,
    fault: Fault,
    intr: Intr,
    s: &Sim,
    out: Option<&Out>,
    crashed: bool,
    v: &mut Vec<Viol>,
) {
    let count = |l: &str| ops_of(&s.trace).filter(|o| o.label == l).count() as u32;
    let (registers, unregisters, evicts) = (count("register"), count("unregister"), count("evict"));
    let unat = unattended(ev);
    let cur = entry.cur.s();
    let mut bad = |kind: &'static str, why: String| {
        v.push((
            kind,
            format!("{why} (event {ev:?}, fault {fault:?}, interruption {intr:?})"),
        ))
    };

    if s.reg_while_off > 0 {
        bad(
            "registered while the OS says the user switched it off",
            String::new(),
        );
    }
    if unat && entry.wanted == Some(false) && registers > 0 {
        bad(
            "registered unattended although the user's switch is off",
            String::new(),
        );
    }
    if unat && entry.wanted.is_none() && registers > 0 {
        bad(
            "registered unattended although the user's switch could not be read",
            String::new(),
        );
    }
    if matches!(ev, Ev::Start | Ev::Update | Ev::Downgrade | Ev::Resume)
        && entry.wanted == Some(true)
        && entry.rec.state == State::Removing
        && (unregisters + evicts) > 0
        // A finished removal forgets the record (what uninstall does last); a replace that stops half-way leaves an attempt.
        && after.rec.state == State::Idle
        && after.rec.registered_version.is_none()
    {
        bad("finished a removal the user took back", String::new());
    }
    if unat && s.prompts > 0 {
        bad("an unasked prompt", format!("{} prompts", s.prompts));
    }
    if unat && entry.caps == Caps::NoAuto && registers + unregisters + evicts > 0 {
        bad(
            "changed the OS unattended where every change needs a prompt",
            String::new(),
        );
    }
    // A service that answers is not removed or registered again, unless the version, the copy or the program changed.
    let answering = entry.reg == Reg::Registered
        && entry.job == Job::Running
        && !entry.stale
        && entry.owner == entry.cur
        && !entry.other_copy
        && !entry.program_gone
        && entry.rec.state == State::Idle
        // A record that does not know the version replaces once by design (ADR 0056 section 1); an unusable one adopts.
        && (entry.rec.registered_version.as_deref() == Some(cur) || entry.rec_damaged);
    if matches!(ev, Ev::Start | Ev::Resume | Ev::FirstRun)
        && answering
        && entry.wanted != Some(false)
        && (unregisters + evicts) > 0
    {
        bad("removed a service that answers", String::new());
    }
    if matches!(ev, Ev::Start | Ev::FirstRun) && answering && registers > 0 {
        bad("registered again a service that answers", String::new());
    }
    // The unattended register budget: what is left of this version's attempts, one free finish, one light resume.
    if unat {
        let finishing = free_finish(entry);
        let budget = MAX_ATTEMPTS.saturating_sub(entry.rec.attempts_of(cur))
            + finishing
            + u32::from(ev == Ev::Resume);
        if registers > budget {
            bad(
                "more unattended registers of one version than its attempts allow",
                format!("{registers} registers, budget {budget}"),
            );
        }
    }
    // Never install unasked.
    if matches!(ev, Ev::Start | Ev::Update | Ev::Downgrade | Ev::Resume)
        && registers > 0
        && entry.reg == Reg::Absent
        && !matches!(
            entry.job,
            Job::Running | Job::NotRunning | Job::Refused | Job::Failed(_)
        )
        && !(entry.wanted == Some(true) && entry.first_run_done)
        && !matches!(
            entry.rec.state,
            State::Attempting {
                phase: Phase::RemovingOld { .. } | Phase::Registering | Phase::RegisterFailed(_),
                ..
            }
        )
    {
        bad("installed unasked", String::new());
    }
    // No OS call without a durable write: after the record stopped being writable no register or unregister (they open a
    // step that is written first); the user's own Repair or Install proceeds, loudly.
    if unat {
        let from = if entry.ro {
            Some(0)
        } else if let Intr::WritesFail(k) = intr {
            Some(k)
        } else {
            None
        };
        if let Some(k) = from
            && ops_of(&s.trace).any(|o| o.k > k && matches!(o.label, "register" | "unregister"))
        {
            bad(
                "an OS call without a durable record of it",
                format!("writes failed from call {k}"),
            );
        }
    }
    // A cancel before the first call leaves everything as it was.
    if intr == Intr::Cancel(0)
        && matches!(ev, Ev::Install | Ev::Repair | Ev::Start)
        && entry.wanted != Some(false)
        && entry.rec.state != State::Removing
        && registers + unregisters + evicts > 0
        && !crashed
    {
        bad(
            "cancelled before the first call and the OS was still changed",
            String::new(),
        );
    }
    // A state of the user or the OS is not an attempt (N1).
    if !crashed
        && matches!(
            fault,
            Fault::RegNeedsApproval
                | Fault::RegUnavailable
                | Fault::RegNeedsElevation
                | Fault::RegDeclined
        )
        && matches!(
            ev,
            Ev::Start | Ev::Update | Ev::Downgrade | Ev::Install | Ev::Repair
        )
        && registers > 0
        && !matches!(entry.rec.state, State::Removing)
        && after.rec.attempts_of(cur) > entry.rec.attempts_of(cur)
    {
        bad(
            "a refusal of the user or the OS counted as an attempt",
            format!(
                "attempts {} to {}",
                entry.rec.attempts_of(cur),
                after.rec.attempts_of(cur)
            ),
        );
    }
    if u32::from(after.ghost) > MAX_ATTEMPTS {
        bad(
            "more unattended registers across starts than the cap allows",
            format!("{} registers", after.ghost),
        );
    }
    // The record only says a version is registered when the OS holds that version's registration.
    if entry.rec.registered_version.as_deref() != Some(cur)
        && after.rec.registered_version.as_deref() == Some(cur)
        // Where the build is pinned (macOS) a running old job is the old registration; elsewhere the definition is what is registered.
        && !(after.reg == Reg::Registered
            && after.owner == after.cur
            && !(caps_of(after.caps).pinned_identity && after.job == Job::Running && after.job_ver != after.cur))
        // An unusable record on a Mac: the version cannot be told, so the app adopts a service that answers (ADR 0056).
        && !(entry.rec_damaged && entry.caps == Caps::Mac)
    {
        bad(
            "the record says this version is registered but the OS holds another registration",
            format!("reg {:?} owner {:?}", after.reg, after.owner),
        );
    }
    // The marker of a stop on request is not lost.
    if entry.rec.resume_pending.is_some()
        && after.rec.resume_pending.is_none()
        && !matches!(ev, Ev::Resume | Ev::Uninstall)
    {
        bad("the marker of a stop on request was lost", String::new());
    }
    if ev == Ev::Resume
        && entry.rec.resume_pending.is_some()
        && matches!(out, Some(Out::Resume(Ok(true))))
        && after.rec.resume_pending.is_some()
    {
        bad(
            "a resume completed and its marker is still pending",
            String::new(),
        );
    }
    // An uninstall that returns Ok leaves nothing registered; one that fails puts the record back.
    match out {
        Some(Out::Uninstall(Ok(())))
            if !matches!(after.reg, Reg::Absent | Reg::SwitchedOff | Reg::Unavailable) =>
        {
            bad(
                "uninstall returned Ok and the OS still holds the registration",
                format!("{:?}", after.reg),
            );
        }
        Some(Out::Uninstall(Ok(())))
            if after.rec.registered_version.is_some() || after.rec.state != State::Idle =>
        {
            bad(
                "uninstall returned Ok and the record still remembers it",
                format!("{:?}", after.rec),
            );
        }
        Some(Out::Uninstall(Err(ServiceError::Busy))) | None => {}
        Some(Out::Uninstall(Err(_)))
            if after.reg == Reg::Registered && after.rec.state != entry.rec.state =>
        {
            bad(
                "a failed uninstall did not put the record back",
                format!("{:?} to {:?}", entry.rec.state, after.rec.state),
            );
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Probes and state invariants

/// The same world with an OS that registers at once.
fn cooperative(w: &World) -> World {
    let mut c = w.clone();
    c.resp = Resp::Fast;
    c
}

fn settled(w: &World) -> bool {
    w.reg == Reg::Registered && w.job == Job::Running
}

fn run_chain(w: &World, evs: &[Ev]) -> World {
    let mut c = w.clone();
    for e in evs {
        c = apply(&c, *e, Fault::None, Intr::None).after;
    }
    c
}

/// What the app would see of a state: the plan of a start, `pending()` and `supervises()`.
fn observe(w: &World) -> (Plan, bool, bool) {
    let rec = RecordFile::memory();
    rec.store(&w.rec)
        .expect("the memory store takes the first write");
    let sim = new_sim(w, Fault::None, Intr::None, &rec);
    let mut m = build(&sim, rec, None);
    let plan = m.check();
    (plan, m.pending(), m.supervises())
}

fn state_checks(w: &World) -> Vec<Viol> {
    let mut v: Vec<Viol> = Vec::new();
    let (plan, pending, supervises) = observe(w);
    let hold = matches!(
        plan,
        Plan::Leave(Hold::Unanswered(_) | Hold::Refused(_) | Hold::RegisterFailed(_))
    );
    let answers = w.job == Job::Running;
    if hold {
        let c = cooperative(w);
        let repair = settled(&run_chain(&c, &[Ev::Repair]));
        let update = w.cur != Ver::V2
            && settled(&run_chain(
                &c,
                &[Ev::Update, Ev::Wait, Ev::Start, Ev::Wait, Ev::Start],
            ));
        if !(answers || repair || update) {
            v.push((
                "a hold with no way out",
                format!("plan {plan:?}; Repair {repair}, update {update}"),
            ));
        }
        if supervises {
            v.push(("a held job is supervised", format!("plan {plan:?}")));
        }
    }
    // The service is not lost: two starts of a cooperative OS leave it registered when the user wants it.
    if w.wanted == Some(true)
        && w.first_run_done
        && w.caps != Caps::NoAuto
        && w.location_ok
        && !w.ro
        && w.rec.state != State::Removing
        && matches!(w.reg, Reg::Absent | Reg::Registered)
        && !w.program_gone
        && !(hold && w.reg == Reg::Registered)
        // Allowed outcome (ADR 0056): a register that keeps failing after the old registration was removed (macOS, R03, and any
        // replace that had to remove first) leaves nothing. The hold says the service could not be registered, with the cause, and
        // Repair tries again.
        && !matches!(plan, Plan::Leave(Hold::RegisterFailed(_)))
    {
        let after = run_chain(&cooperative(w), &[Ev::Start, Ev::Wait, Ev::Start, Ev::Wait]);
        if after.reg == Reg::Absent {
            v.push((
                "the user's service was lost",
                format!("nothing is registered after two starts; plan {plan:?}"),
            ));
        }
    }
    // `pending()` is false in a state with no way forward: where a start and a long wait change nothing.
    if pending {
        let round = run_chain(w, &[Ev::Start, Ev::Wait]);
        if key(&round) == key(w) {
            v.push((
                "pending() is true in a state with no way forward",
                format!("plan {plan:?}"),
            ));
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------------
// The graph

fn bucket(s: Option<u32>) -> &'static str {
    match s {
        None => "-",
        Some(0..=120) => "soon",
        Some(_) => "late",
    }
}

/// The record as the state key sees it: the time of the register call is the wall clock, which differs on every run, so only
/// whether there is one counts.
fn normalized(rec: &Arc<Recorded>) -> Arc<Recorded> {
    match rec.registered_at {
        None | Some(1) => rec.clone(),
        Some(_) => {
            let mut r = Recorded::clone(rec);
            r.registered_at = Some(1);
            Arc::new(r)
        }
    }
}

/// A state of the graph as the exploration tells states apart: every field that matters, compared and hashed as it is (no
/// formatting, no allocation but the record's own strings), so two worlds are one state exactly when the fields agree. The
/// time of the register call is the wall clock, which differs on every run, so only whether there is one counts.
#[derive(PartialEq, Eq, Hash, Debug)]
struct StateKey {
    reg: Reg,
    owner: Ver,
    other_copy: bool,
    program_gone: bool,
    job: Job,
    stale: bool,
    start_in: &'static str,
    lingers: bool,
    cur: Ver,
    wanted: Option<bool>,
    first_run_done: bool,
    location_ok: bool,
    ro: bool,
    rec_damaged: bool,
    /// The version of a running job that is not the owner's.
    foreign_job: Option<Ver>,
    ghost: u8,
    mark_unreadable: bool,
    rare_used: bool,
    rec: Arc<Recorded>,
}

fn key(w: &World) -> StateKey {
    StateKey {
        reg: w.reg,
        owner: w.owner,
        other_copy: w.other_copy,
        program_gone: w.program_gone,
        job: w.job,
        stale: w.stale,
        start_in: bucket(w.start_in),
        lingers: w.linger > 0,
        cur: w.cur,
        wanted: w.wanted,
        first_run_done: w.first_run_done,
        location_ok: w.location_ok,
        ro: w.ro,
        rec_damaged: w.rec_damaged,
        foreign_job: if w.job == Job::Running && w.job_ver != w.owner {
            Some(w.job_ver)
        } else {
            None
        },
        ghost: w.ghost,
        mark_unreadable: w.mark_unreadable,
        rare_used: w.rare_used,
        rec: normalized(&w.rec),
    }
}

fn seeds(caps: Caps, resp: Resp) -> Vec<(&'static str, World)> {
    let base = World {
        caps,
        resp,
        reg: Reg::Absent,
        owner: Ver::V1,
        other_copy: false,
        program_gone: false,
        job: Job::Absent,
        stale: false,
        start_in: None,
        linger: 0,
        mark_unreadable: false,
        cur: Ver::V1,
        wanted: Some(true),
        first_run_done: false,
        location_ok: true,
        ro: false,
        rec_damaged: false,
        job_ver: Ver::V1,
        ghost: 0,
        rare_used: false,
        rec: Arc::default(),
    };
    let mut healthy = base.clone();
    healthy.reg = Reg::Registered;
    healthy.job = Job::Running;
    healthy.job_ver = Ver::V1;
    healthy.first_run_done = true;
    Arc::make_mut(&mut healthy.rec).registered_version = Some(Ver::V1.s().into());
    let mut old = healthy.clone();
    old.owner = Ver::V0;
    old.job_ver = Ver::V0;
    old.job = Job::Refused;
    old.stale = true;
    Arc::make_mut(&mut old.rec).registered_version = Some(Ver::V0.s().into());
    let mut unavailable = base.clone();
    unavailable.reg = Reg::Unavailable;
    vec![
        ("Fresh", base),
        ("Healthy", healthy),
        ("OldRefused", old),
        ("Unavailable", unavailable),
    ]
}

fn token(ev: Ev, fault: Fault, intr: Intr) -> String {
    let mut t = format!("{ev:?}");
    if fault != Fault::None {
        t += &format!("+{fault:?}");
    }
    if intr != Intr::None {
        t += &format!("@{intr:?}");
    }
    t
}

fn parse_token(t: &str) -> (Ev, Fault, Intr) {
    let (head, intr) = t.split_once('@').unwrap_or((t, ""));
    let (ev, fault) = head.split_once('+').unwrap_or((head, ""));
    let ev = *ALL_EVS
        .iter()
        .find(|e| format!("{e:?}") == ev)
        .unwrap_or_else(|| panic!("no event {ev}"));
    let fault = if fault.is_empty() {
        Fault::None
    } else {
        *FAULTS
            .iter()
            .find(|f| format!("{f:?}") == fault)
            .unwrap_or_else(|| panic!("no fault {fault}"))
    };
    let num = |p: &str| {
        intr[p.len()..]
            .trim_matches(|c| c == '(' || c == ')')
            .parse::<u32>()
            .unwrap()
    };
    let intr = if intr.is_empty() {
        Intr::None
    } else if intr.starts_with("CrashBefore") {
        Intr::CrashBefore(num("CrashBefore"))
    } else if intr.starts_with("CrashAfter") {
        Intr::CrashAfter(num("CrashAfter"))
    } else if intr.starts_with("Cancel") {
        Intr::Cancel(num("Cancel"))
    } else if intr.starts_with("WritesFail") {
        Intr::WritesFail(num("WritesFail"))
    } else {
        panic!("no interruption {intr}")
    };
    (ev, fault, intr)
}

/// The interruptions worth trying for an entry point whose fault-free run made these calls.
fn interruptions(ops: impl Iterator<Item = Op>) -> Vec<Intr> {
    let mut v = vec![Intr::Cancel(0), Intr::WritesFail(0)];
    let muts: Vec<Op> = ops.filter(|o| mutating(o.label)).collect();
    for o in &muts {
        v.extend([Intr::CrashBefore(o.k), Intr::CrashAfter(o.k)]);
    }
    // A cancel or a failing write between steps: after the first and after the last mutating call.
    for o in muts.first().into_iter().chain(muts.last()) {
        v.extend([Intr::Cancel(o.k), Intr::WritesFail(o.k)]);
    }
    v.dedup();
    v
}

#[derive(Default)]
struct Found {
    count: u64,
    /// The replay of the shortest path that showed it first, and the details.
    first: Option<(String, String)>,
}

#[derive(Default)]
struct Explored {
    states: usize,
    /// States before any rare event: the closure of the core alphabet.
    core: usize,
    /// States not expanded because of MODEL_DEPTH (0 when the run is a fixed point).
    depth_cut: usize,
    edges: u64,
    crash_edges: u64,
    by_event: BTreeMap<String, u64>,
    by_fault: BTreeMap<String, u64>,
    by_intr: BTreeMap<&'static str, u64>,
    kinds: BTreeMap<&'static str, Found>,
    finished: bool,
    /// The sum of the hashes of all states, so a change that explores other states shows even when their number is the same.
    fingerprint: u64,
    /// Heap allocations the exploring thread made (see `alloc_count`).
    allocs: u64,
}

struct Node {
    w: World,
    parent: Option<(usize, String)>,
    /// Steps from the seed along the shortest path.
    depth: u32,
    /// Steps since the path took its one rare event (0 before it).
    since_rare: u32,
}

/// A rare event is taken from core states up to this depth, and a path goes on this many steps after it (default run).
const RARE_FROM_DEPTH: u32 = 4;
const RARE_STEPS: u32 = 3;

fn path_of(nodes: &[Node], mut i: usize) -> Vec<String> {
    let mut v = Vec::new();
    while let Some((p, t)) = &nodes[i].parent {
        v.push(t.clone());
        i = *p;
    }
    v.reverse();
    v
}

/// The edge of the crash `intr` that the run of `fault` without an interruption handed out.
fn take_crash(
    crashes: &mut [(Fault, Vec<(Intr, Edge)>)],
    fault: Fault,
    intr: Intr,
) -> Option<Edge> {
    let (_, at) = crashes.iter_mut().find(|(f, _)| *f == fault)?;
    let i = at.iter().position(|(i, _)| *i == intr)?;
    Some(at.swap_remove(i).1)
}

fn explore_config(caps: Caps, resp: Resp) -> Explored {
    let before = crate::alloc_count::allocations();
    let mut ex = explore_inner(caps, resp);
    ex.allocs = crate::alloc_count::allocations() - before;
    ex
}

fn explore_inner(caps: Caps, resp: Resp) -> Explored {
    let mut ex = Explored::default();
    let mut nodes: Vec<Node> = Vec::new();
    let mut seen: HashMap<StateKey, usize> = HashMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut roots: Vec<&'static str> = Vec::new();
    for (name, mut w) in seeds(caps, resp) {
        normalize(&mut w);
        if seen.insert(key(&w), nodes.len()).is_none() {
            queue.push_back(nodes.len());
            nodes.push(Node {
                w,
                parent: None,
                depth: 0,
                since_rare: 0,
            });
            roots.push(name);
        }
    }
    let n_roots = roots.len();
    // The seed a node descends from is the first of its path; a replay starts from it by name.
    let root_of = |nodes: &[Node], mut i: usize| {
        while let Some((p, _)) = &nodes[i].parent {
            i = *p;
        }
        roots[i.min(n_roots - 1)]
    };
    let record =
        |ex: &mut Explored, nodes: &[Node], at: usize, tail: Option<&str>, viols: Vec<Viol>| {
            for (kind, why) in viols {
                let f = ex.kinds.entry(kind).or_default();
                f.count += 1;
                if f.first.is_none() {
                    let mut p = path_of(nodes, at);
                    if let Some(t) = tail {
                        p.push(t.to_string());
                    }
                    f.first = Some((
                        format!("{caps:?}/{resp:?}/{}|{}", root_of(nodes, at), p.join(";")),
                        why,
                    ));
                }
            }
        };
    ex.finished = true;
    // Counted by position in the alphabet: formatting a name per edge was an allocation or two of the hot loop.
    let mut ev_n = [0u64; ALL_EVS.len()];
    let mut fault_n = [0u64; FAULTS.len()];
    let mut popped = 0u64;
    while let Some(i) = queue.pop_front() {
        popped += 1;
        if popped.is_multiple_of(500) && std::env::var("MODEL_PROGRESS").is_ok() {
            eprintln!(
                "{caps:?}/{resp:?}: {popped} done, {} queued, {} states, {} edges",
                queue.len(),
                nodes.len(),
                ex.edges
            );
        }
        let w = nodes[i].w.clone();
        let sv = state_checks(&w);
        record(&mut ex, &nodes, i, None, sv);
        // MODEL_DEPTH=n (long run only): states deeper than n steps from a seed are checked but not expanded.
        if deep() && nodes[i].depth >= depth_cap() {
            ex.depth_cut += 1;
            continue;
        }
        if !deep() && nodes[i].since_rare >= RARE_STEPS {
            continue;
        }
        for (evi, &ev) in ALL_EVS.iter().enumerate() {
            if (is_rare(ev) && (w.rare_used || nodes[i].depth > RARE_FROM_DEPTH))
                || (ev == Ev::FirstRun && w.first_run_done)
            {
                continue;
            }
            let mut tries: Vec<(Fault, Intr)> = vec![(Fault::None, Intr::None)];
            // After a rare event only the plain edges are taken (and the interruptions of a start): the faults and the
            // interruptions of every entry point are covered from the core states.
            let light = w.rare_used && !deep();
            let mut crashes: Vec<(Fault, Vec<(Intr, Edge)>)> = Vec::new();
            let mut idx = 0;
            while idx < tries.len() {
                let (fault, intr) = tries[idx];
                let first = idx == 0;
                idx += 1;
                let e = if matches!(intr, Intr::CrashBefore(_) | Intr::CrashAfter(_)) {
                    // Handed out by the run of this fault without an interruption, which came first in `tries`; a crash that
                    // run did not reach (an error before the effect was left) is run for real.
                    take_crash(&mut crashes, fault, intr)
                        .unwrap_or_else(|| apply(&w, ev, fault, intr))
                } else if intr == Intr::None && !real_crashes() {
                    let (e, at) = apply_with_crashes(&w, ev, fault);
                    crashes.push((fault, at));
                    e
                } else {
                    apply(&w, ev, fault, intr)
                };
                ex.edges += 1;
                if e.crashed {
                    ex.crash_edges += 1;
                }
                ev_n[evi] += 1;
                if let Some(fi) = FAULTS.iter().position(|f| *f == fault) {
                    fault_n[fi] += 1;
                }
                let kind = match intr {
                    Intr::None => None,
                    Intr::CrashBefore(_) => Some("CrashBefore"),
                    Intr::CrashAfter(_) => Some("CrashAfter"),
                    Intr::Cancel(_) => Some("Cancel"),
                    Intr::WritesFail(_) => Some("WritesFail"),
                };
                if let Some(k) = kind {
                    *ex.by_intr.entry(k).or_default() += 1;
                }
                if !e.viols.is_empty() {
                    record(&mut ex, &nodes, i, Some(&token(ev, fault, intr)), e.viols);
                }
                if first && is_entry(ev) {
                    let has = |l: &str| ops_of(&e.trace).any(|o| o.label == l);
                    for f in FAULTS.iter().copied().filter(|_| !light) {
                        let applies = match f {
                            f if is_reg_fault(f) => has("register") && ev != Ev::Uninstall,
                            Fault::UnregFails => has("unregister"),
                            Fault::RemovalTimeout | Fault::LingeringJob => has("evict"),
                            Fault::SlowOs => has("register") || has("unregister"),
                            Fault::LockHeld => ops_of(&e.trace).any(|o| mutating(o.label)),
                            _ => false,
                        };
                        let caps_ok = match f {
                            Fault::RegNeedsElevation | Fault::RegDeclined => caps == Caps::NoAuto,
                            Fault::RegNeedsApproval => caps != Caps::NoAuto,
                            _ => true,
                        };
                        if applies && caps_ok {
                            tries.push((f, Intr::None));
                        }
                    }
                    if (!light || ev == Ev::Start) && !(ev == Ev::Repair && !deep()) {
                        for it in interruptions(ops_of(&e.trace)) {
                            tries.push((Fault::None, it));
                        }
                    }
                } else if deep()
                    && intr == Intr::None
                    && matches!(
                        fault,
                        Fault::RegOther | Fault::RemovalTimeout | Fault::RegNotAsWritten
                    )
                {
                    // A crash during a replace that already met a fault.
                    for o in ops_of(&e.trace).filter(|o| mutating(o.label)) {
                        tries.push((fault, Intr::CrashBefore(o.k)));
                        tries.push((fault, Intr::CrashAfter(o.k)));
                    }
                }
                let mut after = e.after;
                normalize(&mut after);
                let k = key(&after);
                if let std::collections::hash_map::Entry::Vacant(slot) = seen.entry(k) {
                    if nodes.len() >= STATES_CEILING {
                        ex.finished = false;
                        continue;
                    }
                    slot.insert(nodes.len());
                    queue.push_back(nodes.len());
                    let since_rare = if !after.rare_used {
                        0
                    } else {
                        nodes[i].since_rare + 1
                    };
                    let depth = nodes[i].depth + 1;
                    nodes.push(Node {
                        w: after,
                        parent: Some((i, token(ev, fault, intr))),
                        depth,
                        since_rare,
                    });
                }
            }
        }
    }
    for (e, n) in ALL_EVS.iter().zip(ev_n) {
        if n > 0 {
            ex.by_event.insert(format!("{e:?}"), n);
        }
    }
    for (f, n) in FAULTS.iter().zip(fault_n) {
        if n > 0 {
            ex.by_fault.insert(format!("{f:?}"), n);
        }
    }
    ex.fingerprint = seen
        .keys()
        .map(|k| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(k, &mut h);
            std::hash::Hasher::finish(&h)
        })
        .fold(0u64, u64::wrapping_add);
    ex.states = nodes.len();
    ex.core = nodes.iter().filter(|n| !n.w.rare_used).count();
    ex
}

/// The configurations: the first [`DEFAULT_CONFIGS`] are the default run, the rest only the long one.
const CONFIGS: [(Caps, Resp); 11] = [
    (Caps::Mac, Resp::Fast),
    (Caps::Mac, Resp::Never),
    (Caps::Mac, Resp::RefusedAlways),
    (Caps::Linux, Resp::Never),
    (Caps::NoAuto, Resp::Fast),
    // The judged wait (ADR 0056 amendment), with the waits shrunk ([`watched_timings`]): a job that restarts in a loop ends the
    // wait early, a late start is waited for past the fixed wait, one that never comes ends at the ceiling of looks.
    (Caps::Mac, Resp::Looping),
    (Caps::Mac, Resp::WatchedLate),
    (Caps::Mac, Resp::WatchedNever),
    // The long run adds the slow starts and the other mechanism with a fast OS.
    (Caps::Mac, Resp::Slow),
    (Caps::Mac, Resp::Late),
    (Caps::Linux, Resp::Fast),
];

/// How many of [`CONFIGS`] the default run explores (each is a test of its own below).
const DEFAULT_CONFIGS: usize = 8;

/// The configurations of the default run (the first [`DEFAULT_CONFIGS`]) or the long one (all of [`CONFIGS`]). A named one
/// (`MODEL_CONFIG=Mac/Slow`) is looked for among all of them, so a configuration of the long run can be run alone with the
/// default bounds.
fn configs_for(deep: bool, only: Option<&str>) -> Vec<(Caps, Resp)> {
    let all: &[(Caps, Resp)] = if deep || only.is_some() {
        &CONFIGS
    } else {
        &CONFIGS[..DEFAULT_CONFIGS]
    };
    all.iter()
        .copied()
        .filter(|(c, r)| only.is_none_or(|o| o == format!("{c:?}/{r:?}")))
        .collect()
}

/// What two edges must agree on to be the same step.
fn same_edge(a: &Edge, b: &Edge) -> bool {
    key(&a.after) == key(&b.after)
        && a.crashed == b.crashed
        && a.trace == b.trace
        && a.viols == b.viols
}

/// The edge of a crash taken from a run that goes on is the edge a real panic unwound through the manager leaves: the same
/// state, the same calls, the same trace and the same findings, for every crash of every entry point and fault, over the
/// first states of three configurations.
#[test]
fn a_crash_taken_from_a_running_run_is_the_edge_a_real_panic_leaves() {
    silence_crashes();
    let mut compared = 0u32;
    for (caps, resp) in [
        (Caps::Mac, Resp::Never),
        (Caps::Linux, Resp::Fast),
        (Caps::NoAuto, Resp::Fast),
    ] {
        let mut seen: std::collections::HashSet<StateKey> = std::collections::HashSet::new();
        let mut queue: VecDeque<World> = VecDeque::new();
        for (_, mut w) in seeds(caps, resp) {
            normalize(&mut w);
            if seen.insert(key(&w)) {
                queue.push_back(w);
            }
        }
        let mut taken = 0;
        while let Some(w) = queue.pop_front() {
            if taken >= 60 {
                break;
            }
            taken += 1;
            for &ev in &ALL_EVS {
                if (ev == Ev::FirstRun && w.first_run_done) || (is_rare(ev) && w.rare_used) {
                    continue;
                }
                let mut faults = vec![Fault::None];
                if is_entry(ev) {
                    faults.extend(faults_of(caps));
                }
                for fault in faults {
                    let (plain, crashes) = apply_with_crashes(&w, ev, fault);
                    assert!(
                        same_edge(&plain, &apply(&w, ev, fault, Intr::None)),
                        "{caps:?}/{resp:?} {ev:?}+{fault:?}: the run that hands out crashes is not the plain run"
                    );
                    for (intr, taken_edge) in crashes {
                        let real = apply(&w, ev, fault, intr);
                        assert!(
                            same_edge(&taken_edge, &real),
                            "{caps:?}/{resp:?} {ev:?}+{fault:?}@{intr:?} from {w:?}\n taken {:?} {:?}\n real  {:?} {:?}",
                            key(&taken_edge.after),
                            taken_edge.viols,
                            key(&real.after),
                            real.viols
                        );
                        assert!(real.crashed, "a real crash at {intr:?} crashed");
                        compared += 1;
                    }
                    if fault == Fault::None && seen.insert(key(&plain.after)) {
                        queue.push_back(plain.after);
                    }
                }
            }
        }
    }
    assert!(compared > 500, "only {compared} crashes compared");
}

#[test]
fn a_named_configuration_of_the_long_run_runs_alone_with_the_default_bounds() {
    assert_eq!(
        configs_for(false, Some("Mac/Slow")),
        vec![(Caps::Mac, Resp::Slow)]
    );
    assert_eq!(
        configs_for(false, None).len(),
        DEFAULT_CONFIGS,
        "the default run is the first DEFAULT_CONFIGS"
    );
    assert_eq!(configs_for(true, None).len(), CONFIGS.len());
    assert!(configs_for(false, Some("Mac/Nonsense")).is_empty());
}

/// The positive control of the judged configurations: the watch really decides how the repair ends there (the exploration
/// alone would also pass if the judge were never asked). A fresh install over each OS behaviour.
#[test]
fn a_judged_wait_ends_as_the_judge_says_in_each_watched_configuration() {
    silence_crashes();
    let install = |resp: Resp| {
        let mut w = seeds(Caps::Mac, resp)
            .into_iter()
            .find(|(n, _)| *n == "Fresh")
            .expect("a fresh seed")
            .1;
        normalize(&mut w);
        let rec = RecordFile::memory();
        let sim = new_sim(&w, Fault::None, Intr::None, &rec);
        let mut m = build(&sim, rec, None);
        let out = m.install(&AtomicBool::new(false));
        let looks = lk(&sim).looks;
        (out, looks)
    };
    // 300 s is past the fixed 120 s wait: the judge waits for the job and the service answers.
    let (late, looks) = install(Resp::WatchedLate);
    assert!(matches!(late, Ok(RepairOutcome::Recovered)), "{late:?}");
    assert!(
        looks >= 8,
        "waited by looking past the fixed 12 s (4 looks), {looks} looks"
    );
    // A job that restarts in a loop fails the wait early with what was seen.
    let (looping, looks) = install(Resp::Looping);
    let Err(ServiceError::Failed(why)) = looping else {
        panic!("{looping:?}")
    };
    assert!(
        why.contains("keeps exiting") && why.contains("last exit code 3"),
        "{why}"
    );
    assert!(looks <= 6, "failed early, after {looks} looks");
    // A job that only shows "not running" spends no patience: the judge's ceiling of polls ends the wait.
    let (never, looks) = install(Resp::WatchedNever);
    let Err(ServiceError::Failed(why)) = never else {
        panic!("{never:?}")
    };
    assert!(
        why.contains("never started after 15 checks"),
        "nothing ever ran: {why}"
    );
    assert!(
        (15..=WATCH_BACKSTOP_LOOKS).contains(&looks),
        "ended at the watch's ceiling, after {looks} looks"
    );
    // Without a watch the same OS (a job that never comes) is the old fixed wait: registered, not failed.
    let (unjudged, looks) = install(Resp::Never);
    assert!(
        matches!(unjudged, Ok(RepairOutcome::Registered)),
        "{unjudged:?}"
    );
    assert_eq!(looks, 0, "no watch: the job is not observed");
}

/// Bugs the model found in the code under test. Each entry fails the test when it is **not** found any more (it was fixed:
/// delete the line), and anything not listed fails the test when found. The text says where it points.
const KNOWN_FAILING: &[(&str, &str)] = &[];

/// What one default-run configuration must at least reach, so a shrunken alphabet or a cut-short exploration fails its test. The
/// floors sit at about 70 % of the measured size (2026-11-07: Mac/Fast 77250 states and 2.23 M edges, Mac/Never 85548 and 2.35 M,
/// Mac/RefusedAlways 63282 and 1.87 M, Linux/Never 76572 and 2.36 M, NoAuto/Fast 27632 and 0.64 M).
struct Floor {
    states: usize,
    edges: u64,
}

/// The faults a configuration can inject (measured 2026-11-07): `NoAuto` has no automatic registration, so it has the elevation
/// faults and never `RegNeedsApproval`; the others have `RegNeedsApproval` and not the elevation faults.
fn faults_of(caps: Caps) -> Vec<Fault> {
    FAULTS
        .iter()
        .copied()
        .filter(|f| match f {
            Fault::RegNeedsElevation | Fault::RegDeclined => caps == Caps::NoAuto,
            Fault::RegNeedsApproval => caps != Caps::NoAuto,
            _ => true,
        })
        .collect()
}

/// Heap allocations one edge of the exploration may cost, measured by `alloc_count` on the exploring thread (about 49 to 57
/// now; 94 to 136 when every OS call formatted a trace line and every snapshot cloned the trace). The Windows heap costs some
/// 200 ns an allocation and release, and the default configurations are 0.6 to 2.4 million edges each.
const ALLOCS_PER_EDGE_BUDGET: f64 = 63.0;

/// One test per default-run configuration: each explores its own state space to a fixed point on its own thread of the test
/// runner, so none is near the 60 s limit of the ci profile (the first five together in one test were 58 s).
macro_rules! model_test {
    ($name:ident, $caps:ident, $resp:ident, $states:expr, $edges:expr) => {
        #[test]
        fn $name() {
            check_model(
                &[(Caps::$caps, Resp::$resp)],
                Some(Floor {
                    states: $states,
                    edges: $edges,
                }),
            );
        }
    };
}

model_test!(
    the_crash_model_mac_fast_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    Fast,
    54_000,
    1_500_000
);
model_test!(
    the_crash_model_mac_never_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    Never,
    60_000,
    1_600_000
);
model_test!(
    the_crash_model_mac_refused_always_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    RefusedAlways,
    44_000,
    1_300_000
);
model_test!(
    the_crash_model_linux_never_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Linux,
    Never,
    54_000,
    1_600_000
);
model_test!(
    the_crash_model_noauto_fast_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    NoAuto,
    Fast,
    19_000,
    450_000
);

// The judged wait (ADR 0056 amendment), with the waits shrunk ([`watched_timings`]) so that all three are in the default run: the
// floors sit at about 70 % of the measured size (2026-11-08: Looping and WatchedNever 86645 states and 2.38 M edges, WatchedLate
// 75658 and 2.24 M).
model_test!(
    the_crash_model_mac_looping_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    Looping,
    60_000,
    1_660_000
);
model_test!(
    the_crash_model_mac_watched_late_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    WatchedLate,
    53_000,
    1_570_000
);
model_test!(
    the_crash_model_mac_watched_never_reaches_a_fixed_point_and_the_invariants_hold_in_every_state,
    Mac,
    WatchedNever,
    60_000,
    1_660_000
);

/// The long run (`MODEL_DEEP=1 MODEL_CONFIG=Mac/Slow`): the named configuration of any of [`CONFIGS`], with the long bounds and no
/// floors. Without `MODEL_CONFIG` it says so and explores nothing: the default configurations are the tests above.
#[test]
fn the_crash_model_explores_the_configuration_named_in_the_environment() {
    let Ok(named) = std::env::var("MODEL_CONFIG") else {
        eprintln!(
            "MODEL_CONFIG is not set: no long-run configuration requested (e.g. MODEL_DEEP=1 MODEL_CONFIG=Mac/Slow)"
        );
        return;
    };
    let configs = configs_for(deep(), Some(&named));
    assert!(
        !configs.is_empty(),
        "MODEL_CONFIG={named} names no configuration (Caps/Resp, e.g. Mac/Slow)"
    );
    check_model(&configs, None);
}

fn check_model(configs: &[(Caps, Resp)], floor: Option<Floor>) {
    silence_crashes();
    let started = Instant::now();
    let results: Vec<(Caps, Resp, Explored)> = std::thread::scope(|sc| {
        let handles: Vec<_> = configs
            .iter()
            .map(|&(c, r)| sc.spawn(move || (c, r, explore_config(c, r))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("an exploration thread panicked"))
            .collect()
    });
    let mut states = 0;
    let mut edges = 0;
    let mut crash_edges = 0;
    let mut by_event: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_fault: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_intr: BTreeMap<&str, u64> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, (u64, String, String)> = BTreeMap::new();
    let mut out = String::new();
    for (c, r, ex) in &results {
        out += &format!(
            "{c:?}/{r:?}: {} states ({} core), {} edges ({} crashes), {} not expanded (depth bound), state fingerprint {:016x}{}, {:.1} allocations per edge\n",
            ex.states,
            ex.core,
            ex.edges,
            ex.crash_edges,
            ex.depth_cut,
            ex.fingerprint,
            if ex.finished { "" } else { "  NOT FINISHED" },
            ex.allocs as f64 / ex.edges.max(1) as f64
        );
        if floor.is_some() {
            let per_edge = ex.allocs as f64 / ex.edges.max(1) as f64;
            assert!(
                per_edge <= ALLOCS_PER_EDGE_BUDGET,
                "{c:?}/{r:?}: {per_edge:.1} allocations per edge (budget {ALLOCS_PER_EDGE_BUDGET}): the hot loop of the exploration \
                 must not format text or clone strings per edge (Windows's heap made 130 per edge a timeout)\n{out}"
            );
        }
        assert!(
            ex.finished,
            "{c:?}/{r:?}: the state ceiling was reached, so the key misses a bucket\n{out}"
        );
        states += ex.states;
        edges += ex.edges;
        crash_edges += ex.crash_edges;
        for (k, n) in &ex.by_event {
            *by_event.entry(k.clone()).or_default() += n;
        }
        for (k, n) in &ex.by_fault {
            *by_fault.entry(k.clone()).or_default() += n;
        }
        for (k, n) in &ex.by_intr {
            *by_intr.entry(k).or_default() += n;
        }
        for (k, f) in &ex.kinds {
            let e = kinds
                .entry(k)
                .or_insert_with(|| (0, String::new(), String::new()));
            e.0 += f.count;
            if e.1.is_empty()
                && let Some((replay, why)) = &f.first
            {
                e.1 = replay.clone();
                e.2 = why.clone();
            }
        }
    }
    out += &format!(
        "total: {states} states, {edges} edges ({crash_edges} crash edges, {} real panics) in {:?}\nedges by event {by_event:?}\nedges by fault {by_fault:?}\nedges by interruption {by_intr:?}\n",
        REAL_PANICS.load(Ordering::Relaxed),
        started.elapsed()
    );
    for (k, (n, replay, why)) in &kinds {
        out += &format!("- {k}: {n} edges or states\n    MODEL_REPLAY='{replay}'\n    {why}\n");
    }
    println!("{out}");
    // The alphabet is not silently trimmed: in a default-run configuration every event and every fault it can inject was taken,
    // and the floors hold. A long-run configuration (no floor) is judged on the invariants only.
    if let Some(floor) = &floor {
        for e in ALL_EVS {
            assert!(
                by_event.get(&format!("{e:?}")).copied().unwrap_or(0) > 0,
                "event {e:?} was never taken\n{out}"
            );
        }
        for &(c, _) in configs {
            for f in faults_of(c) {
                assert!(
                    by_fault.get(&format!("{f:?}")).copied().unwrap_or(0) > 0,
                    "fault {f:?} was never injected\n{out}"
                );
            }
        }
        assert!(
            states >= floor.states,
            "only {states} states (floor {})\n{out}",
            floor.states
        );
        assert!(
            edges >= floor.edges,
            "only {edges} edges (floor {})\n{out}",
            floor.edges
        );
    }
    for i in ["CrashBefore", "CrashAfter", "Cancel", "WritesFail"] {
        assert!(
            by_intr.get(i).copied().unwrap_or(0) > 0,
            "interruption {i} was never taken\n{out}"
        );
    }
    let known: Vec<&str> = KNOWN_FAILING.iter().map(|(k, _)| *k).collect();
    let new: Vec<&&str> = kinds.keys().filter(|k| !known.contains(k)).collect();
    let fixed: Vec<&&str> = known.iter().filter(|k| !kinds.contains_key(*k)).collect();
    assert!(new.is_empty(), "new violations {new:?}\n{out}");
    assert!(
        fixed.is_empty(),
        "known-failing entries that no longer fail (delete them): {fixed:?}\n{out}"
    );
}

/// Reports, in the job log, what the pieces of one edge cost on this machine, so a platform on which the exploration is far
/// slower than elsewhere (the Windows runner: 20 to 35 times a Mac per edge, CI run 7642) can be told apart by measurement and
/// not by guessing: the per-call cost of the system calls an edge makes (the clocks the manager reads several times, an
/// environment lookup, a lock, a small allocation, an unwind) and the cost of whole edges. It asserts nothing about time
/// (ER-12); it asserts only that it measured something, so a broken probe cannot read as a result (ER-33).
#[test]
fn the_cost_of_the_pieces_of_one_edge_is_reported() {
    silence_crashes();
    fn per_call(n: u32, mut f: impl FnMut()) -> f64 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        t.elapsed().as_nanos() as f64 / f64::from(n)
    }
    let m = Mutex::new(0u32);
    let sink = std::sync::atomic::AtomicU64::new(0);
    let mut out = String::from("cost of the pieces of one edge (ns per call)\n");
    let rows: Vec<(&str, f64)> = vec![
        (
            "Instant::now",
            per_call(20_000, || {
                sink.fetch_add(
                    Instant::now().elapsed().as_nanos() as u64,
                    Ordering::Relaxed,
                );
            }),
        ),
        (
            "SystemTime::now",
            per_call(20_000, || {
                sink.fetch_add(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.subsec_nanos()) as u64,
                    Ordering::Relaxed,
                );
            }),
        ),
        (
            "env::var_os (absent name)",
            per_call(20_000, || {
                sink.fetch_add(
                    u64::from(std::env::var_os("MODEL_COST_PROBE_ABSENT").is_some()),
                    Ordering::Relaxed,
                );
            }),
        ),
        (
            "Mutex lock and unlock",
            per_call(20_000, || {
                *lk_u32(&m) += 1;
            }),
        ),
        (
            "Box::new of 64 bytes",
            per_call(20_000, || {
                sink.fetch_add(u64::from(Box::new([1u8; 64])[3]), Ordering::Relaxed);
            }),
        ),
        (
            "catch_unwind without a panic",
            per_call(20_000, || {
                let _ = std::hint::black_box(catch_unwind(|| std::hint::black_box(1u32)));
            }),
        ),
        (
            "catch_unwind with a Crash panic",
            per_call(300, || {
                let _ = catch_unwind(|| panic_any(Crash));
            }),
        ),
    ];
    for (name, ns) in &rows {
        out += &format!("  {name}: {ns:.0}\n");
    }
    // Whole edges: every entry point from the seeds of one configuration, repeated.
    let (caps, resp) = (Caps::NoAuto, Resp::Fast);
    let mut edges = 0u32;
    let t = Instant::now();
    for _ in 0..20 {
        for (_, w) in seeds(caps, resp) {
            for &ev in ALL_EVS.iter().filter(|e| is_entry(**e)) {
                let e = apply(&w, ev, Fault::None, Intr::None);
                edges += 1;
                sink.fetch_add(u64::from(e.crashed), Ordering::Relaxed);
            }
        }
    }
    let whole = t.elapsed().as_nanos() as f64 / f64::from(edges);
    let t = Instant::now();
    for _ in 0..20 {
        for (_, w) in seeds(caps, resp) {
            for &ev in ALL_EVS.iter().filter(|e| is_entry(**e)) {
                let (e, c) = apply_with_crashes(&w, ev, Fault::None);
                sink.fetch_add(u64::from(e.crashed) + c.len() as u64, Ordering::Relaxed);
            }
        }
    }
    let with_crashes = t.elapsed().as_nanos() as f64 / f64::from(edges);
    out += &format!(
        "  whole edge, {edges} entry-point edges of {caps:?}/{resp:?}: {whole:.0}\n  the same with crash snapshots: {with_crashes:.0}\n"
    );
    println!("{out}");
    assert!(
        edges > 0 && whole > 0.0 && rows.iter().all(|(_, ns)| *ns > 0.0),
        "the probe measured nothing\n{out}"
    );
}

fn lk_u32(m: &Mutex<u32>) -> std::sync::MutexGuard<'_, u32> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Replays one path printed by a failure: `MODEL_REPLAY='Mac/Fast/Healthy|Start;Install+RegOther@CrashAfter(2)'` prints the
/// OS calls of every step, the record and the state invariants of the end.
#[test]
fn replay_one_path_from_the_environment() {
    let Ok(spec) = std::env::var("MODEL_REPLAY") else {
        return;
    };
    silence_crashes();
    let (cfg, path) = spec
        .split_once('|')
        .expect("MODEL_REPLAY is config|step;step");
    let parts: Vec<&str> = cfg.split('/').collect();
    let (caps, resp) = CONFIGS
        .iter()
        .copied()
        .find(|(c, r)| format!("{c:?}") == parts[0] && format!("{r:?}") == parts[1])
        .expect("unknown config");
    let mut w = seeds(caps, resp)
        .into_iter()
        .find(|(n, _)| *n == parts[2])
        .expect("unknown seed")
        .1;
    normalize(&mut w);
    println!("start {w:#?}");
    for t in path.split(';').filter(|t| !t.is_empty()) {
        let (ev, fault, intr) = parse_token(t);
        let e = apply(&w, ev, fault, intr);
        println!("{t} -> crashed {}", e.crashed);
        for l in &e.trace {
            println!("{l}");
        }
        for (k, why) in &e.viols {
            println!("  VIOLATION {k}: {why}");
        }
        w = e.after;
        normalize(&mut w);
        println!(
            "  now reg {:?} owner {:?} job {:?} start_in {:?}\n  record {:?}",
            w.reg, w.owner, w.job, w.start_in, w.rec
        );
    }
    let (plan, pending, supervises) = observe(&w);
    println!("plan {plan:?} pending {pending} supervises {supervises}");
    for (k, why) in state_checks(&w) {
        println!("  STATE VIOLATION {k}: {why}");
    }
    let e = apply(&cooperative(&w), Ev::Repair, Fault::None, Intr::None);
    println!("probe: Repair on a cooperative OS");
    for l in &e.trace {
        println!("{l}");
    }
    println!(
        "  now reg {:?} job {:?} record {:?}",
        e.after.reg, e.after.job, e.after.rec
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// The lock, with a real process

const LOCK_CHILD: &str = "MODEL_LOCK_CHILD";

/// Runs in the child: the test binary again, with the lock file in the environment. It takes the lock, says so on stdout and
/// holds it until it is killed (or, as a safety net for a failed run, two minutes).
#[test]
fn lock_holder_child() {
    use paddock_proc::{Claim, InstanceLock};
    use std::io::Write;
    let Ok(path) = std::env::var(LOCK_CHILD) else {
        return;
    };
    let lock = match InstanceLock::claim(std::path::Path::new(&path)) {
        Ok(Claim::Acquired(l)) => l,
        other => panic!("the child could not take the lock: {:?}", other.map(|_| ())),
    };
    println!("LOCK-HELD");
    std::io::stdout().flush().expect("stdout");
    std::thread::sleep(Duration::from_secs(120));
    drop(lock);
}

/// A process that holds the repair lock and is killed frees it: the OS drops the lock with its holder (rule 53), so there is
/// no stale lock and nothing to take over by age. The test waits on the child's own line and on its exit, never on a sleep.
#[test]
fn a_lock_held_by_a_killed_process_is_free_again() {
    use paddock_proc::{Claim, InstanceLock};
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    if std::env::var(LOCK_CHILD).is_ok() {
        return;
    }
    /// Kills and reaps the child whatever the test does, so no process is left behind.
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let record = RecordFile::new(dir.path(), LABEL);
    // The lock file of the record, as the record names it.
    let path = dir.path().join(format!("service-{LABEL}.lock"));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "model::lock_holder_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LOCK_CHILD, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut child = Reap(child);
    let held = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|l| l.contains("LOCK-HELD"));
    assert!(held, "the child ended without taking the lock");
    assert!(
        matches!(InstanceLock::claim(&path), Ok(Claim::Held)),
        "a lock held by a live process must read as held"
    );
    assert!(
        record.lock().is_none(),
        "the repair lock of the record must be busy while the other process lives"
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    match InstanceLock::claim(&path) {
        Ok(Claim::Acquired(_)) => {}
        other => panic!(
            "the lock of a killed process must be free, got {:?}",
            other.map(|_| ())
        ),
    }
    assert!(
        record.lock().is_some(),
        "the repair lock must be free after its holder was killed"
    );
}
