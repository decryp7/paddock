//! The one stream of typed trace events (locate, step and detection events), a bounded ring that keeps the recent past,
//! and a subscriber fan-out. Everything the HUD shows comes from this stream; text in it is [`HudText`], so a raw secret
//! cannot enter it.
//!
//! The ring and the bus are plain single-threaded values. A server wraps the bus in a mutex or feeds it from a channel;
//! the logic (bounds, drop policy, ordering) stays here and is testable without threads.

use super::mask::HudText;
use crate::detect::{ClassId, Score};
use crate::geometry::{Logical, Pt, Rect};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Largest ring or subscriber queue.
pub const MAX_CAPACITY: usize = 65_536;
/// Most subscribers on one bus.
pub const MAX_SUBSCRIBERS: usize = 64;

/// Why a ring or bus could not be built or used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceError {
    /// A capacity of zero or above [`MAX_CAPACITY`].
    #[error("a capacity must be between 1 and {MAX_CAPACITY}, got {0}")]
    Capacity(usize),
    /// The bus is full of subscribers.
    #[error("a bus holds at most {MAX_SUBSCRIBERS} subscribers")]
    TooManySubscribers,
    /// No subscriber with this id (already removed).
    #[error("unknown subscriber {0}")]
    UnknownSubscriber(u32),
}

/// Microseconds since the run started (a monotonic clock read by the caller; the core never reads time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Micros(pub u64);

/// A resolver layer (proposal section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Result cache.
    Cache,
    /// Template matching.
    Template,
    /// OCR anchor.
    OcrAnchor,
    /// The trained detector.
    Detector,
    /// Optional VLM grounding.
    Vlm,
}

/// How a locate ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocateStatus {
    /// One clear match.
    Found,
    /// Several matches too close to call.
    Ambiguous,
    /// Nothing above the threshold.
    NotFound,
    /// The model needs labelling data.
    NeedsLabelling,
}

/// What a layer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerOutcome {
    /// Produced candidates.
    Hit,
    /// Produced none.
    Miss,
    /// Not run (disabled or unnecessary).
    Skipped,
}

/// A locate request's life.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum LocateEvent {
    /// The request started.
    Started {
        /// The request id.
        id: u64,
    },
    /// One layer finished.
    Layer {
        /// The request id.
        id: u64,
        /// Which layer.
        layer: Layer,
        /// What it did.
        outcome: LayerOutcome,
        /// Time spent, in microseconds.
        took_us: u64,
    },
    /// The request ended.
    Finished {
        /// The request id.
        id: u64,
        /// The result.
        status: LocateStatus,
    },
}

/// A step of a test run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StepEvent {
    /// The step began.
    Started {
        /// Step number.
        step: u32,
    },
    /// The step typed text (a secret appears only as a reference).
    Typed {
        /// Step number.
        step: u32,
        /// What was typed.
        text: HudText,
    },
    /// The step clicked at a logical point.
    Clicked {
        /// Step number.
        step: u32,
        /// Where.
        at: Pt<Logical>,
    },
    /// The step ended.
    Finished {
        /// Step number.
        step: u32,
        /// Whether it succeeded.
        ok: bool,
    },
}

/// One detection of a locate, in logical points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvent {
    /// The locate request it belongs to.
    pub locate: u64,
    /// The box.
    pub rect: Rect<Logical>,
    /// The class.
    pub class: ClassId,
    /// The raw detector score.
    pub score: Score,
    /// Whether this is the chosen target.
    pub chosen: bool,
}

/// What happened to a query's hints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum HintEvent {
    /// Every hint fell outside the scope, so the query ran unhinted.
    AllOutsideScope {
        /// The locate request.
        locate: u64,
        /// How many hints there were.
        count: u32,
    },
}

impl HintEvent {
    /// The sentence the HUD and the "Explain this locate" view show.
    pub fn explain(&self) -> String {
        match self {
            Self::AllOutsideScope { count, .. } => format!("all {count} hints were outside the scope; the query ran without hints"),
        }
    }
}

/// The kinds of trace event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum TraceKind {
    /// A locate request's progress.
    Locate(LocateEvent),
    /// A step's progress.
    Step(StepEvent),
    /// A detection.
    Detection(DetectionEvent),
    /// What happened to the query's hints.
    Hint(HintEvent),
}

/// A numbered, timed event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Position in the stream, from 0, assigned by the ring or bus.
    pub seq: u64,
    /// Time since the run started.
    pub at: Micros,
    /// What happened.
    pub event: TraceKind,
}

fn capacity(n: usize) -> Result<usize, TraceError> {
    if (1..=MAX_CAPACITY).contains(&n) {
        Ok(n)
    } else {
        Err(TraceError::Capacity(n))
    }
}

/// A bounded queue that drops its oldest event when full and counts what it dropped.
#[derive(Debug, Clone)]
struct Bounded {
    cap: usize,
    items: VecDeque<TraceEvent>,
    dropped: u64,
}

impl Bounded {
    fn new(cap: usize) -> Result<Self, TraceError> {
        Ok(Self {
            cap: capacity(cap)?,
            items: VecDeque::new(),
            dropped: 0,
        })
    }
    fn push(&mut self, e: TraceEvent) {
        if self.items.len() == self.cap {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(e);
    }
}

/// The ring of the most recent events (what a late-joining viewer, or the "Explain this locate" panel, reads).
#[derive(Debug, Clone)]
pub struct TraceRing {
    inner: Bounded,
    next_seq: u64,
}

impl TraceRing {
    /// A ring holding up to `cap` events.
    pub fn new(cap: usize) -> Result<Self, TraceError> {
        Ok(Self {
            inner: Bounded::new(cap)?,
            next_seq: 0,
        })
    }
    /// Appends an event and returns it with its sequence number.
    pub fn push(&mut self, at: Micros, event: TraceKind) -> TraceEvent {
        let e = TraceEvent {
            seq: self.next_seq,
            at,
            event,
        };
        self.next_seq += 1;
        self.inner.push(e.clone());
        e
    }
    /// Events held, oldest first.
    pub fn events(&self) -> impl Iterator<Item = &TraceEvent> {
        self.inner.items.iter()
    }
    /// Events with `seq >= from`, oldest first (a viewer asks for what it has not seen).
    pub fn since(&self, from: u64) -> Vec<TraceEvent> {
        self.inner.items.iter().filter(|e| e.seq >= from).cloned().collect()
    }
    /// How many events were pushed out of the ring.
    pub fn dropped(&self) -> u64 {
        self.inner.dropped
    }
    /// Number of events held.
    pub fn len(&self) -> usize {
        self.inner.items.len()
    }
    /// True when no event is held.
    pub fn is_empty(&self) -> bool {
        self.inner.items.is_empty()
    }
}

/// A subscriber handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriberId(u32);

/// What a subscriber receives on a drain.
#[derive(Debug, Clone, PartialEq)]
pub struct Drained {
    /// Events in order.
    pub events: Vec<TraceEvent>,
    /// Events lost since the last drain because the subscriber was too slow.
    pub dropped: u64,
}

/// The bus: one ring plus a bounded queue per subscriber. A slow subscriber loses its own oldest events and never blocks
/// the producer or the others. Switched off, [`TraceBus::publish`] does nothing (the "trace off" half of the overhead
/// benchmark).
#[derive(Debug, Clone)]
pub struct TraceBus {
    enabled: bool,
    ring: TraceRing,
    subs: Vec<(u32, Bounded)>,
    next_sub: u32,
}

impl TraceBus {
    /// A bus whose ring keeps `ring_cap` events.
    pub fn new(ring_cap: usize, enabled: bool) -> Result<Self, TraceError> {
        Ok(Self {
            enabled,
            ring: TraceRing::new(ring_cap)?,
            subs: Vec::new(),
            next_sub: 0,
        })
    }
    /// Whether events are recorded.
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    /// Turns recording on or off.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    /// Adds a subscriber with its own queue of `cap` events.
    pub fn subscribe(&mut self, cap: usize) -> Result<SubscriberId, TraceError> {
        if self.subs.len() >= MAX_SUBSCRIBERS {
            return Err(TraceError::TooManySubscribers);
        }
        let q = Bounded::new(cap)?;
        self.next_sub += 1;
        self.subs.push((self.next_sub, q));
        Ok(SubscriberId(self.next_sub))
    }
    /// Removes a subscriber.
    pub fn unsubscribe(&mut self, id: SubscriberId) -> Result<(), TraceError> {
        let before = self.subs.len();
        self.subs.retain(|(i, _)| *i != id.0);
        if self.subs.len() == before {
            Err(TraceError::UnknownSubscriber(id.0))
        } else {
            Ok(())
        }
    }
    /// Records an event and fans it out. Returns its sequence number, or `None` when the bus is off.
    pub fn publish(&mut self, at: Micros, event: TraceKind) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        let e = self.ring.push(at, event);
        let seq = e.seq;
        for (_, q) in &mut self.subs {
            q.push(e.clone());
        }
        Some(seq)
    }
    /// Takes everything queued for a subscriber.
    pub fn drain(&mut self, id: SubscriberId) -> Result<Drained, TraceError> {
        let (_, q) = self
            .subs
            .iter_mut()
            .find(|(i, _)| *i == id.0)
            .ok_or(TraceError::UnknownSubscriber(id.0))?;
        Ok(Drained {
            events: q.items.drain(..).collect(),
            dropped: std::mem::take(&mut q.dropped),
        })
    }
    /// The shared ring.
    pub fn ring(&self) -> &TraceRing {
        &self.ring
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn step(n: u32) -> TraceKind {
        TraceKind::Step(StepEvent::Started { step: n })
    }
    fn steps(e: &[TraceEvent]) -> Vec<u32> {
        e.iter()
            .map(|e| match e.event {
                TraceKind::Step(StepEvent::Started { step }) => step,
                _ => u32::MAX,
            })
            .collect()
    }

    #[test]
    fn ring_keeps_the_newest_and_counts_the_rest() {
        let mut r = TraceRing::new(3).unwrap();
        assert!(r.is_empty());
        for i in 0..5 {
            assert_eq!(r.push(Micros(u64::from(i) * 10), step(i)).seq, u64::from(i));
        }
        assert_eq!((r.len(), r.dropped()), (3, 2));
        assert_eq!(steps(&r.events().cloned().collect::<Vec<_>>()), vec![2, 3, 4]);
        assert_eq!(steps(&r.since(4)), vec![4]);
        assert_eq!(r.since(99), vec![]);
    }

    #[test]
    fn capacities_are_bounded() {
        assert_eq!(TraceRing::new(0).map(|_| ()), Err(TraceError::Capacity(0)));
        assert_eq!(
            TraceRing::new(MAX_CAPACITY + 1).map(|_| ()),
            Err(TraceError::Capacity(MAX_CAPACITY + 1))
        );
        let mut b = TraceBus::new(8, true).unwrap();
        assert_eq!(b.subscribe(0), Err(TraceError::Capacity(0)));
        for _ in 0..MAX_SUBSCRIBERS {
            b.subscribe(1).unwrap();
        }
        assert_eq!(b.subscribe(1), Err(TraceError::TooManySubscribers));
    }

    #[test]
    fn bus_fans_out_and_a_slow_subscriber_only_hurts_itself() {
        let mut b = TraceBus::new(100, true).unwrap();
        let fast = b.subscribe(100).unwrap();
        let slow = b.subscribe(2).unwrap();
        for i in 0..5 {
            assert_eq!(b.publish(Micros(u64::from(i)), step(i)), Some(u64::from(i)));
        }
        let d = b.drain(fast).unwrap();
        assert_eq!((steps(&d.events), d.dropped), (vec![0, 1, 2, 3, 4], 0));
        let d = b.drain(slow).unwrap();
        assert_eq!((steps(&d.events), d.dropped), (vec![3, 4], 3));
        assert_eq!(
            b.drain(slow).unwrap(),
            Drained {
                events: vec![],
                dropped: 0
            }
        );
        assert_eq!(b.ring().len(), 5);
        b.unsubscribe(slow).unwrap();
        assert_eq!(b.drain(slow), Err(TraceError::UnknownSubscriber(slow.0)));
        assert_eq!(b.unsubscribe(slow), Err(TraceError::UnknownSubscriber(slow.0)));
    }

    #[test]
    fn the_all_hints_dropped_event_explains_itself_and_round_trips() {
        let e = HintEvent::AllOutsideScope { locate: 4, count: 3 };
        assert_eq!(e.explain(), "all 3 hints were outside the scope; the query ran without hints");
        let kind = TraceKind::Hint(e);
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(serde_json::from_str::<TraceKind>(&json).unwrap(), kind);
        let mut b = TraceBus::new(4, true).unwrap();
        assert_eq!(b.publish(Micros(0), kind), Some(0));
    }

    #[test]
    fn a_disabled_bus_records_nothing() {
        let mut b = TraceBus::new(10, false).unwrap();
        let s = b.subscribe(10).unwrap();
        assert!(!b.enabled());
        assert_eq!(b.publish(Micros(0), step(0)), None);
        assert!(b.ring().is_empty() && b.drain(s).unwrap().events.is_empty());
        b.set_enabled(true);
        assert_eq!(b.publish(Micros(1), step(1)), Some(0));
    }

    proptest! {
        /// Sequence numbers are gap-free and increasing; a subscriber sees a suffix of the stream, and what it saw plus
        /// what it lost is everything published.
        #[test]
        fn subscribers_see_ordered_suffixes(n in 0usize..200, cap in 1usize..50) {
            let mut b = TraceBus::new(64, true).unwrap();
            let s = b.subscribe(cap).unwrap();
            for i in 0..n {
                b.publish(Micros(i as u64), step(i as u32));
            }
            let d = b.drain(s).unwrap();
            prop_assert_eq!(d.events.len() as u64 + d.dropped, n as u64);
            prop_assert!(d.events.windows(2).all(|w| w[1].seq == w[0].seq + 1));
            prop_assert_eq!(d.events.last().map(|e| e.seq), n.checked_sub(1).map(|v| v as u64));
        }
    }
}
