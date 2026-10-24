//! How long a store keeps what, at which resolution.

use std::time::Duration;

/// Most tiers a [`Retention`] may have.
pub const MAX_TIERS: usize = 8;
/// Most points one tier may keep.
pub const MAX_KEEP: usize = 200_000;
/// Most points all tiers together may keep (the bound on memory and on the state file).
pub const MAX_POINTS: usize = 400_000;

/// One tier: a bucket length and how many buckets are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierSpec {
    /// The bucket length.
    pub bucket: Duration,
    /// How many finished buckets are kept.
    pub keep: usize,
}

impl TierSpec {
    /// What the tier covers: `bucket * keep`.
    pub fn span(&self) -> Duration {
        self.bucket
            .saturating_mul(u32::try_from(self.keep).unwrap_or(u32::MAX))
    }
}

/// Why a retention was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionError {
    /// No tiers.
    Empty,
    /// More than [`MAX_TIERS`].
    TooManyTiers,
    /// A bucket of zero length.
    ZeroBucket,
    /// A tier keeps nothing or more than [`MAX_KEEP`].
    BadKeep,
    /// Buckets must get strictly longer from one tier to the next.
    NotIncreasing,
    /// More than [`MAX_POINTS`] in all.
    TooManyPoints,
}

impl std::fmt::Display for RetentionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "a retention needs at least one tier",
            Self::TooManyTiers => "a retention may have at most 8 tiers",
            Self::ZeroBucket => "a tier's bucket must be longer than zero",
            Self::BadKeep => "a tier must keep between 1 and 200000 points",
            Self::NotIncreasing => "the buckets must get longer from tier to tier",
            Self::TooManyPoints => "the tiers keep more than 400000 points together",
        })
    }
}

impl std::error::Error for RetentionError {}

/// The tiers of a store, finest first. Built checked ([`Retention::new`]) or from a preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retention {
    tiers: Vec<TierSpec>,
}

const SECOND: u64 = 1000;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;

fn days(days: u32) -> usize {
    days.clamp(1, 365) as usize
}

impl Retention {
    /// Checks and keeps `tiers`.
    pub fn new(tiers: Vec<TierSpec>) -> Result<Retention, RetentionError> {
        if tiers.is_empty() {
            return Err(RetentionError::Empty);
        }
        if tiers.len() > MAX_TIERS {
            return Err(RetentionError::TooManyTiers);
        }
        let mut total = 0usize;
        let mut last = Duration::ZERO;
        for t in &tiers {
            if t.bucket.is_zero() || t.bucket.as_millis() == 0 {
                return Err(RetentionError::ZeroBucket);
            }
            if t.keep == 0 || t.keep > MAX_KEEP {
                return Err(RetentionError::BadKeep);
            }
            if t.bucket <= last {
                return Err(RetentionError::NotIncreasing);
            }
            last = t.bucket;
            total += t.keep;
        }
        if total > MAX_POINTS {
            return Err(RetentionError::TooManyPoints);
        }
        Ok(Retention { tiers })
    }

    fn preset(spec: &[(u64, usize)]) -> Retention {
        Retention {
            tiers: spec
                .iter()
                .map(|(ms, keep)| TierSpec {
                    bucket: Duration::from_millis(*ms),
                    keep: *keep,
                })
                .collect(),
        }
    }

    /// Pitwall's table, the default: 5 s for an hour, 1 min for a day, 10 min for a week, 1 h for `retention_days`
    /// (1 to 365).
    pub fn standard(retention_days: u32) -> Retention {
        Self::preset(&[
            (5 * SECOND, 720),
            (MINUTE, 1440),
            (10 * MINUTE, 1008),
            (HOUR, days(retention_days) * 24),
        ])
    }

    /// Pitcrew's table for a sampler that runs every 30 s: 30 s for 6 hours, 5 min for 7 days, 1 h for `retention_days`
    /// (1 to 365).
    pub fn sparse(retention_days: u32) -> Retention {
        Self::preset(&[
            (30 * SECOND, 720),
            (5 * MINUTE, 7 * 24 * 12),
            (HOUR, days(retention_days) * 24),
        ])
    }

    /// The tiers, finest first.
    pub fn tiers(&self) -> &[TierSpec] {
        &self.tiers
    }

    /// The most finished points a store with this retention holds (the bound on its memory).
    pub fn capacity(&self) -> usize {
        self.tiers.iter().map(|t| t.keep).sum()
    }

    /// The index of the finest tier that covers `range`, or the coarsest when none does.
    pub fn tier_for(&self, range: Duration) -> usize {
        self.tiers
            .iter()
            .position(|t| t.span() >= range)
            .unwrap_or(self.tiers.len() - 1)
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self::standard(30)
    }
}
