//! Perceptual hashes. [`GridHash`] is the recorder's screen-change signature: a coarse grid of mean luma, so a blinking
//! caret (one cell) can be told from a new dialog (hundreds of cells), unlike a 64-bit hash that misses small but
//! meaningful changes such as a toggle. [`DHash`] is the classic 64-bit difference hash for near-duplicate lookup.

use crate::downscale::downscale_luma;
use sighter_core::Frame;

/// Grid columns.
pub const GRID_W: usize = 64;
/// Grid rows.
pub const GRID_H: usize = 40;
/// Luma difference (0..=255) at or below which a cell counts as unchanged (compression noise, anti-aliasing).
pub const CELL_TOLERANCE: u8 = 6;

/// A `GRID_W` x `GRID_H` grid of mean luma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridHash(Vec<u8>);

impl GridHash {
    /// The hash of a frame; total, never panics.
    pub fn of(frame: &Frame) -> GridHash {
        GridHash(downscale_luma(frame, GRID_W, GRID_H))
    }

    /// The cells, row-major.
    pub fn cells(&self) -> &[u8] {
        &self.0
    }

    /// Number of cells that changed by more than [`CELL_TOLERANCE`]. Symmetric; 0 against itself.
    pub fn distance(&self, other: &GridHash) -> usize {
        self.0
            .iter()
            .zip(&other.0)
            .filter(|(a, b)| a.abs_diff(**b) > CELL_TOLERANCE)
            .count()
    }

    /// A short hex fingerprint for logs (FNV-1a over the quantised grid).
    pub fn fingerprint(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in &self.0 {
            h ^= u64::from(*v >> 3);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}

/// A 64-bit difference hash: 9x8 luma, one bit per left-right comparison. Invariant to brightness shifts and scaling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DHash(pub u64);

impl DHash {
    /// The hash of a frame; total, never panics.
    pub fn of(frame: &Frame) -> DHash {
        let g = downscale_luma(frame, 9, 8);
        let mut bits = 0u64;
        for y in 0..8 {
            for x in 0..8 {
                bits <<= 1;
                if g.get(y * 9 + x) > g.get(y * 9 + x + 1) {
                    bits |= 1;
                }
            }
        }
        DHash(bits)
    }

    /// Hamming distance, 0..=64. Symmetric.
    pub fn distance(self, other: DHash) -> u32 {
        (self.0 ^ other.0).count_ones()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use sighter_core::Rect;

    fn grey(w: u32, h: u32) -> Frame {
        Frame::solid(w, h, 0xf0f0f0).unwrap()
    }

    #[test]
    fn small_change_is_few_cells_and_a_dialog_is_many() {
        let a = grey(1280, 800);
        let base = GridHash::of(&a);
        assert_eq!(base.distance(&base), 0);
        let mut caret = a.clone();
        caret.fill(Rect::new(100, 100, 2, 16), 0);
        assert!(base.distance(&GridHash::of(&caret)) <= 1);
        let mut toggle = a.clone();
        toggle.fill(Rect::new(600, 300, 44, 22), 0x0067c0);
        assert!((2..=12).contains(&base.distance(&GridHash::of(&toggle))));
        let mut dialog = a.clone();
        dialog.fill(Rect::new(400, 250, 480, 300), 0x202020);
        let d = GridHash::of(&dialog);
        assert!(base.distance(&d) > 300);
        assert_ne!(base.fingerprint(), d.fingerprint());
        assert_eq!(base.fingerprint().len(), 16);
        assert_eq!(base.cells().len(), GRID_W * GRID_H);
    }

    #[test]
    fn tiny_frames_do_not_panic() {
        for (w, h) in [(1, 1), (3, 2), (1, 100), (100, 1)] {
            let f = grey(w, h);
            assert_eq!(GridHash::of(&f).cells().len(), GRID_W * GRID_H);
            let _ = DHash::of(&f);
        }
    }

    #[test]
    fn dhash_ignores_brightness_and_size_but_sees_structure() {
        let mut a = Frame::solid(200, 100, 0x404040).unwrap();
        a.fill(Rect::new(100, 0, 100, 100), 0xc0c0c0);
        let mut brighter = Frame::solid(200, 100, 0x505050).unwrap();
        brighter.fill(Rect::new(100, 0, 100, 100), 0xd0d0d0);
        let mut small = Frame::solid(100, 50, 0x404040).unwrap();
        small.fill(Rect::new(50, 0, 50, 50), 0xc0c0c0);
        assert_eq!(DHash::of(&a).distance(DHash::of(&brighter)), 0);
        assert_eq!(DHash::of(&a).distance(DHash::of(&small)), 0);
        let mut flipped = Frame::solid(200, 100, 0xc0c0c0).unwrap();
        flipped.fill(Rect::new(100, 0, 100, 100), 0x404040);
        assert!(DHash::of(&a).distance(DHash::of(&flipped)) > 0);
    }

    proptest! {
        /// The fuzz test for the hash: any valid frame hashes, deterministically, with a fixed-size result; the distance
        /// is symmetric and zero against itself.
        #[test]
        fn hashes_are_total_symmetric_and_stable(
            w in 1u32..40, h in 1u32..40, seed_a in any::<u32>(), seed_b in any::<u32>(),
        ) {
            let mk = |seed: u32| {
                let mut s = seed;
                let px = (0..w * h).map(|_| { s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345); s >> 8 }).collect();
                Frame::new(w, h, px).unwrap()
            };
            let (a, b) = (mk(seed_a), mk(seed_b));
            let (ga, gb) = (GridHash::of(&a), GridHash::of(&b));
            prop_assert_eq!(ga.cells().len(), GRID_W * GRID_H);
            prop_assert_eq!(&ga, &GridHash::of(&a));
            prop_assert_eq!(ga.distance(&gb), gb.distance(&ga));
            prop_assert_eq!(ga.distance(&ga), 0);
            let (da, db) = (DHash::of(&a), DHash::of(&b));
            prop_assert_eq!(da.distance(db), db.distance(da));
            prop_assert_eq!(da.distance(da), 0);
            prop_assert!(da.distance(db) <= 64);
        }
    }
}
