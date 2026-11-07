/// A set of repeated measurements of one thing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Samples(Vec<f64>);

/// The numbers kept from [`Samples`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// How many samples.
    pub n: usize,
    /// The smallest.
    pub min: f64,
    /// The middle (mean of the two middle ones for an even count). Budgets use this: it ignores one noisy run.
    pub median: f64,
    /// The 95th percentile (nearest rank).
    pub p95: f64,
    /// The largest.
    pub max: f64,
    /// The arithmetic mean.
    pub mean: f64,
    /// The sample standard deviation (n - 1 in the divisor; 0 for a single sample).
    pub std_dev: f64,
}

impl Summary {
    /// The coefficient of variation, `std_dev / |mean|`: how unsteady the repetitions were, as a fraction (0.10 is 10 %).
    /// `None` when the mean is zero, where the ratio means nothing.
    pub fn cv(&self) -> Option<f64> {
        (self.mean != 0.0).then(|| self.std_dev / self.mean.abs())
    }

    /// The spread, `max - min`.
    pub fn range(&self) -> f64 {
        self.max - self.min
    }
}

impl Samples {
    /// No samples yet.
    pub fn new() -> Samples {
        Samples::default()
    }

    /// Adds a sample; NaN and infinite values are ignored (a failed run must not hide behind them).
    pub fn push(&mut self, v: f64) {
        if v.is_finite() {
            self.0.push(v);
        }
    }

    /// How many samples.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The summary, `None` without samples.
    pub fn summary(&self) -> Option<Summary> {
        summarize(&self.0)
    }
}

/// min, median, p95, max, mean and standard deviation of `values` (finite ones only); `None` when there are none.
pub fn summarize(values: &[f64]) -> Option<Summary> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let median = if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    };
    let rank = ((0.95 * n as f64).ceil() as usize).clamp(1, n);
    let mean = v.iter().sum::<f64>() / n as f64;
    let std_dev = if n > 1 {
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    Some(Summary {
        n,
        min: v[0],
        median,
        p95: v[rank - 1],
        max: v[n - 1],
        mean,
        std_dev,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_of_odd_even_and_single_sets() {
        let s = summarize(&[5.0, 1.0, 3.0]).unwrap();
        assert_eq!((s.n, s.min, s.median, s.max), (3, 1.0, 3.0, 5.0));
        let s = summarize(&[4.0, 1.0, 2.0, 3.0]).unwrap();
        assert_eq!(s.median, 2.5);
        let s = summarize(&[7.0]).unwrap();
        assert_eq!((s.min, s.median, s.p95, s.max), (7.0, 7.0, 7.0, 7.0));
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(summarize(&twenty).unwrap().p95, 19.0);
    }

    #[test]
    fn mean_standard_deviation_cv_and_range() {
        // Sample standard deviation of 2, 4, 4, 4, 5, 5, 7, 9 is sqrt(32 / 7).
        let s = summarize(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!(s.mean, 5.0);
        assert!((s.std_dev - (32.0_f64 / 7.0).sqrt()).abs() < 1e-12);
        assert!((s.cv().unwrap() - s.std_dev / 5.0).abs() < 1e-12);
        assert_eq!(s.range(), 7.0);
        let one = summarize(&[3.0]).unwrap();
        assert_eq!((one.std_dev, one.cv(), one.range()), (0.0, Some(0.0), 0.0));
        let zero_mean = summarize(&[-1.0, 1.0]).unwrap();
        assert_eq!(zero_mean.cv(), None);
        let steady = summarize(&[100.0, 100.0, 100.0]).unwrap();
        assert_eq!(steady.cv(), Some(0.0));
    }

    #[test]
    fn non_finite_values_are_dropped_and_empty_has_no_summary() {
        assert!(summarize(&[]).is_none());
        assert!(summarize(&[f64::NAN, f64::INFINITY]).is_none());
        let mut s = Samples::new();
        assert!(s.is_empty() && s.summary().is_none());
        s.push(f64::NAN);
        s.push(2.0);
        assert_eq!(s.len(), 1);
        assert_eq!(s.summary().unwrap().median, 2.0);
    }
}
