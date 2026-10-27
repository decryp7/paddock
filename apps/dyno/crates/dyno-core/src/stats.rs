//! Small statistics used by forecasts and anomaly detection.

/// A least-squares line `y = intercept + slope · x`, with its goodness of fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Change of y per unit of x.
    pub slope: f64,
    /// y at x = 0.
    pub intercept: f64,
    /// Coefficient of determination, 0–1 (1 when y is constant).
    pub r2: f64,
}

/// Fits a line through `(x, y)` points; `None` with fewer than two distinct x values.
pub fn linear_fit(points: &[(f64, f64)]) -> Option<Fit> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx <= f64::EPSILON {
        return None;
    }
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let syy: f64 = points.iter().map(|p| (p.1 - my).powi(2)).sum();
    let slope = sxy / sxx;
    let r2 = if syy <= f64::EPSILON { 1.0 } else { (sxy * sxy) / (sxx * syy) };
    Some(Fit {
        slope,
        intercept: my - slope * mx,
        r2,
    })
}

/// Mean and (population) standard deviation; `(0, 0)` for no values.
pub fn mean_sd(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = values.len() as f64;
    let m = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / n;
    (m, var.sqrt())
}

/// The median of `values` (`None` when empty; NaNs are ignored by the caller's choice of input).
pub fn median(values: &[f64]) -> Option<f64> {
    quantile(values, 0.5)
}

/// The `q` quantile (0–1) of `values` with linear interpolation; `None` when empty.
pub fn quantile(values: &[f64], q: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
}

/// A robust trend line: the median of all pairwise slopes (Theil–Sen), with the quartiles of those slopes as a range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobustSlope {
    /// Median slope per unit of x.
    pub slope: f64,
    /// Lower quartile of the pairwise slopes.
    pub low: f64,
    /// Upper quartile of the pairwise slopes.
    pub high: f64,
    /// The median of `y - slope * x`.
    pub intercept: f64,
}

/// The most points [`robust_slope`] looks at (evenly thinned) so the pairwise work stays bounded.
pub const ROBUST_MAX_POINTS: usize = 120;

/// Theil–Sen slope of `(x, y)` points; `None` with fewer than two distinct x values. Outliers (one wrong reading) do not
/// move it, which is why battery and wear forecasts use it instead of a least-squares line.
pub fn robust_slope(points: &[(f64, f64)]) -> Option<RobustSlope> {
    let finite: Vec<(f64, f64)> = points.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    let step = finite.len().div_ceil(ROBUST_MAX_POINTS).max(1);
    let pts: Vec<(f64, f64)> = finite.iter().step_by(step).copied().collect();
    let mut slopes = Vec::new();
    for (i, a) in pts.iter().enumerate() {
        for b in &pts[i + 1..] {
            let dx = b.0 - a.0;
            if dx.abs() > f64::EPSILON {
                slopes.push((b.1 - a.1) / dx);
            }
        }
    }
    let slope = median(&slopes)?;
    let offsets: Vec<f64> = pts.iter().map(|p| p.1 - slope * p.0).collect();
    Some(RobustSlope {
        slope,
        low: quantile(&slopes, 0.25)?,
        high: quantile(&slopes, 0.75)?,
        intercept: median(&offsets)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_lines() {
        let f = linear_fit(&[(0.0, 1.0), (1.0, 3.0), (2.0, 5.0)]).unwrap();
        assert!((f.slope - 2.0).abs() < 1e-9 && (f.intercept - 1.0).abs() < 1e-9 && (f.r2 - 1.0).abs() < 1e-9);
        let flat = linear_fit(&[(0.0, 4.0), (1.0, 4.0)]).unwrap();
        assert_eq!(flat.slope, 0.0);
        assert_eq!(flat.r2, 1.0);
        assert!(linear_fit(&[(0.0, 1.0)]).is_none());
        assert!(linear_fit(&[(1.0, 1.0), (1.0, 2.0)]).is_none());
        let noisy = linear_fit(&[(0.0, 0.0), (1.0, 5.0), (2.0, 0.0), (3.0, 5.0)]).unwrap();
        assert!(noisy.r2 < 0.5);
    }

    #[test]
    fn means_and_deviations() {
        assert_eq!(mean_sd(&[]), (0.0, 0.0));
        assert_eq!(mean_sd(&[2.0, 4.0]), (3.0, 1.0));
    }

    #[test]
    fn medians_and_quantiles() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
        assert_eq!(quantile(&[1.0, f64::NAN, 3.0], 1.0), Some(3.0));
        assert_eq!(quantile(&[5.0], 0.9), Some(5.0));
        assert_eq!(quantile(&[0.0, 10.0], 0.25), Some(2.5));
    }

    #[test]
    fn robust_slope_ignores_a_wild_reading() {
        let mut pts: Vec<(f64, f64)> = (0..20).map(|x| (f64::from(x), 100.0 - 0.5 * f64::from(x))).collect();
        pts[7].1 = 5.0;
        let r = robust_slope(&pts).unwrap();
        assert!((r.slope + 0.5).abs() < 1e-9, "{r:?}");
        assert!(r.low <= r.slope && r.slope <= r.high);
        assert!((r.intercept - 100.0).abs() < 1e-9);
        assert!(robust_slope(&[(1.0, 1.0)]).is_none());
        assert!(robust_slope(&[(1.0, 1.0), (1.0, 2.0)]).is_none());
        assert!(robust_slope(&[(0.0, f64::NAN), (1.0, 1.0)]).is_none());
    }

    #[test]
    fn robust_slope_thins_long_series() {
        let pts: Vec<(f64, f64)> = (0..1000).map(|x| (f64::from(x), 2.0 * f64::from(x))).collect();
        let r = robust_slope(&pts).unwrap();
        assert!((r.slope - 2.0).abs() < 1e-9);
    }
}
