//! A picture's header is read under a small allocation limit (ADR 0023 WP-5, security review): a PNG's iCCP colour profile is
//! inflated while the header is read, so a file of a few hundred kilobytes can ask for gigabytes before any size check. This test
//! binary counts every allocation of the process and holds the whole preparation of such files to a few tens of megabytes.
//! It is its own binary so that its counting allocator sees nothing else.
#![cfg(feature = "chat")]
// A counting allocator is `unsafe impl GlobalAlloc` by definition: this test binary is the one place that needs it.
#![allow(unsafe_code, clippy::unwrap_used, clippy::expect_used)]

use pitwall_app_lib::chat_host::pictures::{prepare_image, DEFAULT_MAX_PIXELS};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator and only counts the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            PEAK.fetch_max(LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = System.realloc(p, l, new);
        if !q.is_null() {
            if new >= l.size() {
                PEAK.fetch_max(
                    LIVE.fetch_add(new - l.size(), Ordering::Relaxed) + (new - l.size()),
                    Ordering::Relaxed,
                );
            } else {
                LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 == 1 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = kind.to_vec();
    body.extend(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
    out
}

/// A small, valid 8 x 8 PNG with a chunk of `kind` holding `data` after its header; the profile is `mib` MiB of one byte.
fn png_with_profile(mib: usize) -> Vec<u8> {
    let zeros = vec![0x41u8; mib << 20];
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&zeros, 9);
    let mut iccp = b"x\0\0".to_vec();
    iccp.extend(z);
    let mut ihdr = 8u32.to_be_bytes().to_vec();
    ihdr.extend(8u32.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    // The pixels: 8 rows of a filter byte and 32 bytes, deflated.
    let pixels = miniz_oxide::deflate::compress_to_vec_zlib(&[0u8; 8 * 33], 6);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"iCCP", &iccp));
    out.extend(chunk(b"IDAT", &pixels));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// A 16-bit RGBA PNG of `w` x `h` pixels (8 bytes a pixel) with an eXIf chunk that says "rotate 90 degrees".
fn rotated_16_bit_png(w: u32, h: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let raw = vec![0x40u8; (w * h * 8) as usize];
    let mut plain = Vec::new();
    image::codecs::png::PngEncoder::new(&mut plain)
        .write_image(&raw, w, h, image::ExtendedColorType::Rgba16)
        .unwrap();
    // TIFF (big endian), one entry: Orientation (0x0112), SHORT, 1 value, 6.
    let exif = b"MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
    let mut out = plain[..33].to_vec();
    out.extend(chunk(b"eXIf", exif));
    out.extend(&plain[33..]);
    out
}

/// Runs `f` and returns what it returned and the most memory it held at once beyond what was held before it.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let out = f();
    (out, PEAK.load(Ordering::Relaxed).saturating_sub(before))
}

// One test, so nothing else allocates while a peak is measured.
#[test]
fn preparing_an_image_holds_bounded_memory() {
    // A colour profile that inflates to hundreds of megabytes is met within a bounded memory.
    let file = png_with_profile(256);
    assert!(file.len() < 2 * 1024 * 1024, "the file is small: {} bytes", file.len());
    // Refused or read without its profile: either way the memory is bounded (the decoder drops what it may not inflate).
    let (_, peak) = peak_of(|| prepare_image("profile.png", &file, DEFAULT_MAX_PIXELS));
    assert!(peak < 64 * 1024 * 1024, "profile: peak memory {} MiB", peak >> 20);
    // A reasonable profile (1 MiB) is still fine.
    assert!(prepare_image("ok.png", &png_with_profile(1), DEFAULT_MAX_PIXELS).is_ok());

    // A 12 megapixel 16-bit photo that must be turned: its decoded pixels (96 MB) are held once with the decoder's working copy, never a turned or 8-bit copy of them.
    let photo = rotated_16_bit_png(4000, 3000);
    let (result, peak) = peak_of(|| prepare_image("photo.png", &photo, DEFAULT_MAX_PIXELS));
    let prepared = result.unwrap();
    assert!(
        prepared.shrunk && prepared.height > prepared.width,
        "turned upright: {}x{}",
        prepared.width,
        prepared.height
    );
    assert!(
        peak < 160 * 1024 * 1024,
        "photo: peak memory {} MiB for 96 MB of pixels",
        peak >> 20
    );
}
