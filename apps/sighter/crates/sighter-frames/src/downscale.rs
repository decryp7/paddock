//! Area-average downscale of a frame's luma to a fixed grid. Every grid cell reads at least one source pixel, so tiny
//! frames (even 1x1) give a full grid and no cell is left empty.

use sighter_core::Frame;

fn luma(p: u32) -> u32 {
    (299 * ((p >> 16) & 0xff) + 587 * ((p >> 8) & 0xff) + 114 * (p & 0xff)) / 1000
}

/// The mean luma (0..=255) of each of `gw` x `gh` cells, row-major. `gw` and `gh` must be non-zero (callers use constants).
pub fn downscale_luma(frame: &Frame, gw: usize, gh: usize) -> Vec<u8> {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let span = |i: usize, cells: usize, len: usize| {
        let start = (i * len / cells).min(len - 1);
        let end = ((i + 1) * len / cells).clamp(start + 1, len);
        (start, end)
    };
    let mut out = Vec::with_capacity(gw * gh);
    for gy in 0..gh {
        let (y0, y1) = span(gy, gh, h);
        for gx in 0..gw {
            let (x0, x1) = span(gx, gw, w);
            let mut sum = 0u64;
            for y in y0..y1 {
                let row = &frame.pixels()[y * w + x0..y * w + x1];
                sum += row.iter().map(|p| u64::from(luma(*p))).sum::<u64>();
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            out.push((sum / n) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_frames_give_their_luma_everywhere() {
        let f = Frame::solid(100, 60, 0xffffff).unwrap();
        assert!(downscale_luma(&f, 8, 4).iter().all(|v| *v == 255));
        let g = Frame::solid(7, 3, 0).unwrap();
        assert!(downscale_luma(&g, 64, 40).iter().all(|v| *v == 0));
    }

    #[test]
    fn a_single_pixel_fills_the_grid() {
        let f = Frame::solid(1, 1, 0x808080).unwrap();
        let g = downscale_luma(&f, 64, 40);
        assert_eq!(g.len(), 64 * 40);
        assert!(g.iter().all(|v| *v == 128));
    }

    #[test]
    fn halves_are_kept_apart() {
        let mut f = Frame::solid(10, 2, 0).unwrap();
        f.fill(sighter_core::Rect::new(5, 0, 5, 2), 0xffffff);
        assert_eq!(downscale_luma(&f, 2, 1), vec![0, 255]);
    }
}
