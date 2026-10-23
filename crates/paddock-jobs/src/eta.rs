/// Estimates time left from a smoothed rate. Rules (ADR 0007): never for an unknown total; not before 2 s of work and 3 % done
/// (early rates lie); smoothed with an exponential average so one slow second does not jump the estimate; hidden past 99 hours.
#[derive(Debug, Clone)]
pub struct EtaEstimator {
    start_ms: u64,
    last_ms: u64,
    last_done: u64,
    rate: Option<f64>,
}

const MIN_ELAPSED_MS: u64 = 2_000;
const MIN_FRACTION: f64 = 0.03;
const SMOOTHING: f64 = 0.3;
const MAX_ETA_MS: u64 = 99 * 3_600_000;

impl EtaEstimator {
    /// Starts measuring at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        EtaEstimator {
            start_ms: now_ms,
            last_ms: now_ms,
            last_done: 0,
            rate: None,
        }
    }

    /// Records progress and returns the estimate in milliseconds, if one is trustworthy.
    pub fn observe(&mut self, now_ms: u64, done: u64, total: Option<u64>) -> Option<u64> {
        if done < self.last_done {
            // The scale restarted (new phase reusing the estimator): start over.
            *self = EtaEstimator::new(now_ms);
        }
        if now_ms > self.last_ms && done > self.last_done {
            let inst = (done - self.last_done) as f64 / (now_ms - self.last_ms) as f64;
            self.rate = Some(match self.rate {
                Some(r) => r + SMOOTHING * (inst - r),
                None => inst,
            });
            self.last_ms = now_ms;
            self.last_done = done;
        } else if now_ms > self.last_ms {
            self.last_ms = now_ms;
        }
        let total = total.filter(|t| *t > 0)?;
        let done = done.min(total);
        if done >= total {
            return Some(0);
        }
        if now_ms.saturating_sub(self.start_ms) < MIN_ELAPSED_MS
            || (done as f64) < MIN_FRACTION * total as f64
        {
            return None;
        }
        let rate = self.rate.filter(|r| *r > 0.0)?;
        let left = ((total - done) as f64 / rate).ceil();
        (left.is_finite() && left <= MAX_ETA_MS as f64).then_some(left as u64)
    }
}
