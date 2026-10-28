//! A captured frame: `0x00RRGGBB` pixels, row-major. The fields are private, so every `Frame` has non-zero sides within
//! [`MAX_DIM`] / [`MAX_PIXELS`] and exactly `width * height` pixels (parse, don't validate).

use crate::geometry::Rect;

/// Largest side accepted from any source (VNC server, PNG file). Bounds memory for untrusted input.
pub const MAX_DIM: u32 = 8192;
/// Largest pixel count (8K UHD), so a hostile size pair cannot ask for 8192 x 8192 x 4 bytes more than needed.
pub const MAX_PIXELS: u64 = 7680 * 4320;

/// Why a frame could not be built or cut.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// A side is zero.
    #[error("a frame needs at least 1x1 pixels, got {width}x{height}")]
    Empty {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// A side or the pixel count is over the limit.
    #[error("{width}x{height} is over the frame limit ({MAX_DIM} per side, {MAX_PIXELS} pixels)")]
    TooLarge {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The pixel buffer does not match the size.
    #[error("{width}x{height} needs {expected} pixels, got {actual}")]
    LengthMismatch {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
        /// `width * height`.
        expected: usize,
        /// Length given.
        actual: usize,
    },
    /// A crop that lies wholly outside the frame.
    #[error("{rect:?} is outside the {width}x{height} frame")]
    OutsideFrame {
        /// The requested rectangle.
        rect: Rect,
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
}

/// A validated frame.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

fn check_size(width: u32, height: u32) -> Result<usize, FrameError> {
    if width == 0 || height == 0 {
        return Err(FrameError::Empty { width, height });
    }
    let pixels = u64::from(width) * u64::from(height);
    if width > MAX_DIM || height > MAX_DIM || pixels > MAX_PIXELS {
        return Err(FrameError::TooLarge { width, height });
    }
    Ok(pixels as usize)
}

impl Frame {
    /// A frame from existing pixels.
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Result<Frame, FrameError> {
        let expected = check_size(width, height)?;
        if pixels.len() != expected {
            return Err(FrameError::LengthMismatch {
                width,
                height,
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Frame { width, height, pixels })
    }

    /// A frame filled with one colour.
    pub fn solid(width: u32, height: u32, rgb: u32) -> Result<Frame, FrameError> {
        let n = check_size(width, height)?;
        Ok(Frame {
            width,
            height,
            pixels: vec![rgb & 0x00ff_ffff; n],
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

    /// The whole frame as a rectangle.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// One pixel; `None` outside the frame.
    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels.get(y as usize * self.width as usize + x as usize).copied()
    }

    /// Fills a rectangle (clamped to the frame) with one colour.
    pub fn fill(&mut self, r: Rect, rgb: u32) {
        let Some(r) = r.clamp_to(self.width, self.height) else { return };
        for y in r.y..r.y + r.h {
            let row = y as usize * self.width as usize;
            if let Some(cells) = self.pixels.get_mut(row + r.x as usize..row + (r.x + r.w) as usize) {
                cells.fill(rgb & 0x00ff_ffff);
            }
        }
    }

    /// Copy of a sub-rectangle, clamped to the frame.
    pub fn crop(&self, r: Rect) -> Result<Frame, FrameError> {
        let c = r.clamp_to(self.width, self.height).ok_or(FrameError::OutsideFrame {
            rect: r,
            width: self.width,
            height: self.height,
        })?;
        let mut pixels = Vec::with_capacity(c.area() as usize);
        for y in c.y..c.y + c.h {
            let start = y as usize * self.width as usize + c.x as usize;
            if let Some(row) = self.pixels.get(start..start + c.w as usize) {
                pixels.extend_from_slice(row);
            }
        }
        Frame::new(c.w, c.h, pixels)
    }

    /// Luma (BT.601) as `f32` in 0..=255, row-major.
    pub fn gray(&self) -> Vec<f32> {
        self.pixels
            .iter()
            .map(|p| 0.299 * ((p >> 16) & 0xff) as f32 + 0.587 * ((p >> 8) & 0xff) as f32 + 0.114 * (p & 0xff) as f32)
            .collect()
    }

    /// Every colour channel inverted (what a light/dark theme switch does to a crop, roughly).
    pub fn inverted(&self) -> Frame {
        Frame {
            width: self.width,
            height: self.height,
            pixels: self.pixels.iter().map(|p| !p & 0x00ff_ffff).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_is_validated() {
        assert_eq!(Frame::new(0, 4, vec![]), Err(FrameError::Empty { width: 0, height: 4 }));
        assert!(matches!(Frame::new(8193, 1, vec![]), Err(FrameError::TooLarge { .. })));
        assert!(matches!(Frame::solid(8000, 8000, 0), Err(FrameError::TooLarge { .. })));
        assert!(matches!(
            Frame::new(2, 2, vec![0; 3]),
            Err(FrameError::LengthMismatch {
                expected: 4,
                actual: 3,
                ..
            })
        ));
        let f = Frame::new(2, 1, vec![1, 2]).unwrap();
        assert_eq!((f.width(), f.height(), f.pixels()), (2, 1, &[1, 2][..]));
        assert_eq!(format!("{f:?}"), "Frame(2x1)");
        assert_eq!(f.bounds(), Rect::new(0, 0, 2, 1));
        assert!(FrameError::Empty { width: 0, height: 0 }.to_string().contains("1x1"));
    }

    #[test]
    fn get_fill_and_crop() {
        let mut f = Frame::solid(10, 10, 0).unwrap();
        f.fill(Rect::new(2, 3, 4, 2), 0xff_ff0000);
        assert_eq!(f.get(2, 3), Some(0xff0000));
        assert_eq!(f.get(6, 3), Some(0));
        assert_eq!(f.get(10, 0), None);
        assert_eq!(f.get(0, 10), None);
        f.fill(Rect::new(50, 50, 4, 4), 1);
        let c = f.crop(Rect::new(2, 3, 4, 2)).unwrap();
        assert!(c.pixels().iter().all(|p| *p == 0xff0000));
        assert!(matches!(f.crop(Rect::new(20, 0, 1, 1)), Err(FrameError::OutsideFrame { .. })));
        let clamped = f.crop(Rect::new(8, 8, 10, 10)).unwrap();
        assert_eq!((clamped.width(), clamped.height()), (2, 2));
    }

    #[test]
    fn gray_and_invert() {
        let f = Frame::new(2, 1, vec![0xffffff, 0x000000]).unwrap();
        let g = f.gray();
        assert!((g[0] - 255.0).abs() < 0.01 && g[1] == 0.0);
        assert_eq!(f.inverted().pixels(), &[0, 0xffffff]);
    }
}
