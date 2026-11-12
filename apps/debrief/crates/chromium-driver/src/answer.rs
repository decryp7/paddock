//! Waiting for the browser's answers (ADR 0071, on ADR 0031's start judge).
//!
//! The product ([`Answers::Bounded`]), as the canonical `paddock-chromium` does since `8d8424805` and `2dd883b47`: the first
//! answers after a start (the page list, attaching to the start page, enabling its domains) are judged on the browser's
//! liveness with the launch's patience, never a fixed 5 s or 10 s deadline: a browser that is alive and working is waited
//! for however loaded the machine, one that ended or whose DevTools connection closed fails at the next look, one that
//! looks stuck fails with the judge's reason. The browser's time limit (`LaunchOptions::max_lifetime`) is the overall bound.
//! Once the first page is open, each later command has its fixed bound again, so a command late in a long-lived browser's
//! life cannot wait for its whole time limit.
//!
//! Tests ([`Answers::Judged`], set from `paddock_testkit::start`): every answer and every page load is waited for until it
//! comes, the browser ends or its connection closes, and only a wall-clock hang guard ends a wait that never does, as a
//! hang. A loaded host then makes a test slower, never red (Debrief CI 7963: `navigate: Timeout` in the renderer's
//! real-browser tests, behind fixed 5 s and 10 s command deadlines).
//!
//! Every wait that gives up logs the command, the step and the bound or reason (ER-27), and its error says the same.

use crate::cdp::{Connection, Pending};
use crate::{Error, Result};
use paddock_proc::{ExitInfo, Liveness, Pace, Patience, Presence, ProcessId, StallCause, StartProbe, StartState, StartWatch, Watched};
use serde_json::Value;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// After this long without an answer a note goes to the log (the wait goes on).
const NOTE_AFTER: Duration = Duration::from_secs(5);

/// How the driver waits for the browser's answers and page loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Answers {
    /// The product: the answers of the start judged with the launch's patience, later ones within their fixed bounds.
    #[default]
    Bounded,
    /// For tests (ADR 0071): every answer and page load waited for while the browser is there, judged with `patience`
    /// (`paddock_testkit::start::PATIENCE`: only the guard ends a wait), ended as a hang after `guard` of wall time
    /// (`paddock_testkit::HANG_GUARD`). A caller's own load timeout is then not used.
    Judged { patience: Patience, guard: Duration },
}

/// What the waits of one browser share: its connection, its identity and limits, and how answers are waited for.
pub(crate) struct Answering {
    pub(crate) conn: Arc<Connection>,
    id: Option<ProcessId>,
    timed_out: Arc<AtomicBool>,
    max_lifetime: Duration,
    launch_patience: Patience,
    answers: Answers,
    /// Set once the first page is open: the start is over.
    opened: AtomicBool,
}

/// One judged wait's settings: the patience, and the wall-clock guard (none in the product, whose bound is the lifetime).
#[derive(Clone, Copy)]
struct Judging {
    patience: Patience,
    guard: Option<Duration>,
}

impl Answering {
    pub(crate) fn new(
        conn: Arc<Connection>,
        id: Option<ProcessId>,
        timed_out: Arc<AtomicBool>,
        max_lifetime: Duration,
        launch_patience: Patience,
        answers: Answers,
    ) -> Answering {
        Answering {
            conn,
            id,
            timed_out,
            max_lifetime,
            launch_patience,
            answers,
            opened: AtomicBool::new(false),
        }
    }

    /// The start is over: the first page is open.
    pub(crate) fn opened(&self) {
        self.opened.store(true, Ordering::Release);
    }

    fn judging(&self) -> Option<Judging> {
        match self.answers {
            Answers::Judged { patience, guard } => Some(Judging {
                patience,
                guard: Some(guard),
            }),
            Answers::Bounded if !self.opened.load(Ordering::Acquire) => Some(Judging {
                patience: self.launch_patience,
                guard: None,
            }),
            Answers::Bounded => None,
        }
    }

    /// Whether every wait is judged (the test form); a page load then ignores its caller's timeout.
    pub(crate) fn judges_loads(&self) -> bool {
        matches!(self.answers, Answers::Judged { .. })
    }

    /// Sends `method` and waits for its answer: judged while the browser starts (and always in the test form), else within
    /// `bound`. `phase` names the step in the log and the error ("opening the start page").
    pub(crate) fn ask(&self, session: Option<&str>, method: &str, params: Value, phase: &str, bound: Duration) -> Result<Value> {
        let pending = self.conn.send(session, method, params)?;
        match self.judging() {
            Some(j) => self.judged(method, phase, j, |tick| reply_within(&pending, tick)),
            None => match pending.wait(bound) {
                Some(reply) => {
                    self.name_a_limit(&reply, method);
                    reply
                }
                None => {
                    log::warn!("The browser did not answer {method} within {bound:?} while {phase}");
                    Err(Error::Timeout(format!("{method} while {phase}, no answer within {bound:?}")))
                }
            },
        }
    }

    /// Waits for the page event `pred` accepts after `since` (a load), judged like an answer: only in the test form.
    pub(crate) fn await_event(
        &self,
        since: u64,
        what: &str,
        phase: &str,
        pred: impl Fn(&crate::cdp::Event) -> bool,
    ) -> Result<crate::cdp::Event> {
        let j = self.judging().unwrap_or(Judging {
            patience: self.launch_patience,
            guard: None,
        });
        self.judged(what, phase, j, |tick| match self.conn.wait_event(since, tick, &pred) {
            Some(e) => Some(Ok(e)),
            None if self.conn.is_closed() => Some(Err(Error::Closed)),
            None => None,
        })
    }

    fn name_a_limit(&self, reply: &Result<Value>, method: &str) {
        if matches!(reply, Err(Error::Closed)) && self.timed_out.load(Ordering::Acquire) {
            log::warn!(
                "The browser was ended by its time limit ({} s) while {method} was owed",
                self.max_lifetime.as_secs()
            );
        }
    }

    /// Which of the browser's own limits ended it, if one did.
    fn ended_by(&self) -> Option<String> {
        self.timed_out
            .load(Ordering::Acquire)
            .then(|| format!("its time limit ({} s)", self.max_lifetime.as_secs()))
    }

    /// The judged wait: `step(tick)` waits up to one tick for the thing and gives it once it came (or the connection ended).
    fn judged<T>(&self, what: &str, phase: &str, j: Judging, step: impl FnMut(Duration) -> Option<Result<T>>) -> Result<T> {
        let began = Instant::now();
        let pid = self.id.map(ProcessId::pid);
        let slot: RefCell<Option<Result<T>>> = RefCell::new(None);
        let probe = Probe { id: self.id, slot: &slot };
        let pace = Steps {
            step,
            slot: &slot,
            began,
            guard: j.guard,
        };
        let mut watch = StartWatch::new(with_note(j.patience), "the browser", probe, pace);
        let mut noted = false;
        let seen = loop {
            match watch.wait() {
                Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {
                    if !matches!(r.cause, StallCause::Paused) {
                        noted = true;
                    }
                    log::info!(
                        "The browser (pid {pid:?}) has not answered {what} yet after {:.1} s while {phase}; it is there, so the \
                         wait goes on ({})",
                        began.elapsed().as_secs_f64(),
                        r.message
                    );
                }
                seen => break seen,
            }
        };
        drop(watch);
        let secs = began.elapsed().as_secs_f64();
        let came = slot.into_inner();
        match seen {
            // The connection closes when the browser is ended, often before its exit is seen: a limit that ended it is named
            // either way (its flag is set before the kill).
            Watched::State(StartState::Ready | StartState::Exited(_))
                if matches!(&came, None | Some(Err(Error::Closed))) && self.ended_by().is_some() =>
            {
                let why = self.ended_by().unwrap_or_default();
                log::warn!("The browser (pid {pid:?}) was ended by {why} after {secs:.1} s while {phase} ({what} unanswered)");
                Err(Error::Launch(format!("the browser was ended by {why} while {phase}")))
            }
            Watched::State(StartState::Ready) => {
                let reply = came.unwrap_or(Err(Error::Closed));
                match &reply {
                    Err(Error::Closed) => log::warn!(
                        "The browser's DevTools connection closed after {secs:.1} s while {phase} ({what} unanswered, pid {pid:?})"
                    ),
                    _ if noted => log::info!("The browser answered {what} after {secs:.1} s while {phase}"),
                    _ => {}
                }
                reply
            }
            Watched::State(StartState::Exited(ExitInfo { status, .. })) => {
                log::warn!("The browser (pid {pid:?}) ended after {secs:.1} s while {phase} ({what} unanswered; exit {status:?})");
                Err(Error::Closed)
            }
            Watched::State(StartState::Stalled(r)) => {
                log::warn!(
                    "The browser (pid {pid:?}) looks stuck after {secs:.1} s while {phase} ({what} unanswered): {}",
                    r.message
                );
                Err(Error::Timeout(format!(
                    "{what} while {phase}: the browser looks stuck ({})",
                    r.message
                )))
            }
            Watched::Cancelled => {
                let guard = j.guard.unwrap_or_default();
                log::warn!(
                    "The browser (pid {pid:?}) did not answer {what} within the {guard:?} hang guard while {phase}, though it \
                     was there: a hang, not a slow answer"
                );
                Err(Error::Timeout(format!(
                    "{what} while {phase}: no answer within the {guard:?} hang guard though the browser was there (a hang)"
                )))
            }
            // A watch never answers `Starting`.
            Watched::State(StartState::Starting) => Err(Error::Closed),
        }
    }
}

/// One tick of a reply's wait: the reply, or the connection's end, once it came.
fn reply_within(pending: &Pending<'_>, tick: Duration) -> Option<Result<Value>> {
    pending.wait(tick)
}

/// The patience for one wait, with a note after [`NOTE_AFTER`] worth of ticks.
fn with_note(p: Patience) -> Patience {
    let ticks = NOTE_AFTER.as_millis() / p.tick.as_millis().max(1);
    Patience {
        note_after: u32::try_from(ticks).unwrap_or(u32::MAX).max(1),
        ..p
    }
}

/// What the judge looks at: the browser by identity (a table that cannot be read counts as there, rule 53) and whether the
/// thing waited for came.
struct Probe<'a, T> {
    id: Option<ProcessId>,
    slot: &'a RefCell<Option<Result<T>>>,
}

impl<T> StartProbe for Probe<'_, T> {
    fn liveness(&mut self) -> Liveness {
        match self.id.map(ProcessId::presence) {
            Some(Presence::Gone) => Liveness::Exited(ExitInfo {
                status: None,
                stderr_tail: None,
            }),
            _ => Liveness::Running,
        }
    }
    fn pid(&self) -> Option<ProcessId> {
        self.id
    }
    fn ready(&mut self) -> bool {
        self.slot.borrow().is_some()
    }
    fn cpu_time(&mut self) -> Option<Duration> {
        paddock_proc::tree_cpu_time(self.id?)
    }
}

/// One tick is one wait for the thing: it ends as soon as the thing comes, so an answer is never held back by the pace. With
/// a guard, the wait is cancelled once the guard has passed on the wall clock (the judge counts ticks, which a paused host
/// stretches).
struct Steps<'a, F, T> {
    step: F,
    slot: &'a RefCell<Option<Result<T>>>,
    began: Instant,
    guard: Option<Duration>,
}

impl<F: FnMut(Duration) -> Option<Result<T>>, T> Pace for Steps<'_, F, T> {
    fn wait(&mut self, tick: Duration) -> bool {
        if self.guard.is_some_and(|g| self.began.elapsed() >= g) {
            return false;
        }
        if self.slot.borrow().is_none() {
            // pace: a slice of the wait for the answer, which ends at once when it comes
            if let Some(r) = (self.step)(tick) {
                *self.slot.borrow_mut() = Some(r);
            }
        }
        true
    }
}
