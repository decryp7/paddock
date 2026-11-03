//! Renders Dyno's tray design sheets into `docs/design/dyno-tray/` with the renderers of `dyno_core::tray::{menu_bar, pixel}`
//! and, on macOS, the CoreText adapter of their text port: `cargo run -p dyno-core --example tray_sheets` (macOS only; it prints
//! the measurements of the spec). "Today" comes from the current `dyno_core::tray` renderers.

#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::field_reassign_with_default,
    missing_docs
)]

#[cfg(target_os = "macos")]
mod coretext;
#[cfg(target_os = "macos")]
mod sheet;

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("tray_sheets draws with the macOS system font: run it on macOS");
}

#[cfg(target_os = "macos")]
fn main() {
    sheets::run();
}

#[cfg(target_os = "macos")]
mod sheets {
    use crate::sheet::*;
    use dyno_core::tray::menu_bar as mac;
    use dyno_core::tray::pixel as pix;
    use dyno_core::tray::text::TextStyle as Style;
    use tiny_skia::{Color, Pixmap};

    const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../docs/design/dyno-tray");

    fn tpl(t: mac::Template) -> Pixmap {
        let mut pm = Pixmap::new(t.width.max(1), t.height.max(1)).unwrap();
        for (k, p) in pm.pixels_mut().iter_mut().enumerate() {
            *p = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, t.alpha[k]).unwrap();
        }
        pm
    }

    fn mrender(k: &mac::Kind, r: &mac::Reading, st: mac::State, font: &Font, scale: f32) -> Pixmap {
        tpl(dyno_core::tray::menu_bar::render(k, r, st, font.raster(), scale as u32))
    }

    fn prender(k: &pix::Kind, r: &pix::Reading, st: pix::State, size: i32, t: pix::Theme) -> Pixmap {
        icon_to_pixmap(&dyno_core::tray::pixel::render(k, r, st, size, t))
    }

    fn history(seed: u32, n: usize, base: f32, amp: f32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(2_654_435_761) | 1;
        let mut v = base;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let r = (x % 1000) as f32 / 1000.0 - 0.5;
                v = (v + r * amp).clamp(2.0, 98.0);
                let spike = if i % 23 == 7 { amp * 1.6 } else { 0.0 };
                (v + spike).clamp(0.0, 100.0)
            })
            .collect()
    }

    fn cores() -> Vec<f32> {
        vec![62.0, 18.0, 41.0, 9.0, 77.0, 23.0, 12.0, 35.0, 5.0, 28.0]
    }

    /// The nine Dyno items of the mockups, as macOS formats.
    fn mac_items() -> Vec<(mac::Kind, mac::Reading)> {
        use mac::{Kind, Reading, Unit};
        vec![
            (
                Kind::Cores { cores: 10 },
                Reading {
                    cores: cores(),
                    ..Default::default()
                },
            ),
            (
                Kind::Value {
                    label: "cpu".into(),
                    unit: Unit::Percent,
                },
                Reading {
                    value: Some(23.0),
                    ..Default::default()
                },
            ),
            (
                Kind::Stacked {
                    label: "mem".into(),
                    unit: Unit::Percent,
                },
                Reading {
                    value: Some(61.0),
                    ..Default::default()
                },
            ),
            (
                Kind::Meter { label: "gpu".into() },
                Reading {
                    value: Some(38.0),
                    ..Default::default()
                },
            ),
            (
                Kind::Graph { label: "dsk".into() },
                Reading {
                    value: Some(14.0),
                    history: history(3, 40, 14.0, 18.0),
                    ..Default::default()
                },
            ),
            (
                Kind::Bar { label: "swp".into() },
                Reading {
                    value: Some(46.0),
                    ..Default::default()
                },
            ),
            (
                Kind::Network,
                Reading {
                    up: 1_240_000.0,
                    down: 24_300_000.0,
                    ..Default::default()
                },
            ),
            (
                Kind::Temperature,
                Reading {
                    value: Some(54.0),
                    ..Default::default()
                },
            ),
            (
                Kind::GraphValue {
                    label: "cpu".into(),
                    unit: Unit::Percent,
                },
                Reading {
                    value: Some(27.0),
                    history: history(9, 40, 25.0, 14.0),
                    ..Default::default()
                },
            ),
        ]
    }

    fn icon_to_pixmap(i: &dyno_core::tray::Icon) -> Pixmap {
        let mut pm = Pixmap::new(i.size, i.size).unwrap();
        for (k, p) in pm.pixels_mut().iter_mut().enumerate() {
            let c = &i.rgba[k * 4..k * 4 + 4];
            *p = tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], c[3]).premultiply();
        }
        pm
    }

    /// The same nine items as the app draws them today (dyno_core::tray, 36 px square templates shown at 18 pt).
    fn today_items(px: u32, dark: bool, template: bool) -> Vec<Pixmap> {
        use dyno_core::tray::*;
        let h = history(3, 40, 14.0, 18.0);
        let h2 = history(9, 40, 25.0, 14.0);
        let icons = vec![
            render_core_bars(&cores(), px, dark),
            render_number(23.0, px, dark),
            render_number(61.0, px, dark),
            render_meter(38.0, px, dark),
            render(&[(IconMetric::Disk, 14.0)], &h, IconStyle::Graph, px, dark),
            render_ring(46.0, px, dark),
            // Network as a number is today's 0-100 scale against the busiest second, not a rate.
            render_number(48.0, px, dark),
            render_number(54.0, px, dark),
            render_graph_number(27.0, &h2, px, dark),
        ];
        icons
            .into_iter()
            .map(|i| icon_to_pixmap(&if template { i.as_template() } else { i }))
            .collect()
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Bar {
        Light,
        Dark,
        Tinted,
    }

    impl Bar {
        fn tint(self) -> Color {
            match self {
                Bar::Light => rgba(0x000000, 0.85),
                Bar::Dark | Bar::Tinted => rgba(0xffffff, 1.0),
            }
        }
        fn highlight(self) -> Color {
            match self {
                Bar::Light => rgba(0x000000, 0.10),
                _ => rgba(0xffffff, 0.22),
            }
        }
    }

    const BAR_W: f32 = 1180.0;
    const ITEM_GAP: f32 = 16.0;

    /// One menu bar over a wallpaper, `scale` pixels per point, with the items (template images in pixels) placed right to left.
    fn menu_bar(font: &Font, bar: Bar, scale: f32, items: &[Pixmap], highlight: Option<usize>, caption: &str) -> Pixmap {
        let s = scale;
        let (w, bar_h, below, above) = (BAR_W * s, 24.0 * s, 34.0 * s, 26.0 * s);
        let mut pm = Pixmap::new(w as u32, (above + bar_h + below) as u32).unwrap();
        pm.fill(rgba(0xffffff, 1.0));
        text(&mut pm, font, caption, 12.0 * s, 600.0, 10.0 * s, 17.0 * s, rgba(0x1d1d1f, 1.0));
        let y0 = above;
        match bar {
            Bar::Light => {
                wallpaper(&mut pm, 0.0, y0, w, bar_h + below, 0x8fb8e6, 0xe7b3d6, 0xfff1c9);
                fill_rect(&mut pm, 0.0, y0, w, bar_h, rgba(0xf6f6f8, 0.72));
            }
            Bar::Dark => {
                wallpaper(&mut pm, 0.0, y0, w, bar_h + below, 0x141a33, 0x3a1534, 0x5a2a6e);
                fill_rect(&mut pm, 0.0, y0, w, bar_h, rgba(0x1c1c1e, 0.55));
            }
            Bar::Tinted => {
                // The transparent menu bar of recent macOS over a saturated wallpaper.
                wallpaper(&mut pm, 0.0, y0, w, bar_h + below, 0xc2410c, 0x6d28d9, 0xf59e0b);
            }
        }
        let cy = y0 + bar_h / 2.0;
        let tint = bar.tint();
        let left = mac_system(&mut pm, font, w - 12.0 * s, cy, s, tint, "Tue 3 Nov  14:32");
        let mut x = left - ITEM_GAP * s;
        for (i, it) in items.iter().enumerate().rev() {
            let iw = it.width() as f32;
            x -= iw;
            x = x.round();
            let iy = (y0 + (bar_h - it.height() as f32) / 2.0).round();
            if highlight == Some(i) {
                fill_rrect(
                    &mut pm,
                    x - 6.0 * s,
                    y0 + 1.0 * s,
                    iw + 12.0 * s,
                    bar_h - 2.0 * s,
                    4.0 * s,
                    bar.highlight(),
                );
            }
            template(&mut pm, it, x as i32, iy as i32, tint);
            x -= ITEM_GAP * s;
        }
        mac_menus(&mut pm, font, 14.0 * s, cy, s, tint);
        pm
    }

    fn stack(parts: &[Pixmap], gap: u32, bg: Color) -> Pixmap {
        let w = parts.iter().map(|p| p.width()).max().unwrap_or(1);
        let h: u32 = parts.iter().map(|p| p.height()).sum::<u32>() + gap * (parts.len() as u32 + 1);
        let mut pm = Pixmap::new(w + 2 * gap, h).unwrap();
        pm.fill(bg);
        let mut y = gap;
        for p in parts {
            place(&mut pm, p, gap as i32, y as i32);
            y += p.height() + gap;
        }
        pm
    }

    fn title_block(font: &Font, w: u32, scale: f32, title: &str, lines: &[&str]) -> Pixmap {
        let h = ((34.0 + 17.0 * lines.len() as f32) * scale) as u32;
        let mut pm = Pixmap::new(w, h).unwrap();
        pm.fill(rgba(0xffffff, 1.0));
        text(
            &mut pm,
            font,
            title,
            18.0 * scale,
            700.0,
            10.0 * scale,
            24.0 * scale,
            rgba(0x1d1d1f, 1.0),
        );
        for (i, l) in lines.iter().enumerate() {
            text(
                &mut pm,
                font,
                l,
                12.0 * scale,
                400.0,
                10.0 * scale,
                (44.0 + 17.0 * i as f32) * scale,
                rgba(0x4f5764, 1.0),
            );
        }
        pm
    }

    fn mac_sheet(font: &Font, scale: f32) {
        let items = mac_items();
        let normal: Vec<Pixmap> = items.iter().map(|(k, r)| mrender(k, r, mac::State::Normal, font, scale)).collect();
        let mut with_alert = normal.clone();
        with_alert[2] = mrender(
            &items[2].0,
            &mac::Reading {
                value: Some(93.0),
                ..Default::default()
            },
            mac::State::Alert,
            font,
            scale,
        );
        let defaults: Vec<Pixmap> = [
            (
                mac::Kind::Stacked {
                    label: "cpu".into(),
                    unit: mac::Unit::Percent,
                },
                mac::Reading {
                    value: Some(23.0),
                    ..Default::default()
                },
            ),
            items[2].clone(),
            items[6].clone(),
            items[7].clone(),
        ]
        .iter()
        .map(|(k, r)| mrender(k, r, mac::State::Normal, font, scale))
        .collect();
        let tag = if scale > 1.0 { "@2x" } else { "@1x" };
        let parts = vec![
        title_block(
            font,
            (BAR_W * scale) as u32,
            scale,
            &format!("Dyno menu-bar items, proposed ({tag}: {scale} px per point)"),
            &[
                "Nine Dyno items left of the system items: per-core bars, CPU value, memory stack, GPU meter, disk graph, swap bar, network pair, temperature, CPU graph and value.",
                "Every item is a template image (black and alpha) that the menu bar tints; widths are fixed per format. System items are vector stand-ins.",
            ],
        ),
        menu_bar(font, Bar::Light, scale, &defaults, None, "Default set, light menu bar: four items (CPU and memory stacks, network, temperature)"),
        menu_bar(font, Bar::Light, scale, &normal, None, "Nine items (the user's choice), light menu bar, light wallpaper"),
        menu_bar(font, Bar::Dark, scale, &normal, Some(1), "Dark menu bar: the CPU item highlighted (its popover open)"),
        menu_bar(font, Bar::Tinted, scale, &with_alert, None, "Transparent menu bar on a tinted wallpaper: memory over its threshold (93 %, alert plate)"),
    ];
        stack(&parts, (8.0 * scale) as u32, rgba(0xffffff, 1.0))
            .save_png(format!("{OUT}/macos-menubar{tag}.png"))
            .unwrap();
    }

    fn upscale(src: &Pixmap, k: u32) -> Pixmap {
        let mut out = Pixmap::new(src.width() * k, src.height() * k).unwrap();
        zoom(&mut out, src, 0.0, 0.0, k as f32);
        out
    }

    /// The catalogue: every format in every state on a light and a dark bar at @2x, with 1x and @2x close-ups.
    fn mac_catalogue(font: &Font) {
        use mac::{Kind, Reading, State};
        let names = [
            "Per-core bars (10 cores)",
            "Value: label and value",
            "Stack: label over value",
            "Meter",
            "Graph (a sample per point)",
            "Single bar (swap)",
            "Network pair",
            "Temperature",
            "Graph and stack",
        ];
        let mut rows: Vec<(String, Kind, Reading, Reading)> = Vec::new();
        for (i, (k, r)) in mac_items().into_iter().enumerate() {
            let mut hot = r.clone();
            hot.value = Some(if matches!(k, Kind::Temperature) { 97.0 } else { 93.0 });
            rows.push((names[i].to_string(), k, r, hot));
        }
        let s = 2.0;
        let (col_name, col_w, row_h) = (190.0 * s, 96.0 * s, 46.0 * s);
        let zoom_w = 800.0 * s;
        let states = ["Normal", "Highlighted", "Alert", "Unavailable"];
        let w = col_name + col_w * 8.0 + zoom_w;
        let head = 92.0 * s;
        let h = head + row_h * rows.len() as f32 + 12.0 * s;
        let mut pm = Pixmap::new(w as u32, h as u32).unwrap();
        pm.fill(rgba(0xffffff, 1.0));
        text(
            &mut pm,
            font,
            "Dyno menu-bar formats and states (@2x, fixed widths)",
            18.0 * s,
            700.0,
            10.0 * s,
            26.0 * s,
            rgba(0x1d1d1f, 1.0),
        );
        text(
        &mut pm,
        font,
        "Light bar (left four columns), dark bar (right four). Alert: the number (or the label) on a solid plate, knocked out: a shape, not a colour. Unavailable: an en dash, the item at 45 %.",
        12.0 * s,
        400.0,
        10.0 * s,
        46.0 * s,
        rgba(0x4f5764, 1.0),
    );
        text(&mut pm, font, "Close-ups at the right: the 1x image at 8x and the @2x image at 4x, so both show at the same size; one grid cell per device pixel in the 1x close-up.", 12.0 * s, 400.0, 10.0 * s, 63.0 * s, rgba(0x4f5764, 1.0));
        for (j, st) in states.iter().chain(states.iter()).enumerate() {
            text(
                &mut pm,
                font,
                st,
                11.0 * s,
                600.0,
                col_name + col_w * j as f32 + 6.0 * s,
                head - 8.0 * s,
                rgba(0x4f5764, 1.0),
            );
        }
        let zx = col_name + col_w * 8.0 + 16.0 * s;
        text(&mut pm, font, "1x at 8x", 11.0 * s, 600.0, zx, head - 8.0 * s, rgba(0x4f5764, 1.0));
        text(
            &mut pm,
            font,
            "1x alert at 8x",
            11.0 * s,
            600.0,
            zx + 260.0 * s,
            head - 8.0 * s,
            rgba(0x4f5764, 1.0),
        );
        text(
            &mut pm,
            font,
            "@2x at 4x",
            11.0 * s,
            600.0,
            zx + 520.0 * s,
            head - 8.0 * s,
            rgba(0x4f5764, 1.0),
        );
        for (i, (name, kind, r, hot)) in rows.iter().enumerate() {
            let y = head + row_h * i as f32;
            let wpt = mac::width(kind, font.raster());
            text(&mut pm, font, name, 12.0 * s, 600.0, 10.0 * s, y + 20.0 * s, rgba(0x1d1d1f, 1.0));
            text(
                &mut pm,
                font,
                &format!("{wpt:.0} x 18 pt"),
                11.0 * s,
                400.0,
                10.0 * s,
                y + 36.0 * s,
                rgba(0x4f5764, 1.0),
            );
            for (j, bar) in [Bar::Light, Bar::Dark].iter().enumerate() {
                let bg = if *bar == Bar::Light {
                    rgba(0xe9e9ec, 1.0)
                } else {
                    rgba(0x2a2a2e, 1.0)
                };
                fill_rect(
                    &mut pm,
                    col_name + col_w * 4.0 * j as f32,
                    y + 3.0 * s,
                    col_w * 4.0 - 4.0 * s,
                    row_h - 6.0 * s,
                    bg,
                );
                for (k, state) in [State::Normal, State::Normal, State::Alert, State::Unavailable].iter().enumerate() {
                    let reading = if *state == State::Alert { hot } else { r };
                    let img = mrender(kind, reading, *state, font, s);
                    let cx = (col_name + col_w * (4 * j + k) as f32 + (col_w - img.width() as f32) / 2.0).round();
                    let cy = (y + (row_h - img.height() as f32) / 2.0).round();
                    if k == 1 {
                        fill_rrect(
                            &mut pm,
                            cx - 6.0 * s,
                            cy - 2.0 * s,
                            img.width() as f32 + 12.0 * s,
                            img.height() as f32 + 4.0 * s,
                            4.0 * s,
                            bar.highlight(),
                        );
                    }
                    template(&mut pm, &img, cx as i32, cy as i32, bar.tint());
                }
            }
            let one = mrender(kind, r, State::Normal, font, 1.0);
            let two = mrender(kind, r, State::Normal, font, 2.0);
            let zy = (y + (row_h - 18.0 * 4.0) / 2.0).round();
            fill_rect(&mut pm, zx, zy, one.width() as f32 * 4.0, 72.0, rgba(0xf2f2f4, 1.0));
            template(&mut pm, &upscale(&one, 4), zx as i32, zy as i32, rgba(0x000000, 0.85));
            grid(&mut pm, zx, zy, one.width(), one.height(), 4.0, rgba(0x000000, 0.07));
            let hot1 = mrender(kind, hot, State::Alert, font, 1.0);
            let xa = zx + 260.0 * s;
            fill_rect(&mut pm, xa, zy, hot1.width() as f32 * 4.0, 72.0, rgba(0xf2f2f4, 1.0));
            template(&mut pm, &upscale(&hot1, 4), xa as i32, zy as i32, rgba(0x000000, 0.85));
            grid(&mut pm, xa, zy, hot1.width(), hot1.height(), 4.0, rgba(0x000000, 0.07));
            let x1 = zx + 520.0 * s;
            fill_rect(&mut pm, x1, zy, two.width() as f32 * 2.0, 72.0, rgba(0xf2f2f4, 1.0));
            template(&mut pm, &upscale(&two, 2), x1 as i32, zy as i32, rgba(0x000000, 0.85));
        }
        pm.save_png(format!("{OUT}/macos-items@2x.png")).unwrap();
    }

    // --------------------------------------------------------------------------------------------------------- Windows

    fn win_items() -> Vec<(pix::Kind, pix::Reading, pix::State)> {
        use pix::{Kind, Reading, State};
        vec![
            (
                Kind::Percent { label: "CPU".into() },
                Reading {
                    value: Some(23.0),
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Percent { label: "MEM".into() },
                Reading {
                    value: Some(93.0),
                    ..Default::default()
                },
                State::Alert,
            ),
            (
                Kind::Ring { letter: 'G' },
                Reading {
                    value: Some(38.0),
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Graph { label: "DSK".into() },
                Reading {
                    value: Some(14.0),
                    history: history(3, 40, 14.0, 18.0),
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Network,
                Reading {
                    up: 1_240_000.0,
                    down: 24_300_000.0,
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Temperature,
                Reading {
                    value: Some(54.0),
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Cores,
                Reading {
                    cores: cores()[..8].to_vec(),
                    ..Default::default()
                },
                State::Normal,
            ),
            (
                Kind::Percent { label: "CPU".into() },
                Reading {
                    value: Some(100.0),
                    ..Default::default()
                },
                State::Normal,
            ),
        ]
    }

    fn taskbar(font: &Font, scale: f32, dark: bool, icons: &[Pixmap], caption: &str) -> Pixmap {
        let s = scale;
        let (w, h, above) = (520.0 * s, 48.0 * s, 24.0 * s);
        let mut pm = Pixmap::new(w as u32, (above + h) as u32).unwrap();
        pm.fill(rgba(0xffffff, 1.0));
        text(&mut pm, font, caption, 12.0 * s, 600.0, 8.0 * s, 16.0 * s, rgba(0x1d1d1f, 1.0));
        let y0 = above;
        if dark {
            wallpaper(&mut pm, 0.0, y0, w, h, 0x10233f, 0x2c1238, 0x3a2a6e);
            fill_rect(&mut pm, 0.0, y0, w, h, rgba(0x1c1c1c, 0.86));
            fill_rect(&mut pm, 0.0, y0, w, 1.0, rgba(0xffffff, 0.08));
        } else {
            wallpaper(&mut pm, 0.0, y0, w, h, 0x9cc3ea, 0xe8b9d6, 0xfff1c9);
            fill_rect(&mut pm, 0.0, y0, w, h, rgba(0xf3f3f3, 0.88));
            fill_rect(&mut pm, 0.0, y0, w, 1.0, rgba(0x000000, 0.08));
        }
        let ink = if dark { rgba(0xffffff, 1.0) } else { rgba(0x1b1b1f, 1.0) };
        let cy = y0 + h / 2.0;
        let st = Style::new(12.0);
        let (l1, l2) = ("14:32", "03/11/2026");
        let right = w - 14.0 * s;
        let w1 = font.measure(l1, &st) * s;
        let w2 = font.measure(l2, &st) * s;
        font.draw(&mut pm, l1, &st, (right - w1) / s, (cy - 2.0 * s) / s, s, ink);
        font.draw(&mut pm, l2, &st, (right - w2) / s, (cy + 14.0 * s) / s, s, ink);
        let mut x = right - w2.max(w1) - 16.0 * s;
        let p = solid(ink);
        let gw = 70.0 * s;
        x -= gw;
        for i in 0..3 {
            let gx = x + 6.0 * s + i as f32 * 22.0 * s;
            let r = rrect_path(gx, cy - 6.0 * s, 14.0 * s, 12.0 * s, 2.5 * s);
            pm.stroke_path(
                &r,
                &p,
                &tiny_skia::Stroke {
                    width: 1.0 * s,
                    ..Default::default()
                },
                tiny_skia::Transform::identity(),
                None,
            );
        }
        x -= 10.0 * s;
        // Windows 11 gives each notification icon a 32 x 40 px button at 100 % (the icon centred in it).
        let slot = 32.0 * s;
        for ic in icons.iter().rev() {
            x -= slot;
            let ix = (x + (slot - ic.width() as f32) / 2.0).round();
            let iy = (cy - ic.height() as f32 / 2.0).round();
            place(&mut pm, ic, ix as i32, iy as i32);
        }
        x -= 24.0 * s;
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(x + 6.0 * s, cy + 2.0 * s);
        pb.line_to(x + 10.0 * s, cy - 2.0 * s);
        pb.line_to(x + 14.0 * s, cy + 2.0 * s);
        pm.stroke_path(
            &pb.finish().unwrap(),
            &p,
            &tiny_skia::Stroke {
                width: 1.0 * s,
                ..Default::default()
            },
            tiny_skia::Transform::identity(),
            None,
        );
        pm
    }

    fn closeups(font: &Font, items: &[(pix::Kind, pix::Reading, pix::State)], sizes: &[i32], title: &str) -> Pixmap {
        let cell = 112.0;
        let rows = sizes.len() * 2;
        let w = 130.0 + (cell + 10.0) * items.len() as f32;
        let h = 40.0 + (cell + 10.0) * rows as f32;
        let mut z = Pixmap::new(w as u32, h as u32).unwrap();
        z.fill(rgba(0xffffff, 1.0));
        text(&mut z, font, title, 14.0, 700.0, 10.0, 22.0, rgba(0x1d1d1f, 1.0));
        let mut y = 36.0;
        for &size in sizes {
            for dark in [false, true] {
                let t = pix::Theme { dark };
                text(
                    &mut z,
                    font,
                    &format!("{size} px, {}", if dark { "dark" } else { "light" }),
                    11.0,
                    600.0,
                    10.0,
                    y + cell / 2.0,
                    rgba(0x4f5764, 1.0),
                );
                for (i, (k, r, st)) in items.iter().enumerate() {
                    let ic = prender(k, r, *st, size, t);
                    let kz = cell / ic.width() as f32;
                    let x = 120.0 + i as f32 * (cell + 10.0);
                    fill_rect(
                        &mut z,
                        x,
                        y,
                        cell,
                        cell,
                        if dark { rgba(0x202020, 1.0) } else { rgba(0xf3f3f3, 1.0) },
                    );
                    zoom(&mut z, &ic, x, y, kz);
                    grid(
                        &mut z,
                        x,
                        y,
                        ic.width(),
                        ic.height(),
                        kz,
                        if dark { rgba(0xffffff, 0.07) } else { rgba(0x000000, 0.06) },
                    );
                }
                y += cell + 10.0;
            }
        }
        z
    }

    fn windows_sheet(font: &Font) {
        let items = win_items();
        let mut parts = vec![title_block(
        font,
        1040,
        1.0,
        "Dyno notification-area icons, proposed (Windows 11)",
        &[
            "Each icon is drawn for its exact size: 16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 %. Labels from 24 px; at 16 and 20 the tooltip names the metric.",
            "Order: CPU, memory in alert (danger plate: a shape and a colour), GPU gauge, disk graph, network, temperature, per-core bars, CPU at 100 % (three digits fit).",
            "Clock and system glyphs are stand-ins (not Segoe UI). Each strip is at its real pixel size.",
        ],
    )];
        for (pct, size) in [(100, 16), (150, 24), (200, 32)] {
            for dark in [false, true] {
                let t = pix::Theme { dark };
                let icons: Vec<Pixmap> = items.iter().map(|(k, r, st)| prender(k, r, *st, size, t)).collect();
                let cap = format!("{pct} % ({size} px), {} taskbar", if dark { "dark" } else { "light" });
                parts.push(taskbar(font, pct as f32 / 100.0, dark, &icons, &cap));
            }
        }
        parts.push(closeups(
            font,
            &items,
            &[16, 20, 24, 32],
            "Close-ups, every size enlarged to the same cell (nearest neighbour; grid = one pixel)",
        ));
        stack(&parts, 10, rgba(0xffffff, 1.0))
            .save_png(format!("{OUT}/windows-tray.png"))
            .unwrap();
    }

    fn linux_sheet(font: &Font) {
        let items = win_items();
        let mut parts = vec![title_block(
        font,
        1100,
        1.0,
        "Dyno appindicator icons, proposed (Linux, 22 px)",
        &[
            "GNOME's top bar (dark) and a light KDE panel at 1x, the Windows drawing at its 22 px layout (labels from 22 px).",
            "Where the host shows an indicator label, value text can go there instead: app_indicator_set_label with a guide string fixes its width.",
        ],
    )];
        for dark in [true, false] {
            let t = pix::Theme { dark };
            let icons: Vec<Pixmap> = items.iter().map(|(k, r, st)| prender(k, r, *st, 22, t)).collect();
            let (w, h) = (760.0, 32.0);
            let mut pm = Pixmap::new(w as u32, (h + 24.0) as u32).unwrap();
            pm.fill(rgba(0xffffff, 1.0));
            text(
                &mut pm,
                font,
                if dark { "GNOME top bar (dark)" } else { "KDE panel (light)" },
                12.0,
                600.0,
                8.0,
                16.0,
                rgba(0x1d1d1f, 1.0),
            );
            fill_rect(
                &mut pm,
                0.0,
                24.0,
                w,
                h,
                if dark { rgba(0x000000, 1.0) } else { rgba(0xeff0f1, 1.0) },
            );
            let ink = if dark { rgba(0xffffff, 1.0) } else { rgba(0x232627, 1.0) };
            let st = Style::new(11.0).weight(0.3);
            let cw = font.measure("Tue 14:32", &st);
            font.draw(&mut pm, "Tue 14:32", &st, (w - cw) / 2.0 - 120.0, 24.0 + 20.0, 1.0, ink);
            let mut x = w - 10.0;
            for ic in icons.iter().rev() {
                x -= 22.0 + 10.0;
                place(&mut pm, ic, x as i32, 24 + 5);
            }
            parts.push(pm);
        }
        parts.push(closeups(font, &items, &[22], "Close-ups at 22 px"));
        stack(&parts, 10, rgba(0xffffff, 1.0))
            .save_png(format!("{OUT}/linux-panel.png"))
            .unwrap();
    }

    // ---------------------------------------------------------------------------------------------- today vs proposed

    fn today_sheet(font: &Font) {
        let s = 2.0;
        let mut parts = vec![title_block(
        font,
        (BAR_W * s) as u32,
        s,
        "Today vs proposed: the same nine items",
        &[
            "Today (dyno_core::tray): 18 pt squares, a 3 x 5 pixel font doubled, no labels or units; the number re-centres as its digits change; network shows a 0-100 scale, not a rate.",
            "Proposed: the system font with tabular figures, a label on every item, a fixed width per format, real units. Lower half: 1x and Windows at 100 %, enlarged 2x.",
        ],
    )];
        for bar in [Bar::Light, Bar::Dark] {
            let today = today_items(36, bar == Bar::Dark, true);
            let proposed: Vec<Pixmap> = mac_items()
                .iter()
                .map(|(k, r)| mrender(k, r, mac::State::Normal, font, s))
                .collect();
            let name = if bar == Bar::Light { "light" } else { "dark" };
            parts.push(menu_bar(font, bar, s, &today, None, &format!("Today, {name} menu bar, @2x")));
            parts.push(menu_bar(font, bar, s, &proposed, None, &format!("Proposed, {name} menu bar, @2x")));
        }
        let today1: Vec<Pixmap> = today_items(36, false, true).iter().map(|p| resample(p, 18, 18)).collect();
        let prop1: Vec<Pixmap> = mac_items()
            .iter()
            .map(|(k, r)| mrender(k, r, mac::State::Normal, font, 1.0))
            .collect();
        parts.push(upscale(
            &menu_bar(
                font,
                Bar::Light,
                1.0,
                &today1,
                None,
                "Today at 1x (the 36 px image shrunk to 18 by the system)",
            ),
            2,
        ));
        parts.push(upscale(
            &menu_bar(font, Bar::Light, 1.0, &prop1, None, "Proposed at 1x (drawn at 1x)"),
            2,
        ));
        for dark in [false, true] {
            use dyno_core::tray::*;
            let h = history(3, 40, 14.0, 18.0);
            let today: Vec<Pixmap> = [
                render_number(23.0, 32, dark),
                render_number(93.0, 32, dark),
                render_ring(38.0, 32, dark),
                render(&[(IconMetric::Disk, 14.0)], &h, IconStyle::Graph, 32, dark),
                render_number(48.0, 32, dark),
                render_number(54.0, 32, dark),
                render_core_bars(&cores()[..8], 32, dark),
                render_number(100.0, 32, dark),
            ]
            .iter()
            .map(|i| resample(&icon_to_pixmap(i), 16, 16))
            .collect();
            let t = pix::Theme { dark };
            let prop: Vec<Pixmap> = win_items().iter().map(|(k, r, st)| prender(k, r, *st, 16, t)).collect();
            let n = if dark { "dark" } else { "light" };
            let a = taskbar(
                font,
                1.0,
                dark,
                &today,
                &format!("Windows 100 %, {n}: today (the 32 px image shrunk to 16)"),
            );
            let b = taskbar(font, 1.0, dark, &prop, &format!("Windows 100 %, {n}: proposed (drawn at 16)"));
            parts.push(upscale(&a, 2));
            parts.push(upscale(&b, 2));
        }
        stack(&parts, 12, rgba(0xffffff, 1.0))
            .save_png(format!("{OUT}/today-vs-proposed.png"))
            .unwrap();
    }

    fn numerals_sheet(font: &Font) {
        let mut pm = Pixmap::new(1400, 1200).unwrap();
        pm.fill(rgba(0xffffff, 1.0));
        text(
            &mut pm,
            font,
            "Dyno dash numerals and label caps (Windows and Linux icons), 6x",
            18.0,
            700.0,
            10.0,
            26.0,
            rgba(0x1d1d1f, 1.0),
        );
        text(
        &mut pm,
        font,
        "Segments on whole pixels, outer corners chamfered by stroke - 1 (at least 1 px); a narrow 1 so that 100 fits 16 px. Caps: 5 rows (22, 24 px), 7 rows (32 px).",
        12.0,
        400.0,
        10.0,
        46.0,
        rgba(0x4f5764, 1.0),
    );
        let mut y = 66.0;
        let k = 6.0;
        for size in [16, 20, 22, 24, 32] {
            let l = pix::layout(size);
            for (n, what) in [(l.digits, "numerals"), (l.small, "small (network)")] {
                if what.starts_with("small") && size < 22 {
                    continue;
                }
                text(
                    &mut pm,
                    font,
                    &format!("{size} px {what}: {} x {}, stroke {}", n.w, n.h, n.s),
                    12.0,
                    600.0,
                    10.0,
                    y + 14.0,
                    rgba(0x1d1d1f, 1.0),
                );
                let mut x = 250.0;
                for ch in "0123456789.°-".chars() {
                    let g = pix::numeral(ch, n);
                    let mut cv = pix::Canvas::new(g.w.max(g.h)).unwrap();
                    cv.glyph(&g, 0, 0, [0x1b, 0x1b, 0x1f, 0xff]);
                    fill_rect(&mut pm, x, y, g.w as f32 * k, g.h as f32 * k, rgba(0xeeeef0, 1.0));
                    zoom(&mut pm, &cv.pm, x, y, k);
                    grid(&mut pm, x, y, g.w as u32, g.h as u32, k, rgba(0x000000, 0.07));
                    x += g.w as f32 * k + 8.0;
                }
                y += n.h as f32 * k + 16.0;
            }
        }
        for rows in [5, 7] {
            text(
                &mut pm,
                font,
                &format!("caps, {rows} rows"),
                12.0,
                600.0,
                10.0,
                y + 14.0,
                rgba(0x1d1d1f, 1.0),
            );
            let mut x = 250.0;
            for ch in "ABCDEFGKMNPSTUW".chars() {
                let g = pix::cap(ch, rows);
                let mut cv = pix::Canvas::new(g.w.max(g.h)).unwrap();
                cv.glyph(&g, 0, 0, [0x1b, 0x1b, 0x1f, 0xff]);
                fill_rect(&mut pm, x, y, g.w as f32 * k, g.h as f32 * k, rgba(0xeeeef0, 1.0));
                zoom(&mut pm, &cv.pm, x, y, k);
                x += g.w as f32 * k + 8.0;
            }
            y += rows as f32 * k + 16.0;
        }
        let pm = pm
            .clone_rect(tiny_skia::IntRect::from_xywh(0, 0, 1400, y as u32 + 10).unwrap())
            .unwrap();
        pm.save_png(format!("{OUT}/numerals.png")).unwrap();
    }

    // -------------------------------------------------------------------------------------------------- measurements

    fn lin(c: u8) -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn lum(c: [u8; 3]) -> f64 {
        0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    }
    fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn report(font: &Font) {
        println!("## Fixed widths (points), from the font on this Mac");
        for (k, _) in mac_items() {
            println!("{:?}: {} pt", k, mac::width(&k, font.raster()));
        }
        println!("Bar(mem): {}", mac::width(&mac::Kind::Bar { label: "mem".into() }, font.raster()));
        for l in ["cpu", "gpu", "mem", "dsk", "net", "swp", "bat", "fan"] {
            println!(
                "label {l}: value {} stack {} meter {} graph {}",
                mac::width(
                    &mac::Kind::Value {
                        label: l.into(),
                        unit: mac::Unit::Percent
                    },
                    font.raster()
                ),
                mac::width(
                    &mac::Kind::Stacked {
                        label: l.into(),
                        unit: mac::Unit::Percent
                    },
                    font.raster()
                ),
                mac::width(&mac::Kind::Meter { label: l.into() }, font.raster()),
                mac::width(&mac::Kind::Graph { label: l.into() }, font.raster())
            );
        }
        let vs = mac::VALUE;
        print!("13 pt tabular advances: ");
        for d in "0123456789".chars() {
            print!("{d}:{:.3} ", font.measure(&d.to_string(), &vs));
        }
        println!();
        println!(
            "13 pt proportional: 1={:.3} 0={:.3}",
            font.measure("1", &vs.proportional()),
            font.measure("0", &vs)
        );
        println!(
            "cap height 13 pt {:.2}, label {:.2}",
            font.raster().cap_height(&vs),
            font.raster().cap_height(&mac::LABEL)
        );
        for (k, r) in mac_items() {
            let w0 = mrender(&k, &r, mac::State::Normal, font, 2.0).width();
            let w1 = mrender(&k, &r, mac::State::Normal, font, 1.0).width();
            assert_eq!(w0, 2 * w1, "{k:?}: the 1x and @2x images share the point size");
            for v in [0.0, 1.0, 5.0, 9.6, 50.0, 99.6, 100.0, 180.0] {
                let mut r2 = r.clone();
                r2.value = Some(v);
                r2.up = v as f64 * 1e7;
                r2.down = v as f64 * 3e5;
                for st in [mac::State::Normal, mac::State::Alert, mac::State::Unavailable] {
                    assert_eq!(mrender(&k, &r2, st, font, 2.0).width(), w0, "{k:?}");
                }
            }
        }
        println!("widths constant over values and every state, 1x = @2x in points: ok");
        for b in [
            0.0,
            950.0,
            999.4,
            999.6,
            9_940.0,
            12_400.0,
            512_000.0,
            1_240_000.0,
            24_300_000.0,
            1.2e9,
        ] {
            let (t, u) = mac::format_rate(b);
            print!("{b} -> {t} {u}; ");
        }
        println!();
        println!("## Contrast (WCAG) on Windows 11 taskbars (#202020 dark, #F3F3F3 light)");
        let (dk, lt) = ([0x20, 0x20, 0x20], [0xf3, 0xf3, 0xf3]);
        println!(
            "ink dark {:.2}, ink light {:.2}",
            contrast([255, 255, 255], dk),
            contrast([0x1b, 0x1b, 0x1f], lt)
        );
        println!(
            "accent dark #F063A8 {:.2}, accent light #B01E6E {:.2}",
            contrast([0xf0, 0x63, 0xa8], dk),
            contrast([0xb0, 0x1e, 0x6e], lt)
        );
        println!(
            "alert plate dark #FF6B6B vs bar {:.2}, digits #14171C on plate {:.2}",
            contrast([0xff, 0x6b, 0x6b], dk),
            contrast([0x14, 0x17, 0x1c], [0xff, 0x6b, 0x6b])
        );
        println!(
            "alert plate light #C42B2B vs bar {:.2}, white digits on plate {:.2}",
            contrast([0xc4, 0x2b, 0x2b], lt),
            contrast([255, 255, 255], [0xc4, 0x2b, 0x2b])
        );
        let over = |fg: [u8; 3], a: f64, bg: [u8; 3]| -> [u8; 3] {
            let m = |f: u8, b: u8| (f64::from(f) * a + f64::from(b) * (1.0 - a)).round() as u8;
            [m(fg[0], bg[0]), m(fg[1], bg[1]), m(fg[2], bg[2])]
        };
        let (td, tl) = (over([255, 255, 255], 0.30, dk), over([0x1b, 0x1b, 0x1f], 0.30, lt));
        println!("track dark (white 30 % on #202020 = {td:02X?}) {:.2}", contrast(td, dk));
        println!("track light (#1B1B1F 30 % on #F3F3F3 = {tl:02X?}) {:.2}", contrast(tl, lt));
        println!(
            "accent fill against its own track: dark {:.2}, light {:.2}",
            contrast([0xf0, 0x63, 0xa8], td),
            contrast([0xb0, 0x1e, 0x6e], tl)
        );
        println!("## Ink coverage at 16 px (alpha >= 128), dark taskbar");
        for (k, r, st) in win_items() {
            let ic = prender(&k, &r, st, 16, pix::Theme { dark: true });
            let n = ic.pixels().iter().filter(|p| p.alpha() >= 128).count();
            println!("{:?} {:?}: {:.0} %", k, st, n as f32 / 256.0 * 100.0);
        }
    }

    pub fn run() {
        let font = Font::system();
        report(&font);
        mac_sheet(&font, 1.0);
        mac_sheet(&font, 2.0);
        mac_catalogue(&font);
        windows_sheet(&font);
        linux_sheet(&font);
        today_sheet(&font);
        numerals_sheet(&font);
    }
}
