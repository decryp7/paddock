//! The tiered store.

use crate::retention::Retention;
use crate::series::{Millis, Series, bucket_start};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "P: Serialize",
    deserialize = "P: serde::de::DeserializeOwned"
))]
struct Tier<P: Series> {
    bucket_ms: u64,
    keep: usize,
    points: VecDeque<P>,
    #[serde(default)]
    open: Option<P::Acc>,
}

impl<P: Series> Tier<P> {
    fn new(bucket: Duration, keep: usize) -> Self {
        Tier {
            bucket_ms: u64::try_from(bucket.as_millis()).unwrap_or(u64::MAX).max(1),
            keep,
            points: VecDeque::new(),
            open: None,
        }
    }

    /// Whether the sample was taken (false: it is older than the open bucket).
    fn add(&mut self, s: &P) -> bool {
        let start = bucket_start(s.at(), self.bucket_ms);
        let stored = match &mut self.open {
            Some(acc) if P::start(acc) == start => {
                P::absorb(acc, s);
                true
            }
            // The clock went back: drop the sample rather than reorder.
            Some(acc) if P::start(acc) > start => false,
            open => {
                if let Some(done) = open.take() {
                    self.points.push_back(P::point(&done));
                }
                *open = Some(P::begin(start, s));
                true
            }
        };
        self.trim();
        stored
    }

    fn trim(&mut self) {
        while self.points.len() > self.keep {
            self.points.pop_front();
        }
    }

    fn all(&self) -> Vec<P> {
        let mut v: Vec<P> = self.points.iter().cloned().collect();
        if let Some(acc) = &self.open {
            v.push(P::point(acc));
        }
        v
    }
}

/// What [`TieredStore::push`] did with a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pushed {
    /// At least one tier took it.
    Stored,
    /// It is older than the newest bucket of every tier (the clock went back); it was dropped.
    TooLate,
}

/// A time series kept in tiers: every sample goes into every tier, each folding it into buckets of its own length, and a
/// tier forgets its oldest bucket once it holds more than its `keep`. At most [`Retention::capacity`] finished points
/// (plus one open bucket per tier) are held whatever the uptime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "P: Serialize",
    deserialize = "P: serde::de::DeserializeOwned"
))]
pub struct TieredStore<P: Series> {
    tiers: Vec<Tier<P>>,
}

impl<P: Series> Default for TieredStore<P> {
    fn default() -> Self {
        Self::new(&Retention::default())
    }
}

impl<P: Series> TieredStore<P> {
    /// An empty store with `retention`.
    pub fn new(retention: &Retention) -> Self {
        TieredStore {
            tiers: retention
                .tiers()
                .iter()
                .map(|t| Tier::new(t.bucket, t.keep))
                .collect(),
        }
    }

    /// Fits a store read from a file to `retention`: the tiers must have the same bucket lengths (otherwise the saved
    /// history means something else and an empty store is returned); the `keep` of each tier becomes the new one and the
    /// oldest points beyond it are forgotten.
    pub fn fit(mut self, retention: &Retention) -> Self {
        let same = self.tiers.len() == retention.tiers().len()
            && self
                .tiers
                .iter()
                .zip(retention.tiers())
                .all(|(t, spec)| u128::from(t.bucket_ms) == spec.bucket.as_millis());
        if !same {
            return Self::new(retention);
        }
        for (t, spec) in self.tiers.iter_mut().zip(retention.tiers()) {
            t.keep = spec.keep;
            t.trim();
        }
        self
    }

    /// Adds a sample to every tier.
    pub fn push(&mut self, s: P) -> Pushed {
        let mut stored = false;
        for t in &mut self.tiers {
            stored |= t.add(&s);
        }
        if stored {
            Pushed::Stored
        } else {
            Pushed::TooLate
        }
    }

    /// The newest point of the finest tier (its open bucket).
    pub fn latest(&self) -> Option<P> {
        self.tiers
            .first()
            .and_then(|t| t.open.as_ref())
            .map(P::point)
    }

    /// The tier [`query`](Self::query) reads for `range`: the finest whose coverage includes it, else the coarsest.
    pub fn tier_for(&self, range: Duration) -> usize {
        self.tiers
            .iter()
            .position(|t| u128::from(t.bucket_ms) * t.keep as u128 >= range.as_millis())
            .unwrap_or(self.tiers.len().saturating_sub(1))
    }

    /// The points of the last `range` up to `now`, from the finest tier that covers it.
    pub fn query(&self, now: Millis, range: Duration) -> Vec<P> {
        let from = now.saturating_sub(u64::try_from(range.as_millis()).unwrap_or(u64::MAX));
        self.tiers
            .get(self.tier_for(range))
            .map(Tier::all)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.at() >= from && p.at() <= now)
            .collect()
    }

    /// Finished points kept, over all tiers (bounded by the retention).
    pub fn len(&self) -> usize {
        self.tiers.iter().map(|t| t.points.len()).sum()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.tiers.iter().all(|t| t.open.is_none())
    }

    /// How many tiers.
    pub fn tier_count(&self) -> usize {
        self.tiers.len()
    }
}
