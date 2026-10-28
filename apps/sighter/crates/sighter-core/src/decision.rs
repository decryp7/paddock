//! Fusion and the margin rule (design review 1.3, P1): *found* when top-1 >= tau and top-1 - top-2 >= m, *ambiguous* when
//! the margin is smaller, *not found* below tau. Only calibrated candidates are accepted.

use crate::geometry::Rect;
use crate::score::{Candidate, FusedCandidate, Layer, Probability};

/// Candidates whose rectangles overlap at least this much (IoU) are the same place.
pub const SAME_PLACE_IOU: f32 = 0.5;

/// Why thresholds were refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ThresholdsError {
    /// A value outside 0..=1 (or NaN).
    #[error("{name} must be within 0 and 1, got {value}")]
    OutOfRange {
        /// Which setting.
        name: &'static str,
        /// The value given.
        value: f32,
    },
}

/// The accept threshold `tau` and the margin `m`. Settings (rule 21), overridable per step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    tau: Probability,
    margin: Probability,
}

impl Default for Thresholds {
    /// The design review's defaults: tau 0.8, margin 0.2.
    fn default() -> Self {
        Thresholds {
            tau: Probability::literal(0.8),
            margin: Probability::literal(0.2),
        }
    }
}

impl Thresholds {
    /// Validated thresholds.
    pub fn new(tau: f32, margin: f32) -> Result<Thresholds, ThresholdsError> {
        let p = |name, value| Probability::new(value).map_err(|_| ThresholdsError::OutOfRange { name, value });
        Ok(Thresholds {
            tau: p("tau", tau)?,
            margin: p("margin", margin)?,
        })
    }

    /// The accept threshold.
    pub fn tau(&self) -> f32 {
        self.tau.get()
    }

    /// The required lead over the runner-up.
    pub fn margin(&self) -> f32 {
        self.margin.get()
    }
}

/// What the resolver concluded.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    /// One clear winner: safe to act on.
    Found(FusedCandidate),
    /// Two or more plausible targets; the caller must not click (the best two, strongest first).
    Ambiguous(Vec<FusedCandidate>),
    /// Nothing reached tau; carries the best probability seen, for the trace.
    NotFound {
        /// The strongest candidate's probability, if there was any.
        best: Option<Probability>,
    },
}

/// Groups candidates that sit on the same place and combines their evidence: per layer only the strongest member counts,
/// and distinct layers combine as independent evidence, `1 - prod(1 - p)`. Deterministic: ties order by position.
pub fn fuse(candidates: &[Candidate]) -> Vec<FusedCandidate> {
    let mut sorted: Vec<&Candidate> = candidates.iter().collect();
    sorted.sort_by(|a, b| {
        b.score
            .probability()
            .partial_cmp(&a.score.probability())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.rect.y.cmp(&b.rect.y))
            .then(a.rect.x.cmp(&b.rect.x))
            .then(a.score.layer().cmp(&b.score.layer()))
    });
    let mut clusters: Vec<(Rect, Vec<(Layer, f32)>)> = Vec::new();
    for c in sorted {
        let (layer, p) = (c.score.layer(), c.score.probability().get());
        match clusters.iter_mut().find(|(rect, _)| rect.iou(&c.rect) >= SAME_PLACE_IOU) {
            Some((_, members)) => {
                if !members.iter().any(|(l, _)| *l == layer) {
                    members.push((layer, p));
                }
            }
            None => clusters.push((c.rect, vec![(layer, p)])),
        }
    }
    let mut fused: Vec<FusedCandidate> = clusters
        .into_iter()
        .map(|(rect, members)| {
            let miss: f32 = members.iter().map(|(_, p)| 1.0 - p).product();
            let mut layers: Vec<Layer> = members.iter().map(|(l, _)| *l).collect();
            layers.sort();
            FusedCandidate {
                rect,
                probability: Probability::new((1.0 - miss).clamp(0.0, 1.0)).unwrap_or(Probability::ZERO),
                layers,
            }
        })
        .collect();
    fused.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

impl Resolution {
    /// True for [`Resolution::Found`], the only variant that may be acted on.
    pub fn is_found(&self) -> bool {
        matches!(self, Resolution::Found(_))
    }
}

/// Fuses the candidates and applies the margin rule.
pub fn decide(candidates: &[Candidate], t: &Thresholds) -> Resolution {
    let fused = fuse(candidates);
    let Some(top) = fused.first() else {
        return Resolution::NotFound { best: None };
    };
    if top.probability.get() < t.tau() {
        return Resolution::NotFound {
            best: Some(top.probability),
        };
    }
    match fused.get(1) {
        Some(second) if top.probability.get() - second.probability.get() < t.margin() => {
            Resolution::Ambiguous(fused.into_iter().take(2).collect())
        }
        _ => Resolution::Found(top.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{Calibrator, RawScore};

    fn cand(layer: Layer, x: u32, y: u32, raw: f32) -> Candidate {
        let score = Calibrator::new(layer, vec![(0.0, 0.0), (1.0, 1.0)])
            .unwrap()
            .calibrate(RawScore::new(layer, raw).unwrap())
            .unwrap();
        Candidate {
            rect: Rect::new(x, y, 40, 20),
            score,
        }
    }

    #[test]
    fn two_candidates_within_the_margin_are_ambiguous_never_found() {
        let r = decide(
            &[cand(Layer::Template, 0, 0, 0.95), cand(Layer::Template, 300, 0, 0.90)],
            &Thresholds::default(),
        );
        let Resolution::Ambiguous(top) = r else { panic!("{r:?}") };
        assert_eq!(top.len(), 2);
        assert!(top[0].probability.get() >= top[1].probability.get());
    }

    #[test]
    fn a_clear_winner_is_found_and_a_weak_one_is_not() {
        let t = Thresholds::default();
        let r = decide(&[cand(Layer::Template, 0, 0, 0.95), cand(Layer::Template, 300, 0, 0.5)], &t);
        assert!(matches!(&r, Resolution::Found(f) if f.rect.x == 0), "{r:?}");
        let r = decide(&[cand(Layer::Template, 0, 0, 0.7)], &t);
        assert!(matches!(r, Resolution::NotFound { best: Some(p) } if p.get() == 0.7), "{r:?}");
        assert_eq!(decide(&[], &t), Resolution::NotFound { best: None });
        // A lone candidate at exactly tau is found.
        assert!(matches!(decide(&[cand(Layer::Ocr, 0, 0, 0.8)], &t), Resolution::Found(_)));
    }

    #[test]
    fn the_margin_boundary_is_inclusive() {
        let t = Thresholds::new(0.5, 0.25).unwrap();
        assert!(matches!(
            decide(&[cand(Layer::Ocr, 0, 0, 0.75), cand(Layer::Ocr, 200, 0, 0.5)], &t),
            Resolution::Found(_)
        ));
        assert!(matches!(
            decide(&[cand(Layer::Ocr, 0, 0, 0.74), cand(Layer::Ocr, 200, 0, 0.5)], &t),
            Resolution::Ambiguous(_)
        ));
    }

    #[test]
    fn layers_that_agree_on_a_place_reinforce_each_other() {
        let fused = fuse(&[
            cand(Layer::Template, 10, 10, 0.6),
            cand(Layer::Ocr, 12, 11, 0.5),
            cand(Layer::Ocr, 11, 10, 0.1),
        ]);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].layers, vec![Layer::Template, Layer::Ocr]);
        assert!((fused[0].probability.get() - 0.8).abs() < 1e-5);
        assert!(
            decide(
                &[cand(Layer::Template, 10, 10, 0.6), cand(Layer::Ocr, 12, 11, 0.5)],
                &Thresholds::default()
            )
            .is_found()
        );
    }

    #[test]
    fn fusion_is_deterministic_for_ties() {
        let a = [cand(Layer::Ocr, 100, 0, 0.9), cand(Layer::Ocr, 0, 0, 0.9)];
        let b = [a[1], a[0]];
        assert_eq!(fuse(&a), fuse(&b));
        assert_eq!(fuse(&a)[0].rect.x, 0);
    }

    #[test]
    fn thresholds_are_validated() {
        assert_eq!(
            Thresholds::new(1.5, 0.2),
            Err(ThresholdsError::OutOfRange { name: "tau", value: 1.5 })
        );
        assert!(Thresholds::new(0.5, f32::NAN).is_err());
        let d = Thresholds::default();
        assert_eq!((d.tau(), d.margin()), (0.8, 0.2));
        assert!(
            ThresholdsError::OutOfRange { name: "m", value: 2.0 }
                .to_string()
                .contains("within 0 and 1")
        );
    }
}
