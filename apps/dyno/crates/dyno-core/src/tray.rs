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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_from_the_bottom() {
        let icon = render(&[(TrayMetric::Cpu, 100.0), (TrayMetric::Memory, 0.0)], &[], TrayStyle::Bars, 32, true);
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
        let two = render(&[(TrayMetric::Cpu, 50.0), (TrayMetric::Memory, 50.0)], &[], TrayStyle::Bars, 32, true);
        assert_eq!(two.pixel(4, 29), HUE_ON_DARK);
        assert_eq!(two.pixel(20, 29)[3], 0x99, "the second metric is the 60 % tone");
        assert_eq!(short.pixel(20, 20), track(true));
    }

    #[test]
    fn tooltips() {
        assert_eq!(tooltip(&[]), "Dyno");
        assert_eq!(
            tooltip(&[(TrayMetric::Cpu, 23.4), (TrayMetric::Memory, 61.0), (TrayMetric::Disk, 1.0), (TrayMetric::Network, 2.0), (TrayMetric::Gpu, 3.0)]),
            "Dyno — CPU 23% · Memory 61% · Disk 1% · Network 2% · GPU 3%"
        );
    }
}
