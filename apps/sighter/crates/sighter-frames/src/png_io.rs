//! PNG in and out with the frame limits enforced before any allocation. Decoding never panics on any bytes.

use std::io::{Cursor, Write};

use png::{BitDepth, ColorType, Transformations};
use sighter_core::{Frame, MAX_DIM, MAX_PIXELS};

/// Largest PNG file accepted (bounds the work for a hostile file).
pub const MAX_PNG_BYTES: usize = 64 * 1024 * 1024;

/// Why a PNG could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PngError {
    /// The file is larger than [`MAX_PNG_BYTES`].
    #[error("the PNG is {0} bytes; the limit is {MAX_PNG_BYTES}")]
    FileTooLarge(usize),
    /// The image is larger than the frame limits.
    #[error("the PNG is {width}x{height}; images must be 1..{MAX_DIM} pixels per side and at most {MAX_PIXELS} pixels")]
    TooLarge {
        /// Width in the header.
        width: u32,
        /// Height in the header.
        height: u32,
    },
    /// Not a decodable PNG.
    #[error("the file is not a valid PNG: {0}")]
    Invalid(String),
}

/// Decodes a PNG into a frame (alpha dropped, 16-bit reduced to 8, palettes expanded).
pub fn decode_png(bytes: &[u8]) -> Result<Frame, PngError> {
    decode_png_limited(bytes, MAX_PNG_BYTES)
}

fn decode_png_limited(bytes: &[u8], max_bytes: usize) -> Result<Frame, PngError> {
    if bytes.len() > max_bytes {
        return Err(PngError::FileTooLarge(bytes.len()));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| PngError::Invalid(e.to_string()))?;
    let (width, height) = reader.info().size();
    if width == 0 || height == 0 || width > MAX_DIM || height > MAX_DIM || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(PngError::TooLarge { width, height });
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| PngError::Invalid(e.to_string()))?;
    // EXPAND and STRIP_16 leave 8-bit gray, gray+alpha, RGB or RGBA (palettes become RGB), so `samples()` is 1..=4.
    let channels = info.color_type.samples();
    let data = buf
        .get(..info.buffer_size())
        .ok_or_else(|| PngError::Invalid("short image data".into()))?;
    let pixels = data
        .chunks_exact(channels)
        .map(|c| {
            if channels < 3 {
                u32::from(c[0]) * 0x0001_0101
            } else {
                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
            }
        })
        .collect();
    Frame::new(width, height, pixels).map_err(|e| PngError::Invalid(e.to_string()))
}

/// Encodes a frame as an 8-bit RGB PNG.
pub fn encode_png(frame: &Frame) -> Result<Vec<u8>, PngError> {
    let mut out = Vec::new();
    encode_png_to(frame, &mut out)?;
    Ok(out)
}

fn encode_png_to<W: Write>(frame: &Frame, out: W) -> Result<(), PngError> {
    let mut encoder = png::Encoder::new(out, frame.width(), frame.height());
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(BitDepth::Eight);
    let data: Vec<u8> = frame
        .pixels()
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    let mut writer = encoder.write_header().map_err(write_error)?;
    writer.write_image_data(&data).map_err(write_error)?;
    writer.finish().map_err(write_error)
}

fn write_error(e: png::EncodingError) -> PngError {
    PngError::Invalid(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use sighter_core::Rect;

    #[test]
    fn round_trip() {
        let mut f = Frame::solid(3, 2, 0x112233).unwrap();
        f.fill(Rect::new(1, 0, 1, 2), 0x00ff80);
        assert_eq!(decode_png(&encode_png(&f).unwrap()).unwrap(), f);
    }

    fn raw_png(color: ColorType, depth: BitDepth, w: u32, h: u32, data: &[u8], palette: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(depth);
        if let Some(p) = palette {
            e.set_palette(p.to_vec());
        }
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(data).unwrap();
        wr.finish().unwrap();
        out
    }

    #[test]
    fn other_layouts_decode() {
        let gray = raw_png(ColorType::Grayscale, BitDepth::Eight, 2, 1, &[0, 255], None);
        assert_eq!(decode_png(&gray).unwrap().pixels(), &[0, 0xffffff]);
        let ga = raw_png(ColorType::GrayscaleAlpha, BitDepth::Eight, 1, 1, &[10, 0], None);
        assert_eq!(decode_png(&ga).unwrap().pixels(), &[0x0a0a0a]);
        let rgba = raw_png(ColorType::Rgba, BitDepth::Eight, 1, 1, &[1, 2, 3, 0], None);
        assert_eq!(decode_png(&rgba).unwrap().pixels(), &[0x010203]);
        let sixteen = raw_png(ColorType::Rgb, BitDepth::Sixteen, 1, 1, &[0xff, 0xff, 0, 0, 0x80, 0], None);
        assert_eq!(decode_png(&sixteen).unwrap().pixels(), &[0xff0080]);
        let indexed = raw_png(ColorType::Indexed, BitDepth::Eight, 2, 1, &[1, 0], Some(&[0, 0, 0, 9, 8, 7]));
        assert_eq!(decode_png(&indexed).unwrap().pixels(), &[0x090807, 0]);
    }

    /// A writer that accepts `left` bytes and then fails, as a full disk would.
    struct Failing {
        left: usize,
    }

    impl Write for Failing {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Err(std::io::Error::other("disk full"));
            }
            let n = buf.len().min(self.left);
            self.left -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_failing_writer_is_an_error_at_every_point() {
        let f = Frame::solid(4, 4, 0x123456).unwrap();
        Failing { left: 1 }.flush().unwrap();
        let total = encode_png(&f).unwrap().len();
        for left in 0..total {
            assert!(matches!(encode_png_to(&f, Failing { left }), Err(PngError::Invalid(_))), "{left}");
        }
    }

    #[test]
    fn oversized_and_broken_files_are_refused() {
        let wide = raw_png(ColorType::Grayscale, BitDepth::Eight, 8193, 1, &vec![0; 8193], None);
        assert!(matches!(decode_png(&wide), Err(PngError::TooLarge { width: 8193, .. })));
        let big = raw_png(ColorType::Grayscale, BitDepth::One, 8000, 8000, &vec![0; 8000 * 1000], None);
        assert!(matches!(decode_png(&big), Err(PngError::TooLarge { .. })));
        assert!(matches!(decode_png(b"not a png"), Err(PngError::Invalid(_))));
        let mut cut = encode_png(&Frame::solid(50, 50, 7).unwrap()).unwrap();
        cut.truncate(cut.len() / 2);
        assert!(matches!(decode_png(&cut), Err(PngError::Invalid(_))));
        assert_eq!(decode_png_limited(&cut, 10), Err(PngError::FileTooLarge(cut.len())));
        assert!(PngError::FileTooLarge(5).to_string().contains('5'));
        assert_eq!(MAX_PNG_BYTES, 64 * 1024 * 1024);
    }

    proptest! {
        /// Arbitrary bytes, and a valid file with a flipped byte, never panic.
        #[test]
        fn decoding_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..300), at in any::<usize>(), flip in any::<u8>()) {
            let _ = decode_png(&bytes);
            let mut good = encode_png(&Frame::solid(9, 9, 0x123456).unwrap()).unwrap();
            let i = at % good.len();
            good[i] ^= flip;
            let _ = decode_png(&good);
        }
    }
}
