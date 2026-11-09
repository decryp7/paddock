//! Waiting for the answers a browser owes right after it started (ADR 0071, on ADR 0031's start judge): the first
//! commands (its page list, attaching to a page, enabling its domains) are judged on the browser's liveness, not on a
//! fixed wall-clock deadline. A browser that is alive and working is waited for, however loaded the machine; one that
//! ended, or whose DevTools connection closed, fails at the next look; one that looks stuck (no CPU for
//! [`Patience::idle`], spinning for [`Patience::spin`], unobservable for [`Patience::hang_guard`]) fails with the judge's
//! reason. A stopped browser is waited for. The overall bound is the browser's own time limit
//! (`LaunchOptions::max_lifetime`): its watchdog ends the browser, and the wait sees it end.
//!
//! A fixed 5 s or 10 s deadline here failed a browser that had started and was answering slowly on a loaded CI Mac
//! (load 50-124; `Timeout("a command")` from `start_page`, train runs 7771, 7796 and 7810).

use crate::cdp::Pending;
use crate::{Error, Result};
use paddock_proc::{
    ExitInfo, Liveness, Pace, Patience, ProcessId, StartProbe, StartState, StartWatch, Watched,
};
use serde_json::Value;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// After this long without an answer a note goes to the log (the wait goes on).
const NOTE_AFTER: Duration = Duration::from_secs(5);

/// What says whether the browser that owes the answer is still there.
pub(crate) trait Life {
    /// Whether the browser's process runs; [`Liveness::Exited`] with its error output when it ended.
    fn liveness(&mut self) -> Liveness;
    /// The browser's process, by identity (its run state tells a stopped browser from an idle one).
    fn pid(&self) -> Option<ProcessId>;
    /// The CPU time of the browser's tree, `None` when the OS does not say.
    fn cpu_time(&mut self) -> Option<Duration>;
    /// Which of the browser's own limits ended it ("its time limit (1800 s)"), if one did.
    fn ended_by(&self) -> Option<String>;
}

/// The reply, once it came (or the connection ended).
type Slot = RefCell<Option<Result<Value>>>;

struct Probe<'a> {
    life: &'a mut dyn Life,
    slot: &'a Slot,
}

impl StartProbe for Probe<'_> {
    fn liveness(&mut self) -> Liveness {
        self.life.liveness()
    }
    fn pid(&self) -> Option<ProcessId> {
        self.life.pid()
    }
    fn ready(&mut self) -> bool {
        self.slot.borrow().is_some()
    }
    fn cpu_time(&mut self) -> Option<Duration> {
        self.life.cpu_time()
    }
}

/// One tick is one wait for the reply: it ends as soon as the reply comes, so an answer is never held back by the pace.
struct ReplyPace<'a> {
    pending: &'a Pending<'a>,
    slot: &'a Slot,
}

impl Pace for ReplyPace<'_> {
    fn wait(&mut self, tick: Duration) -> bool {
        if self.slot.borrow().is_none()
            && let Some(r) = self.pending.wait(tick)
        {
            *self.slot.borrow_mut() = Some(r);
        }
        true
    }
}

/// The patience for one answer: the launch's, with a note after [`NOTE_AFTER`] worth of ticks.
fn with_note(p: Patience) -> Patience {
    let ticks = NOTE_AFTER.as_millis() / p.tick.as_millis().max(1);
    Patience {
        note_after: u32::try_from(ticks).unwrap_or(u32::MAX).max(1),
        ..p
    }
}

/// Waits for the reply to `method` (sent as `pending`) while `life` says the browser is there; `phase` names the step
/// in the log ("opening the first page").
pub(crate) fn await_answer(
    pending: &Pending<'_>,
    method: &str,
    phase: &str,
    life: &mut dyn Life,
    patience: Patience,
) -> Result<Value> {
    let began = Instant::now();
    let pid = life.pid().map(|p| p.pid());
    let slot: Slot = RefCell::new(None);
    let probe = Probe {
        life: &mut *life,
        slot: &slot,
    };
    let pace = ReplyPace {
        pending,
        slot: &slot,
    };
    let mut watch = StartWatch::new(with_note(patience), "the browser", probe, pace);
    let mut noted = false;
    let seen = loop {
        match watch.wait() {
            Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {
                noted = true;
                log::info!(
                    "The browser (pid {pid:?}) has not answered {method} yet after {:.1} s while {phase}; it is alive, so \
                     the wait goes on ({})",
                    began.elapsed().as_secs_f64(),
                    r.message
                );
            }
            seen => break seen,
        }
    };
    drop(watch);
    let secs = began.elapsed().as_secs_f64();
    match seen {
        // The connection closes when the browser is ended, often before its exit is seen: a limit that ended it is named
        // either way (its flag is set before the kill).
        Watched::State(StartState::Ready | StartState::Exited(_))
            if matches!(&*slot.borrow(), None | Some(Err(Error::Closed)))
                && let Some(why) = life.ended_by() =>
        {
            log::warn!(
                "The browser (pid {pid:?}) was ended by {why} after {secs:.1} s while {phase} ({method} unanswered)"
            );
            Err(Error::Launch(format!(
                "the browser was ended by {why} while {phase}"
            )))
        }
        Watched::State(StartState::Ready) => {
            let reply = slot.into_inner().unwrap_or(Err(Error::Closed));
            match &reply {
                Err(Error::Closed) => log::warn!(
                    "The browser's DevTools connection closed after {secs:.1} s while {phase} ({method} unanswered, pid {pid:?})"
                ),
                _ if noted => {
                    log::info!("The browser answered {method} after {secs:.1} s while {phase}")
                }
                _ => {}
            }
            reply
        }
        Watched::State(StartState::Exited(ExitInfo {
            status,
            stderr_tail,
        })) => {
            log::warn!(
                "The browser (pid {pid:?}) ended after {secs:.1} s while {phase} ({method} unanswered; exit {status:?}; \
                 browser output: {})",
                stderr_tail.as_deref().unwrap_or("none")
            );
            Err(Error::Closed)
        }
        Watched::State(StartState::Stalled(r)) => {
            log::warn!(
                "The browser (pid {pid:?}) looks stuck after {secs:.1} s while {phase} ({method} unanswered): {}",
                r.message
            );
            Err(Error::Timeout(
                "an answer from a browser that stopped working",
            ))
        }
        // The pace never cancels, and a watch never answers `Starting`.
        Watched::State(StartState::Starting) | Watched::Cancelled => Err(Error::Closed),
    }
}

#[cfg(test)]
mod tests;
