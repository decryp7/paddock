//! Review sheets for the "label over value" number item: `cargo run -p dyno-core --example tray_label_sheets -- <out dir> <tag>`.
//! Windows sizes (16, 20, 24, 32 px; light and dark taskbar; 54 % and 100 %, normal and alert), enlarged 8x with nearest
//! neighbour, and on macOS the stacked and one-line number at 1x and @2x on a light and a dark bar.
#![allow(
    unsafe_code,
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::cast_possible_truncation
)]
use dyno_core::tray::pixel::{render, Kind, Reading, State, Theme};

#[cfg(target_os = "macos")]
#[path = "../../../app/src-tauri/src/tray_text.rs"]
mod coretext;

fn blend(bg: [u8; 4], rgba: &[u8], k: usize) -> [u8; 3] {
    let a = f32::from(rgba[k + 3]) / 255.0;
    let mut o = [0u8; 3];
    for (c, v) in o.iter_mut().enumerate() {
        *v = (f32::from(rgba[k + c]) * a + f32::from(bg[c]) * (1.0 - a)) as u8;
    }
    o
}

fn windows(out: &str, tag: &str) {
    let sizes = [16, 20, 24, 32];
    let cases = [(54.0f32, State::Normal), (100.0, State::Normal), (93.0, State::Alert)];
    let z = 8usize;
    let cell = 36 * z;
    for dark in [false, true] {
        let bg: [u8; 4] = if dark { [32, 32, 32, 255] } else { [243, 243, 243, 255] };
        let (w, h) = (cases.len() * cell, sizes.len() * cell);
        let mut img = vec![0u8; w * h * 4];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&bg);
        }
        for (row, &size) in sizes.iter().enumerate() {
            for (ci, &(v, st)) in cases.iter().enumerate() {
                let r = Reading {
                    value: Some(v),
                    ..Reading::default()
                };
                let ic = render(&Kind::Percent { label: "CPU".into() }, &r, st, size, Theme { dark });
                let (ox, oy) = (ci * cell + z, row * cell + z);
                for y in 0..ic.size as usize {
                    for x in 0..ic.size as usize {
                        let px = blend(bg, &ic.rgba, (y * ic.size as usize + x) * 4);
                        for dy in 0..z {
                            for dx in 0..z {
                                let i = ((oy + y * z + dy) * w + ox + x * z + dx) * 4;
                                img[i..i + 3].copy_from_slice(&px);
                            }
                        }
                    }
                }
            }
        }
        let pm = tiny_skia::PixmapRef::from_bytes(&img, w as u32, h as u32).unwrap();
        pm.save_png(format!("{out}/{tag}-windows-{}.png", if dark { "dark" } else { "light" }))
            .unwrap();
    }
}

#[cfg(target_os = "macos")]
fn mac(out: &str, tag: &str) {
    use dyno_core::tray::menu_bar as mb;
    let ct = coretext::CoreText::new();
    let z = 3usize;
    for dark in [false, true] {
        let bg: [u8; 4] = if dark { [30, 30, 30, 255] } else { [236, 236, 236, 255] };
        let ink: u8 = if dark { 255 } else { 0 };
        let kinds = [
            mb::Kind::Stacked {
                label: "CPU".into(),
                unit: mb::Unit::Percent,
            },
            mb::Kind::Value {
                label: "CPU".into(),
                unit: mb::Unit::Percent,
            },
        ];
        let (cw, ch) = (80 * z * 2, 40 * z);
        let (w, h) = (cw * kinds.len(), ch * 2);
        let mut img = vec![0u8; w * h * 4];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&bg);
        }
        for (ki, kind) in kinds.iter().enumerate() {
            for (si, scale) in [1u32, 2].into_iter().enumerate() {
                let r = mb::Reading {
                    value: Some(54.0),
                    ..mb::Reading::default()
                };
                let t = mb::render(kind, &r, mb::State::Normal, &ct, scale);
                // Enlarge to the same on-screen size: z * 2 / scale per template pixel.
                let f = z * 2 / scale as usize;
                for y in 0..t.height as usize {
                    for x in 0..t.width as usize {
                        let a = f32::from(t.alpha[y * t.width as usize + x]) / 255.0;
                        let px = (f32::from(ink) * a + f32::from(bg[0]) * (1.0 - a)) as u8;
                        for dy in 0..f {
                            for dx in 0..f {
                                let (px_x, px_y) = (ki * cw + 10 + x * f + dx, si * ch + 6 + y * f + dy);
                                if px_x < w && px_y < h {
                                    let i = (px_y * w + px_x) * 4;
                                    img[i..i + 3].copy_from_slice(&[px, px, px]);
                                }
                            }
                        }
                    }
                }
            }
        }
        let pm = tiny_skia::PixmapRef::from_bytes(&img, w as u32, h as u32).unwrap();
        pm.save_png(format!("{out}/{tag}-macos-{}.png", if dark { "dark" } else { "light" }))
            .unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (out, tag) = (&args[1], &args[2]);
    windows(out, tag);
    #[cfg(target_os = "macos")]
    mac(out, tag);
}
