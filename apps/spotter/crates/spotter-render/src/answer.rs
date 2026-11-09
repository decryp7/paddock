//! Waiting for the answers a browser owes right after it started (ADR 0071, on ADR 0031's start judge; the same mechanism
//! as `paddock-chromium`'s `answer`, browser-driver copy): the commands that open a page (creating it, attaching to it,
//! enabling its domains) are judged on the browser's liveness, not on a fixed wall-clock deadline. A browser that is alive
//! and working is waited for, however loaded the machine; one that ended, or whose DevTools connection closed, fails at
//! the next look; one that looks stuck (no CPU for [`Patience::idle`], spinning for [`Patience::spin`], unobservable for
//! [`Patience::hang_guard`]) fails with the judge's reason. The overall bound is the browser's own time limit
//! (`LaunchOptions::max_lifetime`): its watchdog ends the browser, and the wait sees it end.
//!
//! A fixed 10 s deadline here failed a headful render on a loaded CI Mac while the browser was up
//! (`Timeout("a command")`, spotter-release run 7853; paddock-chromium had the same failure in train runs 7771, 7796, 7810).

use crate::cdp::{Connection, Pending};
use crate::{Error, Result};
use paddock_proc::{ExitInfo, Liveness, Pace, Patience, ProcessId, StartProbe, StartState, StartWatch, Watched};
use serde_json::Value;
use std::cell::RefCell;
use std::time::Duration;

/// What says whether the browser that owes the answer is still there.
pub(crate) trait Life {
    /// Whether the browser's process runs; [`Liveness::Exited`] with its error output when it ended.
    fn liveness(&mut self) -> Liveness;
    /// The browser's process, by identity (its run state tells a stopped browser from an idle one).
    fn pid(&self) -> Option<ProcessId>;
    /// The CPU time of the browser's tree, `None` when the OS does not say.
    fn cpu_time(&mut self) -> Option<Duration>;
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
        if self.slot.borrow().is_none() {
            if let Some(r) = self.pending.wait(tick) {
                *self.slot.borrow_mut() = Some(r);
            }
        }
        true
    }
}

/// Sends `method` (to `session`, or to the browser for `None`) and waits for its answer while `life` says the browser is
/// alive and working ([`await_answer`]): the [`crate::page::Ask`] of a browser that just started.
pub(crate) fn ask<L: Life>(
    conn: &Connection,
    life: &RefCell<L>,
    patience: Patience,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value> {
    let pending = conn.send(session, method, params)?;
    await_answer(&pending, &mut *life.borrow_mut(), patience)
}

/// Waits for the reply to the command sent as `pending` while `life` says the browser is there.
pub(crate) fn await_answer(pending: &Pending<'_>, life: &mut dyn Life, patience: Patience) -> Result<Value> {
    let slot: Slot = RefCell::new(None);
    let probe = Probe { life, slot: &slot };
    let pace = ReplyPace { pending, slot: &slot };
    let mut watch = StartWatch::new(patience, "the browser", probe, pace);
    let seen = loop {
        match watch.wait() {
            // A slow but working browser is only noted by the judge; the wait goes on.
            Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {}
            seen => break seen,
        }
    };
    drop(watch);
    match seen {
        Watched::State(StartState::Ready) => slot.into_inner().unwrap_or(Err(Error::Closed)),
        Watched::State(StartState::Exited(ExitInfo { .. })) => Err(Error::Closed),
        Watched::State(StartState::Stalled(_)) => Err(Error::Timeout("an answer from a browser that stopped working")),
        // The pace never cancels, and a watch never answers `Starting`.
        Watched::State(StartState::Starting) | Watched::Cancelled => Err(Error::Closed),
    }
}

#[cfg(test)]
mod tests;
