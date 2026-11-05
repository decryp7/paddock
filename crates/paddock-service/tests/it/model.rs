//! The model test of service repair (ADR 0056 section 6): the real [`Manager`] over a simulated OS, every sequence of entry
//! points to a bounded depth, a crash or quit at every OS call that matters, and the invariants that no review round
//! should have to find again by hand. The OS is a struct with a virtual clock; the exploration is exhaustive and ordered,
//! so there is no randomness and a failure prints the sequence that replays it.

use paddock_service::backend::{
    Backend, Capabilities, DisabledMark, Inspection, Job, LoginItemFacts, Mechanism, Registration,
    UnregisterError,
};
use paddock_service::port::{AppServiceStatus, RegisterError};
use paddock_service::{Manager, ServiceError};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

const LABEL: &str = "net.model.agent";
const V1: &str = "1.1.0";
const V2: &str = "1.2.0";

struct Crash;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scenario {
    /// Comes up 2 s after it is registered.
    Fast,
    /// Comes up 50 s after (inside the 120 s wait).
    Slow,
    /// Comes up 300 s after (after the wait, still healthy).
    Late,
    /// Never comes up.
    Never,
    /// The old job is refused until it is removed; a new registration then comes up fast.
    RefusedUntilRemoved,
    /// Refused whatever is registered.
    RefusedAlways,
    /// The user switched the item off.
    Off,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Caps {
    Mac,
    Linux,
    NoAuto,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Initial {
    Fresh,
    Healthy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Entry {
    Start,
    FirstRun,
    Install,
    Uninstall,
    Resume,
    Update,
}

const ENTRIES: [Entry; 6] = [
    Entry::Start,
    Entry::FirstRun,
    Entry::Install,
    Entry::Uninstall,
    Entry::Resume,
    Entry::Update,
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Reg {
    Absent,
    Registered,
    SwitchedOff,
}

struct World {
    scenario: Scenario,
    caps: Caps,
    reg: Reg,
    reg_version: String,
    job: Job,
    stale: bool,
    start_at: Option<Duration>,
    clock: Duration,
    ops: u32,
    crash_at: Option<u32>,
    labels: Vec<&'static str>,
    trace: Vec<String>,
    registers: BTreeMap<String, u32>,
    attended: bool,
    unattended: BTreeMap<String, u32>,
    unregisters: u32,
    wanted: bool,
    first_run_done: bool,
    prompts: u32,
    register_while_off: u32,
}

type Sim = Arc<Mutex<World>>;

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

struct SimBackend {
    w: Sim,
    version: String,
}

fn settle_time(w: &mut World) {
    if let (Reg::Registered, Job::NotRunning, Some(at)) = (w.reg, w.job, w.start_at)
        && w.clock >= at
        && !w.stale
    {
        w.job = Job::Running;
    }
}

impl SimBackend {
    fn tick(&self, label: &'static str) {
        let mut w = self.w.lock().unwrap();
        w.ops += 1;
        w.labels.push(label);
        let line = format!("  {label}: reg {:?} job {:?} clock {:?}", w.reg, w.job, w.clock);
        w.trace.push(line);
        if w.crash_at == Some(w.ops) {
            drop(w);
            panic_any(Crash);
        }
    }

    fn look(&self) -> Inspection {
        let mut w = self.w.lock().unwrap();
        settle_time(&mut w);
        let registration = match w.reg {
            Reg::Absent => Registration::Absent,
            Reg::Registered => Registration::Registered,
            Reg::SwitchedOff => Registration::SwitchedOff,
        };
        let mut i = Inspection::absent();
        i.registration = registration;
        i.job = w.job;
        i.program_exists = Some(true);
        i.points_here = Some(true);
        i.definition_current = Some(w.reg == Reg::Registered && w.reg_version == self.version);
        if caps_of(w.caps).login_items {
            i.login_item = Some(LoginItemFacts {
                status: match w.reg {
                    Reg::Registered => AppServiceStatus::Enabled,
                    Reg::SwitchedOff => AppServiceStatus::RequiresApproval,
                    Reg::Absent => AppServiceStatus::NotRegistered,
                },
                mark: DisabledMark::NotListed,
            });
        }
        i
    }
}

impl Backend for SimBackend {
    fn mechanism(&self) -> Mechanism {
        Mechanism::LaunchdBundled
    }
    fn capabilities(&self) -> Capabilities {
        caps_of(self.w.lock().unwrap().caps)
    }
    fn inspect(&mut self) -> Inspection {
        self.tick("inspect");
        self.look()
    }
    fn register(&mut self) -> Result<Inspection, RegisterError> {
        self.tick("register");
        {
            let mut w = self.w.lock().unwrap();
            if w.reg == Reg::SwitchedOff {
                w.register_while_off += 1;
                return Err(RegisterError::NeedsApproval);
            }
            if !caps_of(w.caps).automatic {
                w.prompts += 1;
            }
            *w.registers.entry(self.version.clone()).or_default() += 1;
            if !w.attended {
                *w.unattended.entry(self.version.clone()).or_default() += 1;
            }
            w.reg = Reg::Registered;
            w.reg_version = self.version.clone();
            let lingering_old = w.job != Job::Absent && w.stale;
            let delay = match w.scenario {
                Scenario::Fast | Scenario::RefusedUntilRemoved => Some(2),
                Scenario::Slow => Some(50),
                Scenario::Late => Some(300),
                Scenario::Never | Scenario::RefusedAlways | Scenario::Off => None,
            };
            if w.scenario == Scenario::RefusedAlways || lingering_old {
                w.job = Job::Refused;
                w.stale = true;
                w.start_at = None;
            } else if w.job != Job::Running {
                w.job = Job::NotRunning;
                w.start_at = delay.map(|d| w.clock + Duration::from_secs(d));
            }
        }
        Ok(self.look())
    }
    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        self.tick("unregister");
        {
            let mut w = self.w.lock().unwrap();
            w.unregisters += 1;
            if w.reg == Reg::Registered {
                w.reg = Reg::Absent;
            }
        }
        Ok(self.look())
    }
    fn evict(&mut self) -> Result<(), String> {
        self.tick("evict");
        let mut w = self.w.lock().unwrap();
        if w.scenario != Scenario::RefusedAlways {
            w.job = Job::Absent;
            w.stale = false;
        }
        Ok(())
    }
    fn job_gone(&mut self) -> bool {
        self.w.lock().unwrap().job == Job::Absent
    }
    fn pause(&mut self, how_long: Duration) {
        self.w.lock().unwrap().clock += how_long;
    }
    fn open_login_items(&mut self) -> bool {
        self.w.lock().unwrap().prompts += 1;
        true
    }
}

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

struct Run {
    sim: Sim,
    #[allow(dead_code)]
    dir: std::path::PathBuf,
    version: &'static str,
    wanted: bool,
    crashes: u32,
    notes: Vec<String>,
    record: paddock_service::record::RecordFile,
}

fn new_world(scenario: Scenario, caps: Caps, initial: Initial) -> World {
    let old = matches!(scenario, Scenario::RefusedUntilRemoved | Scenario::RefusedAlways);
    let (reg, job, stale) = match (scenario, initial) {
        (Scenario::Off, _) => (Reg::SwitchedOff, Job::Absent, false),
        (_, Initial::Healthy) if old => (Reg::Registered, Job::Refused, true),
        (_, Initial::Healthy) => (Reg::Registered, Job::Running, false),
        (_, Initial::Fresh) => (Reg::Absent, Job::Absent, false),
    };
    World {
        scenario,
        caps,
        reg,
        reg_version: if old { "1.0.0".into() } else { V1.into() },
        job,
        stale,
        start_at: None,
        clock: Duration::ZERO,
        ops: 0,
        crash_at: None,
        labels: Vec::new(),
        trace: Vec::new(),
        registers: BTreeMap::new(),
        attended: false,
        unattended: BTreeMap::new(),
        unregisters: 0,
        wanted: false,
        first_run_done: false,
        prompts: 0,
        register_while_off: 0,
    }
}

fn start_run(root: &std::path::Path, n: usize, scenario: Scenario, caps: Caps, initial: Initial) -> Run {
    let dir = root.join(format!("r{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    let old = matches!(scenario, Scenario::RefusedUntilRemoved | Scenario::RefusedAlways);
    let record = paddock_service::record::RecordFile::memory();
    if initial == Initial::Healthy && scenario != Scenario::Off {
        let v = if old { "1.0.0" } else { V1 };
        record.save(v, None).unwrap();
    }
    let sim = Arc::new(Mutex::new(new_world(scenario, caps, initial)));
    sim.lock().unwrap().first_run_done = initial == Initial::Healthy;
    Run {
        sim,
        dir,
        version: V1,
        wanted: initial == Initial::Healthy && scenario != Scenario::Off,
        crashes: 0,
        notes: Vec::new(),
        record,
    }
}

fn manager(run: &Run, timings: Option<paddock_service::Timings>) -> Manager {
    let backend = SimBackend {
        w: run.sim.clone(),
        version: run.version.to_string(),
    };
    let answers = run.sim.clone();
    let (w1, w2) = (run.sim.clone(), run.sim.clone());
    let mut m = Manager::new("Model", Box::new(backend), &run.dir, LABEL, run.version, move || {
        let mut w = answers.lock().unwrap();
        settle_time(&mut w);
        w.job == Job::Running
    });
    m = m
        .with_record(run.record.reopen())
        .with_app_state(
            move || Some(w1.lock().unwrap().wanted),
            move || w2.lock().unwrap().first_run_done,
            || true,
        );
    if let Some(t) = timings {
        m = m.with_timings(t);
    }
    m
}

struct Snap {
    registers: u32,
    unregisters: u32,
    prompts: u32,
    off_registers: u32,
}

fn snap(run: &Run) -> Snap {
    let w = run.sim.lock().unwrap();
    Snap {
        registers: w.registers.values().sum(),
        unregisters: w.unregisters,
        prompts: w.prompts,
        off_registers: w.register_while_off,
    }
}

/// Runs one entry point; `Err` when the process crashed in it.
fn perform(run: &mut Run, e: Entry, crash_at: Option<u32>) -> Result<(), ()> {
    if e == Entry::Update {
        run.version = V2;
    }
    {
        let mut w = run.sim.lock().unwrap();
        w.ops = 0;
        w.labels.clear();
        w.crash_at = crash_at;
        w.attended = matches!(e, Entry::Install | Entry::FirstRun);
    }
    match e {
        Entry::Install | Entry::FirstRun => run.wanted = true,
        Entry::Uninstall => run.wanted = false,
        _ => {}
    }
    run.sim.lock().unwrap().wanted = run.wanted;
    let before = snap(run);
    let (healthy_at_entry, soon_up_at_entry) = {
        let mut w = run.sim.lock().unwrap();
        settle_time(&mut w);
        let mine = w.reg_version == run.version && w.reg == Reg::Registered;
        (
            mine && w.job == Job::Running,
            mine && w.job == Job::NotRunning
                && !w.stale
                && w.start_at.is_some_and(|at| at <= w.clock + Duration::from_secs(100)),
        )
    };
    let without_wait = paddock_service::Timings::default().without_start_wait();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let cancel = AtomicBool::new(false);
        match e {
            Entry::Start | Entry::Update => {
                manager(run, None).on_start(&cancel);
            }
            Entry::FirstRun => {
                let _ = manager(run, Some(without_wait)).install_unattended(&cancel);
            }
            Entry::Install => {
                let _ = manager(run, Some(without_wait)).install(&cancel);
            }
            Entry::Uninstall => {
                let _: Result<(), ServiceError> = manager(run, None).uninstall();
            }
            Entry::Resume => {
                let _ = manager(run, None).resume(&cancel, &mut |_| {});
            }
        }
    }));
    let crashed = result.is_err();
    if crashed {
        run.crashes += 1;
    } else if matches!(e, Entry::FirstRun | Entry::Install) && run.wanted {
        // Dyno writes its first-run marker after a registration that did not fail.
        run.sim.lock().unwrap().first_run_done = true;
    }
    let after = snap(run);
    let (caps, scenario) = {
        let w = run.sim.lock().unwrap();
        (w.caps, w.scenario)
    };
    let d_reg = after.registers - before.registers;
    let d_unreg = after.unregisters - before.unregisters;
    let mut bad = |what: &str| run.notes.push(format!("{what} during {e:?}"));
    if after.off_registers > before.off_registers {
        bad("registered while the user's switch was off");
    }
    if matches!(e, Entry::Start | Entry::Update | Entry::Resume) && after.prompts > before.prompts {
        bad("an unasked prompt");
    }
    if caps == Caps::NoAuto && matches!(e, Entry::Start | Entry::Update | Entry::Resume) && (d_reg > 0 || d_unreg > 0) {
        bad("an unattended change on a mechanism that needs a prompt");
    }
    if matches!(e, Entry::Start | Entry::Resume) && !run.wanted && d_reg > 0 && scenario != Scenario::Off {
        bad("installed unasked or brought back what the user removed");
    }
    if matches!(e, Entry::Start | Entry::Resume) && run.wanted && (healthy_at_entry || soon_up_at_entry) && d_unreg > 0 {
        bad("removed a service that answers or is about to");
    }
    if crashed { Err(()) } else { Ok(()) }
}

fn describe(seq: &[Entry], crash: Option<(usize, u32)>, cfg: &str) -> String {
    format!("{seq:?} crash {crash:?} in {cfg}")
}

fn bound(scenario: Scenario, seq: &[Entry], crashes: u32, per_version: u32) -> bool {
    // Unattended registers of one version are bounded by the attempts (3) when the service never comes up in time, and by
    // one when it does; a resume's light registration is idempotent and counted on top.
    let resumes = seq.iter().filter(|e| matches!(e, Entry::Resume)).count() as u32;
    let limit = match scenario {
        Scenario::Fast | Scenario::Slow | Scenario::RefusedUntilRemoved => 1 + resumes + crashes,
        _ => 3 + resumes + crashes,
    };
    per_version <= limit
}

/// After the sequence: the user's service is still there and a user's request leaves no hold without a way out.
fn settle(run: &mut Run, seq: &[Entry]) {
    let (scenario, caps) = {
        let w = run.sim.lock().unwrap();
        (w.scenario, w.caps)
    };
    {
        let mut w = run.sim.lock().unwrap();
        w.crash_at = None;
        w.clock += Duration::from_secs(1000);
    }
    let cooperative = matches!(
        scenario,
        Scenario::Fast | Scenario::Slow | Scenario::Late | Scenario::RefusedUntilRemoved
    );
    // Dyno's first run is retried at the next start while its marker is missing.
    if run.wanted && !run.sim.lock().unwrap().first_run_done {
        let _ = perform(run, Entry::FirstRun, None);
        run.sim.lock().unwrap().clock += Duration::from_secs(1000);
    }
    for _ in 0..2 {
        let _ = perform(run, Entry::Start, None);
        run.sim.lock().unwrap().clock += Duration::from_secs(1000);
    }
    if run.wanted && cooperative && caps != Caps::NoAuto {
        let (reg, job) = {
            let mut w = run.sim.lock().unwrap();
            settle_time(&mut w);
            (w.reg, w.job)
        };
        if reg == Reg::Absent {
            run.notes
                .push("the user's service was lost (nothing is registered after two starts)".into());
        } else if job != Job::Running {
            // A user's request must bring it up in any case (never a hold with no way out).
            let _ = perform(run, Entry::Install, None);
            run.sim.lock().unwrap().clock += Duration::from_secs(1000);
            let mut w = run.sim.lock().unwrap();
            settle_time(&mut w);
            if w.job != Job::Running {
                drop(w);
                run.notes
                    .push("a hold with no way out: a request does not bring the service up".into());
            }
        }
    }
    let per_version = run.sim.lock().unwrap().unattended.clone();
    let crashes = run.crashes;
    for (v, n) in per_version {
        if !bound(scenario, seq, crashes, n) {
            run.notes
                .push(format!("{n} registrations of version {v} (not bounded)"));
        }
    }
}

struct Counts {
    sequences: usize,
    runs: usize,
    crash_points: usize,
}

type Failures = BTreeMap<String, Vec<String>>;

fn sequences(depth: usize, entries: &[Entry]) -> Vec<Vec<Entry>> {
    let mut seqs: Vec<Vec<Entry>> = vec![vec![]];
    for _ in 0..depth {
        let mut next = Vec::new();
        for s in &seqs {
            for e in entries {
                let mut t = s.clone();
                t.push(*e);
                next.push(t);
            }
        }
        seqs = next;
    }
    seqs
}

/// One run of `seq`, with a crash at call `k` of entry `pos` when given. Returns the notes and, for a crash-free run,
/// the calls worth crashing at per entry.
fn one_run(
    root: &std::path::Path,
    n: usize,
    seq: &[Entry],
    scenario: Scenario,
    caps: Caps,
    initial: Initial,
    crash: Option<(usize, u32)>,
) -> (Vec<String>, Vec<Vec<u32>>) {
    let mut run = start_run(root, n, scenario, caps, initial);
    let mut points = Vec::new();
    for (i, e) in seq.iter().enumerate() {
        let at = crash.filter(|(p, _)| *p == i).map(|(_, k)| k);
        let _ = perform(&mut run, *e, at);
        if crash.is_none() {
            let w = run.sim.lock().unwrap();
            let mutating = |l: &str| matches!(l, "register" | "unregister" | "evict");
            let keep = w
                .labels
                .iter()
                .enumerate()
                .filter(|(k, l)| mutating(l) || *k == 0 || (*k > 0 && mutating(w.labels[k - 1])))
                .map(|(k, _)| k as u32 + 1)
                .collect();
            points.push(keep);
        }
    }
    settle(&mut run, seq);
    (run.notes, points)
}

fn explore(depth: usize, scenarios: &[Scenario], capses: &[Caps], crash: bool, entries: &[Entry]) -> (Counts, Failures) {
    silence_crashes();
    let root = tempfile::tempdir().unwrap();
    let mut failures = Failures::new();
    let mut counts = Counts {
        sequences: 0,
        runs: 0,
        crash_points: 0,
    };
    let seqs = sequences(depth, entries);
    let mut n = 0;
    let note = |notes: Vec<String>, seq: &[Entry], at: Option<(usize, u32)>, cfg: &str, f: &mut Failures| {
        for note in notes {
            let kind = note.split(" during ").next().unwrap_or(&note).to_string();
            f.entry(kind).or_default().push(describe(seq, at, cfg));
        }
    };
    for &scenario in scenarios {
        for &caps in capses {
            for initial in [Initial::Fresh, Initial::Healthy] {
                for seq in &seqs {
                    counts.sequences += 1;
                    n += 1;
                    counts.runs += 1;
                    let (notes, points) = one_run(root.path(), n, seq, scenario, caps, initial, None);
                    note(notes, seq, None, &format!("{scenario:?} {caps:?} {initial:?}"), &mut failures);
                    if crash {
                        for (pos, ks) in points.iter().enumerate() {
                            for &k in ks {
                                n += 1;
                                counts.runs += 1;
                                counts.crash_points += 1;
                                let (notes, _) = one_run(root.path(), n, seq, scenario, caps, initial, Some((pos, k)));
                                note(notes, seq, Some((pos, k)), &format!("{scenario:?} {caps:?} {initial:?}"), &mut failures);
                            }
                        }
                    }
                }
            }
        }
    }
    (counts, failures)
}

fn report(counts: &Counts, failures: &Failures) -> String {
    let mut out = format!(
        "{} sequences, {} runs, {} crash points\n",
        counts.sequences, counts.runs, counts.crash_points
    );
    for (kind, list) in failures {
        out += &format!("- {kind}: {} runs, first: {}\n", list.len(), list[0]);
    }
    out
}

#[test]
fn every_sequence_of_entry_points_keeps_the_invariants_with_a_crash_at_every_os_call() {
    let started = std::time::Instant::now();
    let (counts, failures) = explore(
        3,
        &[Scenario::Fast, Scenario::Slow, Scenario::Never, Scenario::RefusedUntilRemoved],
        &[Caps::Mac],
        true,
        &ENTRIES,
    );
    println!("{} in {:?}", report(&counts, &failures), started.elapsed());
    assert!(failures.is_empty(), "{}", report(&counts, &failures));
}

#[test]
fn every_sequence_without_a_crash_on_every_mechanism_and_scenario() {
    let (counts, failures) = explore(
        3,
        &[
            Scenario::Fast,
            Scenario::Slow,
            Scenario::Late,
            Scenario::Never,
            Scenario::RefusedUntilRemoved,
            Scenario::RefusedAlways,
            Scenario::Off,
        ],
        &[Caps::Mac, Caps::Linux, Caps::NoAuto],
        false,
        &ENTRIES,
    );
    println!("{}", report(&counts, &failures));
    assert!(failures.is_empty(), "{}", report(&counts, &failures));
}

#[test]
fn a_service_that_never_answers_or_stays_refused_is_registered_a_bounded_number_of_times() {
    let (counts, failures) = explore(
        8,
        &[Scenario::Never, Scenario::RefusedAlways, Scenario::RefusedUntilRemoved],
        &[Caps::Mac],
        false,
        &[Entry::Start, Entry::Update],
    );
    println!("{}", report(&counts, &failures));
    assert!(failures.is_empty(), "{}", report(&counts, &failures));
}

/// Replays one run printed by a failure: `MODEL_REPLAY="Start,FirstRun,Install|Fast|Mac|Fresh|2|3"` (crash position and call are
/// optional) prints the OS calls and the notes.
#[test]
fn replay_one_run_from_the_environment() {
    let Ok(spec) = std::env::var("MODEL_REPLAY") else { return };
    silence_crashes();
    let parts: Vec<&str> = spec.split('|').collect();
    let pick = |name: &str| ENTRIES.iter().copied().find(|e| format!("{e:?}") == name).unwrap();
    let seq: Vec<Entry> = parts[0].split(',').map(pick).collect();
    let scenario = [Scenario::Fast, Scenario::Slow, Scenario::Late, Scenario::Never, Scenario::RefusedUntilRemoved, Scenario::RefusedAlways, Scenario::Off]
        .into_iter().find(|s| format!("{s:?}") == parts[1]).unwrap();
    let caps = [Caps::Mac, Caps::Linux, Caps::NoAuto].into_iter().find(|c| format!("{c:?}") == parts[2]).unwrap();
    let initial = if parts[3] == "Fresh" { Initial::Fresh } else { Initial::Healthy };
    let crash: Option<(usize, u32)> = parts.get(4).zip(parts.get(5)).map(|(p, k)| (p.parse().unwrap(), k.parse().unwrap()));
    let root = tempfile::tempdir().unwrap();
    let mut run = start_run(root.path(), 1, scenario, caps, initial);
    for (i, e) in seq.iter().enumerate() {
        let at = crash.filter(|(p, _)| *p == i).map(|(_, k)| k);
        let r = perform(&mut run, *e, at);
        println!("{e:?} -> crashed {} state {:?}", r.is_err(), run.record.load().state);
        for l in run.sim.lock().unwrap().trace.drain(..) { println!("{l}"); }
    }
    settle(&mut run, &seq);
    for l in run.sim.lock().unwrap().trace.drain(..) { println!("{l}"); }
    println!("notes {:?}\nrecord {:?}", run.notes, run.record.load());
}
