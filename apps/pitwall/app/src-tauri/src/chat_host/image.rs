//! An image the user attached to the chat (ADR 0023 Decision 4, WP-5), prepared here before anything keeps it: recognised by
//! its magic bytes (PNG, JPEG, WebP, GIF), at most [`MAX_IMAGE_BYTES`], decoded with limits (a pixel bomb is refused before
//! its pixels are allocated), turned upright by its EXIF orientation, downscaled to the model's pixel budget (its projector's
//! `max_image_pixels`, served as `LocalModel::image_pixels`) and re-encoded, which leaves its metadata (EXIF, GPS, colour
//! profiles, comments) behind. A lossless source (PNG, GIF, WebP with transparency) stays PNG; a photo (JPEG, WebP without
//! transparency) becomes JPEG. An animated GIF keeps its first frame.
//!
//! Pure and blocking: the caller runs it on a bounded worker (`commands::IMAGE_WORKERS`), never on a command thread. Never
//! panics on any input; a refusal says why and what to do.

use super::attach::Refused;
use base64::Engine as _;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use pitwall_core::chat::ImageMime;
use pitwall_core::gguf::FALLBACK_IMAGE_PIXELS;
use std::io::Cursor;

/// Largest image file the chat takes (ADR 0023 Decision 4: 25 MiB per source file, judgement).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Widest or tallest image decoded, in pixels (a larger one is refused before decoding).
pub const MAX_SIDE: u32 = 16_384;
/// Most memory one decode may allocate (a 16-megapixel RGBA image is 64 MiB; this leaves room for 48 megapixels in RGB).
pub const MAX_DECODE_BYTES: u64 = 192 * 1024 * 1024;
/// Most pixels a prepared image keeps, whatever a projector's header claims (2048 × 2048).
pub const MAX_PREPARED_PIXELS: u64 = 2048 * 2048;
/// Largest prepared image kept and sent (a re-encoded image over it is refused in words).
pub const MAX_PREPARED_BYTES: usize = 12 * 1024 * 1024;
/// The thumbnail's longest side, in pixels (twice the chip's 48 px, for sharp high-density screens).
pub const THUMB_SIDE: u32 = 96;
/// JPEG quality of a prepared photo.
const JPEG_QUALITY: u8 = 90;
/// JPEG quality of a thumbnail.
const THUMB_QUALITY: u8 = 80;

/// An image ready to keep and send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    /// Its encoding after preparation.
    pub mime: ImageMime,
    /// The re-encoded bytes (no metadata).
    pub bytes: Vec<u8>,
    /// Its width in pixels, as kept.
    pub width: u32,
    /// Its height in pixels, as kept.
    pub height: u32,
    /// The upright source's size when it was downscaled; `None` when it was kept at its size.
    pub original: Option<(u32, u32)>,
    /// A small preview as a `data:` URL (the CSP allows `img-src data:`).
    pub thumbnail: String,
}

/// The image format `bytes` start with, by magic bytes; `None` when they are not one the engine reads.
pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else {
        None
    }
}

fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        _ => "image",
    }
}

/// The pixel budget used for `budget` (a model's `image_pixels`): the fallback when unknown (0), never above
/// [`MAX_PREPARED_PIXELS`].
pub fn effective_budget(budget: u64) -> u64 {
    let b = if budget == 0 { FALLBACK_IMAGE_PIXELS } else { budget };
    b.min(MAX_PREPARED_PIXELS)
}

/// The largest size with the aspect of `w` × `h` within `budget` pixels (at least 1 × 1); the size itself when it fits.
pub fn fit(w: u32, h: u32, budget: u64) -> (u32, u32) {
    let (w64, h64) = (u64::from(w.max(1)), u64::from(h.max(1)));
    if w64 * h64 <= budget.max(1) {
        return (w.max(1), h.max(1));
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = |side: u64, scale: f64| -> u32 { ((side as f64 * scale).floor() as u32).max(1) };
    #[allow(clippy::cast_precision_loss)]
    let mut scale = (budget.max(1) as f64 / (w64 * h64) as f64).sqrt();
    // Floating point may land one pixel over: step down until the product fits.
    loop {
        let (nw, nh) = (scaled(w64, scale), scaled(h64, scale));
        if u64::from(nw) * u64::from(nh) <= budget.max(1) || (nw == 1 && nh == 1) {
            return (nw, nh);
        }
        scale *= 0.999;
    }
}

/// The image tokens a prepared `w` × `h` image costs a model whose budget is `budget` pixels: the budget is
/// `MAX_IMAGE_TOKENS` tokens, so a smaller image costs proportionally fewer (rounded up; at least 1).
pub fn image_tokens(w: u32, h: u32, budget: u64) -> u64 {
    let pixels = u64::from(w) * u64::from(h);
    let per_token = (effective_budget(budget) / pitwall_core::gguf::MAX_IMAGE_TOKENS).max(1);
    pixels.div_ceil(per_token).max(1)
}

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_SIDE);
    l.max_image_height = Some(MAX_SIDE);
    l.max_alloc = Some(MAX_DECODE_BYTES);
    l
}

fn decode(name: &str, bytes: &[u8], format: ImageFormat) -> Result<DynamicImage, Refused> {
    let kind = format_name(format);
    let damaged = || {
        Refused(format!(
            "\"{name}\" can't be read as a {kind} image; it may be damaged. Attach another copy."
        ))
    };
    let too_big = |w: u32, h: u32| {
        Refused(format!(
            "\"{name}\" is too large in pixels ({w} × {h}). Attach an image at most {MAX_SIDE} pixels wide and tall, or a smaller copy."
        ))
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(|e| match e {
        image::ImageError::Limits(_) => Refused(format!(
            "\"{name}\" is too large in pixels. Attach an image at most {MAX_SIDE} pixels wide and tall, or a smaller copy."
        )),
        _ => damaged(),
    })?;
    let (w, h) = decoder.dimensions();
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(too_big(w, h));
    }
    if w == 0 || h == 0 {
        return Err(damaged());
    }
    // Upright first: the orientation lives in the metadata the re-encode leaves behind.
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| match e {
        image::ImageError::Limits(_) => too_big(w, h),
        _ => damaged(),
    })?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Whether any pixel of `img` is not fully opaque.
fn translucent(img: &DynamicImage) -> bool {
    if !img.color().has_alpha() {
        return false;
    }
    img.to_rgba8().pixels().any(|p| p.0[3] != u8::MAX)
}

fn encode(img: &DynamicImage, as_png: bool, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    if as_png {
        img.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    } else {
        // JPEG has no alpha: an opaque image is written as RGB.
        let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        rgb.write_with_encoder(enc).map_err(|e| e.to_string())?;
    }
    Ok(out.into_inner())
}

/// Prepares the image `bytes` (named `raw_name` by the webview) for a model whose budget is `budget` pixels (0: unknown).
/// `None` when the bytes are not an image the chat reads (the caller treats them as text).
pub fn prepare_image(name: &str, bytes: &[u8], budget: u64) -> Option<Result<PreparedImage, Refused>> {
    let format = sniff(bytes)?;
    Some(prepare_known(name, bytes, format, budget))
}

fn prepare_known(name: &str, bytes: &[u8], format: ImageFormat, budget: u64) -> Result<PreparedImage, Refused> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(Refused(format!(
            "\"{name}\" is larger than {} MiB. Attach a smaller image.",
            MAX_IMAGE_BYTES >> 20
        )));
    }
    let img = decode(name, bytes, format)?;
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = fit(w, h, effective_budget(budget));
    let img = if (nw, nh) == (w, h) {
        img
    } else {
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    };
    let as_png = match format {
        ImageFormat::Png | ImageFormat::Gif => true,
        _ => translucent(&img),
    };
    let failed = |e: String| {
        log::warn!(target: "pitwall_chat", "attach: an image could not be re-encoded ({e})");
        Refused(format!(
            "\"{name}\" could not be prepared for the model. Attach another copy, or a PNG or JPEG."
        ))
    };
    let out = encode(&img, as_png, JPEG_QUALITY).map_err(failed)?;
    if out.len() > MAX_PREPARED_BYTES {
        return Err(Refused(format!(
            "\"{name}\" is still larger than {} MiB after it is made smaller for the model. Attach a JPEG copy, or a simpler image.",
            MAX_PREPARED_BYTES >> 20
        )));
    }
    let thumb = img.thumbnail(THUMB_SIDE, THUMB_SIDE);
    let thumb_png = as_png && translucent(&thumb);
    let thumb_bytes = encode(&thumb, thumb_png, THUMB_QUALITY).map_err(failed)?;
    let thumb_mime = if thumb_png { "image/png" } else { "image/jpeg" };
    Ok(PreparedImage {
        mime: if as_png { ImageMime::Png } else { ImageMime::Jpeg },
        bytes: out,
        width: nw,
        height: nh,
        original: ((nw, nh) != (w, h)).then_some((w, h)),
        thumbnail: format!(
            "data:{thumb_mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(thumb_bytes)
        ),
    })
}

/// A kept prepared image made to fit a smaller `budget` (the chat's model changed to one with a smaller projector after it was
/// attached): decoded and downscaled again in memory for this request only. The bytes as they are when they already fit or
/// cannot be read (the server checks them again).
pub fn refit(mime: ImageMime, bytes: Vec<u8>, w: u32, h: u32, budget: u64) -> (ImageMime, Vec<u8>) {
    if u64::from(w) * u64::from(h) <= effective_budget(budget) {
        return (mime, bytes);
    }
    let format = match mime {
        ImageMime::Png => ImageFormat::Png,
        ImageMime::Jpeg => ImageFormat::Jpeg,
        ImageMime::Webp => ImageFormat::WebP,
        ImageMime::Gif => ImageFormat::Gif,
    };
    match prepare_known("image", &bytes, format, budget) {
        Ok(p) => (p.mime, p.bytes),
        Err(_) => (mime, bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};
    use proptest::prelude::*;

    fn png(img: &DynamicImage) -> Vec<u8> {
        encode(img, true, 0).unwrap()
    }

    fn jpeg(img: &DynamicImage) -> Vec<u8> {
        encode(img, false, 95).unwrap()
    }

    fn gradient(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 128])))
    }

    fn decoded(p: &PreparedImage) -> DynamicImage {
        image::load_from_memory(&p.bytes).unwrap()
    }

    /// A tiny CRC-32 (PNG chunks), so a test can build a header that lies about its size.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for b in bytes {
            c ^= u32::from(*b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    /// A valid one-pixel PNG whose header then claims `w` × `h` (a pixel bomb: tiny file, huge image).
    fn lying_png(w: u32, h: u32) -> Vec<u8> {
        let mut b = png(&gradient(1, 1));
        // The IHDR chunk follows the 8-byte signature: length (4), type (4), width (4), height (4), ... and its CRC.
        b[16..20].copy_from_slice(&w.to_be_bytes());
        b[20..24].copy_from_slice(&h.to_be_bytes());
        let crc = crc32(&b[12..29]);
        b[29..33].copy_from_slice(&crc.to_be_bytes());
        b
    }

    /// A JPEG with an EXIF block naming `orientation` (and a camera model, to see the metadata go).
    fn jpeg_with_exif(img: &DynamicImage, orientation: u16) -> Vec<u8> {
        let plain = jpeg(img);
        let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
        tiff.extend(2u16.to_be_bytes());
        // Orientation: SHORT, 1 value, inline.
        tiff.extend(0x0112u16.to_be_bytes());
        tiff.extend(3u16.to_be_bytes());
        tiff.extend(1u32.to_be_bytes());
        tiff.extend(orientation.to_be_bytes());
        tiff.extend([0, 0]);
        // Model: ASCII, 4 bytes inline ("Cam\0").
        tiff.extend(0x0110u16.to_be_bytes());
        tiff.extend(2u16.to_be_bytes());
        tiff.extend(4u32.to_be_bytes());
        tiff.extend(b"Cam\0");
        tiff.extend(0u32.to_be_bytes());
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(tiff);
        let mut out = plain[..2].to_vec();
        out.extend([0xff, 0xe1]);
        out.extend(u16::try_from(app1.len() + 2).unwrap().to_be_bytes());
        out.extend(app1);
        out.extend(&plain[2..]);
        out
    }

    #[test]
    fn the_four_formats_are_recognised_by_their_magic_bytes_only() {
        let img = gradient(4, 3);
        assert_eq!(sniff(&png(&img)), Some(ImageFormat::Png));
        assert_eq!(sniff(&jpeg(&img)), Some(ImageFormat::Jpeg));
        assert_eq!(sniff(b"GIF89a...."), Some(ImageFormat::Gif));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageFormat::WebP));
        for not in [&b"BM\0\0"[..], b"II*\0", b"plain text", b"", b"RIFF\0\0\0\0WAVE"] {
            assert_eq!(sniff(not), None, "{not:?}");
            assert!(prepare_image("x", not, 0).is_none());
        }
    }

    #[test]
    fn a_large_image_is_downscaled_to_the_models_budget_keeping_its_aspect() {
        let budget = 1024 * 32 * 32;
        let p = prepare_image("photo.png", &png(&gradient(4000, 3000)), budget).unwrap().unwrap();
        assert!(u64::from(p.width) * u64::from(p.height) <= budget, "{} × {}", p.width, p.height);
        assert!(u64::from(p.width) * u64::from(p.height) > budget * 98 / 100, "close to the budget");
        let aspect = f64::from(p.width) / f64::from(p.height);
        assert!((aspect - 4.0 / 3.0).abs() < 0.01, "{aspect}");
        assert_eq!(p.original, Some((4000, 3000)));
        assert_eq!((decoded(&p).width(), decoded(&p).height()), (p.width, p.height));
        // An image within the budget keeps its size.
        let small = prepare_image("s.png", &png(&gradient(64, 48)), budget).unwrap().unwrap();
        assert_eq!((small.width, small.height, small.original), (64, 48, None));
        // An unknown budget is the fallback; a projector's huge claim is capped.
        assert_eq!(effective_budget(0), FALLBACK_IMAGE_PIXELS);
        assert_eq!(effective_budget(u64::MAX), MAX_PREPARED_PIXELS);
    }

    #[test]
    fn fit_never_exceeds_the_budget_and_keeps_at_least_one_pixel() {
        assert_eq!(fit(100, 50, 10_000), (100, 50));
        assert_eq!(fit(100, 50, 1250), (50, 25));
        assert_eq!(fit(100_000, 1, 10), (10, 1));
        assert_eq!(fit(1, 100_000, 10), (1, 10));
        assert_eq!(fit(0, 0, 0), (1, 1));
        assert_eq!(fit(16_384, 16_384, 1), (1, 1));
        for (w, h, b) in [(4032, 3024, 802_816), (1366, 768, 1_048_576), (7, 13_000, 4096), (16_384, 3, 999)] {
            let (nw, nh) = fit(w, h, b);
            assert!(u64::from(nw) * u64::from(nh) <= b.max(1), "{w}×{h} in {b}: {nw}×{nh}");
        }
    }

    #[test]
    fn a_photo_stays_jpeg_a_lossless_source_stays_png_and_transparency_is_kept() {
        let photo = prepare_image("p.jpg", &jpeg(&gradient(40, 30)), 0).unwrap().unwrap();
        assert_eq!(photo.mime, ImageMime::Jpeg);
        let shot = prepare_image("s.png", &png(&gradient(40, 30)), 0).unwrap().unwrap();
        assert_eq!(shot.mime, ImageMime::Png);
        let clear = DynamicImage::ImageRgba8(RgbaImage::from_fn(8, 8, |x, _| Rgba([0, 0, 0, if x < 4 { 0 } else { 255 }])));
        let mut webp = Cursor::new(Vec::new());
        clear.write_to(&mut webp, ImageFormat::WebP).unwrap();
        let p = prepare_image("c.webp", webp.get_ref(), 0).unwrap().unwrap();
        assert_eq!(p.mime, ImageMime::Png, "a translucent WebP keeps its transparency as PNG");
        assert_eq!(decoded(&p).to_rgba8().get_pixel(0, 0).0[3], 0);
        let gif = {
            let mut c = Cursor::new(Vec::new());
            gradient(5, 5).write_to(&mut c, ImageFormat::Gif).unwrap();
            c.into_inner()
        };
        assert_eq!(prepare_image("a.gif", &gif, 0).unwrap().unwrap().mime, ImageMime::Png);
        assert!(photo.thumbnail.starts_with("data:image/jpeg;base64,"), "{}", &photo.thumbnail[..30]);
    }

    #[test]
    fn metadata_is_left_behind_after_the_image_is_turned_upright() {
        // 40 × 20 stored, orientation 6 (rotate 90° clockwise to view): the kept image is 20 × 40.
        let src = jpeg_with_exif(&gradient(40, 20), 6);
        assert!(src.windows(4).any(|w| w == b"Exif") && src.windows(3).any(|w| w == b"Cam"));
        let p = prepare_image("phone.jpg", &src, 0).unwrap().unwrap();
        assert_eq!((p.width, p.height), (20, 40));
        assert!(!p.bytes.windows(4).any(|w| w == b"Exif"), "no EXIF left");
        assert!(!p.bytes.windows(3).any(|w| w == b"Cam"), "no camera model left");
        let png_out = prepare_image("s.png", &png(&gradient(10, 10)), 0).unwrap().unwrap();
        for chunk in [&b"eXIf"[..], b"tEXt", b"iTXt", b"zTXt", b"iCCP"] {
            assert!(!png_out.bytes.windows(4).any(|w| w == chunk), "{chunk:?}");
        }
    }

    #[test]
    fn a_pixel_bomb_and_a_damaged_file_are_refused_in_words() {
        for (w, h) in [(100_000, 100_000), (MAX_SIDE + 1, 10), (10, MAX_SIDE + 1)] {
            let words = prepare_image("bomb.png", &lying_png(w, h), 0).unwrap().unwrap_err().0;
            assert!(words.contains("bomb.png") && words.contains("too large in pixels"), "{words}");
        }
        // Within the side limit but past the decode memory bound: refused, not allocated.
        let words = prepare_image("big.png", &lying_png(MAX_SIDE, MAX_SIDE), 0).unwrap().unwrap_err().0;
        assert!(words.contains("too large in pixels") || words.contains("damaged"), "{words}");
        let mut cut = png(&gradient(64, 64));
        cut.truncate(cut.len() / 2);
        let words = prepare_image("cut.png", &cut, 0).unwrap().unwrap_err().0;
        assert!(
            words.contains("can't be read as a PNG image") && words.contains("damaged"),
            "{words}"
        );
        let words = prepare_image("x.jpg", &[0xff, 0xd8, 0xff, 0xe0, 1, 2], 0).unwrap().unwrap_err().0;
        assert!(words.contains("JPEG"), "{words}");
    }

    #[test]
    fn a_file_over_the_size_limit_is_refused_before_decoding() {
        let mut big = png(&gradient(2, 2));
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        let words = prepare_image("huge.png", &big, 0).unwrap().unwrap_err().0;
        assert!(words.contains("huge.png") && words.contains("25 MiB"), "{words}");
    }

    #[test]
    fn tokens_scale_with_the_pixels_kept() {
        let budget = 1024 * 28 * 28;
        assert_eq!(image_tokens(896, 896, budget), 1024);
        assert_eq!(image_tokens(448, 448, budget), 256);
        assert_eq!(image_tokens(1, 1, budget), 1);
        assert_eq!(image_tokens(896, 896, 0), 1024, "an unknown budget is the fallback");
    }

    #[test]
    fn a_kept_image_is_refitted_only_when_the_new_budget_is_smaller() {
        let p = prepare_image("s.png", &png(&gradient(200, 100)), 0).unwrap().unwrap();
        let (mime, same) = refit(p.mime, p.bytes.clone(), p.width, p.height, 1_000_000);
        assert_eq!((mime, same.as_slice()), (ImageMime::Png, p.bytes.as_slice()));
        let (_, smaller) = refit(p.mime, p.bytes.clone(), p.width, p.height, 5000);
        let img = image::load_from_memory(&smaller).unwrap();
        assert!(u64::from(img.width()) * u64::from(img.height()) <= 5000);
        let (_, junk) = refit(ImageMime::Png, b"junk".to_vec(), 4000, 4000, 10);
        assert_eq!(junk, b"junk", "unreadable bytes go as they are; the server checks them");
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]
        /// Any bytes behind any of the four magic numbers: prepared or refused, never a panic (ADR 0023 "Bad files").
        #[test]
        fn prepare_never_panics(head in 0usize..5, tail in proptest::collection::vec(any::<u8>(), 0..2048), budget in any::<u64>()) {
            let heads: [&[u8]; 5] = [b"\x89PNG\r\n\x1a\n", &[0xff, 0xd8, 0xff], b"GIF89a", b"RIFF\0\0\0\0WEBP", b""];
            let mut bytes = heads[head].to_vec();
            bytes.extend(tail);
            let _ = prepare_image("p", &bytes, budget);
        }

        /// A valid PNG header with any claimed size and damaged data after it: never a panic, never a huge allocation.
        #[test]
        fn lying_headers_never_panic(w in any::<u32>(), h in any::<u32>()) {
            let _ = prepare_image("p", &lying_png(w, h), 0);
        }
    }
}
