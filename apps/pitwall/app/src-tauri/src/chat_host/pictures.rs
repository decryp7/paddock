//! Chat pictures (ADR 0023 Decision 4, WP-5): what the composer may attach as a picture and how it is prepared. The bytes come
//! from the webview and are judged here: the magic bytes name the format (PNG, JPEG, WebP, GIF; the file name and the type the
//! system guessed are never trusted), the header's size is checked before a pixel is decoded, the decoder runs with allocation
//! limits, the picture is turned upright by its EXIF orientation, shrunk to the projector's pixel budget
//! (`gguf::ProjectorInfo::max_image_pixels`) and written again, which drops every metadata block (location, camera, comments).
//! Pure: the caller decides which model it is for and keeps the result.

use super::attach::Refused;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use pitwall_core::chat::ImageMime;
use std::io::Cursor;

/// Largest source file of a picture (ADR 0023 Decision 4: 25 MiB, judgement). Larger files are refused before they are read.
pub const MAX_SOURCE_BYTES: usize = 25 * 1024 * 1024;
/// Most pixels of a source picture (a 40 megapixel camera), checked from the header before decoding. Decoded at 16 bits with alpha
/// it needs 320 MB, which is [`MAX_ALLOC`], and the decoder holds about half as much again while it reads (measured by
/// `tests/picture_memory.rs`); the picture is shrunk before it is turned or written again, so no other copy of that size is held,
/// and one picture is prepared at a time ([`PREPARING`]).
pub const MAX_SOURCE_PIXELS: u64 = 40_000_000;
/// Longest side of a source picture the decoder accepts.
const MAX_SIDE: u32 = 16_384;
/// The decoder's allocation limit while it decodes the pixels.
const MAX_ALLOC: u64 = 320 * 1024 * 1024;
/// The allocation limit while only the header is read: a PNG's iCCP profile and text chunks are inflated while the header is read,
/// and a 2 MB file can ask for gigabytes, so the header is never read without a limit (the pixels' room is given afterwards).
const MAX_HEADER_ALLOC: u64 = 16 * 1024 * 1024;
/// One picture is prepared at a time: preparing is CPU- and memory-heavy and a person attaches one at a time.
static PREPARING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Largest prepared picture the chat reads back to send (a downscaled picture is far below it; the bound only guards the file).
pub const MAX_PREPARED_BYTES: usize = 12 * 1024 * 1024;
/// Pixels the budget allows when no projector header gave one: 1024 image tokens at the default 28 pixels a token side.
pub const DEFAULT_MAX_PIXELS: u64 = pitwall_core::gguf::MAX_IMAGE_TOKENS * 28 * 28;
/// The longest side of a thumbnail, in pixels.
pub const THUMBNAIL_SIDE: u32 = 160;
/// JPEG quality of prepared and thumbnail pictures that were JPEG.
const JPEG_QUALITY: u8 = 90;

/// A picture ready to keep and send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    /// The new file: PNG, or JPEG when the source was JPEG; no metadata.
    pub bytes: Vec<u8>,
    /// Its type.
    pub mime: ImageMime,
    /// Its size in pixels, after turning and shrinking.
    pub width: u32,
    /// Its size in pixels, after turning and shrinking.
    pub height: u32,
    /// The picture was shrunk to fit the budget.
    pub shrunk: bool,
}

/// What the first bytes of a file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    /// A picture the chat reads.
    Supported(ImageFormat),
    /// A picture the chat cannot read (BMP, TIFF, HEIC, AVIF, ICO): named so the refusal can say what to do.
    Unsupported,
    /// Not a picture by its bytes.
    NotAnImage,
}

/// A BMP by its file header and not by two letters (a text file can start with "BM"): the reserved fields (bytes 6..10) are zero
/// and the info header's size (at 14) is one of the sizes BMP has.
fn is_bmp(bytes: &[u8]) -> bool {
    bytes.len() >= 30
        && bytes.starts_with(b"BM")
        && bytes[6..10] == [0, 0, 0, 0]
        && matches!(
            u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]),
            12 | 40 | 52 | 56 | 108 | 124
        )
}

/// Names a file by its magic bytes alone. Never panics on any input.
pub fn sniff(bytes: &[u8]) -> Sniffed {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Supported(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Sniffed::Supported(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Sniffed::Supported(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Sniffed::Supported(ImageFormat::WebP)
    } else if is_bmp(bytes)
        || bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
        || bytes.starts_with(&[0, 0, 1, 0])
        || (bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && matches!(&bytes[8..12], b"heic" | b"heix" | b"mif1" | b"avif"))
    {
        Sniffed::Unsupported
    } else {
        Sniffed::NotAnImage
    }
}

/// The type the chat sends for a prepared picture.
fn mime_of(format: ImageFormat) -> ImageMime {
    if format == ImageFormat::Jpeg {
        ImageMime::Jpeg
    } else {
        ImageMime::Png
    }
}

/// The decoder's limits for the pixels: a side of at most [`MAX_SIDE`] and [`MAX_ALLOC`] bytes of allocation.
fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

/// The limits while only the header is read: [`MAX_HEADER_ALLOC`] bytes of allocation, never none, and no limit on the sides (the
/// size is judged by [`prepare_image`] itself, so a huge claim is refused with its size in the words).
fn header_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_HEADER_ALLOC);
    limits
}

fn refused(words: String) -> Refused {
    Refused(words)
}

/// The words for a picture the chat cannot read (a type it does not know).
pub fn unsupported(name: &str) -> Refused {
    refused(format!(
        "\"{name}\" is an image type the chat can't read. It reads PNG, JPEG, WebP and GIF; convert it, then attach it again."
    ))
}

/// Checks and prepares a picture: refuses a file over [`MAX_SOURCE_BYTES`], a type the chat cannot read, a picture over
/// [`MAX_SOURCE_PIXELS`] or one that does not decode; otherwise turns it upright, shrinks it to at most `max_pixels` and writes it
/// again. `name` is the shown name (already cleaned). Never panics on any input.
pub fn prepare_image(name: &str, bytes: &[u8], max_pixels: u64) -> Result<PreparedImage, Refused> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(refused(format!(
            "\"{name}\" is larger than {} MiB. Attach a smaller image, or export a smaller copy.",
            MAX_SOURCE_BYTES / (1024 * 1024)
        )));
    }
    let format = match sniff(bytes) {
        Sniffed::Supported(f) => f,
        Sniffed::Unsupported | Sniffed::NotAnImage => return Err(unsupported(name)),
    };
    let broken = || {
        refused(format!(
            "\"{name}\" could not be read as an image. It may be damaged; try another copy."
        ))
    };
    let _one_at_a_time = PREPARING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // The header alone gives the size: building the decoder reads only that (no pixels) under a small allocation limit, and the size
    // is judged before the decoder is given room for the pixels.
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(header_limits());
    let mut decoder = reader.into_decoder().map_err(|_| broken())?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 {
        return Err(broken());
    }
    if u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Err(refused(format!(
            "\"{name}\" is {w} \u{d7} {h} pixels, more than the chat opens ({} megapixels). Attach a smaller copy.",
            MAX_SOURCE_PIXELS / 1_000_000
        )));
    }
    decoder.set_limits(decode_limits()).map_err(|_| broken())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let img = DynamicImage::from_decoder(decoder).map_err(|_| broken())?;
    // Shrink first, then turn: the budget is a pixel count, so the same whichever way it is turned, and the turned copy is small.
    let (mut img, shrunk) = shrink(img, max_pixels);
    img.apply_orientation(orientation);
    let (width, height) = (img.width(), img.height());
    let mime = mime_of(format);
    let mut out = Vec::new();
    let encoded = if mime == ImageMime::Jpeg {
        let rgb = img.into_rgb8();
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
    } else {
        // PNG keeps flat colour and sharp text exact; a transparent picture keeps its alpha.
        let rgba = img.into_rgba8();
        PngEncoder::new(&mut out).write_image(rgba.as_raw(), width, height, image::ExtendedColorType::Rgba8)
    };
    encoded.map_err(|_| refused(format!("\"{name}\" could not be prepared. Try another copy of it.")))?;
    Ok(PreparedImage {
        bytes: out,
        mime,
        width,
        height,
        shrunk,
    })
}

/// Shrinks `img` (keeping its shape) until it has at most `max_pixels`; whether it did.
fn shrink(img: DynamicImage, max_pixels: u64) -> (DynamicImage, bool) {
    let (w, h) = (u64::from(img.width()), u64::from(img.height()));
    let max_pixels = max_pixels.max(1);
    if w * h <= max_pixels {
        return (img, false);
    }
    // Pixel counts are far below 2^53, so the float arithmetic is exact enough; the floor keeps the result within the budget.
    let scale = (max_pixels as f64 / (w * h) as f64).sqrt();
    let mut nw = ((w as f64 * scale).floor() as u32).max(1);
    let mut nh = ((h as f64 * scale).floor() as u32).max(1);
    while u64::from(nw) * u64::from(nh) > max_pixels && (nw > 1 || nh > 1) {
        if nw >= nh && nw > 1 {
            nw -= 1;
        } else {
            nh = nh.saturating_sub(1).max(1);
        }
    }
    (img.resize_exact(nw, nh, FilterType::Lanczos3), true)
}

/// A small picture of `bytes` (a prepared picture) as a `data:` URL for the chip and the turn, at most [`THUMBNAIL_SIDE`] pixels
/// on a side; `None` when it cannot be read (the chip then shows only its name).
pub fn thumbnail(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_PREPARED_BYTES {
        return None;
    }
    let format = match sniff(bytes) {
        Sniffed::Supported(f) => f,
        _ => return None,
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(decode_limits());
    let img = reader.decode().ok()?;
    let small = img.thumbnail(THUMBNAIL_SIDE, THUMBNAIL_SIDE);
    let (width, height) = (small.width(), small.height());
    let mut out = Vec::new();
    let mime = mime_of(format);
    if mime == ImageMime::Jpeg {
        let rgb = small.into_rgb8();
        JpegEncoder::new_with_quality(&mut out, 80)
            .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .ok()?;
    } else {
        let rgba = small.into_rgba8();
        PngEncoder::new(&mut out)
            .write_image(rgba.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .ok()?;
    }
    Some(format!(
        "data:{};base64,{}",
        mime.as_str(),
        base64::engine::general_purpose::STANDARD.encode(out)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba([(x % 251) as u8, (y % 241) as u8, 90, 255]));
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn jpeg_with_exif(w: u32, h: u32, orientation: u8) -> Vec<u8> {
        let rgb = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]));
        let mut plain = Vec::new();
        JpegEncoder::new_with_quality(&mut plain, 90)
            .write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        // An APP1 segment with a one-entry little-endian EXIF block (orientation) and a GPS marker string right after SOI.
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        exif.extend([orientation, 0, 0, 0, 0, 0, 0, 0]);
        let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
        out.extend(((exif.len() + 2) as u16).to_be_bytes());
        out.extend(exif);
        // A comment segment standing for camera and location notes: it must not survive.
        let note = b"GPS 52.5200N secret-note";
        out.extend([0xff, 0xfe]);
        out.extend(((note.len() + 2) as u16).to_be_bytes());
        out.extend(note);
        out.extend(&plain[2..]);
        out
    }

    #[test]
    fn the_magic_bytes_name_the_format_and_never_the_file_name() {
        assert_eq!(sniff(&png(2, 2)), Sniffed::Supported(ImageFormat::Png));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Sniffed::Supported(ImageFormat::Jpeg));
        assert_eq!(sniff(b"GIF89a.."), Sniffed::Supported(ImageFormat::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Sniffed::Supported(ImageFormat::WebP));
        let bmp = [b"BM".as_slice(), &[0; 8], &54u32.to_le_bytes(), &40u32.to_le_bytes(), &[0; 20]].concat();
        for other in [&bmp[..], b"II*\0....", b"\0\0\0\x18ftypheic....", b"\0\0\x01\0"] {
            assert_eq!(sniff(other), Sniffed::Unsupported, "{other:?}");
        }
        assert_eq!(sniff(b"hello"), Sniffed::NotAnImage);
        assert_eq!(sniff(b""), Sniffed::NotAnImage);
        // A text file that is called .png is not a picture: the bytes decide.
        let words = prepare_image("fake.png", b"just text", DEFAULT_MAX_PIXELS).unwrap_err().0;
        assert!(words.contains("fake.png") && words.contains("PNG, JPEG, WebP and GIF"), "{words}");
    }

    #[test]
    fn a_picture_is_read_and_written_again_at_its_size_when_it_is_within_the_budget() {
        let p = prepare_image("a.png", &png(40, 30), 10_000).unwrap();
        assert_eq!((p.width, p.height, p.mime, p.shrunk), (40, 30, ImageMime::Png, false));
        assert_eq!(sniff(&p.bytes), Sniffed::Supported(ImageFormat::Png));
        let j = prepare_image("a.jpg", &jpeg_with_exif(32, 16, 1), 10_000).unwrap();
        assert_eq!((j.width, j.height, j.mime), (32, 16, ImageMime::Jpeg));
    }

    /// The budget of the projector is finally used: a picture over it is shrunk to within it, keeping its shape.
    #[test]
    fn a_picture_over_the_budget_is_shrunk_to_within_it_keeping_its_shape() {
        let p = prepare_image("big.png", &png(400, 200), 5_000).unwrap();
        assert!(p.shrunk);
        assert!(u64::from(p.width) * u64::from(p.height) <= 5_000, "{}x{}", p.width, p.height);
        let ratio = f64::from(p.width) / f64::from(p.height);
        assert!((ratio - 2.0).abs() < 0.1, "{ratio}");
        // A one-pixel budget still gives a picture.
        let tiny = prepare_image("big.png", &png(40, 40), 1).unwrap();
        assert_eq!((tiny.width, tiny.height), (1, 1));
    }

    /// Metadata does not survive, and a phone photo that says "rotate" is turned upright (the new file has no orientation tag).
    #[test]
    fn metadata_is_dropped_and_the_orientation_is_applied() {
        let src = jpeg_with_exif(32, 16, 6);
        assert!(src.windows(6).any(|w| w == b"secret"), "the fixture has a note");
        let p = prepare_image("photo.jpg", &src, 10_000).unwrap();
        assert!(!p.bytes.windows(6).any(|w| w == b"secret"), "the note is gone");
        assert!(!p.bytes.windows(4).any(|w| w == b"Exif"), "the EXIF block is gone");
        assert_eq!((p.width, p.height), (16, 32), "orientation 6 turns a landscape file upright");
    }

    #[test]
    fn a_picture_is_refused_in_words_when_too_big_wrong_or_broken() {
        let big = vec![0x89; MAX_SOURCE_BYTES + 1];
        let words = prepare_image("huge.png", &big, DEFAULT_MAX_PIXELS).unwrap_err().0;
        assert!(
            words.contains("huge.png") && words.contains("25 MiB") && words.contains("smaller"),
            "{words}"
        );
        let words = prepare_image("scan.tiff", b"II*\0....", DEFAULT_MAX_PIXELS).unwrap_err().0;
        assert!(words.contains("scan.tiff") && words.contains("convert"), "{words}");
        // A PNG cut short is damaged, not a crash.
        let mut cut = png(20, 20);
        cut.truncate(cut.len() / 2);
        let words = prepare_image("cut.png", &cut, DEFAULT_MAX_PIXELS).unwrap_err().0;
        assert!(words.contains("cut.png") && words.contains("damaged"), "{words}");
        assert!(prepare_image("zero.png", b"\x89PNG\r\n\x1a\n", DEFAULT_MAX_PIXELS).is_err());
    }

    /// A PNG that is only a header: the signature, an IHDR for `w` x `h`, and the start of the pixel data (an empty IDAT).
    fn header_only_png(w: u32, h: u32) -> Vec<u8> {
        let chunk = |kind: &[u8; 4], data: &[u8]| {
            let mut body = kind.to_vec();
            body.extend(data);
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend(&body);
            out.extend(crc32(&body).to_be_bytes());
            out
        };
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &[]));
        out
    }

    /// A pixel bomb: a tiny file whose header claims a huge picture is refused from the header, before anything is allocated.
    #[test]
    fn a_pixel_bomb_is_refused_from_its_header() {
        // 20000 x 20000 (400 megapixels): past the side limit and the pixel limit.
        let words = prepare_image("bomb.png", &header_only_png(20_000, 20_000), DEFAULT_MAX_PIXELS)
            .unwrap_err()
            .0;
        assert!(
            words.contains("bomb.png") && words.contains("20000") && words.contains("megapixels"),
            "{words}"
        );
        // 12000 x 12000: within the side limit, over the pixel limit, and more than the decoder may allocate.
        let words = prepare_image("wide.png", &header_only_png(12_000, 12_000), DEFAULT_MAX_PIXELS)
            .unwrap_err()
            .0;
        assert!(words.contains("12000") && words.contains("megapixels"), "{words}");
        // 4000 x 4000 (16 megapixels) is a fine size: a header with no pixels is then damaged, not a bomb.
        let words = prepare_image("empty.png", &header_only_png(4000, 4000), DEFAULT_MAX_PIXELS)
            .unwrap_err()
            .0;
        assert!(words.contains("damaged"), "{words}");
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for b in data {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn a_thumbnail_is_a_small_data_url_and_none_for_what_is_not_a_picture() {
        let big = prepare_image("a.png", &png(640, 480), 400_000).unwrap();
        let url = thumbnail(&big.bytes).unwrap();
        assert!(url.starts_with("data:image/png;base64,"), "{}", &url[..30]);
        let b64 = url.split_once(',').unwrap().1;
        let small = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let decoded = image::load_from_memory(&small).unwrap();
        assert!(decoded.width().max(decoded.height()) <= THUMBNAIL_SIDE);
        let j = prepare_image("a.jpg", &jpeg_with_exif(300, 200, 1), 400_000).unwrap();
        assert!(thumbnail(&j.bytes).unwrap().starts_with("data:image/jpeg;base64,"));
        assert_eq!(thumbnail(b"not an image"), None);
    }

    /// Many pseudo-random byte strings with the magic numbers in front (a fixed seed, so a failure repeats): nothing panics.
    #[test]
    fn preparing_never_panics() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let heads: [&[u8]; 5] = [b"\x89PNG\r\n\x1a\n", &[0xff, 0xd8, 0xff], b"GIF89a", b"RIFF\0\0\0\0WEBP", b"BM"];
        let good = png(8, 8);
        for round in 0..300 {
            let mut bytes = heads[(next() % 5) as usize].to_vec();
            if round % 3 == 0 {
                // A real PNG with a few bytes flipped.
                bytes = good.clone();
                for _ in 0..3 {
                    let at = (next() as usize) % bytes.len();
                    bytes[at] ^= (next() % 255) as u8 + 1;
                }
            } else {
                bytes.extend((0..(next() % 400)).map(|_| next() as u8));
            }
            let _ = prepare_image("x", &bytes, 1000);
            let _ = thumbnail(&bytes);
            let _ = sniff(&bytes);
        }
    }

    /// A text file that happens to start with "BM" is text, not a BMP; a real BMP header (reserved fields zero, a known info
    /// header size) is a type the chat cannot read.
    #[test]
    fn text_that_starts_with_bm_is_not_a_bmp() {
        assert_eq!(
            sniff(b"BM25 is a ranking function used by search engines to rank documents."),
            Sniffed::NotAnImage
        );
        assert_eq!(
            sniff(b"BMW 320d service notes: oil change due at 15,000 km, brake pads next year."),
            Sniffed::NotAnImage
        );
        let mut bmp = b"BM".to_vec();
        bmp.extend(1000u32.to_le_bytes());
        bmp.extend([0, 0, 0, 0]);
        bmp.extend(54u32.to_le_bytes());
        bmp.extend(40u32.to_le_bytes());
        bmp.extend([0u8; 20]);
        assert_eq!(sniff(&bmp), Sniffed::Unsupported);
        for size in [12u32, 52, 56, 108, 124] {
            bmp[14..18].copy_from_slice(&size.to_le_bytes());
            assert_eq!(sniff(&bmp), Sniffed::Unsupported, "info header {size}");
        }
        bmp[14..18].copy_from_slice(&41u32.to_le_bytes());
        assert_eq!(sniff(&bmp), Sniffed::NotAnImage, "an unknown info header size");
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[6] = 1;
        assert_eq!(sniff(&bmp), Sniffed::NotAnImage, "reserved fields are zero in a BMP");
    }

    /// A phone photo is turned and shrunk without ever holding a turned full-size copy: the budget is a pixel count, so it is the same
    /// whichever way the picture is turned, and the result is the same size either way.
    #[test]
    fn a_turned_picture_is_shrunk_to_the_budget_and_comes_out_upright() {
        let p = prepare_image("photo.jpg", &jpeg_with_exif(200, 100, 6), 5_000).unwrap();
        assert!(u64::from(p.width) * u64::from(p.height) <= 5_000);
        assert!(p.height > p.width, "turned upright: {}x{}", p.width, p.height);
        assert!(p.shrunk);
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut body = kind.to_vec();
        body.extend(data);
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
        out
    }

    /// A PNG with an iCCP profile and an eXIf chunk after its header (both small and valid).
    fn png_with_profile_and_exif() -> Vec<u8> {
        let good = png(8, 8);
        let profile = miniz_oxide::deflate::compress_to_vec_zlib(&[7u8; 300], 6);
        let mut iccp = b"p\0\0".to_vec();
        iccp.extend(profile);
        let mut out = good[..33].to_vec();
        out.extend(chunk(b"iCCP", &iccp));
        out.extend(chunk(b"eXIf", b"MM\0*\0\0\0\x08\0\0\0\0\0\0"));
        out.extend(&good[33..]);
        out
    }

    fn gif_seed() -> Vec<u8> {
        let img = RgbaImage::from_fn(12, 9, |x, y| Rgba([(x * 20) as u8, (y * 25) as u8, 50, 255]));
        let mut out = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut out)
            .encode(img.as_raw(), 12, 9, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn webp_seed() -> Vec<u8> {
        let img = RgbaImage::from_fn(12, 9, |x, y| Rgba([(x * 20) as u8, (y * 25) as u8, 50, 255]));
        let mut out = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut out)
            .write_image(img.as_raw(), 12, 9, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    /// An animated WebP: the still's lossless frame inside the animation chunks (VP8X, ANIM, ANMF).
    fn animated_webp_seed() -> Vec<u8> {
        let still = webp_seed();
        let frame = &still[12..];
        let le24 = |n: u32| n.to_le_bytes()[..3].to_vec();
        let mut anmf = [le24(0), le24(0), le24(11), le24(8), le24(100), vec![0]].concat();
        anmf.extend(frame);
        let mut body = b"WEBP".to_vec();
        let mut vp8x = vec![0x12, 0, 0, 0];
        vp8x.extend(le24(11));
        vp8x.extend(le24(8));
        body.extend(b"VP8X");
        body.extend(10u32.to_le_bytes());
        body.extend(&vp8x);
        body.extend(b"ANIM");
        body.extend(6u32.to_le_bytes());
        body.extend([0, 0, 0, 0, 0, 0]);
        body.extend(b"ANMF");
        body.extend((anmf.len() as u32).to_le_bytes());
        body.extend(&anmf);
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    /// Valid seeds of every format the chat reads, with a PNG carrying iCCP and eXIf and an animated WebP, are prepared; then many
    /// copies of each with bytes flipped, inserted, removed or cut (a fixed seed, so a failure repeats) never panic.
    #[test]
    fn mutated_images_of_every_format_never_panic() {
        let seeds: Vec<(&str, Vec<u8>)> = vec![
            ("png", png(16, 12)),
            ("png+iccp+exif", png_with_profile_and_exif()),
            ("jpeg", jpeg_with_exif(24, 16, 3)),
            ("gif", gif_seed()),
            ("webp", webp_seed()),
            ("webp animated", animated_webp_seed()),
        ];
        for (name, seed) in &seeds {
            let p = prepare_image(name, seed, 10_000);
            assert!(p.is_ok(), "the {name} seed is valid: {:?}", p.err());
        }
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for (name, seed) in &seeds {
            for _ in 0..250 {
                let mut bytes = seed.clone();
                for _ in 0..(1 + next() % 4) {
                    let at = (next() as usize) % bytes.len();
                    match next() % 4 {
                        0 => bytes[at] ^= (next() % 255) as u8 + 1,
                        1 => bytes.insert(at, next() as u8),
                        2 => {
                            bytes.remove(at);
                        }
                        _ => bytes.truncate(at.max(1)),
                    }
                    if bytes.is_empty() {
                        break;
                    }
                }
                let _ = prepare_image(name, &bytes, 1_000);
                let _ = thumbnail(&bytes);
            }
        }
    }
}
