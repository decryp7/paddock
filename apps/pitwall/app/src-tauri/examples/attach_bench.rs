//! Measures what an image attachment costs in the app process (ADR 0023 WP-5; results in `docs/performance.md`):
//!
//! - `ipc <MiB>`: the Rust side of three ways to carry a file's bytes from the webview: a JSON list of numbers (Tauri's default
//!   for a `Uint8Array` inside an argument object: the body becomes a `serde_json::Value`, then `Vec<u8>`), a base64 string,
//!   and a raw request body (`InvokeBody::Raw`). Each way runs in its own process (`ipc-one`) so its peak memory is its own.
//! - `prepare`: `chat_host::image::prepare_image` on synthetic photos and screenshots (decode, upright, downscale, re-encode,
//!   preview), and the resize filters compared.
//! - `workers`: eight 12-megapixel photos prepared with 1, 2 and 4 threads (wall time and peak memory).
//!
//! Peak memory is the child process's maximum resident set, read from `/usr/bin/time` (`-l` on macOS, `-v` on Linux).
//!
//! `cargo run -p pitwall-app --example attach_bench -- <ipc 25 | prepare | workers>` (dependencies are optimised in the dev
//! profile; see `Cargo.toml`). Synthetic data only; no file is read or written.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use base64::Engine as _;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use pitwall_app_lib::chat_host::image::prepare_image;
use std::io::Cursor;
use std::time::{Duration, Instant};

fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s as u8
        })
        .collect()
}

/// Runs this program with `args` under `/usr/bin/time` and returns its output line with the child's peak memory appended
/// (MiB; macOS `-l` reports bytes, Linux `-v` kibibytes; "?" elsewhere).
fn child_with_peak(args: &[&str]) -> String {
    let me = std::env::current_exe().unwrap();
    let flag = if cfg!(target_os = "macos") { "-l" } else { "-v" };
    let out = std::process::Command::new("/usr/bin/time").arg(flag).arg(&me).args(args).output();
    let Ok(out) = out else {
        return "?\n".into();
    };
    let err = String::from_utf8_lossy(&out.stderr);
    let peak = err.lines().find_map(|l| {
        let l = l.trim();
        if let Some(v) = l.strip_suffix("maximum resident set size") {
            v.trim().parse::<f64>().ok().map(|b| b / 1048576.0)
        } else {
            l.strip_prefix("Maximum resident set size (kbytes):")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .map(|k| k / 1024.0)
        }
    });
    format!(
        "{},{}\n",
        String::from_utf8_lossy(&out.stdout).trim(),
        peak.map_or("?".into(), |p| format!("{p:.0}"))
    )
}

fn median(mut v: Vec<Duration>) -> f64 {
    v.sort();
    v[v.len() / 2].as_secs_f64() * 1000.0
}

/// One way, in this process: prints `way,MiB,body bytes,ms (median of 5),rss MiB after`.
fn ipc_one(way: &str, mib: usize) {
    let bytes = noise(mib << 20, 7);
    let body: Vec<u8> = match way {
        "json" => {
            // Written as text directly (a Value here would put its own peak on this process).
            let mut b = br#"{"name":"photo.png","bytes":["#.to_vec();
            for (i, x) in bytes.iter().enumerate() {
                if i > 0 {
                    b.push(b',');
                }
                b.extend(x.to_string().as_bytes());
            }
            b.extend(b"]}");
            b
        }
        "base64" => serde_json::to_vec(&serde_json::json!({
            "name": "photo.png",
            "data": base64::engine::general_purpose::STANDARD.encode(&bytes)
        }))
        .unwrap(),
        _ => bytes.clone(),
    };
    drop(bytes);
    let mut times = vec![];
    let mut kept = 0usize;
    for _ in 0..5 {
        let started = Instant::now();
        let got: Vec<u8> = match way {
            "json" => {
                // What Tauri does with a JSON body: a Value first, then the command's argument.
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                serde_json::from_value(v["bytes"].clone()).unwrap()
            }
            "base64" => {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                base64::engine::general_purpose::STANDARD
                    .decode(v["data"].as_str().unwrap())
                    .unwrap()
            }
            _ => body.clone(),
        };
        times.push(started.elapsed());
        kept = got.len();
    }
    println!("{way},{mib},{},{:.1},{kept}", body.len(), median(times));
}

fn ipc(mib: usize) {
    println!("way,MiB,body bytes,ms (median of 5),bytes out,peak MiB");
    for way in ["json", "base64", "raw"] {
        print!("{}", child_with_peak(&["ipc-one", way, &mib.to_string()]));
    }
}

fn photo(w: u32, h: u32) -> Vec<u8> {
    let n = noise((w * h) as usize, 3);
    let img = RgbImage::from_fn(w, h, |x, y| {
        let r = n[(y * w + x) as usize] / 8;
        Rgb([(x * 255 / w) as u8 + r, (y * 255 / h) as u8 + r, 120 + r])
    });
    let mut out = Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    DynamicImage::ImageRgb8(img).write_with_encoder(enc).unwrap();
    out.into_inner()
}

fn screenshot(w: u32, h: u32) -> Vec<u8> {
    // Flat panels with thin "text" lines: compresses like a real screenshot.
    let img = RgbImage::from_fn(w, h, |x, y| {
        let panel = if x < w / 5 { 236 } else { 250 };
        let text = (y % 22 < 12) && (x % 9 < 6) && (y / 22) % 3 != 0 && x % 400 < 300;
        let v = if text { 40 } else { panel };
        Rgb([v, v, v.saturating_add(4)])
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn noisy_png(side: u32) -> Vec<u8> {
    let n = noise((side * side * 3) as usize, 11);
    let img = RgbImage::from_raw(side, side, n).unwrap();
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn prepare() {
    let budgets = [
        ("802816 px (28 px tokens)", 1024 * 28 * 28),
        ("1048576 px (32 px tokens)", 1024 * 32 * 32),
    ];
    let cases = [
        ("12 MP photo, JPEG 4032x3024", photo(4032, 3024)),
        ("4K screenshot, PNG 3840x2160", screenshot(3840, 2160)),
        ("noise PNG 2900x2900 (worst case, near 25 MiB)", noisy_png(2900)),
    ];
    println!("source,source bytes,budget,kept,kept bytes,ms (median of 5)");
    for (name, bytes) in &cases {
        for (bname, budget) in budgets {
            let mut times = vec![];
            let mut last = None;
            for _ in 0..5 {
                let started = Instant::now();
                let p = prepare_image("x", bytes, budget).unwrap().unwrap();
                times.push(started.elapsed());
                last = Some(p);
            }
            let p = last.unwrap();
            println!(
                "{name},{},{bname},{}x{},{},{:.0}",
                bytes.len(),
                p.width,
                p.height,
                p.bytes.len(),
                median(times)
            );
        }
    }
    println!("\nfilter,decoded 4032x3024 -> 1034x775,ms (median of 5)");
    let img = image::load_from_memory(&cases[0].1).unwrap();
    for (fname, f) in [
        ("Nearest", image::imageops::FilterType::Nearest),
        ("Triangle", image::imageops::FilterType::Triangle),
        ("CatmullRom", image::imageops::FilterType::CatmullRom),
        ("Lanczos3", image::imageops::FilterType::Lanczos3),
    ] {
        let mut times = vec![];
        for _ in 0..5 {
            let started = Instant::now();
            let _ = img.resize_exact(1034, 775, f);
            times.push(started.elapsed());
        }
        println!("{fname},,{:.0}", median(times));
    }
    let mut times = vec![];
    for _ in 0..5 {
        let started = Instant::now();
        let _ = image::load_from_memory(&cases[0].1).unwrap();
        times.push(started.elapsed());
    }
    println!("decode only (JPEG 4032x3024),,{:.0}", median(times));
}

fn workers() {
    println!("threads,8 photos wall ms,peak MiB");
    for threads in [1usize, 2, 4] {
        print!("{}", child_with_peak(&["workers-one", &threads.to_string()]));
    }
}

fn workers_one(threads: usize) {
    let src = std::sync::Arc::new(photo(4032, 3024));
    let queue = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let started = Instant::now();
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (src, queue) = (src.clone(), queue.clone());
            std::thread::spawn(move || {
                while queue.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 8 {
                    let _ = prepare_image("x", &src, 1024 * 28 * 28).unwrap().unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    println!("{threads},{:.0}", started.elapsed().as_secs_f64() * 1000.0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n = |i: usize, d: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    match args.first().map(String::as_str) {
        Some("ipc") => ipc(n(1, 25)),
        Some("ipc-one") => ipc_one(args.get(1).map_or("raw", String::as_str), n(2, 25)),
        Some("prepare") => prepare(),
        Some("workers") => workers(),
        Some("workers-one") => workers_one(n(1, 2)),
        _ => eprintln!("usage: attach_bench <ipc <MiB> | prepare | workers>"),
    }
}
