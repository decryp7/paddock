//! The menu-bar styles as the app draws them, for review: every style of the catalogue through the app's own mapping
//! (`items_from_settings`, `px_kind`, `mac_kind`) and renderers, at every tray size, light and dark, normal, alert and
//! unavailable, with what `style_support` says beside each. Writes one PNG per picture and an `index.html` that lays them out
//! (8x, nearest neighbour, and 1x):
//!
//! `cargo run -p dyno-app --example tray_style_sheet -- <out dir>`
//!
//! On macOS the menu-bar templates are drawn with CoreText (the app's text port); elsewhere that section is left out.

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use dyno_app_lib::tray_look::{px_kind, render_px, Sample};
use dyno_app_lib::tray_model::{items_from_settings, Picture};
use dyno_core::tray::pixel as px;
use dyno_core::widgets::{style_support, MenuBarCatalogue, MenuBarItem, MenuBarKind, Metric, NumberFormat, TrayPlatform};
use paddock_widgets::{assign_spans, Catalogue};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn item(kind: MenuBarKind, metric: Metric) -> MenuBarItem {
    match kind {
        MenuBarKind::LineGraph => MenuBarItem::LineGraph { metric },
        MenuBarKind::VerticalBars => MenuBarItem::VerticalBars { metric },
        MenuBarKind::HorizontalMeter => MenuBarItem::HorizontalMeter { metric },
        MenuBarKind::Ring => MenuBarItem::Ring { metric },
        MenuBarKind::Number => MenuBarItem::Number {
            metric,
            unit: String::new(),
            label: String::new(),
            format: NumberFormat::Stacked,
        },
        MenuBarKind::GraphAndNumber => MenuBarItem::GraphAndNumber { metric },
    }
}

/// A calm, deterministic history with a rise near the end (64 values, oldest first).
fn history() -> Vec<f32> {
    (0..64)
        .map(|i| {
            let t = i as f32;
            (28.0 + 14.0 * (t / 4.0).sin() + 9.0 * (t / 1.7).cos() + if i > 52 { 22.0 } else { 0.0 }).clamp(0.0, 100.0)
        })
        .collect()
}

fn sample(value: Option<f32>) -> Sample {
    Sample {
        value,
        history: history(),
        cores: vec![62.0, 18.0, 41.0, 9.0, 77.0, 23.0, 12.0, 35.0],
        up: 48_000.0,
        down: 2_400_000.0,
    }
}

fn save(dir: &Path, name: &str, p: &Picture, ink: Option<[u8; 3]>) -> String {
    let mut rgba = p.rgba.clone();
    if let Some(ink) = ink {
        for px in rgba.chunks_mut(4) {
            px[..3].copy_from_slice(&ink);
        }
    }
    let pm = tiny_skia::Pixmap::from_vec(
        rgba.chunks(4)
            .flat_map(|c| {
                let a = u16::from(c[3]);
                [
                    (u16::from(c[0]) * a / 255) as u8,
                    (u16::from(c[1]) * a / 255) as u8,
                    (u16::from(c[2]) * a / 255) as u8,
                    c[3],
                ]
            })
            .collect(),
        tiny_skia::IntSize::from_wh(p.width, p.height).unwrap(),
    )
    .unwrap();
    pm.save_png(dir.join(name)).unwrap();
    name.to_string()
}

/// A row's head: the style's title, and the support table's words when they say more ("Ring, drawn as meter").
fn row_head(title: &str, drawn: &str) -> String {
    if drawn == title {
        title.to_string()
    } else {
        format!("{title}<br><span class=note>{drawn}</span>")
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: tray_style_sheet <out dir>"));
    let icons = out.join("icons");
    std::fs::create_dir_all(&icons).unwrap();
    let kinds: Vec<MenuBarKind> = MenuBarCatalogue::kinds().iter().map(|k| k.kind).collect();
    let title = |k: MenuBarKind| MenuBarCatalogue::kinds().iter().find(|s| s.kind == k).unwrap().title;
    let specs = items_from_settings(&assign_spans(kinds.iter().map(|k| (1, item(*k, Metric::CpuTotal)))));
    let win = style_support(TrayPlatform::Windows);
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><title>Dyno tray styles</title><style>\
         body{font:13px/1.4 -apple-system,system-ui,sans-serif;margin:24px;background:#fff;color:#1b1b1f}\
         h1{font-size:18px;margin:0 0 4px}h2{font-size:15px;margin:28px 0 6px}p{margin:0 0 10px;color:#555;max-width:980px}\
         table{border-collapse:collapse}td,th{padding:6px 10px;text-align:center;vertical-align:middle}\
         th{font-weight:600;font-size:12px;color:#444}th.row{text-align:left;width:150px}\
         td.dark{background:#202020}td.light{background:#f3f3f3}td.bar-dark{background:#2b2b2e}td.bar-light{background:#e8e8ea}\
         img{image-rendering:pixelated;display:block;margin:0 auto}.one{margin-top:6px}.note{font-size:11px;color:#777;font-weight:400}\
         </style><h1>Dyno 1.1.1: menu-bar and tray styles</h1>\
         <p>Every style of the catalogue for the CPU, through the app's own mapping and renderers (value 37 %, the same 64-value \
         history for every style). Each cell: 8x nearest-neighbour, then the icon at 1x. Windows taskbar stand-ins: #202020 dark, \
         #F3F3F3 light. Linux uses the 22 px layout.</p>",
    );
    // Windows and Linux: styles by size, light and dark.
    let sizes = [16u32, 20, 22, 24, 32];
    for (state, state_name, value) in [
        (px::State::Normal, "normal", Some(37.0)),
        (px::State::Alert, "alert", Some(93.0)),
        (px::State::Unavailable, "unavailable", None),
    ] {
        let _ = write!(
            html,
            "<h2>Windows and Linux, {state_name}</h2><table><tr><th class=row>Style</th>{}</tr>",
            sizes
                .iter()
                .flat_map(|s| [format!("<th>{s} px dark</th>"), format!("<th>{s} px light</th>")])
                .collect::<String>()
        );
        for (k, spec) in kinds.iter().zip(&specs) {
            let drawn = &win.styles.iter().find(|d| d.kind == *k).unwrap().label;
            let _ = write!(html, "<tr><th class=row>{}</th>", row_head(title(*k), drawn));
            for size in sizes {
                for dark in [true, false] {
                    let look = render_px(&px_kind(spec), &sample(value), state, size, dark);
                    let name = format!("win-{k:?}-{size}-{}-{state_name}.png", if dark { "dark" } else { "light" });
                    let f = save(&icons, &name, &look.pictures[0], None);
                    let _ = write!(
                        html,
                        "<td class={}><img src=\"icons/{f}\" width={} height={}><img class=one src=\"icons/{f}\"></td>",
                        if dark { "dark" } else { "light" },
                        size * 8,
                        size * 8
                    );
                }
            }
            html.push_str("</tr>");
        }
        html.push_str("</table>");
    }
    // The metrics whose picture does not follow the style.
    let _ = write!(html, "<h2>Fixed pictures (any style)</h2><table><tr><th class=row>Metric</th>");
    for size in sizes {
        let _ = write!(html, "<th>{size} px dark</th>");
    }
    html.push_str("</tr>");
    for f in &win.fixed {
        let spec = &items_from_settings(&assign_spans([(1, item(MenuBarKind::Ring, f.metric.clone()))]))[0];
        let _ = write!(html, "<tr><th class=row>{:?}<br><span class=note>{}</span></th>", f.metric, f.drawn);
        let value = if f.metric == Metric::Temperature { Some(64.0) } else { Some(37.0) };
        for size in sizes {
            let look = render_px(&px_kind(spec), &sample(value), px::State::Normal, size, true);
            let name = format!("win-fixed-{:?}-{size}.png", f.metric);
            let file = save(&icons, &name, &look.pictures[0], None);
            let _ = write!(
                html,
                "<td class=dark><img src=\"icons/{file}\" width={} height={}><img class=one src=\"icons/{file}\"></td>",
                size * 8,
                size * 8
            );
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    mac(&mut html, &icons, &kinds, &specs, &title);
    std::fs::write(out.join("index.html"), html).unwrap();
    println!("wrote {}", out.join("index.html").display());
}

#[cfg(target_os = "macos")]
fn mac(
    html: &mut String,
    icons: &Path,
    kinds: &[MenuBarKind],
    specs: &[dyno_app_lib::tray_model::ItemSpec],
    title: &dyn Fn(MenuBarKind) -> &'static str,
) {
    use dyno_app_lib::tray_look::{mac_kind, render_mac};
    use dyno_core::tray::menu_bar as mb;
    let text = dyno_app_lib::tray_text::CoreText::new();
    let support = style_support(TrayPlatform::Mac);
    let _ = write!(
        html,
        "<h2>macOS menu bar (templates, CoreText)</h2><p>@2x shown at 4x of its points, 1x at 8x; tinted as the bar tints them.</p>\
         <table><tr><th class=row>Style</th><th>@2x dark bar</th><th>@2x light bar</th><th>1x dark bar</th><th>1x light bar</th>\
         <th>alert @2x</th><th>unavailable @2x</th></tr>"
    );
    for (k, spec) in kinds.iter().zip(specs) {
        let drawn = &support.styles.iter().find(|d| d.kind == *k).unwrap().label;
        let _ = write!(html, "<tr><th class=row>{}</th>", row_head(title(*k), drawn));
        let kind = mac_kind(spec, 8);
        for (state, tag, value) in [
            (mb::State::Normal, "normal", Some(37.0)),
            (mb::State::Alert, "alert", Some(93.0)),
            (mb::State::Unavailable, "unavailable", None),
        ] {
            let look = render_mac(&kind, &sample(value), state, &text);
            let cells: Vec<(usize, bool)> = if tag == "normal" {
                vec![(1, true), (1, false), (0, true), (0, false)]
            } else {
                vec![(1, true)]
            };
            for (rep, dark) in cells {
                let p = &look.pictures[rep];
                let ink = if dark { [255, 255, 255] } else { [0, 0, 0] };
                let name = format!("mac-{k:?}-{tag}-{}x-{}.png", rep + 1, if dark { "dark" } else { "light" });
                let f = save(icons, &name, p, Some(ink));
                let zoom = if rep == 1 { 4 } else { 8 };
                let _ = write!(
                    html,
                    "<td class={}><img src=\"icons/{f}\" width={} height={}><img class=one src=\"icons/{f}\" width={} height={}></td>",
                    if dark { "bar-dark" } else { "bar-light" },
                    p.width * zoom,
                    p.height * zoom,
                    look.width,
                    look.height
                );
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
}

#[cfg(not(target_os = "macos"))]
fn mac(_: &mut String, _: &Path, _: &[MenuBarKind], _: &[dyno_app_lib::tray_model::ItemSpec], _: &dyn Fn(MenuBarKind) -> &'static str) {}
