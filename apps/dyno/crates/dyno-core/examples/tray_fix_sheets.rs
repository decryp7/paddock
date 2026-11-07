//! Contact sheets of the pixel tray renderer for review: `cargo run -p dyno-core --example tray_fix_sheets -- <out dir> <tag>`.
//! Every size, light and dark, values 0, 7, 17, 33, 88 and 100, with and without alert, enlarged 8x (nearest neighbour).
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]
use dyno_core::tray::pixel::{render, Kind, Reading, State, Theme};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (out, tag) = (&args[1], &args[2]);
    let sizes = [16, 20, 22, 24, 32];
    let values = [0.0f32, 7.0, 17.0, 33.0, 88.0, 100.0];
    let z = 8usize;
    for dark in [false, true] {
        let bg: [u8; 4] = if dark { [32, 32, 32, 255] } else { [243, 243, 243, 255] };
        let cell = 34 * z;
        let (w, h) = (values.len() * 2 * cell, sizes.len() * cell);
        let mut img = vec![0u8; w * h * 4];
        for p in img.chunks_mut(4) {
            p.copy_from_slice(&bg);
        }
        for (row, &size) in sizes.iter().enumerate() {
            for (ci, &v) in values.iter().enumerate() {
                for (alert, st) in [(0, State::Normal), (1, State::Alert)] {
                    let r = Reading {
                        value: Some(v),
                        ..Reading::default()
                    };
                    let ic = render(&Kind::Percent { label: "CPU".into() }, &r, st, size, Theme { dark });
                    let (ox, oy) = ((ci * 2 + alert) * cell + z, row * cell + z);
                    for y in 0..ic.size as usize {
                        for x in 0..ic.size as usize {
                            let k = (y * ic.size as usize + x) * 4;
                            let a = f32::from(ic.rgba[k + 3]) / 255.0;
                            let px: Vec<u8> = (0..3)
                                .map(|c| (f32::from(ic.rgba[k + c]) * a + f32::from(bg[c]) * (1.0 - a)) as u8)
                                .collect();
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
        }
        let pm = tiny_skia::PixmapRef::from_bytes(&img, w as u32, h as u32).unwrap();
        pm.save_png(format!("{out}/{tag}-{}.png", if dark { "dark" } else { "light" }))
            .unwrap();
    }
}
