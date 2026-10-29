//! The tile grid and the tile planner of ADR 0001 ("Screens, tiles and coordinates").
//!
//! One pure function, [`tile_grid`], defines the grid. Inference and training both call it, so training crops fall on
//! exactly the tiles the detector sees at run time. The grid lies on the **remote logical** space (100 % scale), so
//! tiles do not move when a window moves. Frame sizes here are whole logical points (round a fractional logical size
//! up at the edge).

use crate::geometry::{Logical, Map, Pt, Rect, Scale};

/// Overlap between neighbouring tiles in logical points (ADR 0001).
pub const DEFAULT_OVERLAP: u32 = 64;
/// Largest frame side accepted (16K).
pub const MAX_FRAME_SIDE: u32 = 16_384;

/// Marker: coordinates inside one tile, origin at the tile's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileLocal {}

/// Why a grid could not be built.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TilingError {
    /// A frame side was zero or above [`MAX_FRAME_SIDE`].
    #[error("frame {width} x {height} must have sides between 1 and {max} points")]
    FrameSize {
        /// Width given.
        width: u32,
        /// Height given.
        height: u32,
        /// Limit.
        max: u32,
    },
    /// A pixel buffer does not hold `width * height * 3` bytes.
    #[error("a {width} x {height} RGB frame needs {} bytes, got {len}", .width * .height * 3)]
    BadBuffer {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
        /// Buffer length given.
        len: usize,
    },
    /// A normalisation mean was not a number in 0 to 1.
    #[error("a normalisation mean must be a number between 0 and 1, got {0}")]
    BadMean(f32),
    /// The overlap leaves no stride.
    #[error("overlap {overlap} must be smaller than the tile size {tile}")]
    Overlap {
        /// Overlap given.
        overlap: u32,
        /// Tile size.
        tile: u32,
    },
}

/// The detector's tile sizes (ADR 0001: 640, or 960 where the model card says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileSize {
    /// 640 x 640.
    T640,
    /// 960 x 960.
    T960,
}

impl TileSize {
    /// Side length in logical points.
    pub fn points(self) -> u32 {
        match self {
            Self::T640 => 640,
            Self::T960 => 960,
        }
    }
}

/// Tile size and overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSpec {
    tile: TileSize,
    overlap: u32,
}

impl GridSpec {
    /// A spec; the overlap must be smaller than the tile.
    pub fn new(tile: TileSize, overlap: u32) -> Result<Self, TilingError> {
        if overlap >= tile.points() {
            return Err(TilingError::Overlap {
                overlap,
                tile: tile.points(),
            });
        }
        Ok(Self { tile, overlap })
    }
    /// The default: 640 tiles with a 64 point overlap.
    pub fn default_640() -> Self {
        Self {
            tile: TileSize::T640,
            overlap: DEFAULT_OVERLAP,
        }
    }
    /// The tile size.
    pub fn tile(&self) -> TileSize {
        self.tile
    }
    /// The overlap.
    pub fn overlap(&self) -> u32 {
        self.overlap
    }
}

/// The tile origins along one axis. A frame no larger than a tile gets one tile at 0 (the detector pads it); a larger
/// one gets `ceil((extent - overlap) / (tile - overlap))` tiles at a fixed stride, the last pulled back so it ends on
/// the frame edge.
pub fn tile_origins(extent: u32, spec: GridSpec) -> Vec<u32> {
    let tile = spec.tile.points();
    if extent <= tile {
        return vec![0];
    }
    let stride = tile - spec.overlap;
    let n = (extent - spec.overlap).div_ceil(stride);
    (0..n).map(|i| if i + 1 == n { extent - tile } else { i * stride }).collect()
}

/// One tile of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    id: usize,
    col: usize,
    row: usize,
    x: u32,
    y: u32,
    size: u32,
}

impl Tile {
    /// Index in raster order (`row * columns + col`).
    pub fn id(&self) -> usize {
        self.id
    }
    /// Column and row.
    pub fn cell(&self) -> (usize, usize) {
        (self.col, self.row)
    }
    /// Origin in logical points.
    pub fn origin(&self) -> (u32, u32) {
        (self.x, self.y)
    }
    /// Side length (the detector input size; larger than the frame when the frame is smaller than a tile).
    pub fn size(&self) -> u32 {
        self.size
    }
    /// The tile's area in logical points.
    pub fn rect(&self) -> Rect<Logical> {
        let side = f64::from(self.size);
        // Origins and sizes are bounded by MAX_FRAME_SIDE plus a tile, so the checked constructor cannot fail.
        Rect::new(f64::from(self.x), f64::from(self.y), side, side)
            .unwrap_or_else(|_| Rect::from_corners(Pt::from_origin(), Pt::from_origin()))
    }
    /// The map from tile-local coordinates (detector output) to logical points.
    pub fn to_logical(&self) -> Map<TileLocal, Logical> {
        Map::new(Scale::unit(), f64::from(self.x), f64::from(self.y)).unwrap_or_else(|_| Map::identity())
    }
}

/// The tile grid of one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileGrid {
    spec: GridSpec,
    width: u32,
    height: u32,
    cols: usize,
    tiles: Vec<Tile>,
}

/// Builds the grid for a frame of `width x height` logical points. This is the one definition both inference and
/// training use.
pub fn tile_grid(width: u32, height: u32, spec: GridSpec) -> Result<TileGrid, TilingError> {
    let ok = |v: u32| (1..=MAX_FRAME_SIDE).contains(&v);
    if !ok(width) || !ok(height) {
        return Err(TilingError::FrameSize {
            width,
            height,
            max: MAX_FRAME_SIDE,
        });
    }
    let (xs, ys) = (tile_origins(width, spec), tile_origins(height, spec));
    let size = spec.tile.points();
    let mut tiles = Vec::with_capacity(xs.len() * ys.len());
    for (row, &y) in ys.iter().enumerate() {
        for (col, &x) in xs.iter().enumerate() {
            tiles.push(Tile {
                id: row * xs.len() + col,
                col,
                row,
                x,
                y,
                size,
            });
        }
    }
    Ok(TileGrid {
        spec,
        width,
        height,
        cols: xs.len(),
        tiles,
    })
}

impl TileGrid {
    /// The spec the grid was built with.
    pub fn spec(&self) -> GridSpec {
        self.spec
    }
    /// All tiles in raster order.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }
    /// Columns.
    pub fn cols(&self) -> usize {
        self.cols
    }
    /// Rows.
    pub fn rows(&self) -> usize {
        self.tiles.len() / self.cols
    }
    /// The frame size in logical points.
    pub fn frame(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    /// The seam strips: the overlap of every pair of horizontally or vertically adjacent tiles. [`crate::detect::merge_seams`]
    /// takes them.
    pub fn seam_strips(&self) -> Vec<Rect<Logical>> {
        let mut out = Vec::new();
        for t in &self.tiles {
            for n in [
                self.tiles.get(t.id + 1).filter(|n| n.row == t.row),
                self.tiles.get(t.id + self.cols),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(s) = t.rect().intersection(&n.rect()) {
                    out.push(s);
                }
            }
        }
        out
    }
    /// The ids of the tiles whose area overlaps `scope`, in raster order.
    pub fn tiles_intersecting(&self, scope: &Rect<Logical>) -> Vec<usize> {
        self.tiles
            .iter()
            .filter(|t| t.rect().intersection(scope).is_some())
            .map(Tile::id)
            .collect()
    }
    /// The order to run tiles in: only tiles reaching `scope` (all when `None`); the tile under the pointer first, then
    /// by overlap with the `focus` rectangles (the focused window), largest first, then raster order. Deterministic.
    pub fn plan(&self, scope: Option<&Rect<Logical>>, focus: &[Rect<Logical>], pointer: Option<Pt<Logical>>) -> Vec<usize> {
        let mut ids: Vec<usize> = match scope {
            Some(s) => self.tiles_intersecting(s),
            None => self.tiles.iter().map(Tile::id).collect(),
        };
        let key = |id: usize| {
            let rect = self.tiles[id].rect();
            let under_pointer = pointer.is_some_and(|p| rect.contains_pt(p));
            let focus_area: f64 = focus.iter().filter_map(|f| f.intersection(&rect)).map(|r| r.area()).sum();
            (!under_pointer, focus_area)
        };
        ids.sort_by(|&a, &b| {
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0).then(kb.1.total_cmp(&ka.1)).then(a.cmp(&b))
        });
        ids
    }
}

/// The colour that fills the part of a tile outside the frame: the model card's normalisation mean, so it reads as
/// background to the detector. A parameter of dataset assembly and inference, never a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadColour([u8; 3]);

impl PadColour {
    /// From the model card's per-channel mean (RGB, each in 0 to 1).
    pub fn from_mean(mean: [f32; 3]) -> Result<Self, TilingError> {
        let mut rgb = [0u8; 3];
        for (o, m) in rgb.iter_mut().zip(mean) {
            if m.is_nan() || !(0.0..=1.0).contains(&m) {
                return Err(TilingError::BadMean(m));
            }
            *o = (m * 255.0).round() as u8;
        }
        Ok(Self(rgb))
    }
    /// The colour as 8-bit RGB.
    pub fn rgb(self) -> [u8; 3] {
        self.0
    }
}

/// Cuts a tile out of an RGB frame (3 bytes per pixel, row-major), filling whatever lies outside the frame with `pad`.
/// Inference and dataset assembly both call this, so training sees exactly the pulled-back edge tiles and the padded
/// small frames the detector sees at run time.
pub fn cut_tile_rgb(frame: &[u8], width: u32, height: u32, tile: &Tile, pad: PadColour) -> Result<Vec<u8>, TilingError> {
    if frame.len() as u64 != u64::from(width) * u64::from(height) * 3
        || width == 0
        || height == 0
        || width > MAX_FRAME_SIDE
        || height > MAX_FRAME_SIDE
    {
        return Err(TilingError::BadBuffer {
            width,
            height,
            len: frame.len(),
        });
    }
    let side = tile.size() as usize;
    let mut out = Vec::with_capacity(side * side * 3);
    for _ in 0..side * side {
        out.extend_from_slice(&pad.0);
    }
    let (ox, oy) = (tile.origin().0 as usize, tile.origin().1 as usize);
    let (w, h) = (width as usize, height as usize);
    let cols = side.min(w.saturating_sub(ox));
    for row in 0..side.min(h.saturating_sub(oy)) {
        let src = ((oy + row) * w + ox) * 3;
        out[row * side * 3..row * side * 3 + cols * 3].copy_from_slice(&frame[src..src + cols * 3]);
    }
    Ok(out)
}

/// Keeps only the tiles that changed since the last frame. `changed[id]` says whether tile `id` changed; a tile outside
/// the slice counts as changed, so a stale cache never hides a change.
pub fn only_changed(order: &[usize], changed: &[bool]) -> Vec<usize> {
    order
        .iter()
        .copied()
        .filter(|&id| changed.get(id).copied().unwrap_or(true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn spec(t: TileSize) -> GridSpec {
        GridSpec::new(t, DEFAULT_OVERLAP).unwrap()
    }
    fn count(w: u32, h: u32, t: TileSize) -> usize {
        tile_grid(w, h, spec(t)).unwrap().tiles().len()
    }

    #[test]
    fn tile_counts_match_the_adr_table() {
        use TileSize::{T640, T960};
        for (w, h, n640, n960) in [(1920, 1200, 8, 6), (1920, 1220, 12, 6), (2560, 1440, 15, 6), (3840, 2160, 28, 15)] {
            assert_eq!((count(w, h, T640), count(w, h, T960)), (n640, n960), "{w}x{h}");
        }
        // 5K at 200 % is a 2560 x 1440 logical frame.
        assert_eq!(count(2560, 1440, T640), 15);
        // The ADR's remark: with an overlap of at most 40 px, 1920 x 1220 needs only 8 tiles.
        let g = tile_grid(1920, 1220, GridSpec::new(TileSize::T640, 40).unwrap()).unwrap();
        assert_eq!(g.tiles().len(), 8);
    }

    #[test]
    fn origins_are_fixed_and_the_last_tile_ends_on_the_edge() {
        let g = tile_grid(1920, 1200, GridSpec::default_640()).unwrap();
        assert_eq!((g.cols(), g.rows(), g.frame()), (4, 2, (1920, 1200)));
        let xs: Vec<u32> = g.tiles().iter().take(4).map(|t| t.origin().0).collect();
        assert_eq!(xs, vec![0, 576, 1152, 1280]);
        assert_eq!(g.tiles()[4].origin(), (0, 560));
        assert_eq!(g.tiles()[7].cell(), (3, 1));
        assert_eq!(tile_origins(500, GridSpec::default_640()), vec![0]);
        assert_eq!(tile_origins(640, GridSpec::default_640()), vec![0]);
        assert_eq!(tile_origins(641, GridSpec::default_640()), vec![0, 1]);
        assert_eq!(g.spec().tile().points(), 640);
        assert_eq!(g.spec().overlap(), 64);
    }

    #[test]
    fn refuses_bad_sizes_and_overlaps() {
        assert!(matches!(
            tile_grid(0, 100, GridSpec::default_640()),
            Err(TilingError::FrameSize { .. })
        ));
        assert!(matches!(
            tile_grid(100, MAX_FRAME_SIDE + 1, GridSpec::default_640()),
            Err(TilingError::FrameSize { .. })
        ));
        assert_eq!(
            GridSpec::new(TileSize::T640, 640),
            Err(TilingError::Overlap { overlap: 640, tile: 640 })
        );
        assert!(GridSpec::new(TileSize::T960, 639).is_ok());
    }

    #[test]
    fn seam_strips_are_the_overlaps() {
        let g = tile_grid(1920, 1200, GridSpec::default_640()).unwrap();
        let strips = g.seam_strips();
        // 3 vertical seams x 2 rows + 4 horizontal seams x 1.
        assert_eq!(strips.len(), 3 * 2 + 4);
        assert!(
            strips
                .iter()
                .any(|s| s.x() == 576.0 && s.w() == 64.0 && s.y() == 0.0 && s.h() == 640.0)
        );
        assert!(tile_grid(300, 300, GridSpec::default_640()).unwrap().seam_strips().is_empty());
    }

    #[test]
    fn tile_local_boxes_map_to_logical() {
        let g = tile_grid(1920, 1200, GridSpec::default_640()).unwrap();
        let t = g.tiles()[5]; // col 1, row 1: origin (576, 560)
        let r = t.to_logical().rect(Rect::new(10.0, 20.0, 30.0, 40.0).unwrap());
        assert_eq!((r.x(), r.y(), r.w(), r.h()), (586.0, 580.0, 30.0, 40.0));
        assert_eq!(t.rect().x(), 576.0);
        assert_eq!(t.size(), 640);
    }

    #[test]
    fn plan_orders_pointer_then_focus_then_raster() {
        let g = tile_grid(1920, 1200, GridSpec::default_640()).unwrap();
        assert_eq!(g.plan(None, &[], None), (0..8).collect::<Vec<_>>());
        let pointer = Pt::new(1850.0, 1100.0).unwrap(); // only in tile 7 (tile 6 ends at x = 1792)
        assert_eq!(g.plan(None, &[], Some(pointer))[0], 7);
        let window = Rect::new(0.0, 0.0, 1200.0, 800.0).unwrap(); // 1200 x 800 window at the origin
        let scoped = g.plan(Some(&window), &[window], None);
        assert_eq!(scoped.len(), 6.min(g.tiles_intersecting(&window).len()));
        assert_eq!(scoped[0], 0); // the largest overlap with the focus window
        assert!(!scoped.contains(&3));
        assert_eq!(
            g.plan(Some(&Rect::new(5000.0, 5000.0, 10.0, 10.0).unwrap()), &[], None),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn padding_colour_is_the_model_cards_mean() {
        let pad = PadColour::from_mean([0.485, 0.456, 0.406]).unwrap();
        assert_eq!(pad.rgb(), [124, 116, 104]);
        for bad in [[1.1, 0.0, 0.0], [0.0, -0.1, 0.0], [0.0, 0.0, f32::NAN]] {
            assert!(matches!(PadColour::from_mean(bad), Err(TilingError::BadMean(_))));
        }
    }

    #[test]
    fn a_small_frame_is_one_padded_tile_and_the_last_tile_is_pulled_back() {
        let pad = PadColour::from_mean([1.0, 0.0, 0.0]).unwrap();
        // A 2 x 2 frame of grey (7) pixels becomes one 640 x 640 tile: the frame in the corner, the rest red.
        let frame = vec![7u8; 2 * 2 * 3];
        let g = tile_grid(2, 2, GridSpec::default_640()).unwrap();
        assert_eq!(g.tiles().len(), 1);
        let t = cut_tile_rgb(&frame, 2, 2, &g.tiles()[0], pad).unwrap();
        assert_eq!(t.len(), 640 * 640 * 3);
        assert_eq!(&t[0..3], &[7, 7, 7]);
        assert_eq!(&t[2 * 3..2 * 3 + 3], &[255, 0, 0]); // right of the frame
        assert_eq!(&t[640 * 3..640 * 3 + 3], &[7, 7, 7]); // second frame row
        assert_eq!(&t[2 * 640 * 3..2 * 640 * 3 + 3], &[255, 0, 0]); // below the frame
        // A pulled-back edge tile holds only real pixels: 700 x 10 frame, second tile starts at x = 60, 10 rows then pad.
        let wide: Vec<u8> = (0..700 * 10).flat_map(|i| [(i % 251) as u8, 0, 0]).collect();
        let g = tile_grid(700, 10, GridSpec::default_640()).unwrap();
        assert_eq!(g.tiles()[1].origin(), (60, 0));
        let t = cut_tile_rgb(&wide, 700, 10, &g.tiles()[1], pad).unwrap();
        assert_eq!(t[0], wide[60 * 3]);
        assert_eq!(t[639 * 3], wide[699 * 3]); // last column is the frame's last column
        assert_eq!(&t[10 * 640 * 3..10 * 640 * 3 + 3], &[255, 0, 0]);
        assert!(matches!(
            cut_tile_rgb(&frame, 3, 2, &g.tiles()[0], pad),
            Err(TilingError::BadBuffer { .. })
        ));
        assert!(matches!(
            cut_tile_rgb(&[], 0, 0, &g.tiles()[0], pad),
            Err(TilingError::BadBuffer { .. })
        ));
    }

    #[test]
    fn unchanged_tiles_are_skipped_and_unknown_ones_run() {
        assert_eq!(only_changed(&[3, 1, 9], &[false, true, false, false]), vec![1, 9]);
    }

    proptest! {
        /// Every point of the frame lies in some tile; tiles stay inside the frame (or the frame is smaller than one tile).
        #[test]
        fn tiles_cover_the_frame(w in 1u32..6000, h in 1u32..4000, big in any::<bool>(), ov in 0u32..=200) {
            let t = if big { TileSize::T960 } else { TileSize::T640 };
            let g = tile_grid(w, h, GridSpec::new(t, ov).unwrap()).unwrap();
            for ti in g.tiles() {
                let (x, y) = ti.origin();
                prop_assert!(x + ti.size() <= w.max(ti.size()) && y + ti.size() <= h.max(ti.size()));
            }
            for &(px, py) in &[(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1), (w / 2, h / 2), (w / 3, h * 2 / 3)] {
                let p = Pt::new(f64::from(px), f64::from(py)).unwrap();
                prop_assert!(g.tiles().iter().any(|ti| ti.rect().contains_pt(p)));
            }
        }

        /// An element no larger than the overlap lies whole inside some tile (ADR 0001's guarantee).
        #[test]
        fn small_elements_fit_whole_in_a_tile(w in 700u32..6000, h in 700u32..4000, fx in 0.0f64..1.0, fy in 0.0f64..1.0, ew in 1u32..=64, eh in 1u32..=64) {
            let g = tile_grid(w, h, GridSpec::default_640()).unwrap();
            let x = (f64::from(w - ew) * fx).floor();
            let y = (f64::from(h - eh) * fy).floor();
            let e = Rect::<Logical>::new(x, y, f64::from(ew), f64::from(eh)).unwrap();
            prop_assert!(g.tiles().iter().any(|t| t.rect().contains(&e)));
        }

        /// Tile count equals the ADR's formula and the plan is a permutation of the tiles it selects.
        #[test]
        fn count_formula_and_plan_permutation(w in 641u32..6000, h in 641u32..4000, px in 0.0f64..6000.0, py in 0.0f64..4000.0) {
            let g = tile_grid(w, h, GridSpec::default_640()).unwrap();
            let f = |e: u32| (e - 64).div_ceil(576) as usize;
            prop_assert_eq!(g.tiles().len(), f(w) * f(h));
            let mut plan = g.plan(None, &[], Some(Pt::new(px, py).unwrap()));
            plan.sort_unstable();
            prop_assert_eq!(plan, (0..g.tiles().len()).collect::<Vec<_>>());
        }
    }
}
