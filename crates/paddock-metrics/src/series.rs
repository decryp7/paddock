//! What a series is: a point type and how samples fold into a bucket.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (the wall clock), the time of every point.
pub type Millis = u64;

/// Where the bucket of `bucket_ms` that holds `at` starts.
pub fn bucket_start(at: Millis, bucket_ms: u64) -> Millis {
    at - at % bucket_ms.max(1)
}

/// A kind of point a [`TieredStore`](crate::TieredStore) can keep: how its samples are folded into a bucket and back into
/// a point. The bucket (`Acc`) may hold anything bounded (sums, a minimum, a histogram, a capped map of counts).
pub trait Series: Clone + Serialize + DeserializeOwned {
    /// The running aggregate of one bucket.
    type Acc: Clone + PartialEq + std::fmt::Debug + Serialize + DeserializeOwned;
    /// When the point is from.
    fn at(&self) -> Millis;
    /// A bucket starting at `start` holding `s`.
    fn begin(start: Millis, s: &Self) -> Self::Acc;
    /// Adds `s` to the bucket.
    fn absorb(acc: &mut Self::Acc, s: &Self);
    /// The point the bucket stands for (its `at` is the bucket start).
    fn point(acc: &Self::Acc) -> Self;
    /// Where the bucket starts.
    fn start(acc: &Self::Acc) -> Millis;
}

/// A running mean that cannot overflow (128-bit sum) and is zero without samples.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mean {
    sum: u128,
    n: u64,
}

impl Mean {
    /// A mean of one value.
    pub fn of(v: u64) -> Mean {
        let mut m = Mean::default();
        m.add(v);
        m
    }

    /// Adds a value.
    pub fn add(&mut self, v: u64) {
        self.sum += u128::from(v);
        self.n += 1;
    }

    /// The mean, rounded down; 0 without values.
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            0
        } else {
            u64::try_from(self.sum / u128::from(self.n)).unwrap_or(u64::MAX)
        }
    }

    /// How many values.
    pub fn count(&self) -> u64 {
        self.n
    }
}
