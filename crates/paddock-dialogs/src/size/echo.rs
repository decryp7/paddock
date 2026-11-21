//! The requests a dialog window still expects the OS to echo, as one queue in request order (ADR 0043 §3, amendment
//! 2026-11-21).
//!
//! The OS reports each size we ask for with a `Resized`, in order, or merges several requests and reports only the
//! last. A `Resized` that matches no entry is not ours. One that matches is ours, but when several entries have its size
//! it cannot say which of them it echoes: the oldest (the ones after it still echo, in order) or the newest (the OS
//! merged the ones in between). So:
//!
//! * the entries before the oldest match are **proved merged**: had the OS sent them, their echoes would have come
//!   first. They go, and [`Echo::Own`] says how many.
//! * the oldest match goes; the entries after it, up to the newest match, become [`Expect::Maybe`]: their echoes may
//!   still come and are then not foreign, but they no longer hold the window back (the window already has the newest
//!   one's size). The next echo of a newer entry proves them merged.
//! * while more than [`ECHO_CAP`] echoes are expected, the OS is far behind and an unmatched size may be the echo of
//!   a request already given up on: the user is not inferred from it until the queue drains, as after a scale change.
//!   Nothing is dropped at the cap, so in-order echoes still match. Past [`ECHO_LIMIT`] (the OS stopped echoing) the
//!   oldest entry goes, with inference already off.

use super::Request;
use std::collections::VecDeque;

/// Past this many expected echoes the user is not inferred from an unmatched `Resized` until the queue drains.
pub const ECHO_CAP: usize = 8;

/// The most expected echoes kept (the bound on the queue): past it the oldest entry goes. Reached only when the OS has
/// stopped echoing; inference is already off from [`ECHO_CAP`] on.
pub const ECHO_LIMIT: usize = 8 * ECHO_CAP;

/// What an entry still expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// Not echoed yet: the window is on its way to this size.
    Pending,
    /// A later echo of the same size may already have stood for it; its own echo may still come, in order.
    Maybe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    request: Request,
    expect: Expect,
}

/// What a `Resized` was, by the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Echo {
    /// The echo of one of our requests; `merged` older requests were proved merged by it and left the queue.
    Own { merged: usize },
    /// It matches no request we still expect.
    Unknown,
}

/// The requests whose echoes are still expected, oldest first. Every `Maybe` entry is older than every `Pending` one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EchoQueue {
    entries: VecDeque<Entry>,
    /// More than [`ECHO_CAP`] echoes were expected since the queue was last empty.
    over_cap: bool,
}

impl EchoQueue {
    /// We asked the OS for `request`.
    pub fn push(&mut self, request: Request) {
        self.entries.push_back(Entry {
            request,
            expect: Expect::Pending,
        });
        if self.entries.len() > ECHO_CAP {
            self.over_cap = true;
        }
        if self.entries.len() > ECHO_LIMIT {
            self.entries.pop_front();
        }
    }

    /// The OS reports the inner size `width` by `height`.
    pub fn echo(&mut self, width: u32, height: u32) -> Echo {
        let matches = |e: &Entry| e.request.matches(width, height);
        let Some(oldest) = self.entries.iter().position(matches) else {
            return Echo::Unknown;
        };
        let newest = self.entries.iter().rposition(matches).unwrap_or(oldest);
        for e in self.entries.range_mut(oldest + 1..=newest) {
            e.expect = Expect::Maybe;
        }
        self.entries.drain(..=oldest);
        if self.entries.is_empty() {
            self.over_cap = false;
        }
        Echo::Own { merged: oldest }
    }

    /// The newest request the window is still on its way to.
    pub fn newest_pending(&self) -> Option<Request> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.expect == Expect::Pending)
            .map(|e| e.request)
    }

    /// Requests not echoed yet (the `Maybe` ones are not counted: the window no longer waits for them).
    pub fn pending(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.expect == Expect::Pending)
            .count()
    }

    /// Whether an unmatched `Resized` may be taken for the user: not while the queue is, or was since it last drained,
    /// over [`ECHO_CAP`].
    pub fn infers(&self) -> bool {
        !self.over_cap
    }

    /// Forgets every request (a scale change: the OS resizes the window itself).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.over_cap = false;
    }

    /// The expected echoes for the log, physical px, oldest first: `pending [..] maybe [..]`, at most [`ECHO_CAP`] of
    /// each listed, and whether inference is off.
    pub fn note(&self) -> String {
        let list = |expect: Expect| {
            let all: Vec<Request> = self
                .entries
                .iter()
                .filter(|e| e.expect == expect)
                .map(|e| e.request)
                .collect();
            let mut out: Vec<String> = all
                .iter()
                .take(ECHO_CAP)
                .map(|r| format!("{}x{}", r.width, r.height))
                .collect();
            if all.len() > ECHO_CAP {
                out.push(format!("+{} more", all.len() - ECHO_CAP));
            }
            format!("[{}]", out.join(", "))
        };
        let off = if self.over_cap {
            format!(", no inference until drained (over {ECHO_CAP})")
        } else {
            String::new()
        };
        format!(
            "pending {} maybe {}{off}",
            list(Expect::Pending),
            list(Expect::Maybe)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(height: u32) -> Request {
        Request { width: 400, height }
    }

    fn queue(heights: &[u32]) -> EchoQueue {
        let mut q = EchoQueue::default();
        for h in heights {
            q.push(r(*h));
        }
        q
    }

    #[test]
    fn in_order_echoes_take_their_entries_and_prove_nothing_merged() {
        let mut q = queue(&[300, 400, 500]);
        for h in [300, 400, 500] {
            assert_eq!(q.echo(400, h), Echo::Own { merged: 0 });
        }
        assert_eq!(q, EchoQueue::default());
    }

    #[test]
    fn an_echo_of_a_newer_request_proves_the_older_ones_merged() {
        let mut q = queue(&[300, 400, 500]);
        assert_eq!(q.echo(400, 500), Echo::Own { merged: 2 });
        assert_eq!(q.pending(), 0);
        assert_eq!(q.echo(400, 400), Echo::Unknown, "the merged 400 is gone");
    }

    #[test]
    fn an_echo_equal_to_several_keeps_the_ones_after_the_oldest_as_maybe() {
        let mut q = queue(&[300, 400, 300, 500]);
        assert_eq!(q.echo(400, 300), Echo::Own { merged: 0 });
        assert_eq!(q.note(), "pending [400x500] maybe [400x400, 400x300]");
        assert_eq!(q.pending(), 1);
        assert_eq!(q.newest_pending(), Some(r(500)));
        // In order: the 400 and the second 300 still echo.
        assert_eq!(q.echo(400, 400), Echo::Own { merged: 0 });
        assert_eq!(q.echo(400, 300), Echo::Own { merged: 0 });
        assert_eq!(q.echo(400, 500), Echo::Own { merged: 0 });
        // Or merged: the 500's echo proves them so.
        let mut q = queue(&[300, 400, 300, 500]);
        q.echo(400, 300);
        assert_eq!(q.echo(400, 500), Echo::Own { merged: 2 });
    }

    #[test]
    fn a_match_within_one_physical_px_is_an_echo() {
        let mut q = queue(&[300]);
        assert_eq!(q.echo(401, 299), Echo::Own { merged: 0 });
    }

    #[test]
    fn over_the_cap_inference_is_off_until_the_queue_drains_and_nothing_is_dropped() {
        let heights: Vec<u32> = (0..=ECHO_CAP as u32).map(|i| 300 + 10 * i).collect();
        let mut q = queue(&heights);
        assert!(!q.infers());
        assert!(
            q.note().ends_with(", no inference until drained (over 8)"),
            "{}",
            q.note()
        );
        for h in &heights[..heights.len() - 1] {
            assert_eq!(q.echo(400, *h), Echo::Own { merged: 0 });
            assert!(!q.infers(), "still {} expected", q.pending());
        }
        assert_eq!(
            q.echo(400, *heights.last().unwrap()),
            Echo::Own { merged: 0 }
        );
        assert!(q.infers(), "drained");
    }

    #[test]
    fn the_queue_is_bounded_and_drops_the_oldest_past_its_limit() {
        let heights: Vec<u32> = (0..ECHO_LIMIT as u32 + 2).map(|i| 300 + 10 * i).collect();
        let mut q = queue(&heights);
        assert_eq!(q.pending(), ECHO_LIMIT);
        assert_eq!(q.echo(400, 300), Echo::Unknown, "the oldest went");
        assert!(!q.infers());
        assert!(
            q.note()
                .contains(&format!("+{} more", ECHO_LIMIT - ECHO_CAP)),
            "{}",
            q.note()
        );
    }

    #[test]
    fn clear_forgets_every_request_and_turns_inference_back_on() {
        let heights: Vec<u32> = (0..=ECHO_CAP as u32).map(|i| 300 + 10 * i).collect();
        let mut q = queue(&heights);
        q.clear();
        assert_eq!(q, EchoQueue::default());
        assert!(q.infers());
    }
}
