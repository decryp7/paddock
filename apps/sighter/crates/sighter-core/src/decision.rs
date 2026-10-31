//! Fusion and the margin rule (design review 1.3): *found* when top-1 >= tau and top-1 - top-2 >= m, *ambiguous* when
//! the margin is smaller, *not found* below tau. Only calibrated hits are accepted, and only [`decide`] can build a
//! [`Located`], so a click target always passed the rule. Geometry is the core's own typed `Rect<Logical>`; there is no
//! second rectangle type.

use std::cmp::Ordering;

use crate::detect::{Candidate, Provenance};
use crate::geometry::{Logical, Rect};
use crate::score::{CalibratedScore, Calibrator, Layer, Probability, RawScore, ScoreError};

/// The built-in accept threshold. The one place the default lives; everything else reads [`Thresholds::default`].
pub const DEFAULT_TAU: f64 = 0.8;
/// The built-in margin. The one place the default lives.
pub const DEFAULT_MARGIN: f64 = 0.2;
/// Float slack for comparing a lead with the margin: 1.0 - 0.8 is 0.19999999999999996, which must still meet 0.2.
const MARGIN_EPSILON: f64 = 1e-9;
/// The built-in overlap (IoU) at which two hits count as the same place.
pub const DEFAULT_SAME_PLACE_IOU: f64 = 0.5;
/// The most hits one decision looks at; the rest are dropped, weakest first, never a panic; fusion is quadratic in this.
pub const MAX_HITS: usize = 2_000;

/// Why thresholds were refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ThresholdsError {
    /// A value outside 0..=1 (or NaN).
    #[error("{name} must be within 0 and 1, got {value}")]
    OutOfRange {
        /// Which setting.
        name: &'static str,
        /// The value given.
        value: f64,
    },
    /// Zero would accept everything (tau) or ignore look-alikes (margin, overlap).
    #[error("{name} must be above 0")]
    NotPositive {
        /// Which setting.
        name: &'static str,
    },
}

/// An optional override of the thresholds from one source: the app Settings, a plan, or a single step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThresholdsOverride {
    /// Replaces tau when set.
    pub tau: Option<f64>,
    /// Replaces the margin when set.
    pub margin: Option<f64>,
    /// Replaces the same-place overlap when set.
    pub same_place_iou: Option<f64>,
}

/// The accept threshold `tau`, the margin `m` and the overlap that makes two hits one place. Never hard-coded at a call
/// site: the defaults are the `DEFAULT_*` constants, and [`Thresholds::resolve`] layers overrides over them in the order
/// built-in default, Settings, plan, step (a later layer wins; an unset field keeps the earlier value). All three are
/// above 0 and at most 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    tau: f64,
    margin: f64,
    same_place_iou: f64,
}

impl Default for Thresholds {
    /// The design review's defaults: tau 0.8, margin 0.2, same place at IoU 0.5.
    fn default() -> Self {
        Thresholds {
            tau: DEFAULT_TAU,
            margin: DEFAULT_MARGIN,
            same_place_iou: DEFAULT_SAME_PLACE_IOU,
        }
    }
}

fn positive(name: &'static str, value: f64) -> Result<f64, ThresholdsError> {
    match Probability::new(value) {
        Ok(p) if p.get() > 0.0 => Ok(p.get()),
        Ok(_) => Err(ThresholdsError::NotPositive { name }),
        Err(_) => Err(ThresholdsError::OutOfRange { name, value }),
    }
}

impl Thresholds {
    /// Validated thresholds with the default same-place overlap.
    pub fn new(tau: f64, margin: f64) -> Result<Thresholds, ThresholdsError> {
        Ok(Thresholds {
            tau: positive("tau", tau)?,
            margin: positive("margin", margin)?,
            ..Thresholds::default()
        })
    }

    /// The same thresholds with another same-place overlap.
    pub fn with_same_place_iou(self, iou: f64) -> Result<Thresholds, ThresholdsError> {
        Ok(Thresholds {
            same_place_iou: positive("same_place_iou", iou)?,
            ..self
        })
    }

    /// The defaults with each override applied in order (Settings, plan, step); the first invalid value is refused.
    pub fn resolve(layers: &[ThresholdsOverride]) -> Result<Thresholds, ThresholdsError> {
        let mut t = Thresholds::default();
        for layer in layers {
            t = Thresholds::new(layer.tau.unwrap_or(t.tau), layer.margin.unwrap_or(t.margin))?
                .with_same_place_iou(layer.same_place_iou.unwrap_or(t.same_place_iou))?;
        }
        Ok(t)
    }

    /// The accept threshold.
    pub fn tau(&self) -> f64 {
        self.tau
    }

    /// The required lead over the runner-up.
    pub fn margin(&self) -> f64 {
        self.margin
    }

    /// The overlap (IoU) at which two hits are the same place.
    pub fn same_place_iou(&self) -> f64 {
        self.same_place_iou
    }
}

/// One located element from one layer: where, how likely it is the target, and how it was found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerHit {
    /// Where, in logical points.
    pub rect: Rect<Logical>,
    /// How likely it is the target; only a calibrated score is accepted.
    pub score: CalibratedScore,
    /// Whether a hint narrowed the search to it (kept through fusion, for the trace and the "hint-sourced" mark).
    pub provenance: Provenance,
}

impl LayerHit {
    /// A hit from a detector candidate: the detector's score is the raw score of [`Layer::Detector`], calibrated by
    /// `calibrator` (which must be the detector's), and the hint provenance is kept.
    pub fn from_candidate(c: &Candidate, calibrator: &Calibrator) -> Result<LayerHit, ScoreError> {
        let raw = RawScore::new(Layer::Detector, c.detection.score.get())?;
        Ok(LayerHit {
            rect: c.detection.rect,
            score: calibrator.calibrate(raw)?,
            provenance: c.provenance,
        })
    }
}

/// Hits of several layers that agree on one place, combined. Built only by [`fuse`].
#[derive(Clone, Debug, PartialEq)]
pub struct FusedHit {
    rect: Rect<Logical>,
    probability: Probability,
    provenance: Provenance,
    layers: Vec<Layer>,
}

impl FusedHit {
    /// The rectangle of the strongest member.
    pub fn rect(&self) -> Rect<Logical> {
        self.rect
    }

    /// The strongest member's calibrated probability. Max-p fusion: agreeing layers are correlated (they look at the same
    /// pixels), so they never add up; two weak layers cannot make a click.
    pub fn probability(&self) -> Probability {
        self.probability
    }

    /// How the strongest member was found.
    pub fn provenance(&self) -> Provenance {
        self.provenance
    }

    /// The layers that agree, sorted.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
}

/// A target that passed the margin rule: the only thing a click may be built from. Built only by [`decide`].
#[derive(Clone, Debug, PartialEq)]
pub struct Located(FusedHit);

impl Located {
    /// Where to act.
    pub fn rect(&self) -> Rect<Logical> {
        self.0.rect
    }

    /// The calibrated probability.
    pub fn probability(&self) -> Probability {
        self.0.probability
    }

    /// How the winner was found.
    pub fn provenance(&self) -> Provenance {
        self.0.provenance
    }

    /// The layers that agree.
    pub fn layers(&self) -> &[Layer] {
        &self.0.layers
    }
}

/// What the resolver concluded.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    /// One clear winner: safe to act on.
    Found(Located),
    /// Two or more plausible targets; the caller must not click (the best two, strongest first).
    Ambiguous(Vec<FusedHit>),
    /// Nothing reached tau; carries the best probability seen, for the trace.
    NotFound {
        /// The strongest hit's probability, if there was any.
        best: Option<Probability>,
    },
}

impl Resolution {
    /// True for [`Resolution::Found`], the only variant that may be acted on.
    pub fn is_found(&self) -> bool {
        matches!(self, Resolution::Found(_))
    }
}

/// Strongest first, then position, size and layer, so the result never depends on input order.
fn strongest_first(a: &LayerHit, b: &LayerHit) -> Ordering {
    b.score
        .probability()
        .get()
        .total_cmp(&a.score.probability().get())
        .then(a.rect.y().total_cmp(&b.rect.y()))
        .then(a.rect.x().total_cmp(&b.rect.x()))
        .then(a.rect.h().total_cmp(&b.rect.h()))
        .then(a.rect.w().total_cmp(&b.rect.w()))
        .then(a.score.layer().cmp(&b.score.layer()))
}

/// Groups hits that sit on the same place (IoU at least `t.same_place_iou()`) and takes the strongest member's
/// probability and provenance. Deterministic: ties order by position. At most [`MAX_HITS`] of the strongest hits count.
pub fn fuse(hits: &[LayerHit], t: &Thresholds) -> Vec<FusedHit> {
    let mut sorted: Vec<&LayerHit> = hits.iter().collect();
    sorted.sort_by(|a, b| strongest_first(a, b));
    sorted.truncate(MAX_HITS);
    let mut fused: Vec<FusedHit> = Vec::new();
    for h in sorted {
        let layer = h.score.layer();
        match fused.iter_mut().find(|f| f.rect.iou(&h.rect) >= t.same_place_iou()) {
            Some(f) => {
                if !f.layers.contains(&layer) {
                    f.layers.push(layer);
                    f.layers.sort();
                }
            }
            // Sorted strongest first, so the first member of a cluster is its maximum.
            None => fused.push(FusedHit {
                rect: h.rect,
                probability: h.score.probability(),
                provenance: h.provenance,
                layers: vec![layer],
            }),
        }
    }
    // Clusters were created in strongest-first order, so they are already sorted.
    fused
}

/// Fuses the hits and applies the margin rule.
pub fn decide(hits: &[LayerHit], t: &Thresholds) -> Resolution {
    let fused = fuse(hits, t);
    let Some(top) = fused.first() else {
        return Resolution::NotFound { best: None };
    };
    if top.probability.get() < t.tau() {
        return Resolution::NotFound {
            best: Some(top.probability),
        };
    }
    match fused.get(1) {
        Some(second) if top.probability.get() - second.probability.get() + MARGIN_EPSILON < t.margin() => {
            Resolution::Ambiguous(fused.into_iter().take(2).collect())
        }
        _ => Resolution::Found(Located(top.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{Calibrator, RawScore};

    fn linear(layer: Layer, raw: f64) -> CalibratedScore {
        Calibrator::new(layer, vec![(0.0, 0.0), (1.0, 1.0)])
            .unwrap()
            .calibrate(RawScore::new(layer, raw).unwrap())
            .unwrap()
    }

    fn hit(layer: Layer, x: f64, y: f64, raw: f64) -> LayerHit {
        LayerHit {
            rect: Rect::new(x, y, 40.0, 20.0).unwrap(),
            score: linear(layer, raw),
            provenance: Provenance::Detected,
        }
    }

    #[test]
    fn two_hits_within_the_margin_are_ambiguous_never_found() {
        let r = decide(
            &[hit(Layer::Template, 0.0, 0.0, 0.95), hit(Layer::Template, 300.0, 0.0, 0.90)],
            &Thresholds::default(),
        );
        let Resolution::Ambiguous(top) = r else { panic!("{r:?}") };
        assert_eq!(top.len(), 2);
        assert!(top[0].probability().get() >= top[1].probability().get());
    }

    #[test]
    fn a_clear_winner_is_found_and_a_weak_one_is_not() {
        let t = Thresholds::default();
        let r = decide(&[hit(Layer::Template, 0.0, 0.0, 0.95), hit(Layer::Template, 300.0, 0.0, 0.5)], &t);
        assert!(matches!(&r, Resolution::Found(f) if f.rect().x() == 0.0), "{r:?}");
        assert!(r.is_found());
        assert!(!decide(&[], &t).is_found());
        let r = decide(&[hit(Layer::Template, 0.0, 0.0, 0.7)], &t);
        assert!(matches!(r, Resolution::NotFound { best: Some(p) } if p.get() == 0.7), "{r:?}");
        assert_eq!(decide(&[], &t), Resolution::NotFound { best: None });
        // A lone hit at exactly tau is found.
        assert!(matches!(decide(&[hit(Layer::Ocr, 0.0, 0.0, 0.8)], &t), Resolution::Found(_)));
    }

    #[test]
    fn the_margin_boundary_is_inclusive() {
        let t = Thresholds::new(0.5, 0.25).unwrap();
        assert!(matches!(
            decide(&[hit(Layer::Ocr, 0.0, 0.0, 0.75), hit(Layer::Ocr, 200.0, 0.0, 0.5)], &t),
            Resolution::Found(_)
        ));
        assert!(matches!(
            decide(&[hit(Layer::Ocr, 0.0, 0.0, 0.74), hit(Layer::Ocr, 200.0, 0.0, 0.5)], &t),
            Resolution::Ambiguous(_)
        ));
    }

    #[test]
    fn the_margin_edge_is_inclusive_at_the_defaults_despite_float_rounding() {
        // 1.0 - 0.8 is 0.19999999999999996 in f64, which is below the 0.2 margin by rounding alone.
        let t = Thresholds::default();
        for (top, second) in [(1.0, 0.8), (0.95, 0.75), (0.9, 0.7)] {
            let r = decide(&[hit(Layer::Ocr, 0.0, 0.0, top), hit(Layer::Ocr, 200.0, 0.0, second)], &t);
            assert!(matches!(r, Resolution::Found(_)), "{top}/{second}: {r:?}");
        }
        // A real shortfall still counts as ambiguous.
        let r = decide(&[hit(Layer::Ocr, 0.0, 0.0, 0.95), hit(Layer::Ocr, 200.0, 0.0, 0.76)], &t);
        assert!(matches!(r, Resolution::Ambiguous(_)), "{r:?}");
    }

    #[test]
    fn a_detector_candidate_becomes_a_hit_through_the_detector_calibrator() {
        use crate::detect::{Candidate, ClassId, Detection, Score};
        let c = Candidate {
            detection: Detection {
                rect: Rect::new(1.0, 2.0, 30.0, 10.0).unwrap(),
                class: ClassId(3),
                score: Score::new(0.9).unwrap(),
                tile: 0,
            },
            provenance: Provenance::HintSourced(1),
        };
        let cal = Calibrator::conservative(Layer::Detector);
        let h = LayerHit::from_candidate(&c, &cal).unwrap();
        assert_eq!(
            (h.rect, h.provenance, h.score.layer()),
            (c.detection.rect, Provenance::HintSourced(1), Layer::Detector)
        );
        assert_eq!(h.score.raw().value(), 0.9);
        assert_eq!(h.score.probability().get(), 0.6);
        // A calibrator of another layer is refused, not applied.
        let wrong = Calibrator::conservative(Layer::Ocr);
        assert!(LayerHit::from_candidate(&c, &wrong).is_err());
    }

    #[test]
    fn agreeing_layers_take_the_strongest_probability_and_never_add_up() {
        let t = Thresholds::default();
        let fused = fuse(
            &[
                hit(Layer::Template, 10.0, 10.0, 0.6),
                hit(Layer::Ocr, 12.0, 11.0, 0.5),
                hit(Layer::Ocr, 11.0, 10.0, 0.1),
            ],
            &t,
        );
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].layers(), &[Layer::Template, Layer::Ocr]);
        assert_eq!(fused[0].probability().get(), 0.6);
        assert_eq!(fused[0].rect(), Rect::new(10.0, 10.0, 40.0, 20.0).unwrap());
    }

    #[test]
    fn two_sub_threshold_layers_never_make_a_click() {
        // A noisy-OR would turn 0.6 + 0.5 into 0.8 and click on two weak, correlated guesses.
        let t = Thresholds::default();
        let r = decide(&[hit(Layer::Template, 10.0, 10.0, 0.6), hit(Layer::Ocr, 12.0, 11.0, 0.5)], &t);
        assert!(matches!(r, Resolution::NotFound { best: Some(p) } if p.get() == 0.6), "{r:?}");
        let r = decide(&[hit(Layer::Template, 10.0, 10.0, 0.85), hit(Layer::Ocr, 12.0, 11.0, 0.5)], &t);
        let Resolution::Found(found) = r else { panic!("{r:?}") };
        assert_eq!((found.probability().get(), found.layers().len(), found.rect().x()), (0.85, 2, 10.0));
    }

    #[test]
    fn a_hint_sourced_hit_keeps_its_provenance_through_fusion_and_the_decision() {
        let t = Thresholds::default();
        let mut strong = hit(Layer::Detector, 0.0, 0.0, 0.95);
        strong.provenance = Provenance::HintSourced(2);
        // A weaker, unhinted hit on the same place does not take over the mark.
        let weak = hit(Layer::Template, 2.0, 1.0, 0.5);
        let fused = fuse(&[weak, strong], &t);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].provenance(), Provenance::HintSourced(2));
        let Resolution::Found(found) = decide(&[weak, strong], &t) else {
            panic!("a 0.95 hit with a weak companion is found")
        };
        assert_eq!(found.provenance(), Provenance::HintSourced(2));
        // And the other way round: an unhinted winner stays unmarked.
        let r = decide(&[hit(Layer::Detector, 0.0, 0.0, 0.95)], &t);
        assert!(matches!(&r, Resolution::Found(f) if f.provenance() == Provenance::Detected));
    }

    #[test]
    fn the_same_place_overlap_is_a_threshold() {
        let near = [hit(Layer::Template, 0.0, 0.0, 0.9), hit(Layer::Ocr, 20.0, 0.0, 0.9)];
        // IoU of the two 40x20 boxes shifted by 20 px is 1/3.
        assert_eq!(fuse(&near, &Thresholds::default()).len(), 2);
        let loose = Thresholds::default().with_same_place_iou(0.3).unwrap();
        assert_eq!(fuse(&near, &loose).len(), 1);
        assert!(Thresholds::default().with_same_place_iou(0.0).is_err());
        assert_eq!(loose.same_place_iou(), 0.3);
    }

    #[test]
    fn fusion_is_deterministic_for_ties() {
        let a = [hit(Layer::Ocr, 100.0, 0.0, 0.9), hit(Layer::Ocr, 0.0, 0.0, 0.9)];
        let b = [a[1], a[0]];
        let t = Thresholds::default();
        assert_eq!(fuse(&a, &t), fuse(&b, &t));
        assert_eq!(fuse(&a, &t)[0].rect().x(), 0.0);
    }

    #[test]
    fn a_flood_of_hits_is_bounded() {
        let hits: Vec<LayerHit> = (0..MAX_HITS + 50).map(|i| hit(Layer::Ocr, (i as f64) * 100.0, 0.0, 0.5)).collect();
        // Disjoint hits stay separate, but never more than the bound are looked at.
        assert_eq!(fuse(&hits, &Thresholds::default()).len(), MAX_HITS);
    }

    #[test]
    fn thresholds_layer_settings_plan_and_step() {
        assert_eq!(Thresholds::resolve(&[]).unwrap(), Thresholds::default());
        let settings = ThresholdsOverride {
            tau: Some(0.7),
            ..Default::default()
        };
        let plan = ThresholdsOverride {
            margin: Some(0.1),
            ..Default::default()
        };
        let step = ThresholdsOverride {
            tau: Some(0.9),
            ..Default::default()
        };
        let t = Thresholds::resolve(&[settings, plan]).unwrap();
        assert_eq!((t.tau(), t.margin()), (0.7, 0.1));
        let t = Thresholds::resolve(&[settings, plan, step]).unwrap();
        assert_eq!((t.tau(), t.margin()), (0.9, 0.1));
        let bad = ThresholdsOverride {
            tau: Some(2.0),
            ..Default::default()
        };
        assert!(Thresholds::resolve(&[settings, bad]).is_err());
        assert_eq!((DEFAULT_TAU, DEFAULT_MARGIN, DEFAULT_SAME_PLACE_IOU), (0.8, 0.2, 0.5));
        let iou = ThresholdsOverride {
            same_place_iou: Some(0.7),
            ..Default::default()
        };
        assert_eq!(Thresholds::resolve(&[iou]).unwrap().same_place_iou(), 0.7);
    }

    #[test]
    fn thresholds_are_validated() {
        assert_eq!(
            Thresholds::new(1.5, 0.2),
            Err(ThresholdsError::OutOfRange { name: "tau", value: 1.5 })
        );
        assert!(Thresholds::new(0.5, f64::NAN).is_err());
        // Zero would accept everything or ignore look-alikes.
        assert_eq!(Thresholds::new(0.0, 0.2), Err(ThresholdsError::NotPositive { name: "tau" }));
        assert_eq!(Thresholds::new(0.8, 0.0), Err(ThresholdsError::NotPositive { name: "margin" }));
        assert!(ThresholdsError::NotPositive { name: "tau" }.to_string().contains("above 0"));
        let d = Thresholds::default();
        assert_eq!((d.tau(), d.margin()), (0.8, 0.2));
        assert!(
            ThresholdsError::OutOfRange { name: "m", value: 2.0 }
                .to_string()
                .contains("within 0 and 1")
        );
    }
}
