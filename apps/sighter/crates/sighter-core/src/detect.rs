//! The detection result model and the pure functions over it: class-aware non-maximum suppression, the seam merge for
//! boxes cut by a tile border, and adapter hints. All boxes are in the logical space of the one tile grid
//! ([`Logical`]); results are mapped to other spaces through the [`Chain`](crate::chain::Chain).

use crate::chain::Chain;
use crate::geometry::{CapturePx, HostInput, Logical, Rect, RemotePx};
use crate::tiling::TileGrid;
use std::cmp::Ordering;

/// Most detections one call accepts; keeps the quadratic passes bounded.
pub const MAX_DETECTIONS: usize = 10_000;
/// Default overlap fraction of two cut parts for the seam merge (`SEAM_IOU`, a validated setting in (0, 1]).
pub const SEAM_IOU: f64 = 0.5;
/// Default share of a detection's area that must lie inside a grown hint (`HINT_CONTAINMENT`, a validated setting).
pub const HINT_CONTAINMENT: f64 = 0.5;
/// Default growth of a mapped hint on every side, in logical points (a tunable).
pub const HINT_MARGIN: f64 = 8.0;
/// Largest accepted hint margin.
pub const MAX_HINT_MARGIN: f64 = 256.0;
/// Longest part of an untrusted wire tag echoed in an error.
pub const MAX_TAG_ECHO: usize = 64;
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
    /// A detection names a tile the grid does not have.
    #[error("a detection names tile {0}, which the tile grid does not have")]
    UnknownTile(usize),
    /// A hint arrived without a DPI awareness, or with an unknown one.
    #[error("a hint needs the adapter's DPI awareness: per-monitor-v2, per-monitor, system or unaware (got {0:?})")]
    UnknownDpiAwareness(Option<String>),
    /// A hint arrived without a coordinate space tag, or with an unknown one.
    #[error("a hint needs a space tag: host-input, remote-pixels, capture-pixels or logical (got {0:?})")]
    UnknownHintSpace(Option<String>),
    /// A margin was negative, not a number or absurdly large.
    #[error("a hint margin must be between 0 and {MAX_HINT_MARGIN} logical points, got {0}")]
    BadMargin(f64),
    /// A hint's rectangle could not be built.
    #[error(transparent)]
    Geometry(#[from] crate::geometry::GeometryError),
    /// A hint came from a coordinate space that cannot be mapped (an adapter that is not DPI aware sees virtualised
    /// coordinates that no chain can undo).
    #[error("The hint from {adapter} is in DPI-virtualised coordinates; make the adapter Per-Monitor V2 DPI aware")]
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
    /// The id of the tile the detection came from ([`crate::tiling::Tile::id`]).
    pub tile: usize,
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
        .then(a.tile.cmp(&b.tile))
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

/// Joins the parts of one element that a tile border cut apart (ADR 0001, "Decisions for the pure core", 2). Two
/// detections of the same class merge only when they come from **different, neighbouring tiles** (tiles that share an
/// overlap strip), **both** intersect that strip, and their parts inside the strip agree: the IoU of `A ∩ strip` and
/// `B ∩ strip` is at least `seam_iou`. Detections of the same tile never merge. Merging is transitive (a box cut at a
/// four-tile corner); a merged box is the union and keeps the best score and that detection's tile. Two separate
/// neighbours in a strip are each seen whole by both tiles and pair with their own copies, not with each other. Run
/// [`nms`] afterwards to remove the duplicates the overlap produced.
pub fn merge_seams(dets: &[Detection], grid: &TileGrid, seam_iou: Fraction) -> Result<Vec<Detection>, DetectError> {
    bounded(dets.len())?;
    if let Some(d) = dets.iter().find(|d| d.tile >= grid.tiles().len()) {
        return Err(DetectError::UnknownTile(d.tile));
    }
    let mut sorted = dets.to_vec();
    sorted.sort_by(canonical);
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
            let (a, b) = (&sorted[i], &sorted[j]);
            if a.class != b.class || a.tile == b.tile {
                continue;
            }
            let Some(strip) = grid.tiles()[a.tile].rect().intersection(&grid.tiles()[b.tile].rect()) else {
                continue;
            };
            let (Some(pa), Some(pb)) = (a.rect.intersection(&strip), b.rect.intersection(&strip)) else {
                continue;
            };
            if pa.iou(&pb) >= seam_iou.get() {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                parent[ra.max(rb)] = ra.min(rb);
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
                ..g
            },
        });
    }
    // Sorted best score first, so the first member of a group already carries the group's best score and tile.
    let mut out: Vec<Detection> = groups.into_iter().flatten().collect();
    out.sort_by(canonical);
    Ok(out)
}

/// The coordinate space a hint rectangle is given in. The adapter must say which; Sighter never guesses. Wire tags are
/// `host-input`, `remote-pixels`, `capture-pixels`, `logical` and `virtualised`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintSpace {
    /// `host-input`: the host's input space: macOS global points, or physical virtual-screen pixels of a Per-Monitor
    /// aware Windows adapter (for example FlaUI's `BoundingRectangle`).
    HostInput,
    /// `remote-pixels`: pixels of the remote desktop, from an adapter running inside it.
    RemotePixels,
    /// `capture-pixels`: only for boxes Sighter returned earlier (a client narrowing a follow-up query).
    CapturePixels,
    /// `logical`: only for boxes Sighter returned earlier, already on the tile grid.
    Logical,
    /// `virtualised`: coordinates from a system-aware or unaware reader; they cannot be mapped and are refused.
    Virtualised,
}

impl HintSpace {
    /// Parses a wire tag; a missing or unknown tag is refused at the edge.
    pub fn from_tag(tag: Option<&str>) -> Result<Self, DetectError> {
        Ok(match tag {
            Some("host-input") => Self::HostInput,
            Some("remote-pixels") => Self::RemotePixels,
            Some("capture-pixels") => Self::CapturePixels,
            Some("logical") => Self::Logical,
            Some("virtualised") => Self::Virtualised,
            other => return Err(DetectError::UnknownHintSpace(other.map(|t| t.chars().take(MAX_TAG_ECHO).collect()))),
        })
    }
    /// The wire tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::HostInput => "host-input",
            Self::RemotePixels => "remote-pixels",
            Self::CapturePixels => "capture-pixels",
            Self::Logical => "logical",
            Self::Virtualised => "virtualised",
        }
    }
}

/// How DPI aware the adapter's reading thread is, as the client states it with each hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiAwareness {
    /// `per-monitor-v2`.
    PerMonitorV2,
    /// `per-monitor`.
    PerMonitor,
    /// `system`: coordinates are virtualised per monitor.
    System,
    /// `unaware`: coordinates are virtualised.
    Unaware,
}

impl DpiAwareness {
    /// Parses a wire tag; a missing or unknown tag is refused at the edge (the echo is cut to [`MAX_TAG_ECHO`]).
    pub fn from_tag(tag: Option<&str>) -> Result<Self, DetectError> {
        Ok(match tag {
            Some("per-monitor-v2") => Self::PerMonitorV2,
            Some("per-monitor") => Self::PerMonitor,
            Some("system") => Self::System,
            Some("unaware") => Self::Unaware,
            other => {
                return Err(DetectError::UnknownDpiAwareness(
                    other.map(|t| t.chars().take(MAX_TAG_ECHO).collect()),
                ));
            }
        })
    }
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
    /// Builds a hint from its wire form: the space tag and the adapter's DPI awareness are both required. An adapter
    /// that is `system` or `unaware` reads virtualised coordinates, so a `host-input` or `remote-pixels` hint from it
    /// becomes [`HintSpace::Virtualised`] whatever the tag says (`capture-pixels` and `logical` are Sighter's own boxes and
    /// do not depend on the reader).
    pub fn from_wire(
        adapter: &str,
        tag: Option<&str>,
        awareness: Option<&str>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<Self, DetectError> {
        let space = HintSpace::from_tag(tag)?;
        let awareness = DpiAwareness::from_tag(awareness)?;
        let space = match (space, awareness) {
            (HintSpace::HostInput | HintSpace::RemotePixels, DpiAwareness::System | DpiAwareness::Unaware) => HintSpace::Virtualised,
            (s, _) => s,
        };
        Self::new(adapter, space, x, y, w, h)
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
            HintSpace::Virtualised => {
                return Err(DetectError::UnmappableHint {
                    adapter: self.adapter.clone(),
                });
            }
            HintSpace::Logical => Rect::<Logical>::new(self.x, self.y, self.w, self.h)?,
            HintSpace::HostInput => chain
                .input_to_logical()
                .rect(Rect::<HostInput>::new(self.x, self.y, self.w, self.h)?),
            HintSpace::CapturePixels => chain
                .capture_to_logical()
                .rect(Rect::<CapturePx>::new(self.x, self.y, self.w, self.h)?),
            HintSpace::RemotePixels => chain
                .logical_to_remote()
                .inverse()
                .rect(Rect::<RemotePx>::new(self.x, self.y, self.w, self.h)?),
        })
    }
}

/// Hints mapped, grown and clipped for one query. A refused hint does not fail the batch: the usable ones still apply.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedHints {
    /// The usable hints in logical points, in input order (a dropped or refused hint leaves no entry).
    pub rects: Vec<Rect<Logical>>,
    /// Indices of the input hints that fell outside the scope and were dropped (recorded in the trace).
    pub dropped: Vec<usize>,
    /// Hints that could not be used at all, with the reason: a virtualised hint (its message names the adapter and what to
    /// fix), or one whose mapped rectangle leaves the supported coordinate range. The caller reports each and goes on.
    pub refused: Vec<(usize, DetectError)>,
}

impl PreparedHints {
    /// When the query had hints and **every** one fell outside the scope (none usable, none refused), how many there
    /// were. The query then runs unhinted (degrade, don't fail) and the caller records
    /// [`crate::hud::trace::HintEvent::AllOutsideScope`]; `None` otherwise.
    pub fn all_dropped(&self) -> Option<u32> {
        (self.rects.is_empty() && self.refused.is_empty() && !self.dropped.is_empty())
            .then(|| u32::try_from(self.dropped.len()).unwrap_or(u32::MAX))
    }
}

/// Maps each hint through the chain, grows it by `margin` logical points on every side (a hint can be stale), and clips it
/// to the query's `scope`; a hint with nothing left inside the scope is dropped and recorded, and a hint that cannot be
/// used (virtualised, or mapped out of range) is recorded in [`PreparedHints::refused`] while the others still apply.
/// Only a bad `margin` fails the call.
pub fn prepare_hints(hints: &[HintRect], chain: &Chain, scope: &Rect<Logical>, margin: f64) -> Result<PreparedHints, DetectError> {
    if margin.is_nan() || !(0.0..=MAX_HINT_MARGIN).contains(&margin) {
        return Err(DetectError::BadMargin(margin));
    }
    let mut out = PreparedHints {
        rects: Vec::new(),
        dropped: Vec::new(),
        refused: Vec::new(),
    };
    for (i, h) in hints.iter().enumerate() {
        let grown = h.to_logical(chain).and_then(|r| {
            Ok(Rect::new(
                r.x() - margin,
                r.y() - margin,
                r.w() + 2.0 * margin,
                r.h() + 2.0 * margin,
            )?)
        });
        match grown {
            Ok(g) => match g.clip(scope) {
                Some(c) => out.rects.push(c),
                None => out.dropped.push(i),
            },
            Err(e) => out.refused.push((i, e)),
        }
    }
    Ok(out)
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

/// Narrows and ranks detections by hints (already prepared by [`prepare_hints`]; `min_inside` is `HINT_CONTAINMENT`). Without hints every detection passes as
/// [`Provenance::Detected`]. With hints, a detection stays when at least `min_inside` of its area lies inside a hint, or at
/// least `min_inside` of the hint lies inside it (a hint smaller than its element), or, for an empty box, its centre lies inside; it is recorded against the first such hint and ranked by hint index, then score.
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
        // Two-way: enough of the detection lies inside the hint, or enough of the hint lies inside the detection (a hint
        // smaller than its element).
        let inside = |h: &Rect<Logical>| match d.rect.area() {
            a if a > 0.0 => {
                let shared = h.intersection(&d.rect).map_or(0.0, |i| i.area());
                shared / a >= min_inside.get() || (h.area() > 0.0 && shared / h.area() >= min_inside.get())
            }
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
    out.sort_by_key(|c| {
        if let Provenance::HintSourced(i) = c.provenance {
            i
        } else {
            usize::MAX
        }
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{ChainParams, InputSpace};
    use crate::geometry::{Pt, Scale};
    use crate::tiling::{GridSpec, tile_grid};
    use proptest::prelude::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect<Logical> {
        Rect::new(x, y, w, h).unwrap()
    }
    fn det(x: f64, y: f64, w: f64, h: f64, class: u32, score: f64) -> Detection {
        Detection {
            rect: rect(x, y, w, h),
            class: ClassId(class),
            score: Score::new(score).unwrap(),
            tile: 0,
        }
    }
    fn on(tile: usize, d: Detection) -> Detection {
        Detection { tile, ..d }
    }
    /// The 1920 x 1200 grid: tiles 0 (0,0), 1 (576,0), 2 (1152,0), 3 (1280,0), 4..7 the second row at y = 560.
    fn grid() -> TileGrid {
        tile_grid(1920, 1200, GridSpec::default_640()).unwrap()
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
    fn seam_merge_joins_one_wide_box_cut_by_a_seam() {
        // Tiles 0 and 1 share the strip x 576..640, y 0..640. A text box 500..776 is cut: tile 0 sees up to x = 640, tile 1
        // from x = 576. Their parts inside the strip are the same region.
        let left = on(0, det(500.0, 100.0, 140.0, 20.0, 3, 0.7));
        let right = on(1, det(576.0, 100.0, 200.0, 20.0, 3, 0.9));
        let out = merge_seams(&[left, right], &grid(), Fraction::new(SEAM_IOU).unwrap()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            (out[0].rect.x(), out[0].rect.right(), out[0].score.get(), out[0].tile),
            (500.0, 776.0, 0.9, 1)
        );
    }

    #[test]
    fn two_separate_buttons_four_points_apart_in_a_strip_stay_two() {
        // Both tiles see both buttons whole: each button pairs with its own copy, never with the other button.
        let (a, b) = ((590.0, 100.0), (614.0, 100.0)); // 20 wide, 4 apart
        let dets = [
            on(0, det(a.0, a.1, 20.0, 20.0, 3, 0.9)),
            on(1, det(a.0, a.1, 20.0, 20.0, 3, 0.8)),
            on(0, det(b.0, b.1, 20.0, 20.0, 3, 0.7)),
            on(1, det(b.0, b.1, 20.0, 20.0, 3, 0.6)),
        ];
        let merged = merge_seams(&dets, &grid(), Fraction::new(SEAM_IOU).unwrap()).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(nms(&merged, Fraction::new(0.5).unwrap()).unwrap().len(), 2);
    }

    #[test]
    fn a_box_cut_at_a_four_tile_corner_becomes_one() {
        // Tiles 0, 1, 4, 5 meet around x 576..640, y 560..640. One 100 x 100 element at (540, 540) is cut four ways.
        let whole = rect(540.0, 540.0, 100.0, 100.0);
        let parts = [
            (0, rect(540.0, 540.0, 100.0, 100.0)),
            (1, rect(576.0, 540.0, 64.0, 100.0)),
            (4, rect(540.0, 560.0, 100.0, 80.0)),
            (5, rect(576.0, 560.0, 64.0, 80.0)),
        ];
        let dets: Vec<Detection> = parts
            .iter()
            .enumerate()
            .map(|(i, (t, r))| Detection {
                rect: *r,
                class: ClassId(1),
                score: Score::new(0.5 + i as f64 / 10.0).unwrap(),
                tile: *t,
            })
            .collect();
        let out = merge_seams(&dets, &grid(), Fraction::new(SEAM_IOU).unwrap()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rect, whole);
        assert_eq!(out[0].score.get(), 0.8);
    }

    #[test]
    fn detections_of_one_tile_never_merge_and_other_classes_do_not_either() {
        let a = on(0, det(590.0, 100.0, 20.0, 20.0, 3, 0.9));
        let b = on(0, det(590.0, 100.0, 20.0, 20.0, 3, 0.8));
        assert_eq!(merge_seams(&[a, b], &grid(), Fraction::new(0.1).unwrap()).unwrap().len(), 2);
        let c = on(1, det(590.0, 100.0, 20.0, 20.0, 4, 0.8));
        assert_eq!(merge_seams(&[a, c], &grid(), Fraction::new(0.1).unwrap()).unwrap().len(), 2);
        // Tiles that share no strip (0 and 3) never merge even with identical boxes.
        let far = on(3, det(590.0, 100.0, 20.0, 20.0, 3, 0.8));
        assert_eq!(merge_seams(&[a, far], &grid(), Fraction::new(0.1).unwrap()).unwrap().len(), 2);
        // A box outside the strip does not merge with its neighbour tile's copy.
        let outside = on(1, det(700.0, 100.0, 20.0, 20.0, 3, 0.8));
        assert_eq!(merge_seams(&[a, outside], &grid(), Fraction::new(0.1).unwrap()).unwrap().len(), 2);
        // A tile the grid does not have is a typed error.
        assert_eq!(
            merge_seams(&[on(99, a)], &grid(), Fraction::new(0.5).unwrap()),
            Err(DetectError::UnknownTile(99))
        );
        assert!(Fraction::new(0.0).is_err() && Fraction::new(1.0).is_ok());
    }

    #[test]
    fn limits_are_typed_errors() {
        let many = vec![det(0.0, 0.0, 1.0, 1.0, 0, 0.5); MAX_DETECTIONS + 1];
        assert!(matches!(nms(&many, frac(0.5)), Err(DetectError::TooMany { .. })));
        assert!(matches!(
            merge_seams(&many, &grid(), Fraction::new(0.5).unwrap()),
            Err(DetectError::TooMany { .. })
        ));
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
    fn hint_spaces_have_wire_tags_and_untagged_hints_are_refused() {
        for t in ["host-input", "remote-pixels", "capture-pixels", "logical", "virtualised"] {
            assert_eq!(HintSpace::from_tag(Some(t)).unwrap().tag(), t);
        }
        assert_eq!(HintSpace::from_tag(None), Err(DetectError::UnknownHintSpace(None)));
        assert_eq!(
            HintSpace::from_tag(Some("pixels")),
            Err(DetectError::UnknownHintSpace(Some("pixels".into())))
        );
        assert!(HintRect::from_wire("a", None, Some("per-monitor-v2"), 0.0, 0.0, 1.0, 1.0).is_err());
        assert_eq!(
            HintRect::from_wire("a", Some("logical"), Some("unaware"), 0.0, 0.0, 1.0, 1.0)
                .unwrap()
                .space(),
            HintSpace::Logical
        );
    }

    #[test]
    fn untrusted_tags_are_cut_in_errors() {
        let long = "x".repeat(10_000);
        assert_eq!(
            HintSpace::from_tag(Some(&long)),
            Err(DetectError::UnknownHintSpace(Some("x".repeat(MAX_TAG_ECHO))))
        );
        assert_eq!(
            DpiAwareness::from_tag(Some(&long)),
            Err(DetectError::UnknownDpiAwareness(Some("x".repeat(MAX_TAG_ECHO))))
        );
        assert_eq!(DpiAwareness::from_tag(None), Err(DetectError::UnknownDpiAwareness(None)));
    }

    #[test]
    fn system_and_unaware_adapters_make_host_and_remote_hints_virtualised() {
        let wire = |tag, aw| {
            HintRect::from_wire("FlaUI", Some(tag), Some(aw), 1.0, 1.0, 5.0, 5.0)
                .unwrap()
                .space()
        };
        for aw in ["system", "unaware"] {
            assert_eq!(wire("host-input", aw), HintSpace::Virtualised);
            assert_eq!(wire("remote-pixels", aw), HintSpace::Virtualised);
            // Sighter's own earlier boxes do not depend on the reader.
            assert_eq!(wire("logical", aw), HintSpace::Logical);
            assert_eq!(wire("capture-pixels", aw), HintSpace::CapturePixels);
        }
        for aw in ["per-monitor-v2", "per-monitor"] {
            assert_eq!(wire("host-input", aw), HintSpace::HostInput);
            assert_eq!(wire("remote-pixels", aw), HintSpace::RemotePixels);
        }
        assert_eq!(wire("virtualised", "per-monitor-v2"), HintSpace::Virtualised);
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
        let cap = HintRect::new("a", HintSpace::CapturePixels, 40.0, 40.0, 80.0, 40.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((cap.x(), cap.w()), (20.0, 40.0));
        let rem = HintRect::new("a", HintSpace::RemotePixels, 40.0, 40.0, 80.0, 40.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((rem.x(), rem.w()), (20.0, 40.0));
        let lg = HintRect::new("a", HintSpace::Logical, 1.0, 2.0, 3.0, 4.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap();
        assert_eq!((lg.x(), lg.h()), (1.0, 4.0));
        let virt = HintRect::new(&"x".repeat(200), HintSpace::Virtualised, 1.0, 1.0, 1.0, 1.0).unwrap();
        assert_eq!(virt.adapter().len(), MAX_ADAPTER_NAME);
        assert_eq!(virt.space(), HintSpace::Virtualised);
        let err = HintRect::new("FlaUI", HintSpace::Virtualised, 1.0, 1.0, 1.0, 1.0)
            .unwrap()
            .to_logical(&chain)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "The hint from FlaUI is in DPI-virtualised coordinates; make the adapter Per-Monitor V2 DPI aware"
        );
        assert!(HintRect::new("a", HintSpace::HostInput, f64::NAN, 0.0, 1.0, 1.0).is_err());
    }

    #[test]
    fn a_remote_pixels_hint_with_r_1_5_and_s_0_5_names_the_right_logical_rectangle() {
        // Viewer window: remote r = 1.5 shown at s = 0.5 on a 200 % host (content origin (0, 28), window at (100, 50)).
        let chain = Chain::new(ChainParams {
            viewer_scale: Scale::uniform(0.5).unwrap(),
            remote_scale: Scale::uniform(1.5).unwrap(),
            host_scale: Scale::uniform(2.0).unwrap(),
            content_origin: Pt::new(0.0, 28.0).unwrap(),
            window_origin: Pt::new(100.0, 50.0).unwrap(),
            input: InputSpace::MacPoints,
        })
        .unwrap();
        // The element is the logical rectangle (200, 100, 80, 24).
        let element = rect(200.0, 100.0, 80.0, 24.0);
        let remote = chain.logical_to_remote().rect(element);
        let from_remote = HintRect::new("in-vm", HintSpace::RemotePixels, remote.x(), remote.y(), remote.w(), remote.h()).unwrap();
        let from_host_rect = chain.logical_to_input().rect(element);
        let from_host = HintRect::new(
            "fla",
            HintSpace::HostInput,
            from_host_rect.x(),
            from_host_rect.y(),
            from_host_rect.w(),
            from_host_rect.h(),
        )
        .unwrap();
        for h in [from_remote, from_host] {
            let l = h.to_logical(&chain).unwrap();
            assert!(
                (l.x() - 200.0).abs() <= 1.0 && (l.y() - 100.0).abs() <= 1.0 && (l.w() - 80.0).abs() <= 1.0 && (l.h() - 24.0).abs() <= 1.0,
                "{l:?}"
            );
        }
    }

    #[test]
    fn a_hint_on_each_display_of_the_mixed_layout_maps_to_its_element_in_both_host_spaces() {
        use crate::chain::{Display, Layout};
        let pt = |x, y| Pt::new(x, y).unwrap();
        let sc = |v| Scale::uniform(v).unwrap();
        // The ADR's layout: 100 % 1920 x 1200 left, 200 % 3840 x 2160 right, 150 % 2560 x 1440 above with a negative origin.
        let windows = Layout::new(
            InputSpace::WindowsPixels,
            vec![
                Display::new(1, pt(0.0, 0.0), 1920, 1200, sc(1.0)).unwrap(),
                Display::new(2, pt(1920.0, 0.0), 3840, 2160, sc(2.0)).unwrap(),
                Display::new(3, pt(0.0, -1440.0), 2560, 1440, sc(1.5)).unwrap(),
            ],
        )
        .unwrap();
        let mac = Layout::new(
            InputSpace::MacPoints,
            vec![
                Display::new(1, pt(0.0, 0.0), 1920, 1200, sc(1.0)).unwrap(),
                Display::new(2, pt(1920.0, 0.0), 3840, 2160, sc(2.0)).unwrap(),
                Display::new(3, pt(0.0, -960.0), 2560, 1440, sc(1.5)).unwrap(),
            ],
        )
        .unwrap();
        let element = rect(300.0, 200.0, 80.0, 24.0); // logical points on its display
        for layout in [&windows, &mac] {
            for id in [1, 2, 3] {
                let chain = layout.chain_for_display(id).unwrap();
                let host = chain.logical_to_input().rect(element);
                let hint = HintRect::from_wire(
                    "adapter",
                    Some("host-input"),
                    Some("per-monitor-v2"),
                    host.x(),
                    host.y(),
                    host.w(),
                    host.h(),
                )
                .unwrap();
                let l = hint.to_logical(&chain).unwrap();
                let got = [l.x() - 300.0, l.y() - 200.0, l.w() - 80.0, l.h() - 24.0];
                assert!(got.iter().all(|e| e.abs() <= 1.0));
            }
        }
    }

    #[test]
    fn prepared_hints_are_grown_clipped_to_the_scope_and_recorded_when_dropped() {
        let chain = Chain::local(
            Scale::uniform(1.0).unwrap(),
            Pt::new(0.0, 0.0).unwrap(),
            Pt::new(0.0, 0.0).unwrap(),
            InputSpace::WindowsPixels,
        )
        .unwrap();
        let scope = rect(0.0, 0.0, 200.0, 200.0);
        let hints = [
            HintRect::new("a", HintSpace::Logical, 50.0, 50.0, 20.0, 20.0).unwrap(),
            HintRect::new("a", HintSpace::Logical, 195.0, 10.0, 20.0, 20.0).unwrap(),
            HintRect::new("a", HintSpace::Logical, 500.0, 500.0, 20.0, 20.0).unwrap(),
        ];
        let p = prepare_hints(&hints, &chain, &scope, HINT_MARGIN).unwrap();
        assert_eq!(p.dropped, vec![2]);
        assert_eq!(p.rects[0], rect(42.0, 42.0, 36.0, 36.0)); // 8 points each side
        assert_eq!((p.rects[1].x(), p.rects[1].right(), p.rects[1].y()), (187.0, 200.0, 2.0)); // clipped to the scope
        for bad in [-1.0, f64::NAN, MAX_HINT_MARGIN + 1.0] {
            assert!(matches!(prepare_hints(&hints, &chain, &scope, bad), Err(DetectError::BadMargin(_))));
        }
        assert_eq!(p.all_dropped(), None);
        let outside = [
            HintRect::new("a", HintSpace::Logical, 500.0, 500.0, 5.0, 5.0).unwrap(),
            HintRect::new("a", HintSpace::Logical, 600.0, 0.0, 5.0, 5.0).unwrap(),
        ];
        let all = prepare_hints(&outside, &chain, &scope, HINT_MARGIN).unwrap();
        assert_eq!((all.all_dropped(), all.rects.len()), (Some(2), 0));
        // With no usable hint the query runs unhinted: every detection passes, as for a query without hints.
        let d = det(10.0, 10.0, 5.0, 5.0, 1, 0.5);
        assert_eq!(
            apply_hints(&[d], &all.rects, Fraction::new(HINT_CONTAINMENT).unwrap())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(prepare_hints(&[], &chain, &scope, 0.0).unwrap().all_dropped(), None);
        // A refused hint does not fail the batch: the others still apply, and the refusal names its index and reason.
        let mixed = [
            HintRect::new("FlaUI", HintSpace::Virtualised, 0.0, 0.0, 1.0, 1.0).unwrap(),
            HintRect::new("a", HintSpace::Logical, 50.0, 50.0, 20.0, 20.0).unwrap(),
            // Valid on its own, but growing it by the margin leaves the supported range.
            HintRect::new("a", HintSpace::Logical, 0.0, 0.0, 9_999_999.0, 5.0).unwrap(),
        ];
        let m = prepare_hints(&mixed, &chain, &scope, 8.0).unwrap();
        assert_eq!(m.rects, vec![rect(42.0, 42.0, 36.0, 36.0)]);
        assert_eq!(m.refused.len(), 2);
        assert!(matches!(m.refused[0], (0, DetectError::UnmappableHint { .. })));
        assert!(matches!(m.refused[1], (2, DetectError::Geometry(_))));
        assert_eq!(m.all_dropped(), None);
        // Only refused and dropped hints: not "all outside the scope".
        let only = prepare_hints(&mixed[..1], &chain, &scope, 0.0).unwrap();
        assert!(only.rects.is_empty() && only.all_dropped().is_none());
        // Two-way containment of the grown hint (42..78 x 42..78): a detection qualifies when at least HINT_CONTAINMENT of it
        // lies inside the hint, or at least HINT_CONTAINMENT of the hint lies inside it.
        let grown = [p.rects[0]];
        let t = Fraction::new(HINT_CONTAINMENT).unwrap();
        // Direction 1: the detection is mostly inside the hint (36 of 72 wide = 0.5).
        let half = det(42.0, 42.0, 72.0, 36.0, 1, 0.9);
        // Direction 2: the hint is smaller than its element; the detection holds the whole hint (hint share 1.0) but only
        // 36 x 36 of 400 x 400 of it lies inside the hint.
        let big = det(0.0, 0.0, 400.0, 400.0, 1, 0.8);
        // Neither: it overlaps 12 of 90 wide of itself (0.13) and 12 of 36 of the hint (0.33).
        let neither = det(66.0, 42.0, 90.0, 36.0, 1, 0.7);
        assert_eq!(apply_hints(&[half], &grown, t).unwrap().len(), 1);
        assert_eq!(apply_hints(&[big], &grown, t).unwrap().len(), 1);
        assert!(apply_hints(&[neither], &grown, t).unwrap().is_empty());
        let kept = apply_hints(&[half, big, neither], &grown, t).unwrap();
        assert_eq!(kept.iter().map(|c| c.detection).collect::<Vec<_>>(), vec![half, big]);
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
            let out = merge_seams(&dets, &grid(), Fraction::new(0.05 + gap / 6.0).unwrap()).unwrap();
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
            prop_assert_eq!(merge_seams(&rev, &grid(), Fraction::new(0.05 + gap / 6.0).unwrap()).unwrap(), out);
        }

        #[test]
        fn hints_only_select_never_alter(dets in arb_dets(), hx in 0.0f64..600.0, hy in 0.0f64..600.0, t in arb_frac()) {
            let out = apply_hints(&dets, &[rect(hx, hy, 150.0, 150.0)], t).unwrap();
            prop_assert!(out.len() <= dets.len());
            prop_assert!(out.iter().all(|c| dets.contains(&c.detection) && matches!(c.provenance, Provenance::HintSourced(0))));
        }
    }
}
