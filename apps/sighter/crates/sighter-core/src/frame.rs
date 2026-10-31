//! A captured frame: `0x00RRGGBB` pixels, row-major, in capture pixels. The fields are private, so every `Frame` has
//! non-zero sides within [`MAX_DIM`] / [`MAX_PIXELS`] and exactly `width * height` pixels (parse, don't validate).
//! Pixel work (crops, matching, decoding) lives in the crates that need it; the core only carries the validated buffer.

/// Largest side accepted from any source (VNC server, PNG file). Bounds memory for untrusted input.
pub const MAX_DIM: u32 = 8192;
/// Largest pixel count (8K UHD), so a hostile size pair cannot ask for more than needed.
pub const MAX_PIXELS: u64 = 7680 * 4320;

/// Why a frame could not be built.
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

    /// The pixel at `(x, y)`, or `None` outside the frame.
    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.width && y < self.height {
            self.pixels.get(y as usize * self.width as usize + x as usize).copied()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_validated_before_any_allocation() {
        assert_eq!(Frame::solid(0, 5, 0), Err(FrameError::Empty { width: 0, height: 5 }));
        assert_eq!(
            Frame::solid(MAX_DIM + 1, 1, 0),
            Err(FrameError::TooLarge {
                width: MAX_DIM + 1,
                height: 1
            })
        );
        assert!(matches!(Frame::solid(8192, 8192, 0), Err(FrameError::TooLarge { .. })));
        assert!(matches!(
            Frame::new(2, 2, vec![0; 3]),
            Err(FrameError::LengthMismatch {
                expected: 4,
                actual: 3,
                ..
            })
        ));
        assert!(FrameError::Empty { width: 0, height: 5 }.to_string().contains("1x1"));
    }

    #[test]
    fn pixels_are_row_major_and_masked() {
        let f = Frame::new(2, 2, vec![1, 2, 3, 4]).unwrap();
        assert_eq!(
            (f.get(0, 0), f.get(1, 0), f.get(0, 1), f.get(1, 1)),
            (Some(1), Some(2), Some(3), Some(4))
        );
        assert_eq!((f.get(2, 0), f.get(0, 2)), (None, None));
        assert_eq!(f.pixels().len(), 4);
        assert_eq!(Frame::solid(1, 1, 0xff12_3456).unwrap().get(0, 0), Some(0x0012_3456));
        assert_eq!((f.width(), f.height()), (2, 2));
        assert_eq!(format!("{f:?}"), "Frame(2x2)");
    }
}
