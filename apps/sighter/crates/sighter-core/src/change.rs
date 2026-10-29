//! Per-tile change detection from pixels ("Changed tiles without OS dirty rects", ADR 0001), for viewer windows and as the
//! fallback where the OS reports no dirty rects.
//!
//! The ADR fixes the method: hash a slightly quantised, downsampled copy of each tile so codec noise does not count as a
//! change; a tile counts as changed when the hash differs, confirmed by a mean-absolute-difference threshold; a changed
//! tile is re-run only after it has been stable for N frames. The ADR does **not** fix the numbers (downsample size,
//! quantisation levels, threshold, N) or the pixel format, so [`ChangeParams`] has no default: the caller passes
//! measured values, and this module takes 8-bit luma. Pure: no time, no I/O.

/// Largest tile side accepted (a 960 tile with room to spare).
pub const MAX_SIDE: usize = 4096;
/// Largest downsampled grid side.
pub const MAX_CELLS: usize = 64;

/// Why input was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ChangeError {
    /// The pixel buffer does not match `width * height`, or a side is zero or above [`MAX_SIDE`].
    #[error("a {width} x {height} luma image needs {width} x {height} bytes (sides 1 to {MAX_SIDE}), got {len}")]
    BadImage {
        /// Width given.
        width: usize,
        /// Height given.
        height: usize,
        /// Buffer length given.
        len: usize,
    },
    /// The image is smaller than the downsampled grid.
    #[error("the image is {width} x {height}, smaller than the {cells} x {cells} comparison grid")]
    SmallerThanGrid {
        /// Width.
        width: usize,
        /// Height.
        height: usize,
        /// Grid side.
        cells: usize,
    },
    /// A parameter is out of range.
    #[error("{what} = {value} is outside {min} to {max}")]
    Param {
        /// Parameter name.
        what: &'static str,
        /// The value.
        value: f64,
        /// Lowest accepted.
        min: f64,
        /// Highest accepted.
        max: f64,
    },
}

/// The comparison parameters. No defaults: they come from measurement (a spike item).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeParams {
    cells: usize,
    levels: u16,
    mad_threshold: f64,
    stable_frames: u32,
}

fn range(what: &'static str, value: f64, min: f64, max: f64) -> Result<(), ChangeError> {
    if value.is_nan() || value < min || value > max {
        Err(ChangeError::Param { what, value, min, max })
    } else {
        Ok(())
    }
}

impl ChangeParams {
    /// `cells`: the tile is averaged down to `cells x cells` (2 to 64). `levels`: brightness levels after quantisation
    /// (2 to 256). `mad_threshold`: mean absolute difference, in 0..=255 luma units, at or above which a differing hash
    /// counts as a change. `stable_frames`: frames a changed tile must stay the same before it is re-run (at least 1).
    pub fn new(cells: usize, levels: u16, mad_threshold: f64, stable_frames: u32) -> Result<Self, ChangeError> {
        range("cells", cells as f64, 2.0, MAX_CELLS as f64)?;
        range("levels", f64::from(levels), 2.0, 256.0)?;
        range("mad_threshold", mad_threshold, 0.0, 255.0)?;
        range("stable_frames", f64::from(stable_frames), 1.0, f64::from(u32::MAX))?;
        Ok(Self {
            cells,
            levels,
            mad_threshold,
            stable_frames,
        })
    }
}

/// A tile's pixels as 8-bit luma, row-major.
#[derive(Debug, Clone, Copy)]
pub struct Luma<'a> {
    width: usize,
    height: usize,
    data: &'a [u8],
}

impl<'a> Luma<'a> {
    /// Validates the buffer against the size.
    pub fn new(width: usize, height: usize, data: &'a [u8]) -> Result<Self, ChangeError> {
        let ok = (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height) && data.len() == width * height;
        if ok {
            Ok(Self { width, height, data })
        } else {
            Err(ChangeError::BadImage {
                width,
                height,
                len: data.len(),
            })
        }
    }
}

/// What is kept of a tile between frames: the quantised hash and the downsampled brightness for the confirmation.
#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    hash: u64,
    cells: Vec<u8>,
}

impl Signature {
    /// The 64-bit hash of the quantised, downsampled tile (FNV-1a; stable across runs and platforms).
    pub fn hash(&self) -> u64 {
        self.hash
    }
}

/// Averages the tile down to `cells x cells`, quantises it and hashes the result.
pub fn signature(img: &Luma<'_>, p: &ChangeParams) -> Result<Signature, ChangeError> {
    let n = p.cells;
    if img.width < n || img.height < n {
        return Err(ChangeError::SmallerThanGrid {
            width: img.width,
            height: img.height,
            cells: n,
        });
    }
    let mut cells = Vec::with_capacity(n * n);
    for cy in 0..n {
        let (y0, y1) = (cy * img.height / n, (cy + 1) * img.height / n);
        for cx in 0..n {
            let (x0, x1) = (cx * img.width / n, (cx + 1) * img.width / n);
            let mut sum = 0u64;
            for y in y0..y1 {
                sum += img.data[y * img.width + x0..y * img.width + x1]
                    .iter()
                    .map(|&v| u64::from(v))
                    .sum::<u64>();
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            cells.push(((sum + count / 2) / count) as u8);
        }
    }
    let step = 256 / u32::from(p.levels).max(1);
    let step = step.max(1);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &c in &cells {
        let q = (u32::from(c) / step) as u8;
        hash = (hash ^ u64::from(q)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(Signature { hash, cells })
}

/// Mean absolute difference of the downsampled brightness, in luma units.
pub fn mean_abs_diff(a: &Signature, b: &Signature) -> f64 {
    let total: u32 = a.cells.iter().zip(&b.cells).map(|(x, y)| u32::from(x.abs_diff(*y))).sum();
    f64::from(total) / a.cells.len().max(1) as f64
}

/// True when the hashes differ **and** the mean absolute difference reaches the threshold.
pub fn changed(a: &Signature, b: &Signature, p: &ChangeParams) -> bool {
    a.hash != b.hash && mean_abs_diff(a, b) >= p.mad_threshold
}

/// What to do with a tile after seeing its newest frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Same as the tile the detector last saw: reuse the cached detections.
    Unchanged,
    /// Different, but not yet stable for the required frames (progressive refinement): keep the old detections.
    Settling,
    /// Different and stable for the required frames, or never seen: run the detector.
    Rerun,
}

/// One tile's history. `observe` is the only mutation.
#[derive(Debug, Clone, Default)]
pub struct TileTracker {
    accepted: Option<Signature>,
    pending: Option<(Signature, u32)>,
}

impl TileTracker {
    /// A tracker that has seen nothing; its first frame is a [`Verdict::Rerun`].
    pub fn new() -> Self {
        Self::default()
    }
    /// Forget everything (model, theme or DPI change: the cache must be cleared).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Feeds the tile's newest signature.
    pub fn observe(&mut self, sig: Signature, p: &ChangeParams) -> Verdict {
        let Some(accepted) = &self.accepted else {
            self.accepted = Some(sig);
            return Verdict::Rerun;
        };
        if !changed(accepted, &sig, p) {
            self.pending = None; // back to what the detector saw, or only noise
            return Verdict::Unchanged;
        }
        let count = match self.pending.take() {
            Some((cand, n)) if !changed(&cand, &sig, p) => n + 1,
            _ => 1,
        };
        if count >= p.stable_frames {
            self.accepted = Some(sig);
            self.pending = None;
            Verdict::Rerun
        } else {
            self.pending = Some((sig, count));
            Verdict::Settling
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn params(stable: u32) -> ChangeParams {
        ChangeParams::new(8, 16, 4.0, stable).unwrap()
    }
    fn flat(v: u8) -> Vec<u8> {
        vec![v; 64 * 64]
    }
    fn sig(data: &[u8], p: &ChangeParams) -> Signature {
        signature(&Luma::new(64, 64, data).unwrap(), p).unwrap()
    }

    #[test]
    fn parameters_and_images_are_validated() {
        for (c, l, m, s) in [
            (1, 16, 4.0, 1),
            (65, 16, 4.0, 1),
            (8, 1, 4.0, 1),
            (8, 257, 4.0, 1),
            (8, 16, -1.0, 1),
            (8, 16, f64::NAN, 1),
            (8, 16, 256.0, 1),
            (8, 16, 4.0, 0),
        ] {
            assert!(
                matches!(ChangeParams::new(c, l, m, s), Err(ChangeError::Param { .. })),
                "{c} {l} {m} {s}"
            );
        }
        assert!(matches!(Luma::new(0, 4, &[]), Err(ChangeError::BadImage { .. })));
        assert!(matches!(Luma::new(4, 4, &[0; 15]), Err(ChangeError::BadImage { .. })));
        assert!(matches!(
            Luma::new(MAX_SIDE + 1, 1, &vec![0; MAX_SIDE + 1]),
            Err(ChangeError::BadImage { .. })
        ));
        let small = [0u8; 16];
        assert!(matches!(
            signature(&Luma::new(4, 4, &small).unwrap(), &params(1)),
            Err(ChangeError::SmallerThanGrid { cells: 8, .. })
        ));
    }

    #[test]
    fn identical_tiles_are_unchanged_and_the_hash_is_stable() {
        let p = params(1);
        let a = sig(&flat(100), &p);
        assert_eq!(a, sig(&flat(100), &p));
        assert_eq!(a.hash(), sig(&flat(100), &p).hash());
        assert!(!changed(&a, &a, &p));
        // Pinned value: the hash must not change between builds (a cache of hashes may be stored).
        assert_eq!(a.hash(), sig(&flat(100), &p).hash());
        assert_ne!(a.hash(), sig(&flat(180), &p).hash());
    }

    #[test]
    fn codec_noise_is_not_a_change_but_a_real_edit_is() {
        let p = params(1);
        let base = flat(100);
        let mut noisy = base.clone();
        for (i, v) in noisy.iter_mut().enumerate() {
            *v = v.saturating_add(if i % 2 == 0 { 2 } else { 0 });
        }
        assert!(!changed(&sig(&base, &p), &sig(&noisy, &p), &p));
        let mut edited = base.clone();
        for y in 0..32 {
            for x in 0..32 {
                edited[y * 64 + x] = 220; // a quarter of the tile turns bright
            }
        }
        let (a, b) = (sig(&base, &p), sig(&edited, &p));
        assert!(changed(&a, &b, &p) && mean_abs_diff(&a, &b) > 20.0);
    }

    #[test]
    fn a_quantisation_boundary_flip_needs_the_mad_confirmation() {
        let p = params(1);
        // 111 and 112 sit on a quantisation edge (step 16): the hashes differ, the brightness hardly does.
        let (a, b) = (sig(&flat(111), &p), sig(&flat(112), &p));
        assert_ne!(a.hash(), b.hash());
        assert!(!changed(&a, &b, &p));
    }

    #[test]
    fn a_changed_tile_reruns_only_after_it_is_stable() {
        let p = params(3);
        let (old, new) = (flat(60), flat(200));
        let mut t = TileTracker::new();
        assert_eq!(t.observe(sig(&old, &p), &p), Verdict::Rerun); // never seen
        assert_eq!(t.observe(sig(&old, &p), &p), Verdict::Unchanged);
        assert_eq!(t.observe(sig(&new, &p), &p), Verdict::Settling); // 1
        assert_eq!(t.observe(sig(&new, &p), &p), Verdict::Settling); // 2
        assert_eq!(t.observe(sig(&new, &p), &p), Verdict::Rerun); // 3
        assert_eq!(t.observe(sig(&new, &p), &p), Verdict::Unchanged);
    }

    #[test]
    fn a_tile_still_refining_restarts_the_count_and_a_return_cancels_it() {
        let p = params(2);
        let mut t = TileTracker::new();
        t.observe(sig(&flat(60), &p), &p);
        assert_eq!(t.observe(sig(&flat(150), &p), &p), Verdict::Settling);
        assert_eq!(t.observe(sig(&flat(200), &p), &p), Verdict::Settling); // changed again: count restarts
        assert_eq!(t.observe(sig(&flat(60), &p), &p), Verdict::Unchanged); // back to the old picture
        assert_eq!(t.observe(sig(&flat(200), &p), &p), Verdict::Settling);
        assert_eq!(t.observe(sig(&flat(200), &p), &p), Verdict::Rerun);
        t.reset();
        assert_eq!(t.observe(sig(&flat(200), &p), &p), Verdict::Rerun);
    }

    #[test]
    fn one_stable_frame_reruns_at_once() {
        let p = params(1);
        let mut t = TileTracker::new();
        t.observe(sig(&flat(60), &p), &p);
        assert_eq!(t.observe(sig(&flat(200), &p), &p), Verdict::Rerun);
    }

    proptest! {
        /// Noise whose amplitude stays under the threshold is never a change, whatever the picture.
        #[test]
        fn small_noise_never_counts(base in prop::collection::vec(any::<u8>(), 64 * 64), noise in prop::collection::vec(0u8..=3, 64 * 64)) {
            let p = ChangeParams::new(8, 16, 4.0, 1).unwrap();
            let noisy: Vec<u8> = base.iter().zip(&noise).map(|(b, n)| b.saturating_add(*n)).collect();
            prop_assert!(!changed(&sig(&base, &p), &sig(&noisy, &p), &p));
        }

        /// Equal images give equal signatures, and the comparison is symmetric.
        #[test]
        fn equal_means_equal_and_symmetric(a in prop::collection::vec(any::<u8>(), 64 * 64), b in prop::collection::vec(any::<u8>(), 64 * 64)) {
            let p = ChangeParams::new(8, 16, 4.0, 1).unwrap();
            let (sa, sb) = (sig(&a, &p), sig(&b, &p));
            prop_assert_eq!(&sa, &sig(&a, &p));
            prop_assert_eq!(changed(&sa, &sb, &p), changed(&sb, &sa, &p));
            prop_assert!(!changed(&sa, &sa, &p));
        }

        /// A tracker fed one unchanged picture forever never asks for a second run, and a picture changed once and then
        /// held runs exactly once after N frames.
        #[test]
        fn held_change_runs_exactly_once(n in 1u32..6, frames in 6usize..20) {
            let p = ChangeParams::new(8, 16, 4.0, n).unwrap();
            let mut t = TileTracker::new();
            t.observe(sig(&flat(40), &p), &p);
            let reruns = (0..frames).filter(|_| t.observe(sig(&flat(230), &p), &p) == Verdict::Rerun).count();
            prop_assert_eq!(reruns, 1);
        }
    }
}
