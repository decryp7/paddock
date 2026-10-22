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
}
