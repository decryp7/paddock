//! The mutable framebuffer the decoders paint into. The core's `Frame` is immutable and validated, so the client keeps
//! its own canvas (same limits, checked before any allocation) and publishes a `Frame` snapshot after each update.
//! Every accessor clips, so a decoder bug can never index out of range.

use crate::error::RfbError;
use sighter_core::frame::{Frame, MAX_DIM, MAX_PIXELS};

/// A rectangle in framebuffer pixels (unsigned, as on the wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    /// Left.
    pub x: u32,
    /// Top.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl Area {
    /// The area at `(x, y)` of `w` by `h` pixels.
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Area {
        Area { x, y, w, h }
    }

    /// One past the last column.
    pub fn right(&self) -> u64 {
        u64::from(self.x) + u64::from(self.w)
    }

    /// One past the last row.
    pub fn bottom(&self) -> u64 {
        u64::from(self.y) + u64::from(self.h)
    }
}

/// A `0x00RRGGBB` pixel buffer of validated size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Canvas {
    /// A black canvas; refuses empty sizes and sizes over the core's frame limits before allocating.
    pub fn new(width: u32, height: u32) -> Result<Canvas, RfbError> {
        let n = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM || n > MAX_PIXELS {
            return Err(RfbError::Protocol(format!(
                "the server desktop of {width}x{height} pixels is unusable (limit {MAX_DIM} per side, {MAX_PIXELS} pixels)"
            )));
        }
        Ok(Canvas {
            width,
            height,
            pixels: vec![0; n as usize],
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, row-major.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// The pixel at `(x, y)`, or `None` outside.
    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.width && y < self.height {
            self.pixels.get(y as usize * self.width as usize + x as usize).copied()
        } else {
            None
        }
    }

    /// The whole canvas as an area.
    pub fn bounds(&self) -> Area {
        Area::new(0, 0, self.width, self.height)
    }

    /// Fills the part of `area` that lies inside the canvas.
    pub fn fill(&mut self, area: Area, colour: u32) {
        let (right, bottom) = (
            area.right().min(u64::from(self.width)) as u32,
            area.bottom().min(u64::from(self.height)) as u32,
        );
        for y in area.y..bottom {
            let row = y as usize * self.width as usize;
            if let Some(span) = self.pixels.get_mut(row + area.x as usize..row + right as usize) {
                span.fill(colour);
            }
        }
    }

    /// Writes `src` into row `y` from column `x`, clipped to the canvas.
    pub fn put_row(&mut self, x: u32, y: u32, src: &[u32]) {
        if y >= self.height || x >= self.width {
            return;
        }
        let room = (self.width - x) as usize;
        let n = src.len().min(room);
        let start = y as usize * self.width as usize + x as usize;
        if let (Some(dst), Some(from)) = (self.pixels.get_mut(start..start + n), src.get(..n)) {
            dst.copy_from_slice(from);
        }
    }

    /// The pixels of `area` row by row (clipped), as an owned copy.
    pub fn copy_out(&self, area: Area) -> Vec<u32> {
        let mut out = Vec::new();
        let right = area.right().min(u64::from(self.width)) as u32;
        for y in area.y..area.bottom().min(u64::from(self.height)) as u32 {
            let row = y as usize * self.width as usize;
            if let Some(span) = self.pixels.get(row + area.x as usize..row + right as usize) {
                out.extend_from_slice(span);
            }
        }
        out
    }

    /// A validated core frame of the current pixels.
    pub fn snapshot(&self) -> Result<Frame, RfbError> {
        Frame::new(self.width, self.height, self.pixels.clone()).map_err(|e| RfbError::Protocol(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_validated_before_allocation() {
        for (w, h) in [(0, 1), (1, 0), (MAX_DIM + 1, 1), (8000, 8000)] {
            assert!(matches!(Canvas::new(w, h), Err(RfbError::Protocol(_))), "{w}x{h}");
        }
        assert_eq!(Canvas::new(3, 2).unwrap().pixels().len(), 6);
    }

    #[test]
    fn every_accessor_clips() {
        let mut c = Canvas::new(4, 3).unwrap();
        c.fill(Area::new(2, 1, 100, 100), 7);
        assert_eq!(
            (c.get(1, 1), c.get(2, 1), c.get(3, 2), c.get(4, 0), c.get(0, 3)),
            (Some(0), Some(7), Some(7), None, None)
        );
        c.fill(Area::new(9, 9, 1, 1), 5);
        c.put_row(3, 0, &[1, 2, 3]);
        c.put_row(4, 0, &[9]);
        c.put_row(0, 3, &[9]);
        assert_eq!((c.get(3, 0), c.get(0, 0)), (Some(1), Some(0)));
        assert_eq!(c.copy_out(Area::new(2, 1, 9, 9)), vec![7, 7, 7, 7]);
        assert_eq!(c.bounds(), Area::new(0, 0, 4, 3));
        assert_eq!(c.snapshot().unwrap().get(3, 0), Some(1));
        assert_eq!((c.width(), c.height()), (4, 3));
        assert_eq!(Area::new(1, 2, 3, 4).right(), 4);
        assert_eq!(Area::new(1, 2, 3, 4).bottom(), 6);
    }
}
