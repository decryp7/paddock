//! Scores that mean the same thing (design review 1.3). Each resolver layer emits a [`RawScore`] (NCC, class score, OCR
//! confidence, ...), which is not comparable across layers. Only a [`Calibrator`] turns it into a [`CalibratedScore`], the
//! probability that the candidate is the intended target; [`crate::decision`] accepts nothing else, so an uncalibrated
//! score cannot be fused or compared: it does not type-check.
//!
//! ```compile_fail
//! use sighter_core::score::{Layer, RawScore};
//! use sighter_core::decision::{Thresholds, decide};
//! // A raw score is not a calibrated one: `decide` takes `LayerHit`s, which need a `CalibratedScore`.
//! let raw = RawScore::new(Layer::Template, 0.99).unwrap();
//! let _ = decide(&[raw], &Thresholds::default());
//! ```

/// The resolver layer that produced a score.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// Template matching (NCC) of a saved crop.
    Template,
    /// The trained detector.
    Detector,
    /// OCR text match.
    Ocr,
    /// A vision-language model (optional, user-started only).
    Vlm,
    /// The last known location of the element.
    Cache,
}

/// Why a score could not be built or calibrated.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ScoreError {
    /// NaN or infinite.
    #[error("a score must be a finite number")]
    NotFinite,
    /// A probability outside 0..=1.
    #[error("a probability must be within 0 and 1, got {0}")]
    OutOfRange(f64),
    /// A calibrator curve that is not usable.
    #[error("calibration curve is invalid: {0}")]
    BadCurve(&'static str),
    /// A raw score given to the calibrator of another layer.
    #[error("a {score:?} score cannot be calibrated by the {calibrator:?} calibrator")]
    WrongLayer {
        /// The layer of the score.
        score: Layer,
        /// The layer the calibrator was fitted for.
        calibrator: Layer,
    },
}

/// A layer's own number, in the layer's own units. Never compared across layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawScore {
    layer: Layer,
    value: f64,
}

impl RawScore {
    /// A finite raw score from `layer`.
    pub fn new(layer: Layer, value: f64) -> Result<RawScore, ScoreError> {
        if value.is_finite() {
            Ok(RawScore { layer, value })
        } else {
            Err(ScoreError::NotFinite)
        }
    }

    /// The layer.
    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// The value, for traces only.
    pub fn value(&self) -> f64 {
        self.value
    }
}

/// A probability in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
pub struct Probability(f64);

impl Probability {
    /// Zero.
    pub const ZERO: Probability = Probability(0.0);

    /// A probability; refuses NaN and values outside 0..=1.
    pub fn new(p: f64) -> Result<Probability, ScoreError> {
        if !p.is_finite() {
            Err(ScoreError::NotFinite)
        } else if !(0.0..=1.0).contains(&p) {
            Err(ScoreError::OutOfRange(p))
        } else {
            Ok(Probability(p))
        }
    }

    /// The value.
    pub fn get(self) -> f64 {
        self.0
    }
}

/// The probability that a candidate is the intended target, with the raw score kept for the trace. Built only by
/// [`Calibrator::calibrate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibratedScore {
    raw: RawScore,
    probability: Probability,
}

impl CalibratedScore {
    /// The layer.
    pub fn layer(&self) -> Layer {
        self.raw.layer
    }

    /// The calibrated probability; the only value thresholds apply to.
    pub fn probability(&self) -> Probability {
        self.probability
    }

    /// The raw score, for the trace.
    pub fn raw(&self) -> RawScore {
        self.raw
    }
}

/// A monotone piecewise-linear map from a layer's raw scores to probabilities (an isotonic fit has this shape). Values
/// below the first point map to its probability, values above the last to the last point's.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibrator {
    layer: Layer,
    points: Vec<(f64, f64)>,
}

/// The most points a curve may have; a fitted curve is far smaller.
pub const MAX_CURVE_POINTS: usize = 1024;

impl Calibrator {
    /// A calibrator from `(raw, probability)` points: two to [`MAX_CURVE_POINTS`], raw strictly increasing, probability
    /// non-decreasing and within 0..=1.
    pub fn new(layer: Layer, points: Vec<(f64, f64)>) -> Result<Calibrator, ScoreError> {
        if points.len() < 2 {
            return Err(ScoreError::BadCurve("needs at least two points"));
        }
        if points.len() > MAX_CURVE_POINTS {
            return Err(ScoreError::BadCurve("too many points"));
        }
        for (raw, p) in &points {
            if !raw.is_finite() {
                return Err(ScoreError::NotFinite);
            }
            Probability::new(*p)?;
        }
        if points.windows(2).any(|w| w[1].0 <= w[0].0) {
            return Err(ScoreError::BadCurve("raw scores must be strictly increasing"));
        }
        if points.windows(2).any(|w| w[1].1 < w[0].1) {
            return Err(ScoreError::BadCurve("probabilities must not decrease"));
        }
        Ok(Calibrator { layer, points })
    }

    /// Conservative placeholder curves for a layer until one is fitted on reviewed run outcomes. They are deliberately
    /// strict (a template needs NCC 0.97 to reach 0.82; a VLM alone never exceeds 0.5), so nothing clicks on a guess.
    pub fn conservative(layer: Layer) -> Calibrator {
        let points: &[(f64, f64)] = match layer {
            Layer::Template => &[(0.6, 0.0), (0.9, 0.5), (0.97, 0.82), (1.0, 0.95)],
            Layer::Detector => &[(0.3, 0.0), (0.9, 0.6), (1.0, 0.85)],
            Layer::Ocr => &[(0.5, 0.0), (0.95, 0.7), (1.0, 0.85)],
            Layer::Vlm => &[(0.0, 0.0), (1.0, 0.5)],
            Layer::Cache => &[(0.0, 0.0), (1.0, 0.6)],
        };
        Calibrator {
            layer,
            points: points.to_vec(),
        }
    }

    /// The layer this calibrator is fitted for.
    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// Calibrates a raw score of this calibrator's layer.
    pub fn calibrate(&self, raw: RawScore) -> Result<CalibratedScore, ScoreError> {
        if raw.layer != self.layer {
            return Err(ScoreError::WrongLayer {
                score: raw.layer,
                calibrator: self.layer,
            });
        }
        // `new` and `conservative` guarantee at least two points.
        let (first, last) = (self.points[0], self.points[self.points.len() - 1]);
        let p = if raw.value <= first.0 {
            first.1
        } else if raw.value >= last.0 {
            last.1
        } else {
            let i = self.points.partition_point(|(x, _)| *x <= raw.value);
            let ((x0, y0), (x1, y1)) = (self.points[i - 1], self.points[i]);
            y0 + (y1 - y0) * (raw.value - x0) / (x1 - x0)
        };
        Ok(CalibratedScore {
            raw,
            probability: Probability(p.clamp(0.0, 1.0)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(l: Layer, v: f64) -> RawScore {
        RawScore::new(l, v).unwrap()
    }

    #[test]
    fn raw_and_probability_refuse_bad_numbers() {
        assert_eq!(RawScore::new(Layer::Ocr, f64::NAN), Err(ScoreError::NotFinite));
        assert_eq!(RawScore::new(Layer::Ocr, f64::INFINITY), Err(ScoreError::NotFinite));
        assert_eq!(Probability::new(f64::NAN), Err(ScoreError::NotFinite));
        assert_eq!(Probability::new(1.5), Err(ScoreError::OutOfRange(1.5)));
        assert_eq!(Probability::new(-0.1), Err(ScoreError::OutOfRange(-0.1)));
        assert_eq!(Probability::new(0.25).unwrap().get(), 0.25);
        assert_eq!(Probability::default(), Probability::ZERO);
        let r = raw(Layer::Template, 0.5);
        assert_eq!((r.layer(), r.value()), (Layer::Template, 0.5));
    }

    #[test]
    fn a_calibrator_refuses_another_layers_score() {
        let c = Calibrator::conservative(Layer::Template);
        let err = c.calibrate(raw(Layer::Detector, 0.9)).unwrap_err();
        assert_eq!(
            err,
            ScoreError::WrongLayer {
                score: Layer::Detector,
                calibrator: Layer::Template
            }
        );
        assert!(err.to_string().contains("Detector"));
        assert_eq!(c.layer(), Layer::Template);
    }

    #[test]
    fn calibration_interpolates_clamps_and_is_monotone() {
        let c = Calibrator::new(Layer::Ocr, vec![(0.0, 0.0), (1.0, 0.5), (2.0, 1.0)]).unwrap();
        let p = |v| c.calibrate(raw(Layer::Ocr, v)).unwrap().probability().get();
        assert_eq!(p(-5.0), 0.0);
        assert_eq!(p(0.5), 0.25);
        assert_eq!(p(1.0), 0.5);
        assert_eq!(p(1.5), 0.75);
        assert_eq!(p(9.0), 1.0);
        let s = c.calibrate(raw(Layer::Ocr, 0.5)).unwrap();
        assert_eq!((s.layer(), s.raw().value()), (Layer::Ocr, 0.5));
    }

    #[test]
    fn curves_are_validated() {
        let bad = |pts: Vec<(f64, f64)>| Calibrator::new(Layer::Cache, pts).unwrap_err();
        assert!(matches!(bad(vec![(0.0, 0.0)]), ScoreError::BadCurve(_)));
        assert!(matches!(bad(vec![(0.0, 0.0), (0.0, 0.5)]), ScoreError::BadCurve(_)));
        assert!(matches!(bad(vec![(0.0, 0.5), (1.0, 0.4)]), ScoreError::BadCurve(_)));
        assert_eq!(bad(vec![(f64::NAN, 0.0), (1.0, 0.4)]), ScoreError::NotFinite);
        assert_eq!(bad(vec![(0.0, 0.0), (1.0, 1.2)]), ScoreError::OutOfRange(1.2));
        let many: Vec<(f64, f64)> = (0..=MAX_CURVE_POINTS).map(|i| (i as f64, 0.0)).collect();
        assert!(matches!(bad(many), ScoreError::BadCurve("too many points")));
    }

    #[test]
    fn conservative_defaults_are_valid_and_strict() {
        for l in [Layer::Template, Layer::Detector, Layer::Ocr, Layer::Vlm, Layer::Cache] {
            let c = Calibrator::conservative(l);
            // The shipped curves satisfy the constructor's own rules.
            assert!(Calibrator::new(l, c.points.clone()).is_ok(), "{l:?}");
        }
        let vlm = Calibrator::conservative(Layer::Vlm).calibrate(raw(Layer::Vlm, 1.0)).unwrap();
        assert!(vlm.probability().get() <= 0.5, "a VLM alone must never reach the found threshold");
    }
}
