//! Scene change, SSIM and near-duplicate removal on top of the hashes.

use crate::downscale::downscale_luma;
use crate::hash::GridHash;
use sighter_core::Frame;

/// Two frames show different screens when at least this many grid cells differ (about 2.5 % of the grid). A caret or a
/// toggle stays below it; a dialog or a page change goes far above.
pub const SCENE_CHANGE_CELLS: usize = 64;

const SSIM_GRID: usize = 64;
const SSIM_BLOCK: usize = 8;

/// True when `b` shows a different screen than `a`.
pub fn is_scene_change(a: &GridHash, b: &GridHash) -> bool {
    a.distance(b) >= SCENE_CHANGE_CELLS
}

/// Structural similarity (mean SSIM over 8x8 blocks) of both frames' luma downscaled to 64x64, in -1..=1; 1 for identical
/// frames. Frames of different sizes compare through the common grid.
pub fn ssim(a: &Frame, b: &Frame) -> f32 {
    const C1: f32 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f32 = (0.03 * 255.0) * (0.03 * 255.0);
    let (ga, gb) = (downscale_luma(a, SSIM_GRID, SSIM_GRID), downscale_luma(b, SSIM_GRID, SSIM_GRID));
    let n = (SSIM_BLOCK * SSIM_BLOCK) as f32;
    let mut total = 0.0;
    let mut blocks = 0.0;
    for by in (0..SSIM_GRID).step_by(SSIM_BLOCK) {
        for bx in (0..SSIM_GRID).step_by(SSIM_BLOCK) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for y in by..by + SSIM_BLOCK {
                for x in bx..bx + SSIM_BLOCK {
                    let i = y * SSIM_GRID + x;
                    let (p, q) = (f32::from(ga[i]), f32::from(gb[i]));
                    sa += p;
                    sb += q;
                    saa += p * p;
                    sbb += q * q;
                    sab += p * q;
                }
            }
            let (ma, mb) = (sa / n, sb / n);
            let (va, vb, cov) = (saa / n - ma * ma, sbb / n - mb * mb, sab / n - ma * mb);
            total += ((2.0 * ma * mb + C1) * (2.0 * cov + C2)) / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            blocks += 1.0;
        }
    }
    total / blocks
}

/// Indices of the frames to keep: a frame is dropped when it shows the same screen as one already kept (so a screen that
/// comes back later is dropped too). Order is preserved; the result is deterministic.
pub fn dedupe(frames: &[Frame]) -> Vec<usize> {
    let mut kept: Vec<GridHash> = Vec::new();
    let mut indices = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let h = GridHash::of(f);
        if kept.iter().all(|k| is_scene_change(k, &h)) {
            kept.push(h);
            indices.push(i);
        }
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use sighter_core::Rect;

    fn screen(kind: u8) -> Frame {
        let mut f = Frame::solid(640, 400, 0xf0f0f0).unwrap();
        match kind {
            0 => {}
            1 => f.fill(Rect::new(100, 80, 400, 240), 0x202020),
            _ => {
                f.fill(Rect::new(0, 0, 640, 100), 0x0067c0);
                f.fill(Rect::new(0, 200, 640, 100), 0x20a020);
            }
        }
        f
    }

    #[test]
    fn ten_frames_with_three_distinct_screens_give_three() {
        let kinds = [0u8, 0, 1, 1, 1, 0, 2, 2, 1, 0];
        let mut frames: Vec<Frame> = kinds.iter().map(|k| screen(*k)).collect();
        // A blinking caret on some of the repeats must not make them distinct screens.
        frames[1].fill(Rect::new(50, 50, 2, 16), 0);
        frames[3].fill(Rect::new(300, 150, 2, 16), 0xffffff);
        assert_eq!(dedupe(&frames), vec![0, 2, 6]);
        assert!(dedupe(&[]).is_empty());
    }

    #[test]
    fn scene_change_ignores_a_caret_and_sees_a_dialog() {
        let a = GridHash::of(&screen(0));
        let mut caret = screen(0);
        caret.fill(Rect::new(10, 10, 2, 16), 0);
        assert!(!is_scene_change(&a, &GridHash::of(&caret)));
        assert!(is_scene_change(&a, &GridHash::of(&screen(1))));
    }

    #[test]
    fn ssim_is_one_for_identical_and_lower_for_different_frames() {
        let a = screen(1);
        assert!((ssim(&a, &a) - 1.0).abs() < 1e-6);
        let flat = ssim(&screen(0), &screen(0));
        assert!((flat - 1.0).abs() < 1e-6);
        assert!(ssim(&screen(0), &screen(1)) < 0.9);
        assert!(ssim(&screen(1), &screen(2)) < 0.9);
        let mut smaller = Frame::solid(320, 200, 0xf0f0f0).unwrap();
        smaller.fill(Rect::new(50, 40, 200, 120), 0x202020);
        assert!(ssim(&a, &smaller) > 0.95, "the same screen at another size is similar");
        assert!(ssim(&screen(0), &Frame::solid(1, 1, 0).unwrap()).is_finite());
    }
}
