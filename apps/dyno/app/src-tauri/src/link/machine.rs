//! The agent supervisor as a pure state machine (ADR 0025): `step(event) -> commands`, no clock, no I/O, no threads.
//!
//! The phase says what the link is waiting for, so "which wait are we in" is data and not a call stack. Every answer from the
//! world carries the number of the thing it is about (a look and a connect their [`Seq`], a child its [`Generation`], a
//! connection its [`PeerId`]) and `step` drops an answer whose number is not the current one: an answer about an old process
//! never changes the decision about a new one (rule 53 for events).
//!
//! Invariants (checked after every step by the model check in `model.rs`):
//! 1. at most one agent of this app: a child is held from `Launch` until its `ChildEnded` (or a failed launch), and nothing
//!    is launched while one is held, after a quit began, or while the data folder is held (except by Retry's `anyway`);
//! 2. the only process ended is the held child of the generation named; any other agent is only asked to exit;
//! 3. every phase but `Connected`, `Quitting` and the ends has a cause and a Retry, and a wait of [`NOTE_AFTER`] ticks has
//!    shown its cause ([`Supervisor::overdue`]);
//! 4. no fixed time limit: ticks count observations; the one deliberate limit is the grace the adapter gives a child to exit
//!    before it is killed at a quit (`// cap:` in `ports.rs`);
//! 5. the quit outcome is decided once, from the state at the `Quit` event;
//! 6. one writer: only the owner thread calls `step`;
//! 7. a discovery file is a hint: only a connection that answered makes the agent "connected".

use super::{backoff, state_after_failures, stop_agent_on_quit, Child, ServiceState, Stall, FAILED_AFTER};
use crate::dto::{AgentState, AgentStatus};
use crate::supervise::Machine;
use paddock_proc::{Cpu, ExitInfo, Liveness, Patience, Reading, StallCause, StartJudge, StartState};

/// The number of a look or a connect.
pub type Seq = u64;
/// The number of a launch: the child it started.
pub type Generation = u64;
/// The number of a connection.
pub type PeerId = u64;
/// The number of a shown stall.
pub type StallId = u64;

/// After this many ticks in one wait the status says what it is waiting for (a note, never a limit). Ticks are looks of the
/// world, about 5 s at the production pace.
pub const NOTE_AFTER: u32 = 50;
/// While waiting for the service's agent, the service is asked again every this many ticks.
pub const SERVICE_RECHECK: u32 = 50;
/// The login service is rechecked this many times in a row with no process holding the agent lock before the app takes it
/// that the service starts nothing: four rechecks outlast launchd's `ThrottleInterval` (10 s) and systemd's `RestartSec`.
pub const SERVICE_IDLE_RECHECKS: u32 = 4;
/// Ticks per second at the production pace (a backoff of n seconds is n times this many ticks).
pub const TICKS_PER_SECOND: u32 = 10;
/// A connection that lasted this many ticks counts as having worked: only then the failures in a row start from zero.
pub const CONNECTED_FOR: u32 = 30;
/// The start judge reads the child's CPU time every this many ticks (one system call per process of its tree).
pub const CPU_EVERY: u32 = 20;
/// While waiting for the service's agent or a holder, the lock is probed every this many ticks.
pub const LOCK_EVERY: u32 = 5;
/// The agent's exit code when another agent holds the data folder.
const EXIT_ALREADY_RUNNING: i32 = dyno_agent::cli::EXIT_ALREADY_RUNNING;

/// The counts that decide when a wait shows its cause and when a service is given up on. The production values are the
/// constants above; the model check shrinks them so that every interleaving up to a wait's note fits in a bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tuning {
    /// Ticks in one wait before its cause is shown.
    pub note_after: u32,
    /// Ticks between the looks at the login service while it is awaited.
    pub service_recheck: u32,
    /// Rechecks in a row with no agent running before the app starts its own beside a service that starts nothing.
    pub idle_rechecks: u32,
    /// Ticks a connection must last before the failures in a row start from zero.
    pub connected_for: u32,
    /// Ticks per second of backoff.
    pub ticks_per_second: u32,
    /// The start judge reads the child's CPU time every this many ticks.
    pub cpu_every: u32,
    /// While waiting for the service's agent or a holder, the agent lock is probed every this many ticks (a probe holds the
    /// lock for an instant, which would refuse an agent that starts at that very moment).
    pub lock_every: u32,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning {
            note_after: NOTE_AFTER,
            service_recheck: SERVICE_RECHECK,
            idle_rechecks: SERVICE_IDLE_RECHECKS,
            connected_for: CONNECTED_FOR,
            ticks_per_second: TICKS_PER_SECOND,
            cpu_every: CPU_EVERY,
            lock_every: LOCK_EVERY,
        }
    }
}

/// What a look found about the discovery file: a hint, never proof.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HintSeen {
    /// The pid the file names.
    pub pid: u32,
    /// When the agent wrote it (ms since the epoch).
    pub started_ms: i64,
    /// The agent's version.
    pub version: String,
    /// The agent's protocol.
    pub protocol: u32,
    /// The file is of this build (version and protocol): its agent can be used, otherwise it is asked to exit.
    pub same_build: bool,
}

/// Whether some process holds the agent's lock (the authority for "is an agent running here").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockSeen {
    /// Nobody.
    Free,
    /// An agent (not necessarily one this app started).
    Held,
}

/// The answer to a [`Command::Look`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// The look it answers.
    pub seq: Seq,
    /// When it was taken (ms since the epoch).
    pub now_ms: i64,
    /// The discovery file, if any.
    pub hint: Option<HintSeen>,
    /// The agent lock; `None` when the look did not probe it (a probe takes the lock for an instant, which would refuse an
    /// agent that starts at that very moment, so the looks of a start in progress leave it alone).
    pub lock: Option<LockSeen>,
    /// The login service, when asked (and a probe exists).
    pub service: Option<ServiceState>,
    /// The launcher's child with the generation the launcher held when it looked.
    pub child: Option<(Generation, Child)>,
    /// The child's tree CPU time when it was asked for ([`Command::Look`] `cpu`), `Unavailable` when the OS did not say.
    pub cpu: Cpu,
}

/// Who runs the agent we are connected to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The very process this app started (held, so its pid is its own).
    ThisApp(Generation),
    /// An agent somebody else started (the login service, another app instance): never ended, only asked to exit.
    Other,
}

/// What a quit decided, once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QuitOutcome {
    /// The agent this app started is stopped (see [`stop_agent_on_quit`]).
    pub stopped_own: bool,
}

/// Why a connect was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// To use the agent.
    Use,
    /// To ask the agent of another build to exit.
    AskToExit,
}

/// What the link is doing or waiting for.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Phase {
    /// Not begun.
    Idle,
    /// A look (the discovery file, the lock, the service) is out and decides what to do next.
    Looking {
        /// The look.
        seq: Seq,
    },
    /// A connection is being made.
    Connecting {
        /// The connect.
        seq: Seq,
        /// Why.
        purpose: Purpose,
        /// The look the decision was taken from (a failed connect decides again from it).
        look: Box<Look>,
    },
    /// The agent this app started is starting (judged by the shared start judge).
    Starting {
        /// Its generation.
        child: Generation,
    },
    /// The login service supervises: its agent is awaited.
    WaitingForService,
    /// An agent nobody here started holds the data folder.
    WaitingForHolder,
    /// An agent of another build was asked to exit; waits for the data folder to be free.
    Replacing {
        /// The connection it was asked over.
        peer: PeerId,
    },
    /// The service was removed and its agent asked to exit; waits for the data folder to be free.
    TakingBack {
        /// The connection it was asked over.
        peer: PeerId,
    },
    /// The held child is being ended (Retry, or handed over to the service); waits for it to be gone.
    Ending {
        /// Its generation.
        child: Generation,
    },
    /// Connected to an agent.
    Connected {
        /// The connection.
        peer: PeerId,
        /// Whose agent.
        owner: Owner,
    },
    /// An attempt failed; looks again after a number of ticks.
    BackingOff {
        /// Failed attempts in a row.
        failures: u32,
        /// Ticks left.
        ticks_left: u32,
    },
    /// Quit: the held child is being ended; waits for it.
    Quitting {
        /// The decision.
        outcome: QuitOutcome,
    },
    /// The end.
    Stopped {
        /// The decision.
        outcome: QuitOutcome,
    },
}

impl Phase {
    /// The variant's name, for the log (a fixed word, never the fields: they hold sequence numbers and connection ids).
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Idle => "Idle",
            Phase::Looking { .. } => "Looking",
            Phase::Connecting { .. } => "Connecting",
            Phase::Starting { .. } => "Starting",
            Phase::WaitingForService => "WaitingForService",
            Phase::WaitingForHolder => "WaitingForHolder",
            Phase::Replacing { .. } => "Replacing",
            Phase::TakingBack { .. } => "TakingBack",
            Phase::Ending { .. } => "Ending",
            Phase::BackingOff { .. } => "BackingOff",
            Phase::Connected { .. } => "Connected",
            Phase::Quitting { .. } => "Quitting",
            Phase::Stopped { .. } => "Stopped",
        }
    }
}

/// What the world or the person tells the machine.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Event {
    /// Start supervising.
    Begin,
    /// One observation of the world is due (a pace tick).
    Tick,
    /// The answer to a look.
    Looked(Look),
    /// The answer to a launch.
    Launched {
        /// The generation launched.
        child: Generation,
        /// When it was launched (ms since the epoch, taken before the launch), and its pid; or why not.
        result: Result<Launch, String>,
    },
    /// The held child of this generation is gone (the answer to [`Command::EndHeldChild`]).
    ChildEnded {
        /// Its generation.
        child: Generation,
        /// Its exit code.
        code: Option<i32>,
    },
    /// A connection was made.
    Connected {
        /// The connect.
        seq: Seq,
        /// The connection.
        peer: PeerId,
        /// The pid the discovery file named.
        pid: Option<u32>,
    },
    /// A connection could not be made.
    ConnectFailed {
        /// The connect.
        seq: Seq,
        /// Why.
        why: String,
    },
    /// A connection ended.
    PeerClosed {
        /// The connection.
        peer: PeerId,
    },
    /// The person chose Retry on the stall with this id (`None`: on whatever wait is in progress).
    Retry {
        /// The stall it answers.
        stall: Option<StallId>,
    },
    /// Quit.
    Quit,
    /// The login service may have changed (the switch was turned, or a connection was made).
    ServiceChanged,
    /// A service is registered or not (decides what a quit does).
    ServiceInstalled(bool),
}

/// A launch that worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Launch {
    /// When it was launched (ms since the epoch).
    pub at_ms: i64,
    /// The process id the launcher reports for the child.
    pub pid: Option<u32>,
}

/// What the machine asks the world to do. None of them blocks the owner thread: a slow one runs on a worker and answers
/// with an [`Event`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Read the discovery file and the lock (and ask the service when `service`); answers [`Event::Looked`].
    Look {
        /// The look.
        seq: Seq,
        /// Ask the OS about the login service too.
        service: bool,
        /// Read the CPU time of the held child's tree (the start judge reads it every few ticks, never every look).
        cpu: bool,
        /// Probe the agent lock. Not while the agent this app launched is starting: the probe holds the lock for an instant and
        /// would make that agent's own claim fail (exit 3) when the two meet.
        lock: bool,
        /// The look that starts an attempt and decides what to do: a discovery file beside a held lock is connected to without
        /// asking the login service (`SMAppService.status` can take tens of seconds at login).
        deciding: bool,
    },
    /// Start the agent (never while a child is held); answers [`Event::Launched`].
    Launch {
        /// The generation.
        child: Generation,
        /// Retry chose to launch although the data folder is held (the agent's own lock makes the loser exit with code 3).
        anyway: bool,
    },
    /// Connect to the agent the discovery file names; answers [`Event::Connected`] or [`Event::ConnectFailed`].
    Connect {
        /// The connect.
        seq: Seq,
        /// The look whose file this connect is for (the adapter acts on that file or re-reads, never on an older look's).
        look: Seq,
        /// Why: the file must be of this build to be used, of another to be asked to exit.
        purpose: Purpose,
    },
    /// Ask the agent on this connection to exit, then close it: the only act towards an agent this app did not start.
    AskToExit {
        /// The connection.
        peer: PeerId,
    },
    /// End the held child of this generation, nothing else: waits a grace, then ends it through its handle; answers
    /// [`Event::ChildEnded`].
    EndHeldChild {
        /// The generation.
        child: Generation,
    },
    /// Close this connection.
    CloseConnection {
        /// The connection.
        peer: PeerId,
    },
    /// Remove the discovery file seen at the last look if it is still that very file (it names an agent that is gone).
    RemoveHint {
        /// The look that saw the file.
        look: Seq,
    },
    /// What requests go to.
    UsePeer(Option<PeerId>),
    /// The status for the window and the chip.
    Report(AgentStatus),
    /// A wait is taking long: what it waits for, with Retry.
    Stall {
        /// Its id (Retry names it).
        id: StallId,
        /// What.
        stall: Stall,
        /// The sentence.
        message: String,
    },
    /// The stall is over.
    ClearStall,
    /// The end: the outcome of the quit.
    Done(QuitOutcome),
}

/// What observers see after every step.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// The phase.
    pub phase: Phase,
    /// The stall shown, if any.
    pub stall: Option<(StallId, Stall)>,
    /// A connection was made at least once.
    pub ever_connected: bool,
    /// The generation of the child held, if any.
    pub child: Option<Generation>,
    /// How many looks have been answered (counted observations of the world, for tests and the chip).
    pub looks: u64,
    /// How many `ServiceChanged` events have had their look answered.
    pub changes_answered: u64,
}

impl Snapshot {
    /// Connected to an agent.
    pub fn connected(&self) -> bool {
        matches!(self.phase, Phase::Connected { .. })
    }

    /// Connected to the agent this app started.
    pub fn started_by_this_app(&self) -> bool {
        matches!(
            self.phase,
            Phase::Connected {
                owner: Owner::ThisApp(_),
                ..
            }
        )
    }

    /// Ended by a quit.
    pub fn stopped(&self) -> bool {
        matches!(self.phase, Phase::Stopped { .. })
    }

    /// The quit decision, once it is taken.
    pub fn outcome(&self) -> Option<QuitOutcome> {
        match self.phase {
            Phase::Quitting { outcome } | Phase::Stopped { outcome } => Some(outcome),
            _ => None,
        }
    }

    /// Nothing more to wait for a first connection: connected, or ended, or a stall or failure is shown.
    pub fn settled(&self) -> bool {
        self.ever_connected
            || self.stall.is_some()
            || matches!(
                self.phase,
                Phase::Idle | Phase::Connected { .. } | Phase::BackingOff { .. } | Phase::Quitting { .. } | Phase::Stopped { .. }
            )
    }
}

/// The kind of wait a phase is, for counting its ticks: the count starts anew when the kind changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cat {
    Deciding,
    Starting,
    Service,
    Holder,
    Retiring,
    Ending,
    Backoff,
}

fn cat_of(phase: &Phase) -> Option<Cat> {
    match phase {
        Phase::Looking { .. } | Phase::Connecting { .. } => Some(Cat::Deciding),
        Phase::Starting { .. } => Some(Cat::Starting),
        Phase::WaitingForService => Some(Cat::Service),
        Phase::WaitingForHolder => Some(Cat::Holder),
        Phase::Replacing { .. } | Phase::TakingBack { .. } => Some(Cat::Retiring),
        Phase::Ending { .. } => Some(Cat::Ending),
        Phase::BackingOff { .. } => Some(Cat::Backoff),
        Phase::Idle | Phase::Connected { .. } | Phase::Quitting { .. } | Phase::Stopped { .. } => None,
    }
}

/// The child this app started and holds until it is gone.
#[derive(Debug, Clone, Hash)]
struct Held {
    generation: Generation,
    pid: Option<u32>,
    /// The launch happened (its answer arrived).
    launched: bool,
    launched_ms: i64,
}

#[derive(Debug, Clone, Default, Hash)]
struct Wait {
    cat: Option<Cat>,
    /// The wait this deciding step interrupted: coming back to it keeps its count and its stall.
    from: Option<Cat>,
    ticks: u32,
    since_ms: Option<i64>,
    idle_rechecks: u32,
}

/// Faults the model check must catch: each puts back the shape of a bug (ADR 0025 section 3).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mutant {
    /// No fault.
    None,
    /// Bug 1: a fixed wait ends a slow start and launches another agent.
    LaunchOnFixedWait,
    /// Bug 2: a wait shows no cause.
    EndlessWait,
    /// Bug 3: a stale discovery file counts as the agent.
    StaleHintAsAgent,
    /// Bug 4: the connection closing is taken for the lock being released.
    LaunchOnPeerClosed,
    /// Bug 5: the quit outcome is read again after the child has gone.
    QuitOutcomeReread,
    /// The lock is never probed: a holder that has not written its file yet is not seen, and an agent is launched beside it.
    NeverProbeLock,
    /// The hole of the first draft: a child's end is taken for the current child's, and Retry lets go of the child at once.
    GenerationBlind,
}

/// The supervisor's whole state; only [`Supervisor::step`] changes it.
#[derive(Debug, Clone)]
pub struct Supervisor {
    phase: Phase,
    child: Option<Held>,
    generation: Generation,
    seq: Seq,
    look_out: Option<Seq>,
    service: ServiceState,
    fallback: bool,
    launch_anyway: bool,
    reask: bool,
    wait: Wait,
    stall: Option<(StallId, Stall)>,
    stall_ids: StallId,
    judge: Option<StartJudge>,
    failures: u32,
    connected_ticks: u32,
    ever_connected: bool,
    looks: u64,
    changed_seq: Option<Seq>,
    changes_answered: u64,
    tuning: Tuning,
    #[cfg(test)]
    mutant: Mutant,
}

impl Default for Supervisor {
    fn default() -> Self {
        Supervisor::new()
    }
}

impl Supervisor {
    /// A supervisor that has not begun.
    pub fn new() -> Supervisor {
        Supervisor {
            phase: Phase::Idle,
            child: None,
            generation: 0,
            seq: 0,
            look_out: None,
            service: ServiceState::default(),
            fallback: false,
            launch_anyway: false,
            reask: false,
            wait: Wait::default(),
            stall: None,
            stall_ids: 0,
            judge: None,
            failures: 0,
            connected_ticks: 0,
            ever_connected: false,
            looks: 0,
            changed_seq: None,
            changes_answered: 0,
            tuning: Tuning::default(),
            #[cfg(test)]
            mutant: Mutant::None,
        }
    }

    /// A supervisor with other counts than the production ones (a test that must not meet a stall).
    pub fn with_tuning(tuning: Tuning) -> Supervisor {
        Supervisor {
            tuning,
            ..Supervisor::new()
        }
    }

    /// A supervisor with other counts and a fault put back (the model check).
    #[cfg(test)]
    pub fn tuned(tuning: Tuning, mutant: Mutant) -> Supervisor {
        Supervisor {
            tuning,
            mutant,
            ..Supervisor::new()
        }
    }

    /// What observers see of this state.
    pub fn snapshot_now(&self) -> Snapshot {
        self.snapshot()
    }

    /// A supervisor with a fault put back (tests of the model check).
    #[cfg(test)]
    pub fn with_mutant(mutant: Mutant) -> Supervisor {
        Supervisor {
            mutant,
            ..Supervisor::new()
        }
    }

    /// The phase.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The generation of the child held, if any.
    pub fn held(&self) -> Option<Generation> {
        self.child.as_ref().map(|h| h.generation)
    }

    /// Whether the login service is known to supervise the agent.
    pub fn service(&self) -> ServiceState {
        self.service
    }

    /// Whether a wait has gone on for [`NOTE_AFTER`] ticks without showing its cause (invariant 3: never, after a step).
    pub fn overdue(&self) -> bool {
        self.wait.cat.is_some()
            && self.wait.ticks >= self.tuning.note_after
            && self.stall.is_none()
            && !matches!(self.phase, Phase::Quitting { .. })
    }

    /// The ticks the current wait has counted.
    pub fn wait_ticks(&self) -> u32 {
        self.wait.ticks
    }

    // ------------------------------------------------------------------------------------------------ helpers

    fn next_seq(&mut self) -> Seq {
        self.seq += 1;
        self.seq
    }

    /// Changes the phase. The count of a wait (and the stall it showed) starts anew when the kind of wait changes, except
    /// that a look or a connect made from inside a wait (`Deciding`) and then back into the same wait carries it on.
    fn set_phase(&mut self, phase: Phase, out: &mut Vec<Command>) {
        let cat = cat_of(&phase);
        if cat != self.wait.cat {
            let interrupts =
                cat == Some(Cat::Deciding) && matches!(self.wait.cat, Some(Cat::Starting | Cat::Service | Cat::Holder | Cat::Retiring));
            let resumes = self.wait.cat == Some(Cat::Deciding) && cat.is_some() && self.wait.from == cat;
            if interrupts {
                self.wait.from = self.wait.cat;
                self.wait.cat = cat;
            } else if resumes {
                self.wait.from = None;
                self.wait.cat = cat;
            } else {
                self.reset_wait(out);
                self.wait.cat = cat;
            }
        }
        self.phase = phase;
    }

    /// Forgets the count and the stall of the wait in progress (a Retry, a new kind of wait).
    fn reset_wait(&mut self, out: &mut Vec<Command>) {
        self.wait = Wait::default();
        self.clear_stall(out);
    }

    fn clear_stall(&mut self, out: &mut Vec<Command>) {
        if self.stall.take().is_some() {
            out.push(Command::ClearStall);
        }
    }

    fn show_stall(&mut self, stall: Stall, message: &str, out: &mut Vec<Command>) {
        if self.stall.is_some() {
            return;
        }
        self.stall_ids += 1;
        self.stall = Some((self.stall_ids, stall));
        out.push(Command::Stall {
            id: self.stall_ids,
            stall,
            message: message.to_string(),
        });
    }

    fn issue_look(&mut self, service: bool, cpu: bool, lock: bool, out: &mut Vec<Command>) -> Seq {
        let seq = self.next_seq();
        self.look_out = Some(seq);
        out.push(Command::Look {
            seq,
            service,
            cpu,
            lock,
            deciding: false,
        });
        seq
    }

    /// Looks to decide what to do: the start of every attempt.
    fn start_looking(&mut self, out: &mut Vec<Command>) {
        let seq = self.next_seq();
        self.look_out = Some(seq);
        self.set_phase(Phase::Looking { seq }, out);
        out.push(Command::Look {
            seq,
            service: true,
            cpu: false,
            lock: true,
            deciding: true,
        });
    }

    fn connect(&mut self, purpose: Purpose, look: Look, out: &mut Vec<Command>) {
        let look_seq = look.seq;
        let seq = self.next_seq();
        self.set_phase(
            Phase::Connecting {
                seq,
                purpose,
                look: Box::new(look),
            },
            out,
        );
        out.push(Command::Connect {
            seq,
            look: look_seq,
            purpose,
        });
    }

    fn launch(&mut self, out: &mut Vec<Command>) {
        debug_assert!(self.child.is_none(), "a launch while a child is held");
        self.generation += 1;
        let anyway = std::mem::take(&mut self.launch_anyway);
        self.child = Some(Held {
            generation: self.generation,
            pid: None,
            launched: false,
            launched_ms: 0,
        });
        self.begin_judging();
        self.set_phase(Phase::Starting { child: self.generation }, out);
        out.push(Command::Launch {
            child: self.generation,
            anyway,
        });
    }

    fn begin_judging(&mut self) {
        self.judge = Some(StartJudge::new(
            Patience {
                note_after: self.tuning.note_after,
                tick: std::time::Duration::from_millis(1000 / u64::from(self.tuning.ticks_per_second.max(1))),
                cpu_every: self.tuning.cpu_every,
                ..Patience::default()
            },
            "The Dyno agent",
        ));
    }

    /// An attempt failed: say so and look again after a backoff counted in ticks.
    fn fail(&mut self, message: String, out: &mut Vec<Command>) {
        self.failures += 1;
        out.push(Command::Report(AgentStatus::new(
            state_after_failures(self.failures),
            Some(message),
        )));
        self.set_phase(
            Phase::BackingOff {
                failures: self.failures,
                ticks_left: u32::try_from(backoff(self.failures).as_secs()).unwrap_or(15) * self.tuning.ticks_per_second,
            },
            out,
        );
        if self.failures >= FAILED_AFTER {
            self.show_stall(
                Stall::AgentKeepsFailing,
                "The Dyno agent keeps failing to start. Its log is in the Dyno data folder.",
                out,
            );
        }
    }

    // ------------------------------------------------------------------------------------------------ the step

    fn on_tick(&mut self, out: &mut Vec<Command>) {
        if matches!(self.phase, Phase::Connected { .. }) {
            self.connected_ticks = self.connected_ticks.saturating_add(1);
            if self.connected_ticks >= self.tuning.connected_for {
                self.failures = 0;
            }
            return;
        }
        if self.wait.cat.is_none() {
            return;
        }
        if let Phase::BackingOff { failures, ticks_left } = self.phase {
            if ticks_left <= 1 {
                out.push(Command::Report(AgentStatus::new(state_after_failures(failures), None)));
                self.start_looking(out);
                return;
            }
            self.phase = Phase::BackingOff {
                failures,
                ticks_left: ticks_left - 1,
            };
        }
        self.wait.ticks = self.wait.ticks.saturating_add(1);
        if self.wait.ticks >= self.tuning.note_after && !self.mutated_endless() {
            self.show_wait_stall(out);
        }
        // The look of a wait: one at a time, so a slow answer is never piled on.
        let wants_look = matches!(
            self.phase,
            Phase::Starting { .. }
                | Phase::WaitingForService
                | Phase::WaitingForHolder
                | Phase::Replacing { .. }
                | Phase::TakingBack { .. }
        );
        if wants_look && self.look_out.is_none() {
            let service = matches!(self.phase, Phase::WaitingForService) && self.wait.ticks.is_multiple_of(self.tuning.service_recheck);
            let cpu = matches!(self.phase, Phase::Starting { .. }) && self.wait.ticks.is_multiple_of(self.tuning.cpu_every);
            // The lock is probed on the ticks that need it: never while our own agent starts, on the recheck ticks while a
            // service's agent or a holder is awaited, every tick only while a retired agent is awaited (its release is the
            // event waited for, and nothing is starting).
            let lock = match self.phase {
                Phase::Starting { .. } => false,
                Phase::WaitingForService | Phase::WaitingForHolder => service || self.wait.ticks.is_multiple_of(self.tuning.lock_every),
                _ => true,
            };
            self.issue_look(service, cpu, lock, out);
        }
        #[cfg(test)]
        if self.mutant == Mutant::LaunchOnFixedWait
            && matches!(self.phase, Phase::Starting { .. })
            && self.wait.ticks >= self.tuning.note_after
        {
            // Bug 1: the old fixed wait gave up on the slow agent and launched another beside it.
            self.child = None;
            self.launch(out);
        }
    }

    fn mutated_endless(&self) -> bool {
        #[cfg(test)]
        return self.mutant == Mutant::EndlessWait;
        #[cfg(not(test))]
        false
    }

    /// The cause of the wait the link is in, shown after [`NOTE_AFTER`] ticks.
    fn show_wait_stall(&mut self, out: &mut Vec<Command>) {
        let (stall, message) = match self.phase {
            Phase::Starting { .. } => (
                Stall::AgentNotAnswering,
                "The Dyno agent is still starting. A slow disk or a busy computer can take a while.",
            ),
            Phase::WaitingForService => (
                Stall::WaitingForService,
                "Waiting for the Dyno agent that your system starts at login.",
            ),
            Phase::WaitingForHolder | Phase::Replacing { .. } | Phase::TakingBack { .. } => (
                Stall::OtherAgentHolds,
                "Another Dyno agent holds the data folder and has not answered. It may be starting slowly, or running on another computer that shares the folder.",
            ),
            Phase::Looking { .. } | Phase::Connecting { .. } => (
                Stall::ServiceNotAnswering,
                "Dyno is waiting for the system to answer (the login service or the agent's files).",
            ),
            Phase::Ending { .. } => (Stall::AgentNotAnswering, "The Dyno agent is being ended."),
            Phase::BackingOff { .. } => (
                Stall::AgentKeepsFailing,
                "The Dyno agent keeps failing to start. Its log is in the Dyno data folder.",
            ),
            Phase::Idle | Phase::Connected { .. } | Phase::Quitting { .. } | Phase::Stopped { .. } => return,
        };
        self.show_stall(stall, message, out);
    }

    fn on_looked(&mut self, look: Look, out: &mut Vec<Command>) {
        if self.look_out != Some(look.seq) {
            return; // the answer to a look that was superseded: stale by construction
        }
        self.look_out = None;
        self.looks += 1;
        if self.changed_seq == Some(look.seq) {
            self.changes_answered += 1;
        }
        let was_supervised = self.service.supervises;
        if let Some(s) = look.service {
            self.service = s;
            if !s.supervises {
                self.fallback = false;
            }
        }
        self.wait.since_ms.get_or_insert(look.now_ms);
        match self.phase.clone() {
            Phase::Looking { .. } => self.decide(look, true, out),
            Phase::Starting { child } => self.judge_start(child, look, out),
            Phase::WaitingForService => {
                if look.service.is_some() {
                    // The lock tells a service that supervises on paper from one that starts nothing.
                    if look.lock == Some(LockSeen::Held) {
                        self.wait.idle_rechecks = 0;
                    } else if look.lock == Some(LockSeen::Free) && self.service.supervises {
                        self.wait.idle_rechecks += 1;
                        if self.wait.idle_rechecks >= self.tuning.idle_rechecks {
                            self.fallback = true;
                            out.push(Command::Report(AgentStatus::new(
                                AgentState::Starting,
                                Some(
                                    "The login service did not start the Dyno agent, so Dyno started its own. Check Dyno in System Settings, Login Items."
                                        .into(),
                                ),
                            )));
                        }
                    }
                }
                self.decide(look, true, out);
            }
            Phase::WaitingForHolder => self.decide(look, true, out),
            Phase::Replacing { .. } | Phase::TakingBack { .. } => self.retiring_look(look, out),
            Phase::Connected { peer, owner } => self.service_look(was_supervised, peer, owner, out),
            _ => {}
        }
    }

    /// Looks while a retired agent is awaited: nothing is launched, and nothing reconnects to the agent that was asked to
    /// exit, until the data folder is free (invariant: the lock, not the connection, says the agent is gone).
    fn retiring_look(&mut self, look: Look, out: &mut Vec<Command>) {
        if look.lock == Some(LockSeen::Free) {
            if look.hint.is_some() {
                out.push(Command::RemoveHint { look: look.seq });
            }
            self.reask = false;
            self.reset_wait(out);
            self.start_looking(out);
            return;
        }
        if std::mem::take(&mut self.reask) && look.hint.is_some() {
            self.connect(Purpose::AskToExit, look, out);
        }
    }

    /// A look while connected: the service may have changed.
    fn service_look(&mut self, was_supervised: bool, peer: PeerId, owner: Owner, out: &mut Vec<Command>) {
        let s = self.service;
        match owner {
            Owner::ThisApp(g) if s.supervises && !self.fallback && self.child.is_some() => {
                // The service supervises now: ours is ended (only the process this app started) so the service's, refused
                // while ours held the data folder, takes over.
                out.push(Command::UsePeer(None));
                out.push(Command::AskToExit { peer });
                out.push(Command::EndHeldChild { child: g });
                self.set_phase(Phase::Ending { child: g }, out);
            }
            Owner::Other if was_supervised && !s.supervises && !s.installed => {
                // The service was removed but its agent still answers: asked to exit over its connection, never killed.
                out.push(Command::UsePeer(None));
                out.push(Command::AskToExit { peer });
                self.set_phase(Phase::TakingBack { peer }, out);
            }
            _ => {}
        }
    }

    /// What to do from a look when no child is held and no agent answers yet.
    fn decide(&mut self, look: Look, usable_hint: bool, out: &mut Vec<Command>) {
        if let Some(held) = self.child.clone() {
            // A child is held (the connection to it dropped): it is starting again until a look says it is gone.
            match look.child {
                Some((g, Child::Exited { .. })) if g == held.generation => self.child = None,
                _ => {
                    self.begin_judging();
                    self.set_phase(Phase::Starting { child: held.generation }, out);
                    return;
                }
            }
        }
        if usable_hint {
            if let Some(h) = look.hint.clone() {
                #[cfg(test)]
                if self.mutant == Mutant::StaleHintAsAgent {
                    // Bug 3: the file is taken for the agent.
                    out.push(Command::UsePeer(Some(0)));
                    self.set_phase(
                        Phase::Connected {
                            peer: 0,
                            owner: Owner::Other,
                        },
                        out,
                    );
                    return;
                }
                let fresh = self.wait.since_ms.is_some_and(|s| h.started_ms >= s);
                if h.same_build {
                    // Worth a connection only if something may answer: the lock is held, or the file is newer than the wait.
                    if look.lock == Some(LockSeen::Held) || fresh {
                        self.connect(Purpose::Use, look, out);
                        return;
                    }
                } else if look.lock == Some(LockSeen::Held) {
                    self.connect(Purpose::AskToExit, look, out);
                    return;
                } else {
                    out.push(Command::RemoveHint { look: look.seq });
                }
            }
        }
        if self.service.supervises && !self.fallback {
            self.set_phase(Phase::WaitingForService, out);
            return;
        }
        #[cfg(test)]
        let look = Look {
            lock: if self.mutant == Mutant::NeverProbeLock {
                Some(LockSeen::Free)
            } else {
                look.lock
            },
            ..look
        };
        match look.lock {
            Some(LockSeen::Held) if !self.launch_anyway => self.set_phase(Phase::WaitingForHolder, out),
            Some(_) => self.launch(out),
            // A look that did not probe the lock decides nothing about launching: a wait goes on, and a decision asks again
            // with the lock (the file and the service were all it could tell).
            None => {
                if !matches!(self.phase, Phase::WaitingForService | Phase::WaitingForHolder) {
                    self.start_looking(out);
                }
            }
        }
    }

    fn judge_start(&mut self, child: Generation, look: Look, out: &mut Vec<Command>) {
        let Some(held) = self.child.clone().filter(|h| h.generation == child) else {
            return;
        };
        let (liveness, pid) = match look.child {
            Some((g, Child::Exited { code })) if g == child => (
                Liveness::Exited(ExitInfo {
                    status: code,
                    stderr_tail: None,
                }),
                held.pid,
            ),
            Some((g, Child::Running { pid })) if g == child => (Liveness::Running, Some(pid)),
            Some((g, Child::Stopped { pid })) if g == child => (Liveness::Paused, Some(pid)),
            _ => (Liveness::Running, held.pid),
        };
        if let (Some(h), None) = (self.child.as_mut(), held.pid) {
            h.pid = pid;
        }
        let ready = held.launched
            && matches!(liveness, Liveness::Running)
            && look.hint.as_ref().is_some_and(|h| match pid {
                Some(p) => h.pid == p && h.started_ms >= held.launched_ms,
                None => h.started_ms >= held.launched_ms,
            });
        let liveness_now = liveness.clone();
        let reading = Reading {
            liveness,
            ready,
            progress: 0,
            cpu: look.cpu,
        };
        let state = match self.judge.as_mut() {
            Some(j) => j.observe(&reading),
            None => return,
        };
        match state {
            StartState::Starting => {
                // It runs again after a pause: the note of the pause is withdrawn.
                if matches!(self.stall, Some((_, Stall::AgentPaused))) && !matches!(liveness_now, Liveness::Paused) {
                    self.clear_stall(out);
                }
            }
            StartState::Ready => self.connect(Purpose::Use, look, out),
            StartState::Stalled(report) => {
                if matches!(report.cause, StallCause::Slow | StallCause::Paused) || report.cause.is_stuck() {
                    self.show_stall(Stall::from(report.cause), &report.message, out);
                }
            }
            StartState::Exited(info) => {
                self.child = None;
                self.judge = None;
                if info.status == Some(EXIT_ALREADY_RUNNING) {
                    // Another agent holds the data folder: waits for it, with the stall and Retry.
                    self.set_phase(Phase::WaitingForHolder, out);
                } else {
                    let message = match info.status {
                        Some(c) => format!("The Dyno agent stopped while starting (exit code {c}). Its log is in the Dyno data folder."),
                        None => "The Dyno agent was ended while starting.".into(),
                    };
                    self.fail(message, out);
                }
            }
        }
    }

    fn on_connected(&mut self, seq: Seq, peer: PeerId, pid: Option<u32>, out: &mut Vec<Command>) {
        let purpose = match &self.phase {
            Phase::Connecting { seq: s, purpose, .. } if *s == seq => *purpose,
            _ => {
                out.push(Command::CloseConnection { peer }); // an answer to a connect that was given up
                return;
            }
        };
        match purpose {
            Purpose::Use => {
                let owner = match (&self.child, pid) {
                    (Some(h), Some(p)) if h.pid == Some(p) => Owner::ThisApp(h.generation),
                    _ => Owner::Other,
                };
                if owner == Owner::Other {
                    self.fallback = false; // the service's agent is up after all
                }
                self.ever_connected = true;
                self.connected_ticks = 0;
                self.look_out = None;
                out.push(Command::UsePeer(Some(peer)));
                self.set_phase(Phase::Connected { peer, owner }, out);
                out.push(Command::Report(AgentStatus::new(AgentState::Running, None)));
                // What the service says now decides hand-over and take-back, and what a quit does.
                self.issue_look(true, false, false, out);
            }
            Purpose::AskToExit => {
                out.push(Command::AskToExit { peer });
                self.look_out = None;
                self.set_phase(Phase::Replacing { peer }, out);
            }
        }
    }

    fn on_connect_failed(&mut self, seq: Seq, out: &mut Vec<Command>) {
        let look = match &self.phase {
            Phase::Connecting { seq: s, look, .. } if *s == seq => look.clone(),
            _ => return,
        };
        if let Some(h) = self.child.clone() {
            // The agent this app started did not answer: it is still starting, as far as the judge can tell.
            self.begin_judging_keep();
            self.set_phase(Phase::Starting { child: h.generation }, out);
        } else {
            self.decide(*look, false, out);
        }
    }

    fn begin_judging_keep(&mut self) {
        if self.judge.is_none() {
            self.begin_judging();
        } else if let Some(j) = self.judge.as_mut() {
            // The file named the agent but it did not answer: a tick that did not become ready.
            let _ = j.observe(&Reading {
                liveness: Liveness::Running,
                ready: false,
                progress: 0,
                cpu: Cpu::NotRead,
            });
        }
    }

    fn on_quit(&mut self, out: &mut Vec<Command>) {
        if matches!(self.phase, Phase::Quitting { .. } | Phase::Stopped { .. }) {
            return;
        }
        // Decided here, once, from the state at this event; nothing read later changes it.
        let outcome = QuitOutcome {
            stopped_own: stop_agent_on_quit(self.child.is_some(), self.service.installed),
        };
        self.look_out = None;
        match self.phase.clone() {
            Phase::Connected { peer, owner } => {
                out.push(Command::UsePeer(None));
                if outcome.stopped_own && matches!(owner, Owner::ThisApp(_)) {
                    out.push(Command::AskToExit { peer });
                } else {
                    out.push(Command::CloseConnection { peer });
                }
            }
            Phase::Replacing { peer } | Phase::TakingBack { peer } => out.push(Command::CloseConnection { peer }),
            _ => out.push(Command::UsePeer(None)),
        }
        match (&self.child, outcome.stopped_own) {
            (Some(h), true) => {
                out.push(Command::EndHeldChild { child: h.generation });
                self.set_phase(Phase::Quitting { outcome }, out);
            }
            _ => {
                self.set_phase(Phase::Stopped { outcome }, out);
                out.push(Command::Done(outcome));
            }
        }
    }

    fn on_retry(&mut self, stall: Option<StallId>, out: &mut Vec<Command>) {
        if let Some(id) = stall {
            if self.stall.map(|(s, _)| s) != Some(id) {
                return; // it answers a stall that is over
            }
        }
        // A paused agent is never ended or replaced by a Retry (ADR 0031): it continues by itself.
        if matches!(self.stall, Some((_, Stall::AgentPaused))) {
            return;
        }
        match self.phase.clone() {
            Phase::Starting { child } => {
                // The child is let go only when it is gone: nothing is launched before its end is reported.
                self.judge = None;
                self.look_out = None;
                out.push(Command::EndHeldChild { child });
                #[cfg(test)]
                if self.mutant == Mutant::GenerationBlind {
                    self.child = None;
                    self.start_looking(out);
                    return;
                }
                self.set_phase(Phase::Ending { child }, out);
            }
            Phase::WaitingForService => {
                self.fallback = true;
                self.reset_wait(out);
                self.start_looking(out);
            }
            Phase::WaitingForHolder => {
                self.launch_anyway = true;
                self.reset_wait(out);
                self.start_looking(out);
            }
            Phase::Replacing { .. } | Phase::TakingBack { .. } => {
                self.reask = true;
                if self.look_out.is_none() {
                    self.issue_look(false, false, true, out);
                }
            }
            Phase::Looking { .. } | Phase::Connecting { .. } | Phase::BackingOff { .. } => {
                // A look that never came is asked for again; a backoff is cut short.
                self.reset_wait(out);
                self.start_looking(out);
            }
            Phase::Idle | Phase::Ending { .. } | Phase::Connected { .. } | Phase::Quitting { .. } | Phase::Stopped { .. } => {}
        }
    }
}

impl Machine for Supervisor {
    type Event = Event;
    type Command = Command;
    type Snapshot = Snapshot;

    fn step(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        if let Phase::Stopped { .. } = self.phase {
            if let Event::Connected { peer, .. } = event {
                out.push(Command::CloseConnection { peer });
            }
            return out;
        }
        match event {
            Event::Begin => {
                if self.phase == Phase::Idle {
                    out.push(Command::Report(AgentStatus::new(AgentState::Starting, None)));
                    self.start_looking(&mut out);
                }
            }
            Event::Tick => self.on_tick(&mut out),
            Event::Looked(look) => self.on_looked(look, &mut out),
            Event::Launched { child, result } => self.on_launched(child, result, &mut out),
            Event::ChildEnded { child, .. } => self.on_child_ended(child, &mut out),
            Event::Connected { seq, peer, pid } => {
                if matches!(self.phase, Phase::Quitting { .. }) {
                    out.push(Command::CloseConnection { peer });
                } else {
                    self.on_connected(seq, peer, pid, &mut out);
                }
            }
            Event::ConnectFailed { seq, .. } => self.on_connect_failed(seq, &mut out),
            Event::PeerClosed { peer } => self.on_peer_closed(peer, &mut out),
            Event::Retry { stall } => self.on_retry(stall, &mut out),
            Event::Quit => self.on_quit(&mut out),
            Event::ServiceChanged => {
                // A look already out may have been taken before the change: the newest look wins.
                if !matches!(self.phase, Phase::Idle | Phase::Quitting { .. }) {
                    self.changed_seq = Some(self.issue_look(true, false, true, &mut out));
                }
            }
            Event::ServiceInstalled(installed) => self.service.installed = installed,
        }
        out
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase.clone(),
            stall: self.stall,
            ever_connected: self.ever_connected,
            child: self.held(),
            looks: self.looks,
            changes_answered: self.changes_answered,
        }
    }

    fn finished(&self) -> bool {
        matches!(self.phase, Phase::Stopped { .. })
    }
}

impl Supervisor {
    fn on_launched(&mut self, child: Generation, result: Result<Launch, String>, out: &mut Vec<Command>) {
        let Some(held) = self.child.as_mut().filter(|h| h.generation == child) else {
            return; // about a child this machine no longer holds
        };
        match result {
            Ok(l) => {
                held.launched = true;
                held.launched_ms = l.at_ms;
                held.pid = l.pid.or(held.pid);
                // A quit that began while the launch was in flight needs nothing more: the adapter runs the launch and the
                // `EndHeldChild` that follows it in order, so the child that now exists is the one that gets ended.
            }
            Err(message) => {
                self.child = None;
                match self.phase {
                    Phase::Starting { child: g } if g == child => {
                        self.judge = None;
                        self.fail(message, out);
                    }
                    Phase::Ending { child: g } if g == child => {
                        // Retry was ending a child that never existed: nothing to wait for (the end queued behind the launch
                        // finds nothing and its report is stale by then).
                        self.start_looking(out);
                    }
                    Phase::Quitting { outcome } => {
                        self.set_phase(Phase::Stopped { outcome }, out);
                        out.push(Command::Done(outcome));
                    }
                    _ => {}
                }
            }
        }
    }

    fn on_child_ended(&mut self, child: Generation, out: &mut Vec<Command>) {
        let current = self.child.as_ref().map(|h| h.generation);
        if current != Some(child) && !self.mutated_blind() {
            return; // the end of an old child never touches the current one
        }
        if let Phase::Quitting { outcome } = self.phase {
            self.child = None;
            #[cfg(test)]
            let outcome = if self.mutant == Mutant::QuitOutcomeReread {
                // Bug 5: the outcome is read again, now that the child is gone.
                QuitOutcome {
                    stopped_own: self.child.is_some(),
                }
            } else {
                outcome
            };
            self.set_phase(Phase::Stopped { outcome }, out);
            out.push(Command::Done(outcome));
            return;
        }
        self.child = None;
        if let Phase::Ending { child: g } = self.phase {
            if g == child || self.mutated_blind() {
                self.start_looking(out);
            }
        }
    }

    fn mutated_blind(&self) -> bool {
        #[cfg(test)]
        return self.mutant == Mutant::GenerationBlind;
        #[cfg(not(test))]
        false
    }

    fn on_peer_closed(&mut self, peer: PeerId, out: &mut Vec<Command>) {
        match self.phase.clone() {
            Phase::Connected { peer: p, .. } if p == peer => {
                out.push(Command::UsePeer(None));
                let message = "The connection to the Dyno agent dropped; reconnecting.".to_string();
                if self.connected_ticks >= self.tuning.connected_for {
                    out.push(Command::Report(AgentStatus::new(AgentState::Restarting, Some(message))));
                    self.start_looking(out);
                } else {
                    // It dropped soon after it was made: a crash loop backs off instead of hammering.
                    self.fail(message, out);
                }
            }
            #[cfg(test)]
            Phase::TakingBack { peer: p } if p == peer && self.mutant == Mutant::LaunchOnPeerClosed => {
                // Bug 4: the connection closing is taken for the lock being released.
                self.launch(out);
            }
            _ => {} // a connection that is not the current one, or one asked to exit: the lock says when it is gone
        }
    }
}

impl std::hash::Hash for Look {
    /// Everything but the CPU reading (a duration the model check keeps constant).
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (self.seq, self.now_ms, &self.hint, self.lock, self.service, self.child).hash(h);
    }
}

#[cfg(test)]
impl std::hash::Hash for Supervisor {
    /// The state as the model check sees it: every field, the judge by its (small) debug text.
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (
            &self.phase,
            &self.child,
            self.generation,
            self.seq,
            self.look_out,
            self.service,
            self.fallback,
        )
            .hash(h);
        (
            self.launch_anyway,
            self.reask,
            &self.wait,
            self.stall,
            self.stall_ids,
            self.failures,
        )
            .hash(h);
        (
            self.connected_ticks,
            self.ever_connected,
            self.looks,
            self.changed_seq,
            self.changes_answered,
            self.tuning,
            self.mutant,
        )
            .hash(h);
        format!("{:?}", self.judge).hash(h);
    }
}

#[cfg(test)]
mod table;
