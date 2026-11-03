//! How a menu-bar / tray item looks: the mapping from an item's spec and reading to the approved renderers
//! (`dyno_core::tray::{menu_bar, pixel}`, design `docs/design/dyno-tray/README.md`), and the state rules (alert with hysteresis).
//! Pure: the text port, the pixel size and the clock (the sample's own timestamp) are passed in. One function per platform
//! returns the pictures and the item's fixed width; the mechanics around them (identity, one image set, sync) do not care.

use crate::tray_model::{Glyph, ItemSpec, Picture};
use dyno_core::tray::text::TextRaster;
use dyno_core::tray::{menu_bar as mb, pixel as px};
use dyno_core::widgets::Metric;

/// The percentage at which a load item goes into alert, and the degrees for a temperature (the design leaves the level to the app).
pub const ALERT_LEVEL: f32 = 90.0;
/// How long the level must hold before the alert shows (the sample timestamps' milliseconds).
pub const ALERT_HOLD_MS: i64 = 3_000;
/// How far below the level the value must fall before the alert goes.
pub const ALERT_HYSTERESIS: f32 = 5.0;

/// A picture set and the fixed size of the item it makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// macOS: the 1x and @2x templates; Windows and Linux: one icon.
    pub pictures: Vec<Picture>,
    /// Width in points (macOS) or pixels, fixed for the format.
    pub width: u32,
    /// Height in points (macOS) or pixels.
    pub height: u32,
    /// Whether the pictures are templates (macOS).
    pub template: bool,
}

/// The state and numbers of an item, platform-neutral.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    /// The value (percent, or degrees); `None` when it cannot be measured.
    pub value: Option<f32>,
    /// Recent values, oldest first.
    pub history: Vec<f32>,
    /// Per-core loads.
    pub cores: Vec<f32>,
    /// Bytes per second up.
    pub up: f64,
    /// Bytes per second down.
    pub down: f64,
}

/// The short label an item carries (up to three capitals), or the user's own.
pub fn short_label(spec: &ItemSpec) -> String {
    if !spec.label.is_empty() {
        return spec.label.chars().take(4).collect::<String>().to_uppercase();
    }
    match &spec.metric {
        Metric::CpuTotal | Metric::CpuPerCore => "CPU".into(),
        Metric::Memory | Metric::MemoryPressure => "MEM".into(),
        Metric::Swap => "SWP".into(),
        Metric::DiskActivity | Metric::DiskFree => "DSK".into(),
        Metric::NetworkUp | Metric::NetworkDown => "NET".into(),
        Metric::GpuLoad => "GPU".into(),
        Metric::Temperature => "TMP".into(),
        Metric::Fan => "FAN".into(),
        Metric::Battery => "BAT".into(),
        Metric::Process(n) | Metric::PerfCounter(n) => n.chars().take(3).collect::<String>().to_uppercase(),
    }
}

fn is_network(m: &Metric) -> bool {
    matches!(m, Metric::NetworkUp | Metric::NetworkDown)
}

/// The macOS format of an item (`Ring` is the meter there; `Number` is the stacked number, the default).
pub fn mac_kind(spec: &ItemSpec, cores: usize) -> mb::Kind {
    if is_network(&spec.metric) {
        return mb::Kind::Network;
    }
    if spec.metric == Metric::Temperature {
        return mb::Kind::Temperature;
    }
    let label = short_label(spec);
    let unit = mb::Unit::Percent;
    match spec.glyph {
        Glyph::CoreBars => mb::Kind::Cores { cores },
        Glyph::Number => mb::Kind::Stacked { label, unit },
        Glyph::Bars => mb::Kind::Bar { label },
        Glyph::Graph => mb::Kind::Graph { label },
        Glyph::GraphNumber => mb::Kind::GraphValue { label, unit },
        Glyph::Meter | Glyph::Ring => mb::Kind::Meter { label },
    }
}

/// The Windows / Linux format of an item.
pub fn px_kind(spec: &ItemSpec) -> px::Kind {
    if is_network(&spec.metric) {
        return px::Kind::Network;
    }
    if spec.metric == Metric::Temperature {
        return px::Kind::Temperature;
    }
    let label: String = short_label(spec).chars().take(3).collect();
    match spec.glyph {
        Glyph::CoreBars => px::Kind::Cores,
        Glyph::Number => px::Kind::Percent { label },
        Glyph::Bars | Glyph::Meter => px::Kind::Meter { label },
        Glyph::Graph | Glyph::GraphNumber => px::Kind::Graph { label },
        Glyph::Ring => px::Kind::Ring {
            letter: label.chars().next().unwrap_or('?'),
        },
    }
}

/// The level above which a metric is in alert; none for metrics with no "too high".
pub fn alert_level(m: &Metric) -> Option<f32> {
    match m {
        Metric::CpuTotal
        | Metric::CpuPerCore
        | Metric::Memory
        | Metric::Swap
        | Metric::DiskActivity
        | Metric::GpuLoad
        | Metric::Temperature => Some(ALERT_LEVEL),
        _ => None,
    }
}

/// An item's alert: on once the level has held [`ALERT_HOLD_MS`], off once the value is [`ALERT_HYSTERESIS`] below it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AlertGate {
    on: bool,
    since: Option<i64>,
}

impl AlertGate {
    /// Feeds one sample (`now_ms` is the sample's own time) and says whether the alert shows.
    pub fn update(&mut self, value: Option<f32>, level: Option<f32>, now_ms: i64) -> bool {
        let (Some(v), Some(level)) = (value.filter(|v| v.is_finite()), level) else {
            *self = AlertGate::default();
            return false;
        };
        if self.on {
            if v < level - ALERT_HYSTERESIS {
                *self = AlertGate::default();
            }
        } else if v >= level {
            let since = *self.since.get_or_insert(now_ms);
            self.on = now_ms - since >= ALERT_HOLD_MS;
        } else {
            self.since = None;
        }
        self.on
    }
}

/// Renders the macOS pictures: a 1x and a @2x template of the same point size.
pub fn render_mac(kind: &mb::Kind, s: &Sample, state: mb::State, text: &dyn TextRaster) -> Look {
    let r = mb::Reading {
        value: s.value,
        history: s.history.clone(),
        cores: s.cores.clone(),
        up: s.up,
        down: s.down,
    };
    let width = mb::width(kind, text) as u32;
    let pictures = [1u32, 2]
        .iter()
        .map(|scale| {
            let t = mb::render(kind, &r, state, text, *scale);
            Picture {
                rgba: t.rgba(),
                width: t.width,
                height: t.height,
            }
        })
        .collect();
    Look {
        pictures,
        width,
        height: mb::HEIGHT as u32,
        template: true,
    }
}

/// Renders the Windows / Linux icon at `size` pixels.
pub fn render_px(kind: &px::Kind, s: &Sample, state: px::State, size: u32, dark: bool) -> Look {
    let r = px::Reading {
        value: s.value,
        history: s.history.clone(),
        cores: s.cores.clone(),
        up: s.up,
        down: s.down,
    };
    let icon = px::render(kind, &r, state, size as i32, px::Theme { dark });
    Look {
        pictures: vec![Picture {
            rgba: icon.rgba,
            width: icon.size,
            height: icon.size,
        }],
        width: icon.size,
        height: icon.size,
        template: false,
    }
}

/// The notification-area icon size for a taskbar DPI (100 % is 96): 16, 20, 24 and 32 px at 100, 125, 150 and 200 %, and the 32
/// layout from 250 % up (the design's table).
pub fn taskbar_px(dpi: u32) -> u32 {
    match dpi {
        0..=108 => 16,
        109..=132 => 20,
        133..=167 => 24,
        _ => 32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::tray::text::BoxText;

    fn spec(metric: Metric, glyph: Glyph) -> ItemSpec {
        ItemSpec::new("x-1", metric, glyph)
    }

    #[test]
    fn formats_follow_the_design_table() {
        assert!(matches!(
            mac_kind(&spec(Metric::CpuTotal, Glyph::Number), 8),
            mb::Kind::Stacked { .. }
        ));
        assert!(
            matches!(mac_kind(&spec(Metric::Memory, Glyph::Ring), 8), mb::Kind::Meter { .. }),
            "ring is the meter on macOS"
        );
        assert!(matches!(
            mac_kind(&spec(Metric::CpuPerCore, Glyph::CoreBars), 10),
            mb::Kind::Cores { cores: 10 }
        ));
        assert!(matches!(mac_kind(&spec(Metric::NetworkDown, Glyph::Number), 8), mb::Kind::Network));
        assert!(matches!(mac_kind(&spec(Metric::NetworkUp, Glyph::Graph), 8), mb::Kind::Network));
        assert!(matches!(
            mac_kind(&spec(Metric::Temperature, Glyph::Ring), 8),
            mb::Kind::Temperature
        ));
        assert!(matches!(
            px_kind(&spec(Metric::Memory, Glyph::Ring)),
            px::Kind::Ring { letter: 'M' }
        ));
        assert!(matches!(px_kind(&spec(Metric::CpuTotal, Glyph::Number)), px::Kind::Percent { .. }));
    }

    #[test]
    fn labels_are_short_capitals_or_the_users_own() {
        assert_eq!(short_label(&spec(Metric::Memory, Glyph::Number)), "MEM");
        let mut s = spec(Metric::Memory, Glyph::Number);
        s.label = "ram".into();
        assert_eq!(short_label(&s), "RAM");
        assert_eq!(short_label(&spec(Metric::Process("firefox".into()), Glyph::Number)), "FIR");
    }

    #[test]
    fn the_alert_needs_three_seconds_and_leaves_five_points_below() {
        let mut g = AlertGate::default();
        let l = Some(90.0);
        assert!(!g.update(Some(95.0), l, 0));
        assert!(!g.update(Some(95.0), l, 2_999));
        assert!(g.update(Some(95.0), l, 3_000));
        assert!(g.update(Some(86.0), l, 4_000), "still on inside the hysteresis band");
        assert!(!g.update(Some(84.0), l, 5_000), "off five points below");
        assert!(!g.update(Some(95.0), l, 6_000), "the hold starts again");
        assert!(!g.update(Some(10.0), l, 7_000));
        assert!(!g.update(Some(95.0), l, 8_000));
        assert!(!g.update(Some(f32::NAN), l, 20_000), "NaN is no alert");
        assert!(!g.update(None, l, 21_000));
        assert!(!g.update(Some(99.0), None, 30_000), "a metric with no level never alerts");
    }

    #[test]
    fn a_dip_below_the_level_restarts_the_hold() {
        let mut g = AlertGate::default();
        let l = Some(90.0);
        g.update(Some(95.0), l, 0);
        g.update(Some(50.0), l, 2_000);
        assert!(!g.update(Some(95.0), l, 3_500));
        assert!(g.update(Some(95.0), l, 6_500));
    }

    #[test]
    fn the_taskbar_size_follows_the_dpi_table() {
        let sizes: Vec<u32> = [96, 120, 144, 192, 240, 288].iter().map(|d| taskbar_px(*d)).collect();
        assert_eq!(sizes, [16, 20, 24, 32, 32, 32]);
    }

    #[test]
    fn mac_pictures_share_one_point_size_and_the_width_never_follows_the_value() {
        let kinds = [
            mb::Kind::Stacked {
                label: "CPU".into(),
                unit: mb::Unit::Percent,
            },
            mb::Kind::Meter { label: "GPU".into() },
            mb::Kind::Network,
            mb::Kind::Temperature,
            mb::Kind::Cores { cores: 8 },
            mb::Kind::Graph { label: "DSK".into() },
        ];
        for kind in &kinds {
            let mut shapes = std::collections::HashSet::new();
            for (v, st) in [
                (Some(0.0), mb::State::Normal),
                (Some(7.0), mb::State::Normal),
                (Some(100.0), mb::State::Alert),
                (None, mb::State::Unavailable),
            ] {
                let look = render_mac(
                    kind,
                    &Sample {
                        value: v,
                        history: vec![5.0; 30],
                        cores: vec![50.0; 8],
                        up: 1e6,
                        down: 5e3,
                    },
                    st,
                    &BoxText,
                );
                assert_eq!(look.pictures.len(), 2);
                assert_eq!(look.pictures[0].width, look.width, "1x is one pixel per point: {kind:?}");
                assert_eq!(look.pictures[1].width, look.width * 2, "@2x: {kind:?}");
                assert_eq!(look.pictures[1].height, look.height * 2);
                shapes.insert((look.width, look.height));
            }
            assert_eq!(shapes.len(), 1, "{kind:?} changed size");
        }
    }

    #[test]
    fn windows_icons_are_square_at_the_exact_size_in_every_state() {
        for size in [16u32, 20, 24, 32] {
            for (v, st) in [
                (Some(5.0), px::State::Normal),
                (Some(99.0), px::State::Alert),
                (None, px::State::Unavailable),
            ] {
                let look = render_px(
                    &px::Kind::Percent { label: "CPU".into() },
                    &Sample {
                        value: v,
                        ..Sample::default()
                    },
                    st,
                    size,
                    true,
                );
                assert_eq!(
                    (look.width, look.height, look.pictures[0].rgba.len()),
                    (size, size, (size * size * 4) as usize)
                );
            }
        }
    }
}
