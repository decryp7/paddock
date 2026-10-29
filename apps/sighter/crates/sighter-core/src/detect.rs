//! The detection result model and the pure functions over it: class-aware non-maximum suppression, the seam merge for
//! boxes cut by a tile border, and adapter hints. All boxes are in the logical space of the one tile grid
//! ([`Logical`]); results are mapped to other spaces through the [`Chain`](crate::chain::Chain).

use crate::chain::Chain;
use crate::geometry::{CapturePx, HostInput, Logical, Rect, RemotePx};
use std::cmp::Ordering;

/// Most detections one call accepts; keeps the quadratic passes bounded.
pub const MAX_DETECTIONS: usize = 10_000;
/// Longest adapter name kept in a hint.
pub const MAX_ADAPTER_NAME: usize = 64;

/// Why a detection operation refused its input.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DetectError {
    /// A score was NaN or outside `[0, 1]`.
    #[error("a score must be a number between 0 and 1, got {0}")]
    BadScore(f64),
    /// An IoU or containment threshold was NaN or outside `(0, 1]`.
    #[error("a threshold must be a number above 0 and at most 1, got {0}")]
    BadThreshold(f64),
    /// More detections than [`MAX_DETECTIONS`].
    #[error("{got} detections is more than the limit of {max}")]
    TooMany {
        /// Number given.
        got: usize,
        /// Limit.
        max: usize,
    },
    /// A hint's rectangle could not be built.
    #[error(transparent)]
    Geometry(#[from] crate::geometry::GeometryError),
    /// A hint came from a coordinate space that cannot be mapped (an adapter that is not DPI aware sees virtualised
    /// coordinates that no chain can undo).
    #[error("hint from adapter '{adapter}' is in DPI-virtualised coordinates and cannot be mapped; make the adapter Per-Monitor V2 aware")]
    UnmappableHint {
        /// The adapter that sent it.
        adapter: String,
    },
}

/// A detector class (an index into the model card's class list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ClassId(pub u32);

/// A confidence in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Score(f64);

impl TryFrom<f64> for Score {
    type Error = DetectError;
    fn try_from(v: f64) -> Result<Self, DetectError> {
        Self::new(v)
    }
}
impl From<Score> for f64 {
    fn from(s: Score) -> f64 {
        s.0
    }
}

impl Score {
    /// Builds a score; NaN and values outside `[0, 1]` are refused.
    pub fn new(v: f64) -> Result<Self, DetectError> {
        if v.is_nan() || !(0.0..=1.0).contains(&v) {
            Err(DetectError::BadScore(v))
        } else {
            Ok(Self(v))
        }
    }
    /// The value.
    pub fn get(self) -> f64 {
        self.0
    }
}

/// A threshold in `(0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fraction(f64);

impl Fraction {
    /// Builds a threshold; NaN, zero, negatives and values above one are refused.
    pub fn new(v: f64) -> Result<Self, DetectError> {
        if v.is_nan() || v <= 0.0 || v > 1.0 {
            Err(DetectError::BadThreshold(v))
        } else {
            Ok(Self(v))
        }
    }
    /// The value.
    pub fn get(self) -> f64 {
        self.0
    }
}

/// One detected element: a box in logical points, a class and a score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    /// The box in logical points.
    pub rect: Rect<Logical>,
    /// The class.
    pub class: ClassId,
    /// The detector's score.
    pub score: Score,
}

/// Total order used everywhere: higher score first, then position, size and class, so results never depend on input
/// order.
fn canonical(a: &Detection, b: &Detection) -> Ordering {
    b.score
        .get()
        .total_cmp(&a.score.get())
        .then(a.rect.x().total_cmp(&b.rect.x()))
        .then(a.rect.y().total_cmp(&b.rect.y()))
        .then(a.rect.w().total_cmp(&b.rect.w()))
        .then(a.rect.h().total_cmp(&b.rect.h()))
        .then(a.class.cmp(&b.class))
}

fn bounded(n: usize) -> Result<(), DetectError> {
    if n > MAX_DETECTIONS {
        Err(DetectError::TooMany {
            got: n,
            max: MAX_DETECTIONS,
        })
    } else {
        Ok(())
    }
}

/// Drops detections scoring below `floor`, in canonical order.
pub fn filter_scores(dets: &[Detection], floor: Score) -> Result<Vec<Detection>, DetectError> {
    bounded(dets.len())?;
    let mut out: Vec<Detection> = dets.iter().filter(|d| d.score >= floor).copied().collect();
    out.sort_by(canonical);
    Ok(out)
}

/// Greedy class-aware non-maximum suppression: walking from the best score down, a box is dropped when a kept box of the
/// same class overlaps it by more than `iou`. The result is in canonical order and independent of the input order.
pub fn nms(dets: &[Detection], iou: Fraction) -> Result<Vec<Detection>, DetectError> {
    bounded(dets.len())?;
    let mut sorted = dets.to_vec();
    sorted.sort_by(canonical);
    let mut kept: Vec<Detection> = Vec::with_capacity(sorted.len());
    for d in sorted {
        if !kept.iter().any(|k| k.class == d.class && k.rect.iou(&d.rect) > iou.get()) {
            kept.push(d);
        }
    }
    Ok(kept)
}

/// Joins boxes of the same class that a tile border cut apart: two boxes merge when they touch or overlap (allowing
/// `gap` logical points) and **both** touch the same seam strip (the overlap of two neighbouring tiles). Merging is
/// transitive; a merged box is the union and keeps the best score. Boxes away from every seam are never merged, so two
/// separate neighbouring buttons stay two. Run [`nms`] afterwards for duplicates the overlap produced.
pub fn merge_seams(dets: &[Detection], seams: &[Rect<Logical>], gap: f64) -> Result<Vec<Detection>, DetectError> {
    bounded(dets.len())?;
    let mut sorted = dets.to_vec();
    sorted.sort_by(canonical);
    let on_seam: Vec<Vec<usize>> = sorted
        .iter()
        .map(|d| (0..seams.len()).filter(|&i| d.rect.touches(&seams[i], 0.0)).collect())
        .collect();
    let mut parent: Vec<usize> = (0..sorted.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..sorted.len() {
        for j in i + 1..sorted.len() {
            let shares_seam = on_seam[i].iter().any(|s| on_seam[j].contains(s));
            if sorted[i].class == sorted[j].class && shares_seam && sorted[i].rect.touches(&sorted[j].rect, gap) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[b.max(a)] = a.min(b);
            }
        }
    }
    let mut groups: Vec<Option<Detection>> = vec![None; sorted.len()];
    for (i, d) in sorted.iter().enumerate() {
        let root = find(&mut parent, i);
        groups[root] = Some(match groups[root] {
            None => *d,
            Some(g) => Detection {
                rect: g.rect.union(&d.rect),
                class: g.class,
                score: g.score,
            },
        });
    }
    // Sorted best score first, so the first member of a group already carries the group's best score.
    let mut out: Vec<Detection> = groups.into_iter().flatten().collect();
    out.sort_by(canonical);
    Ok(out)
}

/// The coordinate space a hint rectangle is given in. The adapter must say which; Sighter never guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintSpace {
    /// The host's input space: macOS global points, or physical virtual-screen pixels of a Per-Monitor V2 aware Windows
    /// adapter (for example FlaUI's `BoundingRectangle` in an aware process).
    HostInput,
    /// Capture pixels of the frame Sighter sees.
    Capture,
    /// Remote pixels.
    RemotePx,
    /// Logical points (already on the tile grid).
    Logical,
    /// Coordinates virtualised by a DPI-unaware process: they cannot be mapped and are refused.
    DpiVirtualised,
}

/// A rectangle a user's own framework adapter passed as an optional hint.
#[derive(Debug, Clone, PartialEq)]
pub struct HintRect {
    adapter: String,
    space: HintSpace,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl HintRect {
    /// Builds a hint; the adapter name is truncated to [`MAX_ADAPTER_NAME`] characters.
    pub fn new(adapter: &str, space: HintSpace, x: f64, y: f64, w: f64, h: f64) -> Result<Self, DetectError> {
        Rect::<Logical>::new(x, y, w, h)?;
        Ok(Self {
            adapter: adapter.chars().take(MAX_ADAPTER_NAME).collect(),
            space,
            x,
            y,
            w,
            h,
        })
    }
    /// The adapter that sent the hint.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }
    /// The declared space.
    pub fn space(&self) -> HintSpace {
        self.space
    }
    /// Maps the hint onto the logical grid through the one chain.
    pub fn to_logical(&self, chain: &Chain) -> Result<Rect<Logical>, DetectError> {
        Ok(match self.space {
            HintSpace::DpiVirtualised => {
                return Err(DetectError::UnmappableHint {
                    adapter: self.adapter.clone(),
                });
            }
            HintSpace::Logical => Rect::<Logical>::new(self.x, self.y, self.w, self.h)?,
            HintSpace::HostInput => chain
                .input_to_logical()
                .rect(Rect::<HostInput>::new(self.x, self.y, self.w, self.h)?),
            HintSpace::Capture => chain
                .capture_to_logical()
                .rect(Rect::<CapturePx>::new(self.x, self.y, self.w, self.h)?),
            HintSpace::RemotePx => chain
                .logical_to_remote()
                .inverse()
                .rect(Rect::<RemotePx>::new(self.x, self.y, self.w, self.h)?),
        })
    }
}

/// Where a candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Found by the detector with no hint involved.
    Detected,
    /// Found by the detector inside the hint with this index (recorded as hint-sourced).
    HintSourced(usize),
}

/// A detection together with how it was found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// The detection; its box and score always come from the detector, never from a hint.
    pub detection: Detection,
    /// Whether a hint narrowed the search to it.
    pub provenance: Provenance,
}

/// Narrows and ranks detections by hints (already mapped to logical points). Without hints every detection passes as
/// [`Provenance::Detected`]. With hints, a detection stays when at least `min_inside` of its area (or, for an empty box,
/// its centre) lies inside a hint; it is recorded against the first such hint and ranked by hint index, then score.
/// A hint with no detection inside it yields nothing: hints never create candidates.
pub fn apply_hints(dets: &[Detection], hints: &[Rect<Logical>], min_inside: Fraction) -> Result<Vec<Candidate>, DetectError> {
    bounded(dets.len())?;
    let mut sorted = dets.to_vec();
    sorted.sort_by(canonical);
    if hints.is_empty() {
        return Ok(sorted
            .into_iter()
            .map(|detection| Candidate {
                detection,
                provenance: Provenance::Detected,
            })
            .collect());
    }
    let mut out = Vec::new();
    for d in sorted {
        let inside = |h: &Rect<Logical>| match d.rect.area() {
            a if a > 0.0 => h.intersection(&d.rect).map_or(0.0, |i| i.area()) / a >= min_inside.get(),
            _ => h.contains_pt(d.rect.center()),
        };
        if let Some(i) = hints.iter().position(inside) {
            out.push(Candidate {
                detection: d,
                provenance: Provenance::HintSourced(i),
            });
        }
    }
    // Stable sort: equal hint index keeps the canonical (score) order.
    out.sort_by_key(|c| match c.provenance {
        Provenance::HintSourced(i) => i,
        Provenance::Detected => usize::MAX,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::InputSpace;
    use crate::geometry::{Pt, Scale};
    use proptest::prelude::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect<Logical> {
        Rect::new(x, y, w, h).unwrap()
    }
    fn det(x: f64, y: f64, w: f64, h: f64, class: u32, score: f64) -> Detection {
        Detection {
            rect: rect(x, y, w, h),
            class: ClassId(class),
            score: Score::new(score).unwrap(),
        }
    }
    fn frac(v: f64) -> Fraction {
        Fraction::new(v).unwrap()
    }

    #[test]
    fn newtypes_refuse_bad_values() {
        assert!(matches!(Score::new(f64::NAN), Err(DetectError::BadScore(_))));
        assert!(Score::new(1.01).is_err() && Score::new(-0.1).is_err() && Score::new(0.0).is_ok() && Score::new(1.0).is_ok());
        assert!(Fraction::new(0.0).is_err() && Fraction::new(f64::NAN).is_err() && Fraction::new(1.1).is_err());
        assert_eq!(Fraction::new(0.5).map(Fraction::get), Ok(0.5));
        assert_eq!(Score::new(0.25).map(Score::get), Ok(0.25));
    }

    #[test]
    fn nms_keeps_the_best_of_overlapping_same_class_boxes_only() {
        let dets = [
            det(0.0, 0.0, 10.0, 10.0, 1, 0.9),
            det(1.0, 0.0, 10.0, 10.0, 1, 0.8),
            det(1.0, 0.0, 10.0, 10.0, 2, 0.7),
            det(50.0, 50.0, 5.0, 5.0, 1, 0.6),
        ];
        let out = nms(&dets, frac(0.5)).unwrap();
        let scores: Vec<f64> = out.iter().map(|d| d.score.get()).collect();
        assert_eq!(scores, vec![0.9, 0.7, 0.6]); // the 0.8 duplicate goes; the other class stays
    }

    #[test]
    fn nms_ties_break_deterministically() {
        let a = det(0.0, 0.0, 10.0, 10.0, 1, 0.5);
        let b = det(2.0, 0.0, 10.0, 10.0, 1, 0.5);
        assert_eq!(nms(&[a, b], frac(0.3)).unwrap(), nms(&[b, a], frac(0.3)).unwrap());
        assert_eq!(nms(&[a, b], frac(0.3)).unwrap(), vec![a]);
    }

    #[test]
    fn seam_merge_joins_a_wide_box_cut_by_a_seam_but_not_distinct_neighbours() {
        let seam = rect(576.0, 0.0, 64.0, 640.0);
        // A text box cut at the right tile edge (x = 640) and at the left edge of the next tile (x = 576).
        let left = det(500.0, 100.0, 140.0, 20.0, 3, 0.7);
        let right = det(576.0, 100.0, 200.0, 20.0, 3, 0.9);
        let out = merge_seams(&[left, right], &[seam], 0.0).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].rect.x(), out[0].rect.right(), out[0].score.get()), (500.0, 776.0, 0.9));
        // Two buttons that touch but sit away from the seam stay two.
        let (a, b) = (det(10.0, 10.0, 20.0, 20.0, 3, 0.9), det(30.0, 10.0, 20.0, 20.0, 3, 0.9));
        assert_eq!(merge_seams(&[a, b], &[seam], 0.0).unwrap().len(), 2);
        // A different class never merges.
        let other = det(576.0, 100.0, 200.0, 20.0, 4, 0.9);
        assert_eq!(merge_seams(&[left, other], &[seam], 0.0).unwrap().len(), 2);
        // A gap within tolerance merges.
        let far = det(780.0, 100.0, 10.0, 20.0, 3, 0.4);
        let near_seam = rect(700.0, 0.0, 100.0, 640.0);
        assert_eq!(merge_seams(&[right, far], &[near_seam], 5.0).unwrap().len(), 1);
        assert_eq!(merge_seams(&[right, far], &[near_seam], 1.0).unwrap().len(), 2);
    }

    #[test]
    fn limits_are_typed_errors() {
        let many = vec![det(0.0, 0.0, 1.0, 1.0, 0, 0.5); MAX_DETECTIONS + 1];
        assert!(matches!(nms(&many, frac(0.5)), Err(DetectError::TooMany { .. })));
        assert!(matches!(merge_seams(&many, &[], 0.0), Err(DetectError::TooMany { .. })));
        assert!(matches!(
            filter_scores(&many, Score::new(0.0).unwrap()),
            Err(DetectError::TooMany { .. })
        ));
        assert!(matches!(apply_hints(&many, &[], frac(0.5)), Err(DetectError::TooMany { .. })));
        assert!(!DetectError::BadScore(2.0).to_string().is_empty());
    }

    #[test]
    fn filter_drops_low_scores() {
        let dets = [det(0.0, 0.0, 1.0, 1.0, 0, 0.2), det(5.0, 0.0, 1.0, 1.0, 0, 0.8)];
        assert_eq!(filter_scores(&dets, Score::new(0.5).unwrap()).unwrap().len(), 1);
    }

    #[test]
    fn hints_narrow_and_rank_but_never_create_candidates() {
        let dets = [det(10.0, 10.0, 20.0, 20.0, 1, 0.6), det(200.0, 200.0, 20.0, 20.0, 1, 0.9)];
        let none = apply_hints(&dets, &[], frac(0.5)).unwrap();
        assert!(none.iter().all(|c| c.provenance == Provenance::Detected) && none.len() == 2);
        let hint = rect(0.0, 0.0, 100.0, 100.0);
        let some = apply_hints(&dets, &[hint], frac(0.5)).unwrap();
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].provenance, Provenance::HintSourced(0));
        assert_eq!(some[0].detection, dets[0]); // box and score are the detector's
        // A hint with nothing detected inside it is "not found".
        assert!(apply_hints(&dets, &[rect(500.0, 500.0, 50.0, 50.0)], frac(0.5)).unwrap().is_empty());
        // Hints rank: the detection in hint 1 (listed first in the input) comes after the one in hint 0.
        let two = apply_hints(&dets, &[rect(190.0, 190.0, 50.0, 50.0), hint], frac(0.5)).unwrap();
        assert_eq!(
            two.iter().map(|c| c.provenance).collect::<Vec<_>>(),
            vec![Provenance::HintSourced(0), Provenance::HintSourced(1)]
        );
        // An empty box counts by its centre.
        let dot = det(5.0, 5.0, 0.0, 0.0, 1, 0.5);
        assert_eq!(apply_hints(&[dot], &[hint], frac(0.5)).unwrap().len(), 1);
    }

    #[test]
    fn hints_map_through_the_chain_by_their_declared_space() {
        // 200 % macOS host, local window at (100, 50) points: logical = (point - origin).
        let chain = Chain::local(
            Scale::uniform(2.0).unwrap(),
            Pt::new(0.0, 0.0).unwrap(),
            Pt::new(100.0, 50.0).unwrap(),
            InputSpace::MacPoints,
        )
        .unwrap();
        let host = HintRect::new("test-adapter", HintSpace::HostInput, 120.0, 70.0, 40.0, 20.0).unwrap();
        let l = host.to_logical(&chain).unwrap();
        assert_eq!((l.x(), l.y(), l.w(), l.h()), (20.0, 20.0, 40.0, 20.0));
        let cap = HintRect::new("a", HintSpace::Capture, 40.0, 40.0, 80.0, 40.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((cap.x(), cap.w()), (20.0, 40.0));
        let rem = HintRect::new("a", HintSpace::RemotePx, 40.0, 40.0, 80.0, 40.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((rem.x(), rem.w()), (20.0, 40.0));
        let lg = HintRect::new("a", HintSpace::Logical, 1.0, 2.0, 3.0, 4.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((lg.x(), lg.h()), (1.0, 4.0));
        let virt = HintRect::new(&"x".repeat(200), HintSpace::DpiVirtualised, 1.0, 1.0, 1.0, 1.0).unwrap();
        assert_eq!(virt.adapter().len(), MAX_ADAPTER_NAME);
        assert_eq!(virt.space(), HintSpace::DpiVirtualised);
        assert!(matches!(virt.to_logical(&chain), Err(DetectError::UnmappableHint { .. })));
        assert!(HintRect::new("a", HintSpace::HostInput, f64::NAN, 0.0, 1.0, 1.0).is_err());
    }

    fn arb_det() -> impl Strategy<Value = Detection> {
        (0.0f64..700.0, 0.0f64..700.0, 0.0f64..200.0, 0.0f64..200.0, 0u32..3, 0.0f64..=1.0)
            .prop_map(|(x, y, w, h, c, s)| det(x, y, w, h, c, s))
    }
    fn arb_dets() -> impl Strategy<Value = Vec<Detection>> {
        prop::collection::vec(arb_det(), 0..40)
    }
    fn arb_frac() -> impl Strategy<Value = Fraction> {
        (0.01f64..=1.0).prop_map(frac)
    }

    proptest! {
        #[test]
        fn nms_output_is_a_subset_with_no_same_class_overlap_above_the_threshold(dets in arb_dets(), t in arb_frac()) {
            let out = nms(&dets, t).unwrap();
            prop_assert!(out.iter().all(|o| dets.contains(o)));
            for (i, a) in out.iter().enumerate() {
                for b in &out[i + 1..] {
                    prop_assert!(a.class != b.class || a.rect.iou(&b.rect) <= t.get());
                }
            }
            // Every dropped box is covered by a kept box of the same class with a score at least as high.
            for d in &dets {
                prop_assert!(out.iter().any(|k| k.class == d.class && k.score >= d.score && (k == d || k.rect.iou(&d.rect) > t.get())));
            }
        }

        #[test]
        fn nms_is_idempotent_and_order_independent(dets in arb_dets(), t in arb_frac()) {
            let out = nms(&dets, t).unwrap();
            prop_assert_eq!(nms(&out, t).unwrap(), out.clone());
            let mut rev = dets.clone();
            rev.reverse();
            prop_assert_eq!(nms(&rev, t).unwrap(), out);
        }

        #[test]
        fn merge_covers_every_input_and_never_grows(dets in arb_dets(), gap in 0.0f64..5.0) {
            let seams = [rect(576.0, 0.0, 64.0, 700.0), rect(0.0, 576.0, 700.0, 64.0)];
            let out = merge_seams(&dets, &seams, gap).unwrap();
            prop_assert!(out.len() <= dets.len());
            for d in &dets {
                // Containment up to floating-point rounding of `x + (right - x)`.
                let eps = 1e-9;
                prop_assert!(out.iter().any(|m| m.class == d.class
                    && m.rect.x() <= d.rect.x() + eps && m.rect.y() <= d.rect.y() + eps
                    && m.rect.right() + eps >= d.rect.right() && m.rect.bottom() + eps >= d.rect.bottom()
                    && m.score >= d.score));
            }
            let mut rev = dets.clone();
            rev.reverse();
            prop_assert_eq!(merge_seams(&rev, &seams, gap).unwrap(), out);
        }

        #[test]
        fn hints_only_select_never_alter(dets in arb_dets(), hx in 0.0f64..600.0, hy in 0.0f64..600.0, t in arb_frac()) {
            let out = apply_hints(&dets, &[rect(hx, hy, 150.0, 150.0)], t).unwrap();
            prop_assert!(out.len() <= dets.len());
            prop_assert!(out.iter().all(|c| dets.contains(&c.detection) && matches!(c.provenance, Provenance::HintSourced(0))));
        }
    }
}
