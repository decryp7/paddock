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
    connected_for: 2,
    ticks_per_second: 1,
    cpu_every: 2,
};

/// The most states a search may visit; a search that gets there fails.
const MAX_STATES: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Other {
    None,
    Ready,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Own,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Job {
    Launch(Generation, i64),
    End(Generation),
}

/// What is left of the person's and the clock's choices.
#[derive(Debug, Clone, Copy)]
struct Budget {
    ticks: u8,
    retry: u8,
    quit: u8,
    removal: u8,
    add: u8,
}

#[derive(Debug, Clone)]
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
            works: false,
            installed: false,
            held: None,
            jobs: vec![],
            looks: vec![],
            connects: vec![],
            peers: vec![],
            next_peer: 0,
            inbox: vec![],
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
        };
        if matches!(self.own, Proc::Ready | Proc::Exiting) {
            Some(seen(self.own_pid(), self.own_started_ms, true))
        } else if self.other != Other::None {
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
        } else if self.other != Other::None {
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
            Command::Look { seq, service, lock, .. } => {
                if self.looks.len() >= 2 {
                    self.looks.remove(0);
                }
                let look = Look {
                    seq,
                    now_ms: self.clock,
                    hint: self.hint(),
                    lock: lock.then_some(if self.lock_held() { LockSeen::Held } else { LockSeen::Free }),
                    service: service.then_some(ServiceState {
                        installed: self.installed,
                        supervises: self.supervises,
                    }),
                    child: self.child_view(),
                    cpu: Cpu::Unavailable,
                };
                self.looks.push(look);
            }
            Command::Launch { child, anyway } => {
                if matches!(self.m.phase(), Phase::Quitting { .. } | Phase::Stopped { .. }) {
                    self.violation = Some("launched after the quit began");
                }
                if self.own.alive() || self.jobs.iter().any(|j| matches!(j, Job::Launch(..))) {
                    self.violation = Some("a second own agent was launched while the first was still held");
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
            w.feed(Event::Looked(l));
            v.push((label, w));
        }
        for i in 0..self.connects.len() {
            let mut w = self.clone();
            let (seq, target) = w.connects.remove(i);
            let still = target.filter(|t| match t {
                Target::Own => matches!(w.own, Proc::Ready | Proc::Exiting),
                Target::Other => w.other != Other::None,
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
fn scenarios(mutant: Mutant, budget: Budget) -> Vec<(&'static str, World)> {
    let mk = |f: &dyn Fn(&mut World)| {
        let mut w = base(mutant, budget);
        f(&mut w);
        w.feed(Event::Begin);
        w
    };
    vec![
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
            "the login switch is turned on while the app runs",
            mk(&|w| {
                w.budget.removal = 0;
                w.budget.retry = 0;
                w.budget.ticks = 2;
            }),
        ),
    ]
}

/// A 128-bit key of a state: its `Debug` text hashed as it is written (no string is built).
fn key_of(w: &World) -> (u64, u64) {
    use std::fmt::Write;
    use std::hash::Hasher;
    struct Two(std::collections::hash_map::DefaultHasher, std::collections::hash_map::DefaultHasher);
    impl Write for Two {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            self.1.write(s.as_bytes());
            self.1.write_u8(0xA5);
            Ok(())
        }
    }
    let mut h = Two(Default::default(), Default::default());
    let _ = write!(h, "{w:?}");
    (h.0.finish(), h.1.finish())
}

fn check_all(mutant: Mutant, budget: Budget) -> Vec<(&'static str, Explored)> {
    // One thread per scenario: they share nothing.
    std::thread::scope(|scope| {
        let handles: Vec<_> = scenarios(mutant, budget)
            .into_iter()
            .map(|(name, w)| {
                scope.spawn(move || {
                    let r = explore(w, MAX_STATES, key_of, |w| w.successors(), |w| w.violation.map(str::to_string));
                    (name, r)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("a scenario panicked")).collect()
    })
}

#[test]
fn the_machine_keeps_every_invariant_in_every_interleaving_up_to_the_bound() {
    let t = std::time::Instant::now();
    let mut total = 0;
    for (name, r) in check_all(Mutant::None, BUDGET) {
        assert!(
            !r.truncated,
            "{name}: the search reached its bound of {MAX_STATES} states without finishing"
        );
        if let Some(v) = &r.violation {
            panic!("{name}: {}\n  {}", v.what, v.trace.join("\n  "));
        }
        eprintln!("  {name}: {} states", r.states);
        total += r.states;
    }
    eprintln!("model check: {total} states in {:?}", t.elapsed());
}

/// Each mutant must be found by at least one scenario.
fn caught(mutant: Mutant) -> String {
    for (name, r) in check_all(mutant, BUDGET) {
        if let Some(v) = r.violation {
            return format!("{name}: {} (after {} steps)", v.what, v.trace.len());
        }
    }
    panic!("{mutant:?} was not caught: the model check cannot fail on it");
}

#[test]
fn bug_1_a_fixed_wait_that_launches_a_second_agent_is_caught() {
    let found = caught(Mutant::LaunchOnFixedWait);
    assert!(found.contains("second own agent"), "{found}");
}

#[test]
fn bug_2_a_wait_without_a_cause_is_caught() {
    let found = caught(Mutant::EndlessWait);
    assert!(found.contains("without a cause"), "{found}");
}

#[test]
fn bug_3_a_stale_discovery_file_taken_for_the_agent_is_caught() {
    let found = caught(Mutant::StaleHintAsAgent);
    assert!(found.contains("does not exist"), "{found}");
}

#[test]
fn bug_4_launching_when_the_connection_closes_before_the_lock_is_released_is_caught() {
    let found = caught(Mutant::LaunchOnPeerClosed);
    assert!(
        found.contains("released the data folder") || found.contains("second own agent"),
        "{found}"
    );
}

#[test]
fn bug_5_a_quit_outcome_read_again_after_the_exit_is_caught() {
    let found = caught(Mutant::QuitOutcomeReread);
    assert!(found.contains("different outcome"), "{found}");
}

#[test]
fn the_generation_hole_of_the_first_draft_is_caught() {
    let found = caught(Mutant::GenerationBlind);
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
