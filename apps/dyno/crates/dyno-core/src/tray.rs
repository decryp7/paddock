//! The tray / menu-bar icon: small live bars or a graph, drawn as RGBA pixels (pure; the shell hands them to the OS).
//!
//! Bars: one rounded column per metric (up to three), filled from the bottom, on a faint track. Graph: the first
//! metric's recent values as a filled area. Colours follow the suite icon family: Dyno's hue, the first metric at full
//! ink and the others at 60 %, the high-load orange when pegged; the track adapts to a light or dark taskbar. The tooltip carries the numbers, so the icon never stands alone.

use crate::settings::{TrayMetric, TrayStyle};

/// An RGBA colour.
pub type Rgba = [u8; 4];

/// Dyno's hue on a dark taskbar (the suite icon's top tone, `#DE559F`).
pub const HUE_ON_DARK: Rgba = [0xde, 0x55, 0x9f, 0xff];
/// Dyno's hue on a light taskbar (the icon's bottom tone, `#A0226A`).
pub const HUE_ON_LIGHT: Rgba = [0xa0, 0x22, 0x6a, 0xff];
/// The suite's "high load" tray colour (`#F2711C`), used at or above [`HIGH_LOAD`].
pub const HIGH: Rgba = [0xf2, 0x71, 0x1c, 0xff];
/// A value at or above this draws in [`HIGH`].
pub const HIGH_LOAD: f32 = 90.0;

/// The fill of the metric at `index` showing `value`: the family's two-tone rule (the first metric at full ink, the
/// others at 60 %), in Dyno's hue for the taskbar, or the high-load colour when pegged.
pub fn colour(index: usize, value: f32, dark_taskbar: bool) -> Rgba {
    let mut c = if value >= HIGH_LOAD {
        HIGH
    } else if dark_taskbar {
        HUE_ON_DARK
    } else {
        HUE_ON_LIGHT
    };
    if index > 0 {
        c[3] = 0x99;
    }
    c
}

/// A square icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    /// Width and height in pixels.
    pub size: u32,
    /// Row-major RGBA bytes (`size * size * 4`).
    pub rgba: Vec<u8>,
}

impl Icon {
    fn new(size: u32) -> Icon {
        Icon {
            size,
            rgba: vec![0; (size * size * 4) as usize],
        }
    }

    fn put(&mut self, x: u32, y: u32, c: Rgba) {
        if x < self.size && y < self.size {
            let i = ((y * self.size + x) * 4) as usize;
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }

    /// The pixel at (x, y).
    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        let i = ((y * self.size + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    fn rect(&mut self, x0: u32, y0: u32, x1: u32, y1: u32, c: Rgba) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y, c);
            }
        }
    }
}

fn track(dark_taskbar: bool) -> Rgba {
    if dark_taskbar {
        [0xff, 0xff, 0xff, 0x38]
    } else {
        [0x00, 0x00, 0x00, 0x30]
    }
}

/// Draws the icon. `values` are the metrics with their current value (0–100); `graph` the first metric's recent
/// values, oldest first, for [`TrayStyle::Graph`]. `size` is clamped to 16–64.
pub fn render(values: &[(TrayMetric, f32)], graph: &[f32], style: TrayStyle, size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let pad = size / 16;
    match style {
        TrayStyle::Bars => {
            let n = values.len().clamp(1, 3) as u32;
            let gap = (size / 10).max(1);
            let width = (size - 2 * pad - gap * (n - 1)) / n;
            for (i, (_, v)) in values.iter().take(3).enumerate() {
                let x0 = pad + i as u32 * (width + gap);
                let (top, bottom) = (pad, size - pad);
                icon.rect(x0, top, x0 + width, bottom, track(dark_taskbar));
                let h = ((bottom - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as u32;
                // At least one row when above zero, so "busy a little" is visible.
                let h = if *v > 0.5 { h.max(1) } else { h };
                icon.rect(x0, bottom - h, x0 + width, bottom, colour(i, *v, dark_taskbar));
            }
        }
        TrayStyle::Graph => {
            let (top, bottom) = (pad, size - pad);
            icon.rect(pad, top, size - pad, bottom, track(dark_taskbar));
            let cols = size - 2 * pad;
            let recent: Vec<f32> = graph.iter().rev().take(cols as usize).rev().copied().collect();
            let start = cols - recent.len() as u32;
            for (i, v) in recent.iter().enumerate() {
                let h = ((bottom - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as u32;
                let x = pad + start + i as u32;
                icon.rect(x, bottom - h, x + 1, bottom, colour(0, *v, dark_taskbar));
            }
        }
    }
    icon
}

/// The tooltip next to the icon, e.g. "Dyno — CPU 23% · Memory 61%".
pub fn tooltip(values: &[(TrayMetric, f32)]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|(m, v)| {
            let name = match m {
                TrayMetric::Cpu => "CPU",
                TrayMetric::Memory => "Memory",
                TrayMetric::Disk => "Disk",
                TrayMetric::Network => "Network",
                TrayMetric::Gpu => "GPU",
            };
            format!("{name} {v:.0}%")
        })
        .collect();
    if parts.is_empty() {
        "Dyno".into()
    } else {
        format!("Dyno — {}", parts.join(" · "))
    }
}

// ---------------------------------------------------------------------------------------------- more glyphs
//
// Independent menu-bar items (one per metric) draw one of these: a horizontal meter, a ring, per-core bars, a number, or a
// graph with the number above it. Same colours and track as the bars above.

/// 3x5 digit shapes, one row per `u8` (bit 2 = left column).
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

fn ink(value: f32, dark_taskbar: bool) -> Rgba {
    colour(0, value, dark_taskbar)
}

/// A horizontal meter: a track across the middle, filled from the left.
pub fn render_meter(value: f32, size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let pad = size / 16;
    let (h, x1) = ((size / 4).max(3), size - pad);
    let top = (size - h) / 2;
    icon.rect(pad, top, x1, top + h, track(dark_taskbar));
    let w = ((x1 - pad) as f32 * value.clamp(0.0, 100.0) / 100.0).round() as u32;
    let w = if value > 0.5 { w.max(1) } else { w };
    icon.rect(pad, top, pad + w, top + h, ink(value, dark_taskbar));
    icon
}

/// A ring filled clockwise from the top.
pub fn render_ring(value: f32, size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let c = size as f32 / 2.0;
    let outer = c - (size / 16) as f32 - 0.5;
    let inner = outer - (size as f32 / 6.0).max(2.0);
    let filled = value.clamp(0.0, 100.0) / 100.0 * std::f32::consts::TAU;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let d = (dx * dx + dy * dy).sqrt();
            if d < inner || d > outer {
                continue;
            }
            let mut angle = dx.atan2(-dy);
            if angle < 0.0 {
                angle += std::f32::consts::TAU;
            }
            icon.put(
                x,
                y,
                if angle <= filled {
                    ink(value, dark_taskbar)
                } else {
                    track(dark_taskbar)
                },
            );
        }
    }
    icon
}

/// One thin column per core (up to 32), filled from the bottom.
pub fn render_core_bars(values: &[f32], size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let n = values.len().clamp(1, 32) as u32;
    let pad = size / 16;
    let gap = u32::from(n * 2 <= size - 2 * pad);
    let width = ((size - 2 * pad + gap) / n).saturating_sub(gap).max(1);
    let (top, bottom) = (pad, size - pad);
    for (i, v) in values.iter().take(32).enumerate() {
        let x0 = pad + i as u32 * (width + gap);
        let h = ((bottom - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as u32;
        let h = if *v > 0.5 { h.max(1) } else { h };
        icon.rect(x0, top, x0 + width, bottom, track(dark_taskbar));
        icon.rect(x0, bottom - h, x0 + width, bottom, ink(*v, dark_taskbar));
    }
    icon
}

fn draw_number(icon: &mut Icon, value: f32, top: u32, scale: u32, c: Rgba) {
    let text = format!("{}", value.clamp(0.0, 100.0).round() as u32);
    let n = text.len() as u32;
    let width = n * 3 * scale + (n - 1) * scale;
    let x0 = icon.size.saturating_sub(width) / 2;
    for (i, ch) in text.bytes().enumerate() {
        let rows = DIGITS[usize::from(ch - b'0')];
        for (ry, row) in rows.iter().enumerate() {
            for rx in 0..3u32 {
                if row & (0b100 >> rx) != 0 {
                    let x = x0 + i as u32 * 4 * scale + rx * scale;
                    icon.rect(x, top + ry as u32 * scale, x + scale, top + (ry as u32 + 1) * scale, c);
                }
            }
        }
    }
}

/// The value as a number (0-100, no unit; the tooltip names the metric), as large as fits.
pub fn render_number(value: f32, size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let digits = format!("{}", value.clamp(0.0, 100.0).round() as u32).len() as u32;
    let scale = (size / (digits * 4)).clamp(1, size / 6);
    draw_number(&mut icon, value, (size - 5 * scale) / 2, scale, ink(value, dark_taskbar));
    icon
}

/// The value as a small number over a graph of its recent values.
pub fn render_graph_number(value: f32, graph: &[f32], size: u32, dark_taskbar: bool) -> Icon {
    let size = size.clamp(16, 64);
    let mut icon = Icon::new(size);
    let scale = (size / 16).max(1);
    let pad = size / 16;
    draw_number(&mut icon, value, pad, scale, ink(value, dark_taskbar));
    let top = pad + 5 * scale + scale.max(1);
    let bottom = size - pad;
    icon.rect(pad, top, size - pad, bottom, track(dark_taskbar));
    let cols = size - 2 * pad;
    let recent: Vec<f32> = graph.iter().rev().take(cols as usize).rev().copied().collect();
    let start = cols - recent.len() as u32;
    for (i, v) in recent.iter().enumerate() {
        let h = ((bottom - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as u32;
        let x = pad + start + i as u32;
        icon.rect(x, bottom - h, x + 1, bottom, ink(*v, dark_taskbar));
    }
    icon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_from_the_bottom() {
        let icon = render(
            &[(TrayMetric::Cpu, 100.0), (TrayMetric::Memory, 0.0)],
            &[],
            TrayStyle::Bars,
            32,
            true,
        );
        assert_eq!(icon.rgba.len(), 32 * 32 * 4);
        assert_eq!(icon.pixel(4, 29), HIGH);
        assert_eq!(icon.pixel(4, 3), HIGH, "pegged: the high-load colour");
        assert_eq!(icon.pixel(24, 29), track(true), "empty bar shows its track");
        assert_eq!(icon.pixel(0, 0)[3], 0, "padding is transparent");
        let half = render(&[(TrayMetric::Disk, 50.0)], &[], TrayStyle::Bars, 32, false);
        assert_eq!(half.pixel(16, 29), HUE_ON_LIGHT);
        assert_eq!(half.pixel(16, 4), track(false));
        let tiny = render(&[(TrayMetric::Gpu, 1.0)], &[], TrayStyle::Bars, 16, false);
        assert_eq!(tiny.pixel(8, 14), HUE_ON_LIGHT, "a little load still shows");
        let none = render(&[], &[], TrayStyle::Bars, 8, false);
        assert_eq!(none.size, 16);
    }

    #[test]
    fn graphs_scroll_left() {
        let history: Vec<f32> = (0..100).map(|i| if i == 99 { 100.0 } else { 0.0 }).collect();
        let icon = render(&[(TrayMetric::Network, 0.0)], &history, TrayStyle::Graph, 32, true);
        assert_eq!(icon.pixel(29, 5), HIGH, "the newest value is at the right edge");
        assert_eq!(icon.pixel(10, 29), track(true));
        let short = render(&[], &[100.0], TrayStyle::Graph, 32, true);
        assert_eq!(short.pixel(29, 20), HIGH);
        let two = render(
            &[(TrayMetric::Cpu, 50.0), (TrayMetric::Memory, 50.0)],
            &[],
            TrayStyle::Bars,
            32,
            true,
        );
        assert_eq!(two.pixel(4, 29), HUE_ON_DARK);
        assert_eq!(two.pixel(20, 29)[3], 0x99, "the second metric is the 60 % tone");
        assert_eq!(short.pixel(20, 20), track(true));
    }

    #[test]
    fn tooltips() {
        assert_eq!(tooltip(&[]), "Dyno");
        assert_eq!(
            tooltip(&[
                (TrayMetric::Cpu, 23.4),
                (TrayMetric::Memory, 61.0),
                (TrayMetric::Disk, 1.0),
                (TrayMetric::Network, 2.0),
                (TrayMetric::Gpu, 3.0)
            ]),
            "Dyno — CPU 23% · Memory 61% · Disk 1% · Network 2% · GPU 3%"
        );
    }

    fn ink_pixels(i: &Icon) -> usize {
        (0..i.size)
            .flat_map(|y| (0..i.size).map(move |x| (x, y)))
            .filter(|&(x, y)| i.pixel(x, y)[3] == 0xff)
            .count()
    }

    #[test]
    fn meters_fill_from_the_left() {
        let m = render_meter(50.0, 32, true);
        assert_eq!(m.pixel(3, 16), HUE_ON_DARK);
        assert_eq!(m.pixel(28, 16), track(true));
        assert_eq!(render_meter(100.0, 32, true).pixel(28, 16), HIGH);
        assert_eq!(m.pixel(3, 2)[3], 0);
    }

    #[test]
    fn rings_fill_clockwise_from_the_top() {
        let r = render_ring(50.0, 32, true);
        assert_eq!(r.pixel(16, 4), HUE_ON_DARK, "top is filled");
        assert_eq!(r.pixel(27, 16), HUE_ON_DARK, "right side is filled at 50 %");
        assert_eq!(r.pixel(4, 16), track(true), "left side is not");
        assert_eq!(r.pixel(16, 16)[3], 0, "the middle is open");
        assert!(ink_pixels(&render_ring(25.0, 32, true)) < ink_pixels(&r));
    }

    #[test]
    fn core_bars_draw_one_column_per_core() {
        let v: Vec<f32> = (0..8).map(|i| i as f32 * 12.0).collect();
        let b = render_core_bars(&v, 32, true);
        assert_eq!(b.pixel(2, 29), track(true), "an idle core shows its track");
        assert_eq!(render_core_bars(&[100.0; 32], 32, true).rgba.len(), 32 * 32 * 4);
        assert_eq!(render_core_bars(&[], 16, true).size, 16, "no cores: no panic");
    }

    #[test]
    fn numbers_and_graph_numbers_draw_digits() {
        assert!(ink_pixels(&render_number(8.0, 32, true)) > 0);
        assert_ne!(render_number(8.0, 32, true), render_number(88.0, 32, true));
        assert!(
            ink_pixels(&render_number(100.0, 16, true)) > 0,
            "three digits fit the smallest icon"
        );
        let g = render_graph_number(42.0, &[10.0, 90.0], 32, true);
        assert!(ink_pixels(&g) > 0);
        assert_eq!(render_graph_number(0.0, &[], 8, false).size, 16);
    }
}
