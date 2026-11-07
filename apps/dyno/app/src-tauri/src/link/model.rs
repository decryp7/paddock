//! The bounded model check of the supervisor (ADR 0025 section 4): a nondeterministic model of everything around the pure
//! machine (the agent processes and their lock, the discovery file, the login service, the launcher's queue, answers that
//! arrive late and out of order, Retry, Quit, the login switch) and an exhaustive breadth-first search of every interleaving
//! up to a bound, with the invariants checked after every step.
//!
//! The check must be able to fail: each historical bug is a named [`Mutant`] of the machine that the search has to catch,
//! and so is the generation hole the first prototype found. A search that reaches its state bound fails (it would only have
//! said "no violation up to the bound").
//!
//! The counts of the machine are shrunk ([`TUNING`]): a wait shows its cause after three ticks instead of fifty, so every
//! interleaving up to a note fits in the bound. The table tests use the production values.

use super::machine::{Command, Event, Generation, HintSeen, Launch, LockSeen, Look, Mutant, Owner, PeerId, Phase, Seq, Supervisor, Tuning};
use super::{Child, ServiceState};
use crate::supervise::explore::{explore, Explored};
use crate::supervise::Machine;
use paddock_proc::Cpu;

const TUNING: Tuning = Tuning {
    note_after: 3,
    service_recheck: 2,
    idle_rechecks: 2,
    pending_recheck: 1,
    pending_max: u32::MAX,
    connected_for: 2,
    ticks_per_second: 1,
    cpu_every: 2,
    lock_every: 2,
};

/// The most states a search at the full bound may visit; a search that gets there fails. The CI bound has a much lower one
/// ([`Bound::max_states`]) so that a runaway search fails in seconds.
const MAX_STATES: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Proc {
    NotStarted,
    Starting,
    Locked,
    Ready,
    /// Asked to exit; it still holds the lock and still answers until it is gone (Windows releases the lock last).
    Exiting,
    Gone(i32),
}

impl Proc {
    fn alive(self) -> bool {
        matches!(self, Proc::Starting | Proc::Locked | Proc::Ready | Proc::Exiting)
    }
    fn holds(self) -> bool {
        matches!(self, Proc::Locked | Proc::Ready | Proc::Exiting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Other {
    None,
    /// Holds the lock, has not written its file yet (a slow start).
    Slow,
    Ready,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    Own,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Job {
    Launch(Generation, i64),
    End(Generation),
}

/// What is left of the person's and the clock's choices.
#[derive(Debug, Clone, Copy, Hash)]
struct Budget {
    ticks: u8,
    retry: u8,
    quit: u8,
    removal: u8,
    add: u8,
}

#[derive(Debug, Clone, Hash)]
struct World {
    m: Supervisor,
    clock: i64,
    own: Proc,
    own_gen: Generation,
    own_started_ms: i64,
    other: Other,
    other_old_build: bool,
    asked_other: bool,
    stale_hint: bool,
    supervises: bool,
    /// A registration is under way (ADR 0056): the job is absent or not yet answering, and the service's agent will come.
    pending: bool,
    /// The registration never ends (a stuck flag): only the longest wait gets the app out of it.
    pending_sticks: bool,
    /// A registration may begin later (an update replaces the job, the switch is turned on): once.
    may_begin_pending: bool,
    works: bool,
    installed: bool,
    /// The generation the executor holds (it tags the launcher's child view with it).
    held: Option<Generation>,
    jobs: Vec<Job>,
    looks: Vec<Look>,
    connects: Vec<(Seq, Option<Target>)>,
    peers: Vec<(PeerId, Target)>,
    next_peer: PeerId,
    inbox: Vec<Event>,
    /// For each look in flight: whether another agent held the lock when it was taken (the truth, whatever the look says).
    truth: Vec<(Seq, bool)>,
    /// The look being decided on found another agent holding the lock.
    deciding_held: bool,
    budget: Budget,
    at_quit: Option<(bool, bool)>,
    violation: Option<&'static str>,
}

impl World {
    fn new(m: Supervisor, budget: Budget) -> World {
        World {
            m,
            clock: 0,
            own: Proc::NotStarted,
            own_gen: 0,
            own_started_ms: 0,
            other: Other::None,
            other_old_build: false,
            asked_other: false,
            stale_hint: false,
            supervises: false,
            pending: false,
            may_begin_pending: false,
            pending_sticks: false,
            works: false,
            installed: false,
            held: None,
            jobs: vec![],
            looks: vec![],
            connects: vec![],
            peers: vec![],
            next_peer: 0,
            inbox: vec![],
            truth: vec![],
            deciding_held: false,
            budget,
            at_quit: None,
            violation: None,
        }
    }

    fn lock_held(&self) -> bool {
        self.own.holds() || self.other != Other::None
    }

    fn own_pid(&self) -> u32 {
        100 + u32::try_from(self.own_gen).unwrap_or(0)
    }

    fn hint(&self) -> Option<HintSeen> {
        let seen = |pid, started_ms, same_build| HintSeen {
            pid,
            started_ms,
            version: "1".into(),
            protocol: 1,
            same_build,
            of_disk_file: false,
        };
        if matches!(self.own, Proc::Ready | Proc::Exiting) {
            Some(seen(self.own_pid(), self.own_started_ms, true))
        } else if matches!(self.other, Other::Ready | Other::Exiting) {
            Some(seen(9, 0, !self.other_old_build))
        } else if self.stale_hint {
            Some(seen(1, 0, true))
        } else {
            None
        }
    }

    fn child_view(&self) -> Option<(Generation, Child)> {
        let g = self.held?;
        Some((
            g,
            if self.own_gen != g {
                Child::None
            } else {
                match self.own {
                    Proc::Gone(code) => Child::Exited { code: Some(code) },
                    Proc::NotStarted => Child::None,
                    _ => Child::Running { pid: self.own_pid() },
                }
            },
        ))
    }

    /// Whether anything answers a connection to the file's agent now: its target.
    fn answering(&self) -> Option<Target> {
        if matches!(self.own, Proc::Ready | Proc::Exiting) {
            Some(Target::Own)
        } else if matches!(self.other, Other::Ready | Other::Exiting) {
            Some(Target::Other)
        } else {
            None
        }
    }

    fn close_peers_of(&mut self, target: Target) {
        let closed: Vec<PeerId> = self.peers.iter().filter(|(_, t)| *t == target).map(|(p, _)| *p).collect();
        self.peers.retain(|(_, t)| *t != target);
        for peer in closed {
            self.inbox.push(Event::PeerClosed { peer });
        }
    }

    fn own_gone(&mut self, code: i32) {
        self.own = Proc::Gone(code);
        self.close_peers_of(Target::Own);
    }

    fn feed(&mut self, e: Event) {
        if e == Event::Quit {
            self.at_quit = Some((self.m.held().is_some(), self.m.service().installed));
        }
        for c in self.m.step(e) {
            self.execute(c);
        }
        match self.m.phase() {
            Phase::Connected { peer, owner } => {
                let target = self.peers.iter().find(|(p, _)| p == peer).map(|(_, t)| *t);
                if let Some(t) = target {
                    if matches!(owner, Owner::ThisApp(_)) != (t == Target::Own) {
                        self.violation = Some("connected as this app's own agent to an agent it did not start (or the reverse)");
                    }
                }
            }
            Phase::Stopped { outcome } => {
                if let Some((held, installed)) = self.at_quit {
                    if outcome.stopped_own != (held && !installed) {
                        self.violation = Some("quit reported a different outcome than the state at Quit");
                    }
                }
                if outcome.stopped_own && self.own.alive() {
                    self.violation = Some("an agent this app started was left running after Quit");
                }
            }
            _ => {}
        }
        if self.m.overdue() {
            self.violation = Some("a long wait without a cause shown");
        }
    }

    fn execute(&mut self, c: Command) {
        match c {
            Command::Look {
                seq,
                service,
                lock,
                deciding,
                recheck,
                ..
            } => {
                if self.looks.len() >= 2 {
                    self.looks.remove(0);
                }
                let hint = self.hint();
                let lock = lock.then_some(if self.lock_held() { LockSeen::Held } else { LockSeen::Free });
                // As `ports::look` does: a decision look with a file beside a held lock is answered without the login service
                // (the machine connects to that agent; the service is asked right after the connection).
                let needs_service =
                    !((deciding && hint.is_some() && lock == Some(LockSeen::Held)) || (recheck && lock == Some(LockSeen::Held)));
                let look = Look {
                    seq,
                    now_ms: self.clock,
                    hint,
                    lock,
                    service: (service && needs_service).then_some(ServiceState {
                        installed: self.installed,
                        supervises: self.supervises,
                        pending: self.pending,
                    }),
                    child: self.child_view(),
                    cpu: Cpu::Unavailable,
                };
                self.truth.push((seq, self.other != Other::None));
                if self.truth.len() > 6 {
                    self.truth.remove(0);
                }
                self.looks.push(look);
            }
            Command::Launch { child, anyway } => {
                if matches!(self.m.phase(), Phase::Quitting { .. } | Phase::Stopped { .. }) {
                    self.violation = Some("launched after the quit began");
                }
                if self.own.alive() || self.jobs.iter().any(|j| matches!(j, Job::Launch(..))) {
                    self.violation = Some("a second own agent was launched while the first was still held");
                }
                if self.m.service().pending && !self.m.gave_up_on_pending() {
                    self.violation = Some("launched an agent of its own while a registration of the service was pending");
                }
                if self.deciding_held && !anyway {
                    self.violation = Some("launched on a look that did not see the agent holding the data folder");
                }
                if self.asked_other && self.other == Other::Exiting && !anyway {
                    self.violation = Some("launched before the agent it asked to exit released the data folder");
                }
                self.held = Some(child);
                self.jobs.push(Job::Launch(child, self.clock));
            }
            Command::EndHeldChild { child } => {
                if self.held.is_some_and(|g| g != child) {
                    self.violation = Some("ended a child that is not the one held (rule 53)");
                }
                self.jobs.push(Job::End(child));
            }
            Command::Connect { seq, .. } => {
                let target = self.hint().and_then(|_| self.answering());
                self.connects.push((seq, target));
            }
            Command::AskToExit { peer } => {
                if let Some(i) = self.peers.iter().position(|(p, _)| *p == peer) {
                    let (_, t) = self.peers.remove(i);
                    self.ask(t);
                    self.inbox.push(Event::PeerClosed { peer });
                }
            }
            Command::CloseConnection { peer } => {
                if let Some(i) = self.peers.iter().position(|(p, _)| *p == peer) {
                    self.peers.remove(i);
                    self.inbox.push(Event::PeerClosed { peer });
                }
            }
            Command::RemoveHint { .. } => {
                if self.answering().is_some() {
                    self.violation = Some("removed the discovery file of an agent that is alive");
                } else {
                    self.stale_hint = false;
                }
            }
            Command::UsePeer(Some(p)) => match self.peers.iter().find(|(id, _)| *id == p) {
                None => self.violation = Some("used a connection that does not exist (a file taken for the agent)"),
                Some((_, Target::Other)) if self.asked_other && self.other == Other::Exiting => {
                    self.violation = Some("reconnected to an agent that was asked to exit");
                }
                _ => {}
            },
            Command::UsePeer(None) | Command::Report(_) | Command::Stall { .. } | Command::ClearStall | Command::Done(_) => {}
        }
    }

    fn ask(&mut self, t: Target) {
        match t {
            Target::Own if self.own == Proc::Ready => self.own = Proc::Exiting,
            Target::Other if self.other == Other::Ready => {
                self.other = Other::Exiting;
                self.asked_other = true;
            }
            _ => {}
        }
    }

    /// Every next world: one nondeterministic step.
    fn successors(&self) -> Vec<(String, World)> {
        let mut v: Vec<(String, World)> = Vec::new();
        if let Some(e) = self.inbox.first() {
            let label = format!("deliver {e:?}");
            let mut w = self.clone();
            let e = w.inbox.remove(0);
            w.feed(e);
            v.push((label, w));
        }
        for i in 0..self.looks.len() {
            let mut w = self.clone();
            let l = w.looks.remove(i);
            let label = format!("a look (seq {}) arrives", l.seq);
            w.deciding_held = w.truth.iter().find(|(s, _)| *s == l.seq).is_some_and(|(_, held)| *held);
            let was_connected = matches!(w.m.phase(), Phase::Connected { .. });
            w.feed(Event::Looked(l));
            if was_connected && matches!(w.m.phase(), Phase::Ending { .. } | Phase::TakingBack { .. }) && w.m.service().pending {
                w.violation = Some("an agent was retired while a registration of the service was pending");
            }
            w.deciding_held = false;
            v.push((label, w));
        }
        for i in 0..self.connects.len() {
            let mut w = self.clone();
            let (seq, target) = w.connects.remove(i);
            let still = target.filter(|t| match t {
                Target::Own => matches!(w.own, Proc::Ready | Proc::Exiting),
                Target::Other => matches!(w.other, Other::Ready | Other::Exiting),
            });
            let label = format!("connect {seq} answered {still:?}");
            match still {
                Some(t) => {
                    w.next_peer += 1;
                    let peer = w.next_peer;
                    w.peers.push((peer, t));
                    let pid = if t == Target::Own { w.own_pid() } else { 9 };
                    w.feed(Event::Connected { seq, peer, pid: Some(pid) });
                }
                None => w.feed(Event::ConnectFailed { seq, why: "x".into() }),
            }
            v.push((label, w));
        }
        if let Some(Job::Launch(g, _)) = self.jobs.first().copied() {
            // The launch can fail (no program, the OS refused): the executor then holds nothing.
            let mut w = self.clone();
            w.jobs.remove(0);
            if w.held == Some(g) {
                w.held = None;
            }
            w.inbox.push(Event::Launched {
                child: g,
                result: Err("could not start".into()),
            });
            v.push((format!("launch of {g} fails"), w));
        }
        if let Some(job) = self.jobs.first().copied() {
            let mut w = self.clone();
            w.jobs.remove(0);
            match job {
                Job::Launch(g, at_ms) => {
                    w.own = Proc::Starting;
                    w.own_gen = g;
                    w.own_started_ms = w.clock;
                    let pid = w.own_pid();
                    w.inbox.push(Event::Launched {
                        child: g,
                        result: Ok(Launch { at_ms, pid: Some(pid) }),
                    });
                }
                Job::End(g) => {
                    if w.own.alive() {
                        w.own_gone(0);
                    }
                    if w.held == Some(g) {
                        w.held = None;
                    }
                    w.inbox.push(Event::ChildEnded { child: g, code: Some(0) });
                }
            }
            v.push((format!("launcher does {job:?}"), w));
        }
        // The agent this app started progresses.
        match self.own {
            Proc::Starting => {
                let mut w = self.clone();
                if w.lock_held() {
                    w.own_gone(3);
                } else {
                    w.own = Proc::Locked;
                }
                v.push(("own agent starts".into(), w));
            }
            Proc::Locked => {
                let mut w = self.clone();
                w.own = Proc::Ready;
                v.push(("own agent writes its file".into(), w));
            }
            Proc::Ready => {
                // It dies by itself (a crash): its lock and its connections go with it, its file stays.
                let mut w = self.clone();
                w.own_gone(1);
                w.stale_hint = true;
                v.push(("own agent crashes".into(), w));
            }
            Proc::Exiting => {
                let mut w = self.clone();
                w.own_gone(0);
                v.push(("own agent exits".into(), w));
            }
            _ => {}
        }
        // The other agent (the service's, or a foreign one).
        if self.other == Other::Slow {
            let mut w = self.clone();
            w.other = Other::Ready;
            v.push(("the foreign agent writes its file".into(), w));
        }
        if self.other == Other::Exiting {
            let mut w = self.clone();
            w.other = Other::None;
            w.close_peers_of(Target::Other);
            v.push(("other agent exits".into(), w));
        }
        if self.supervises && self.works && self.other == Other::None && !self.lock_held() {
            let mut w = self.clone();
            w.other = Other::Ready;
            v.push(("the service starts its agent".into(), w));
        }
        if self.pending && !self.pending_sticks {
            // The registration's three ends, and the OS loading the job before it ends (the job runs, the agent is late).
            let mut w = self.clone();
            w.pending = false;
            w.installed = true;
            w.supervises = true;
            w.works = true;
            v.push(("the registration ends with the service".into(), w));
            let mut w = self.clone();
            w.pending = false;
            w.installed = false;
            w.supervises = false;
            w.works = false;
            v.push(("the registration ends with no service".into(), w));
            let mut w = self.clone();
            w.pending = false;
            w.installed = true;
            w.supervises = false;
            w.works = false;
            v.push(("the registration ends with a job the OS refuses".into(), w));
            if !self.supervises {
                let mut w = self.clone();
                w.installed = true;
                w.supervises = true;
                v.push(("the OS loads the job (its agent is not up yet)".into(), w));
            }
        }
        if self.may_begin_pending {
            // An update or the switch: the old job is removed first, so the job is absent while the registration is pending.
            let mut w = self.clone();
            w.may_begin_pending = false;
            w.pending = true;
            w.installed = false;
            w.supervises = false;
            w.works = false;
            w.feed(Event::ServiceChanged);
            v.push(("a registration begins".into(), w));
        }
        if self.budget.ticks > 0 {
            let mut w = self.clone();
            w.budget.ticks -= 1;
            w.clock += 1;
            w.feed(Event::Tick);
            v.push(("tick".into(), w));
        }
        if self.budget.retry > 0 {
            let shown = self.m.snapshot().stall.map(|(id, _)| id);
            let mut choices = vec![None];
            choices.extend(shown.map(Some));
            for stall in choices {
                let mut w = self.clone();
                w.budget.retry -= 1;
                w.feed(Event::Retry { stall });
                v.push((format!("Retry {stall:?}"), w));
            }
        }
        if self.budget.quit > 0 {
            let mut w = self.clone();
            w.budget.quit -= 1;
            w.feed(Event::Quit);
            v.push(("Quit".into(), w));
        }
        if self.budget.removal > 0 && self.installed {
            let mut w = self.clone();
            w.budget.removal -= 1;
            w.installed = false;
            w.supervises = false;
            w.feed(Event::ServiceChanged);
            v.push(("the login switch is turned off".into(), w));
        }
        if self.budget.add > 0 && !self.installed {
            let mut w = self.clone();
            w.budget.add -= 1;
            w.installed = true;
            w.supervises = true;
            w.works = true;
            w.feed(Event::ServiceChanged);
            v.push(("the login switch is turned on".into(), w));
        }
        v
    }
}

fn base(mutant: Mutant, budget: Budget) -> World {
    World::new(Supervisor::tuned(TUNING, mutant), budget)
}

const BUDGET: Budget = Budget {
    ticks: 2,
    retry: 1,
    quit: 1,
    removal: 1,
    add: 1,
};

/// The worlds the machine is checked in, each begun with `Begin`.
fn scenarios(mutant: Mutant, budget: Budget, bound: Bound) -> Vec<(&'static str, World)> {
    let mk = |f: &dyn Fn(&mut World)| {
        let mut w = base(mutant, budget);
        f(&mut w);
        w.feed(Event::Begin);
        w
    };
    let all = vec![
        (
            "no agent, no service",
            mk(&|w| {
                w.budget.add = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "the service runs the agent, then is removed",
            mk(&|w| {
                w.supervises = true;
                w.works = true;
                w.installed = true;
                w.other = Other::Ready;
                w.budget.add = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "the service starts nothing, Retry and Quit interleaved with the fallback",
            mk(&|w| {
                // One recheck and one idle recheck are enough to reach the fallback within the tick budget.
                w.m = Supervisor::tuned(
                    Tuning {
                        service_recheck: 1,
                        idle_rechecks: 1,
                        ..TUNING
                    },
                    mutant,
                );
                w.supervises = true;
                w.installed = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.ticks = 4;
            }),
        ),
        (
            "an exiting foreign holder",
            mk(&|w| {
                w.other = Other::Exiting;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "a foreign agent holds the folder and is slow to write its file",
            mk(&|w| {
                w.other = Other::Slow;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "a stale discovery file",
            mk(&|w| {
                w.stale_hint = true;
                w.budget.removal = 0;
                w.budget.add = 0;
                w.budget.ticks = 2;
            }),
        ),
        (
            "an agent of another build",
            mk(&|w| {
                w.other = Other::Ready;
                w.other_old_build = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "a registration is pending, then ends with the service, with no service or with a refused job",
            mk(&|w| {
                w.pending = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "a registration stays pending past the longest wait",
            mk(&|w| {
                w.m = Supervisor::tuned(Tuning { pending_max: 2, ..TUNING }, mutant);
                w.pending = true;
                w.pending_sticks = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.retry = 0;
                w.budget.quit = 0;
                w.budget.ticks = 5;
            }),
        ),
        (
            "a registration begins while the app's own agent runs",
            mk(&|w| {
                w.may_begin_pending = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.retry = 0;
                w.budget.quit = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "an update replaces the job while the service's agent answers",
            mk(&|w| {
                w.supervises = true;
                w.works = true;
                w.installed = true;
                w.other = Other::Ready;
                w.may_begin_pending = true;
                w.budget.add = 0;
                w.budget.removal = 0;
                w.budget.retry = 0;
                w.budget.ticks = 3;
            }),
        ),
        (
            "the login switch is turned on while the app runs",
            mk(&|w| {
                w.budget.removal = 0;
                w.budget.retry = 0;
                w.budget.ticks = 2;
            }),
        ),
    ];
    all.into_iter()
        .map(|(name, mut w)| {
            if bound == Bound::Ci {
                // Re-run the scenario's start with the smaller budget: the budget is part of the begun world.
                w.budget.ticks = w.budget.ticks.min(ci_ticks(name));
            }
            (name, w)
        })
        .collect()
}

/// How far a search goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// What every CI leg runs: the scenarios with as few ticks as still show their invariants (and every named mutant).
    Ci,
    /// The full search (`cargo test -p dyno-app --lib link::model -- --ignored`): more ticks in every scenario.
    Full,
}

impl Bound {
    /// The most states one scenario's search may visit at this bound. The largest CI scenario visits about 121 000, so the CI
    /// cap is 500 000: a machine change that makes the search diverge fails in about a minute instead of running fifteen threads
    /// to 2 000 000 states each (an hour of CPU).
    fn max_states(self) -> usize {
        match self {
            Bound::Ci => 500_000,
            Bound::Full => MAX_STATES,
        }
    }
}

/// The ticks a scenario gets in the CI bound. Three ticks reach a wait's note (the "cause shown" invariant), so the scenarios
/// whose waits matter most keep three; the others look twice.
fn ci_ticks(name: &str) -> u8 {
    match name {
        "no agent, no service"
        | "a registration is pending, then ends with the service, with no service or with a refused job"
        | "an agent of another build"
        | "the service starts nothing, Retry and Quit interleaved with the fallback" => 3,
        _ => 2,
    }
}

/// A 128-bit key of a state: two hashes of its fields (no text is built).
fn key_of(w: &World) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let mut a = std::collections::hash_map::DefaultHasher::new();
    let mut b = std::collections::hash_map::DefaultHasher::new();
    b.write_u8(0xA5);
    w.hash(&mut a);
    w.hash(&mut b);
    (a.finish(), b.finish())
}

fn check_all(mutant: Mutant, budget: Budget, bound: Bound) -> Vec<(&'static str, Explored)> {
    // One thread per scenario: they share nothing.
    std::thread::scope(|scope| {
        let handles: Vec<_> = scenarios(mutant, budget, bound)
            .into_iter()
            .map(|(name, w)| {
                scope.spawn(move || {
                    let r = explore(
                        w,
                        bound.max_states(),
                        key_of,
                        |w| w.successors(),
                        |w| w.violation.map(str::to_string),
                    );
                    (name, r)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("a scenario panicked")).collect()
    })
}

/// Searches every scenario at `bound` and fails on a violation or on a search that hit its state bound; returns the states
/// visited and the time taken.
fn assert_clean(bound: Bound) -> (usize, std::time::Duration) {
    let t = std::time::Instant::now();
    let mut total = 0;
    for (name, r) in check_all(Mutant::None, BUDGET, bound) {
        assert!(
            !r.truncated,
            "{name}: the search reached its bound of {} states without finishing",
            bound.max_states()
        );
        if let Some(v) = &r.violation {
            panic!("{name}: {}\n  {}", v.what, v.trace.join("\n  "));
        }
        eprintln!("  {name}: {} states", r.states);
        total += r.states;
    }
    (total, t.elapsed())
}

/// The CI bound: every invariant in every interleaving of the scenarios at two or three ticks. It has to stay cheap enough for
/// every CI leg (nextest's limit is 60 s and a shared runner is slow); the numbers measured on the development Mac are in the
/// commit that introduced the two bounds.
/// A test must never spin for an hour: a search that diverges (a machine change that lets looks or waits multiply) has to hit its
/// state bound within seconds. The largest CI scenario visits about 121 000 states, so the CI cap is a few times that, not the
/// full bound's 2 000 000 per scenario (fifteen scenarios in parallel threads cost an hour of CPU at that cap, 2026-10-31).
#[test]
fn a_diverging_search_hits_the_ci_cap_in_seconds_and_every_real_scenario_stays_well_inside_it() {
    assert!(Bound::Ci.max_states() <= 600_000, "the CI cap is what bounds a runaway search");
    assert!(Bound::Ci.max_states() < Bound::Full.max_states());
    let t = std::time::Instant::now();
    let r = explore(
        0u64,
        Bound::Ci.max_states(),
        |n| *n,
        |n| vec![("up".to_string(), n + 1), ("up2".to_string(), n + 2)],
        |_| None,
    );
    assert!(r.truncated && r.states >= Bound::Ci.max_states());
    assert!(t.elapsed() < std::time::Duration::from_secs(30), "{:?}", t.elapsed());
    for (name, r) in check_all(Mutant::None, BUDGET, Bound::Ci) {
        assert!(
            r.states * 3 <= Bound::Ci.max_states(),
            "{name}: {} states is too close to the CI cap {}",
            r.states,
            Bound::Ci.max_states()
        );
    }
}

#[test]
fn the_machine_keeps_every_invariant_in_every_interleaving_up_to_the_ci_bound() {
    let (total, took) = assert_clean(Bound::Ci);
    eprintln!("model check (CI bound): {total} states in {took:?}");
}

/// The full bound: one more tick in most scenarios, many times the states. Run it by hand, and in a nightly that runs ignored
/// tests: `cargo test -p dyno-app --lib link::model -- --ignored`.
#[test]
#[ignore = "the full bound takes minutes in a loaded test build; the CI bound runs on every leg"]
fn the_machine_keeps_every_invariant_in_every_interleaving_up_to_the_full_bound() {
    let (total, took) = assert_clean(Bound::Full);
    eprintln!("model check (full bound): {total} states in {took:?}");
}

/// The named scenarios (at the CI bound) must show `mutant`'s violation. Only those run, and each stops at its first violation,
/// so a mutant costs what it takes to find it.
fn caught(mutant: Mutant, scenario_names: &[&str]) -> String {
    for (name, w) in scenarios(mutant, BUDGET, Bound::Ci)
        .into_iter()
        .filter(|(n, _)| scenario_names.contains(n))
    {
        let r = explore(
            w,
            Bound::Ci.max_states(),
            key_of,
            |w| w.successors(),
            |w| w.violation.map(str::to_string),
        );
        if let Some(v) = r.violation {
            return format!("{name}: {} (after {} steps, {} states)", v.what, v.trace.len(), r.states);
        }
    }
    panic!("{mutant:?} was not caught in {scenario_names:?}: the model check cannot fail on it");
}

#[test]
fn bug_1_a_fixed_wait_that_launches_a_second_agent_is_caught() {
    let found = caught(Mutant::LaunchOnFixedWait, &["no agent, no service"]);
    assert!(found.contains("second own agent"), "{found}");
}

#[test]
fn bug_2_a_wait_without_a_cause_is_caught() {
    let found = caught(Mutant::EndlessWait, &["no agent, no service"]);
    assert!(found.contains("without a cause"), "{found}");
}

#[test]
fn bug_3_a_stale_discovery_file_taken_for_the_agent_is_caught() {
    let found = caught(Mutant::StaleHintAsAgent, &["a stale discovery file"]);
    assert!(found.contains("does not exist"), "{found}");
}

#[test]
fn bug_4_launching_when_the_connection_closes_before_the_lock_is_released_is_caught() {
    let found = caught(Mutant::LaunchOnPeerClosed, &["the service runs the agent, then is removed"]);
    assert!(
        found.contains("released the data folder") || found.contains("second own agent"),
        "{found}"
    );
}

#[test]
fn bug_5_a_quit_outcome_read_again_after_the_exit_is_caught() {
    let found = caught(Mutant::QuitOutcomeReread, &["no agent, no service"]);
    assert!(found.contains("different outcome"), "{found}");
}

#[test]
fn never_probing_the_lock_is_caught_beside_a_holder_that_has_not_written_its_file() {
    let found = caught(
        Mutant::NeverProbeLock,
        &["a foreign agent holds the folder and is slow to write its file"],
    );
    assert!(found.contains("did not see the agent holding"), "{found}");
}

#[test]
fn the_generation_hole_of_the_first_draft_is_caught() {
    let found = caught(Mutant::GenerationBlind, &["no agent, no service"]);
    assert!(found.contains("second own agent") || found.contains("not the one held"), "{found}");
}

#[test]
fn a_login_service_that_starts_nothing_ends_with_an_agent_of_the_apps_own_without_retry() {
    // Reachability, not safety: with no Retry and no Quit, some interleaving reaches a connection to the app's own agent.
    let budget = Budget {
        ticks: 14,
        retry: 0,
        quit: 0,
        removal: 0,
        add: 0,
    };
    let mut w = base(Mutant::None, budget);
    w.supervises = true;
    w.installed = true;
    w.feed(Event::Begin);
    let r = explore(
        w,
        MAX_STATES,
        key_of,
        |w| w.successors(),
        |w| w.m.snapshot().started_by_this_app().then(|| "reached".to_string()),
    );
    assert!(
        r.violation.is_some_and(|v| v.what == "reached"),
        "the app never started its own agent beside a broken service"
    );
}

#[test]
fn a_registration_that_ends_with_no_service_ends_with_an_agent_of_the_apps_own_without_retry() {
    // Reachability, not safety: never waiting forever with no agent. Pending, then nothing registered: the app starts its own.
    let budget = Budget {
        ticks: 6,
        retry: 0,
        quit: 0,
        removal: 0,
        add: 0,
    };
    let mut w = base(Mutant::None, budget);
    w.pending = true;
    w.feed(Event::Begin);
    let r = explore(
        w,
        MAX_STATES,
        key_of,
        |w| w.successors(),
        |w| (!w.pending && !w.supervises && w.m.snapshot().started_by_this_app()).then(|| "reached".to_string()),
    );
    assert!(
        r.violation.is_some_and(|v| v.what == "reached"),
        "the app never fell back after the registration ended with no service"
    );
}

#[test]
fn a_registration_that_ends_with_the_service_ends_with_a_connection_to_the_services_agent_and_never_an_agent_of_the_apps_own() {
    let budget = Budget {
        ticks: 6,
        retry: 0,
        quit: 0,
        removal: 0,
        add: 0,
    };
    let mut w = base(Mutant::None, budget);
    w.pending = true;
    w.feed(Event::Begin);
    let r = explore(
        w,
        MAX_STATES,
        key_of,
        |w| w.successors(),
        |w| {
            w.violation
                .map(str::to_string)
                .or_else(|| (w.works && matches!(w.m.phase(), Phase::Connected { owner: Owner::Other, .. })).then(|| "reached".to_string()))
        },
    );
    assert!(
        r.violation.is_some_and(|v| v.what == "reached"),
        "no interleaving connects to the service's agent after its registration ends"
    );
}

#[test]
fn a_registration_that_never_ends_ends_with_an_agent_of_the_apps_own_after_the_longest_wait() {
    // Reachability: the stuck flag is given up on, so the link never waits forever with no agent.
    let budget = Budget {
        ticks: 8,
        retry: 0,
        quit: 0,
        removal: 0,
        add: 0,
    };
    let mut w = base(Mutant::None, budget);
    w.m = Supervisor::tuned(Tuning { pending_max: 2, ..TUNING }, Mutant::None);
    w.pending = true;
    w.pending_sticks = true;
    w.feed(Event::Begin);
    let r = explore(
        w,
        MAX_STATES,
        key_of,
        |w| w.successors(),
        |w| {
            w.violation
                .map(str::to_string)
                .or_else(|| w.m.snapshot().started_by_this_app().then(|| "reached".to_string()))
        },
    );
    assert!(
        r.violation.is_some_and(|v| v.what == "reached"),
        "the app waited forever for a registration that never ended"
    );
}
