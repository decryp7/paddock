//! macOS menu-bar items: template images (black plus alpha) 18 pt high, one fixed width per format, drawn separately at 1x
//! and @2x (design: `docs/design/dyno-tray/README.md`).
//!
//! Lengths are in points; `scale` is pixels per point. Rectangles, bars and baselines snap to whole device pixels at each
//! scale; text (through the [`TextRaster`] port) and arcs keep their anti-aliased edges.

use super::text::{TextRaster, TextStyle};
use tiny_skia::{BlendMode, Color, FillRule, Mask, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke, Transform};

/// Image height in points (tray-icon sets every status-item image to 18 pt).
pub const HEIGHT: f32 = 18.0;
/// Baseline of one-line text, points below the image top: 13 pt caps centred on the 24 pt bar like the clock's.
pub const BASELINE: f32 = 13.5;
/// Alpha of tracks and empty parts.
pub const TRACK: f32 = 0.5;
/// Alpha of an area graph's fill.
pub const AREA: f32 = 0.38;
/// Alpha of a graph's plate.
pub const PLATE: f32 = 0.14;
/// Alpha of an unavailable item.
pub const DIM: f32 = 0.45;
/// Space between a label and what follows it.
pub const GAP: f32 = 4.0;
/// Inset of a label from the item's left edge, so its alert plate fits inside the image.
pub const LABEL_INSET: f32 = 1.5;

/// One-line values: the menu bar's own font and size.
pub const VALUE: TextStyle = TextStyle::new(13.0);
/// Labels before a value or a mark.
pub const LABEL: TextStyle = TextStyle::new(9.5).weight(0.3).tracking(0.35);
/// The stack's label line.
pub const STACK_LABEL: TextStyle = TextStyle::new(8.0).weight(0.3).tracking(0.45);
/// The stack's value line.
pub const STACK_VALUE: TextStyle = TextStyle::new(11.0).weight(0.23);
/// The network pair's lines.
pub const PAIR: TextStyle = TextStyle::new(9.0).weight(0.23);

/// What a value means, for its format and field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// 0 to 100 with "%".
    Percent,
    /// Degrees, 0 to 199, with "°".
    Degrees,
}

/// The formats of a menu-bar item.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A label and a value on one line ("CPU 23%"); the label hugs the number.
    Value {
        /// The label.
        label: String,
        /// The unit.
        unit: Unit,
    },
    /// The label over the value (the default number format: the narrowest).
    Stacked {
        /// The label.
        label: String,
        /// The unit.
        unit: Unit,
    },
    /// A label and a horizontal meter.
    Meter {
        /// The label.
        label: String,
    },
    /// A label and one vertical bar.
    Bar {
        /// The label.
        label: String,
    },
    /// One column per core.
    Cores {
        /// The number of cores.
        cores: usize,
    },
    /// A label and an area graph of the recent values, one sample per point.
    Graph {
        /// The label.
        label: String,
    },
    /// An area graph with the stack beside it.
    GraphValue {
        /// The label.
        label: String,
        /// The unit.
        unit: Unit,
    },
    /// One direction's rate with its arrow and unit.
    Network(super::NetDir),
    /// A thermometer and the degrees.
    Temperature,
}

/// What an item shows now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    /// The value (0 to 100, or degrees); `None` when unavailable.
    pub value: Option<f32>,
    /// Recent values, oldest first (graphs).
    pub history: Vec<f32>,
    /// Per-core loads.
    pub cores: Vec<f32>,
    /// Bytes per second up.
    pub up: f64,
    /// Bytes per second down.
    pub down: f64,
}

/// The item's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// As measured.
    Normal,
    /// Over its threshold: the number (or the label) on a solid plate, knocked out. A shape, so it reads in a template.
    Alert,
    /// Not measurable, or the agent away: an en dash in the field and the item dimmed; the same width.
    Unavailable,
}

/// A template image: black, with this alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels per point.
    pub scale: u32,
    /// Row-major alpha.
    pub alpha: Vec<u8>,
}

impl Template {
    /// RGBA bytes: black with the alpha. The colour is 0, so these bytes are both straight and premultiplied, whichever the OS
    /// representation expects.
    pub fn rgba(&self) -> Vec<u8> {
        self.alpha.iter().flat_map(|a| [0, 0, 0, *a]).collect()
    }

    /// The alpha at (x, y).
    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.alpha.get((y * self.width + x) as usize).copied().unwrap_or(0)
    }
}

/// `v` points snapped to whole device pixels.
pub fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

/// The baseline of one-line text at `scale`, on a whole device pixel, rounded **down**: 13.5 pt is half a pixel at 1x, and
/// rounding it up put the digits one pixel below the clock's in the first macOS VM measurement (ink rows 11 to 19 against the
/// clock's 9 to 18 at 1x). At @2x 13.5 pt is already a whole pixel (27), which stays; @2x could not be measured in Tart.
pub fn baseline(scale: f32) -> f32 {
    (BASELINE * scale).floor() / scale
}

/// The number as the field shows it.
pub fn format_value(v: f32, unit: Unit) -> String {
    match unit {
        Unit::Percent => format!("{}", v.clamp(0.0, 100.0).round() as u32),
        Unit::Degrees => format!("{}", v.clamp(0.0, 199.0).round() as u32),
    }
}

fn unit_text(unit: Unit) -> &'static str {
    match unit {
        Unit::Percent => "%",
        Unit::Degrees => "°",
    }
}

/// A rate as up to three figures and a unit: "0", "9.4", "12", "512", then the next unit ("1.2" MB/s); never "1000".
pub fn format_rate(bps: f64) -> (String, &'static str) {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut v = if bps.is_finite() { bps.max(0.0) } else { 0.0 };
    let mut i = 0;
    while v >= 999.5 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    let text = if v < 9.95 && i > 0 {
        format!("{v:.1}")
    } else {
        format!("{}", v.round() as u64)
    };
    (text, UNITS[i])
}

fn field(t: &dyn TextRaster, s: &TextStyle) -> f32 {
    t.advance("000", s)
}

fn label_width(t: &dyn TextRaster, label: &str) -> f32 {
    LABEL_INSET + t.advance(&label.to_uppercase(), &LABEL)
}

/// Inset of the stack's text from its sides, so its alert plate (the whole stack) has room around the text.
pub const STACK_INSET: f32 = 2.0;

fn stack_width(t: &dyn TextRaster, label: &str, unit: Unit) -> f32 {
    let l = t.advance(&label.to_uppercase(), &STACK_LABEL);
    let v = field(t, &STACK_VALUE) + t.advance(unit_text(unit), &STACK_VALUE);
    l.max(v) + 2.0 * STACK_INSET
}

struct NetLayout {
    arrow: f32,
    num: f32,
    total: f32,
}

fn net_layout(t: &dyn TextRaster) -> NetLayout {
    let arrow = 5.0;
    let num = field(t, &PAIR);
    let unit = ["B/s", "KB/s", "MB/s", "GB/s"]
        .iter()
        .map(|u| t.advance(u, &PAIR))
        .fold(0.0, f32::max);
    NetLayout {
        arrow,
        num,
        total: arrow + 2.0 + num + 2.0 + unit,
    }
}

/// The fixed width of an item in whole points (so the 1x and @2x images share one point size). It depends only on the
/// format and the font, never on the value or the state.
pub fn width(kind: &Kind, t: &dyn TextRaster) -> f32 {
    let w = match kind {
        Kind::Value { label, unit } => label_width(t, label) + 3.0 + field(t, &VALUE) + t.advance(unit_text(*unit), &VALUE),
        Kind::Stacked { label, unit } => stack_width(t, label, *unit),
        Kind::Meter { label } => label_width(t, label) + GAP + 24.0,
        Kind::Bar { label } => label_width(t, label) + GAP - 1.0 + 5.0,
        Kind::Cores { cores } => (*cores).clamp(1, 64) as f32 * 3.0 - 1.0,
        Kind::Graph { label } => label_width(t, label) + GAP + 30.0,
        Kind::GraphValue { label, unit } => 24.0 + GAP + stack_width(t, label, *unit),
        Kind::Network(_) => net_layout(t).total,
        Kind::Temperature => 6.0 + 3.0 + field(t, &VALUE) + t.advance("°", &VALUE),
    };
    w.ceil()
}

fn ink(a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba(0.0, 0.0, 0.0, a.clamp(0.0, 1.0)).unwrap_or(Color::BLACK));
    p.anti_alias = true;
    p
}

fn knock() -> Paint<'static> {
    let mut p = ink(1.0);
    p.blend_mode = BlendMode::DestinationOut;
    p
}

struct Canvas<'a> {
    pm: Pixmap,
    scale: f32,
    text: &'a dyn TextRaster,
}

impl Canvas<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, p: &Paint) {
        let s = self.scale;
        let (x0, y0) = (snap(x, s) * s, snap(y, s) * s);
        let (x1, y1) = (snap(x + w, s) * s, snap(y + h, s) * s);
        if let Some(r) = Rect::from_ltrb(x0, y0, x1.max(x0 + 1.0), y1.max(y0 + 1.0)) {
            self.pm.fill_rect(r, p, Transform::identity(), None);
        }
    }

    fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, p: &Paint) {
        let s = self.scale;
        let (x0, y0) = (snap(x, s) * s, snap(y, s) * s);
        let (x1, y1) = (snap(x + w, s) * s, snap(y + h, s) * s);
        let r = (r * s).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
        let k = 0.5523 * r;
        let mut pb = PathBuilder::new();
        pb.move_to(x0 + r, y0);
        pb.line_to(x1 - r, y0);
        pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
        pb.line_to(x1, y1 - r);
        pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
        pb.line_to(x0 + r, y1);
        pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
        pb.line_to(x0, y0 + r);
        pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
        pb.close();
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
        }
    }

    fn poly(&mut self, pts: &[(f32, f32)], p: &Paint) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(x * s, y * s);
            } else {
                pb.line_to(x * s, y * s);
            }
        }
        pb.close();
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, p: &Paint) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        pb.push_circle(cx * s, cy * s, r * s);
        if let Some(path) = pb.finish() {
            self.pm.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
        }
    }

    /// Text with its pen at `x` points and its baseline at `base` points (both snapped to whole pixels); `cut` knocks it out.
    fn text(&mut self, text: &str, style: &TextStyle, x: f32, base: f32, cut: bool) {
        let s = self.scale;
        let m = self.text.mask(text, style, s);
        let ox = (x * s).round() as i64 - i64::from(m.origin);
        let oy = (base * s).round() as i64 - i64::from(m.baseline);
        let (w, h) = (i64::from(self.pm.width()), i64::from(self.pm.height()));
        let pixels = self.pm.pixels_mut();
        for my in 0..m.height {
            for mx in 0..m.width {
                let a = m.alpha[(my * m.width + mx) as usize];
                let (px, py) = (ox + i64::from(mx), oy + i64::from(my));
                if a == 0 || px < 0 || py < 0 || px >= w || py >= h {
                    continue;
                }
                let p = &mut pixels[(py * w + px) as usize];
                let (src, dst) = (u32::from(a), u32::from(p.alpha()));
                let out = if cut {
                    dst * (255 - src) / 255
                } else {
                    src + dst * (255 - src) / 255
                };
                if let Some(c) = PremultipliedColorU8::from_rgba(0, 0, 0, out.min(255) as u8) {
                    *p = c;
                }
            }
        }
    }

    /// A number right-aligned in a field ending at `right`; in alert, on a plate flush with `right`, knocked out and moved in
    /// by the plate's padding. Returns the number's left edge.
    #[allow(clippy::too_many_arguments)]
    fn number(&mut self, text: &str, unit: &str, s: &TextStyle, right: f32, base: f32, alert: bool, pad_y: f32) -> f32 {
        let uw = self.text.advance(unit, s);
        let tw = self.text.advance(text, s);
        if alert {
            let cap = self.text.cap_height(s);
            let pad_x = 1.5;
            let r = right - pad_x;
            self.rrect(
                r - uw - tw - pad_x,
                base - cap - pad_y,
                tw + uw + 2.0 * pad_x,
                cap + 2.0 * pad_y,
                2.5,
                &ink(1.0),
            );
            self.text(text, s, r - uw - tw, base, true);
            self.text(unit, s, r - uw, base, true);
            r - uw - tw - pad_x
        } else {
            self.text(text, s, right - uw - tw, base, false);
            self.text(unit, s, right - uw, base, false);
            right - uw - tw
        }
    }

    /// A label at the inset (or ending at `right`); in alert, knocked out of a plate. Returns where what follows may start.
    fn label(&mut self, text: &str, right: Option<f32>, base: f32, alert: bool) -> f32 {
        let t = text.to_uppercase();
        let w = self.text.advance(&t, &LABEL);
        let x = right.map_or(LABEL_INSET, |r| r - w);
        if alert {
            let cap = self.text.cap_height(&LABEL);
            self.rrect(x - LABEL_INSET, base - cap - 2.0, w + 2.0 * LABEL_INSET, cap + 4.0, 2.0, &ink(1.0));
            self.text(&t, &LABEL, x, base, true);
        } else {
            self.text(&t, &LABEL, x, base, false);
        }
        x + w
    }

    fn graph(&mut self, x: f32, y: f32, w: f32, h: f32, history: &[f32]) {
        let s = self.scale;
        self.rrect(x, y, w, h, 2.0, &ink(PLATE));
        let n = w.round().max(1.0) as usize;
        let recent: Vec<f32> = history.iter().rev().take(n).rev().copied().collect();
        let Some(last) = recent.last().copied() else { return };
        let step = w / n as f32;
        let x0 = x + w - recent.len() as f32 * step;
        let line_w = (0.75 * s).max(1.0);
        let inset = line_w / 2.0 / s;
        // A sample above zero lifts the line at least one device pixel off the floor.
        let yv = |v: f32| {
            let d = (h - 2.0 * inset) * v.clamp(0.0, 100.0) / 100.0;
            let d = if v > 0.5 { d.max(1.0 / s) } else { d };
            y + h - inset - d
        };
        let mut area = PathBuilder::new();
        let mut line = PathBuilder::new();
        area.move_to(x0 * s, (y + h) * s);
        for (i, v) in recent.iter().enumerate() {
            let (px, py) = ((x0 + (i as f32 + 0.5) * step) * s, yv(*v) * s);
            if i == 0 {
                area.line_to(x0 * s, py);
                line.move_to(x0 * s, py);
            }
            area.line_to(px, py);
            line.line_to(px, py);
        }
        let ly = yv(last) * s;
        area.line_to((x + w) * s, ly);
        line.line_to((x + w) * s, ly);
        area.line_to((x + w) * s, (y + h) * s);
        area.close();
        let clip = Mask::new(self.pm.width(), self.pm.height()).and_then(|mut m| {
            let r = Rect::from_xywh(snap(x, s) * s, snap(y, s) * s, w * s, h * s)?;
            let mut pb = PathBuilder::new();
            pb.push_rect(r);
            m.fill_path(&pb.finish()?, FillRule::Winding, true, Transform::identity());
            Some(m)
        });
        if let Some(p) = area.finish() {
            self.pm
                .fill_path(&p, &ink(AREA), FillRule::Winding, Transform::identity(), clip.as_ref());
        }
        if let Some(p) = line.finish() {
            let st = Stroke {
                width: line_w,
                line_join: tiny_skia::LineJoin::Round,
                ..Stroke::default()
            };
            self.pm.stroke_path(&p, &ink(1.0), &st, Transform::identity(), clip.as_ref());
        }
    }

    fn stack(&mut self, label: &str, unit: Unit, value: Option<f32>, right: f32, alert: bool) {
        let s = self.scale;
        let lb = snap(7.0, s);
        let vb = snap(16.5, s);
        let t = label.to_uppercase();
        let text = value.map_or("–".to_string(), |v| format_value(v, unit));
        let (lw, uw, tw) = (
            self.text.advance(&t, &STACK_LABEL),
            self.text.advance(unit_text(unit), &STACK_VALUE),
            self.text.advance(&text, &STACK_VALUE),
        );
        let r = right - STACK_INSET;
        if alert {
            // The whole stack on one plate, both lines knocked out: room around the text at 1x as at @2x.
            let w = lw.max(self.text.advance("000", &STACK_VALUE) + uw) + 2.0 * STACK_INSET;
            self.rrect(right - w, 0.0, w, HEIGHT, 3.0, &ink(1.0));
        }
        self.text(&t, &STACK_LABEL, r - lw, lb, alert);
        self.text(&text, &STACK_VALUE, r - uw - tw, vb, alert);
        self.text(unit_text(unit), &STACK_VALUE, r - uw, vb, alert);
    }
}

/// Renders one item as a template at `scale` (1 or 2) pixels per point.
pub fn render(kind: &Kind, r: &Reading, state: State, text: &dyn TextRaster, scale: u32) -> Template {
    let s = scale.clamp(1, 4) as f32;
    let w = width(kind, text);
    let (pw, ph) = ((w * s) as u32, (HEIGHT * s) as u32);
    let Some(pm) = Pixmap::new(pw.max(1), ph) else {
        return Template {
            width: pw,
            height: ph,
            scale,
            alpha: vec![0; (pw * ph) as usize],
        };
    };
    let mut c = Canvas { pm, scale: s, text };
    let alert = state == State::Alert;
    let base = baseline(s);
    let value = if state == State::Unavailable {
        None
    } else {
        r.value.filter(|v| v.is_finite())
    };
    let fill = |v: f32, len: f32, min: f32| {
        let f = snap(len * v.clamp(0.0, 100.0) / 100.0, s);
        if v > 0.5 {
            f.max(min)
        } else {
            f
        }
    };
    match kind {
        Kind::Value { label, unit } => {
            let text = value.map_or("–".to_string(), |v| format_value(v, *unit));
            let left = c.number(&text, unit_text(*unit), &VALUE, w, base, alert, 2.0);
            c.label(label, Some(left - 3.0), base, false);
        }
        Kind::Stacked { label, unit } => c.stack(label, *unit, value, w, alert),
        Kind::Meter { label } => {
            let end = c.label(label, None, base, alert);
            let cap = text.cap_height(&LABEL);
            let (mh, mx) = (6.0, end + GAP);
            let my = snap(base - cap / 2.0 - mh / 2.0, s);
            let mw = w - mx;
            c.rrect(mx, my, mw, mh, mh / 2.0, &ink(TRACK));
            if let Some(v) = value {
                let fw = fill(v, mw, mh);
                if fw > 0.0 {
                    c.rrect(mx, my, fw, mh, mh / 2.0, &ink(1.0));
                }
            }
        }
        Kind::Bar { label } => {
            let end = c.label(label, None, base, alert);
            let (bx, bw, by, bh) = (end + GAP - 1.0, 5.0, 2.0, 14.0);
            c.rrect(bx, by, bw, bh, 1.5, &ink(TRACK));
            if let Some(v) = value {
                let fh = fill(v, bh, 1.0 / s);
                if fh > 0.0 {
                    c.rrect(bx, by + bh - fh, bw, fh, 1.5_f32.min(fh / 2.0), &ink(1.0));
                }
            }
        }
        Kind::Cores { cores } => {
            for i in 0..(*cores).clamp(1, 64) {
                let x = i as f32 * 3.0;
                c.rect(x, 2.0, 2.0, 14.0, &ink(TRACK));
                if state != State::Unavailable {
                    let fh = fill(r.cores.get(i).copied().unwrap_or(0.0), 14.0, 1.0 / s);
                    if fh > 0.0 {
                        c.rect(x, 16.0 - fh, 2.0, fh, &ink(1.0));
                    }
                }
            }
        }
        Kind::Graph { label } => {
            let end = c.label(label, None, base, alert);
            let hist: &[f32] = if state == State::Unavailable { &[] } else { &r.history };
            c.graph(end + GAP, 2.0, w - end - GAP, 14.0, hist);
        }
        Kind::GraphValue { label, unit } => {
            let hist: &[f32] = if state == State::Unavailable { &[] } else { &r.history };
            c.graph(0.0, 2.0, 24.0, 14.0, hist);
            c.stack(label, *unit, value, w, alert);
        }
        Kind::Network(dir) => {
            let l = net_layout(text);
            let cap = text.cap_height(&PAIR);
            let rows = dir.rows();
            for (row, &up) in rows.iter().enumerate() {
                let bps = if up { r.up } else { r.down };
                let b = snap(
                    match (rows.len(), row) {
                        (1, _) => 12.0,
                        (_, 0) => 7.5,
                        _ => 16.5,
                    },
                    s,
                );
                let mid = b - cap / 2.0;
                let (a0, a1) = (0.0, l.arrow);
                if up {
                    c.poly(&[((a0 + a1) / 2.0, mid - 2.4), (a1, mid + 1.8), (a0, mid + 1.8)], &ink(1.0));
                } else {
                    c.poly(&[(a0, mid - 1.8), (a1, mid - 1.8), ((a0 + a1) / 2.0, mid + 2.4)], &ink(1.0));
                }
                let (t, unit) = if state == State::Unavailable {
                    ("–".to_string(), "")
                } else {
                    format_rate(bps)
                };
                let right = l.arrow + 2.0 + l.num;
                let tw = text.advance(&t, &PAIR);
                c.text(&t, &PAIR, right - tw, b, false);
                c.text(unit, &PAIR, right + 2.0, b, false);
            }
        }
        Kind::Temperature => {
            // A 1 pt outlined stem filling from 20 to 110 degrees, and a solid bulb.
            let (sx, sw, top, bulb_c, bulb_r) = (1.5, 3.0, 2.5, 13.0, 2.75);
            let line = 1.0;
            c.rrect(sx, top, sw, bulb_c - top, sw / 2.0, &ink(1.0));
            c.rrect(
                sx + line,
                top + line,
                sw - 2.0 * line,
                bulb_c - top - line,
                (sw - 2.0 * line) / 2.0,
                &knock(),
            );
            if let Some(v) = value {
                let f = snap((bulb_c - top - line) * ((v - 20.0) / 90.0).clamp(0.0, 1.0), s);
                if f > 0.0 {
                    c.rect(sx + line, bulb_c - f, sw - 2.0 * line, f, &ink(1.0));
                }
            }
            c.circle(sx + sw / 2.0, bulb_c, bulb_r, &ink(1.0));
            let t = value.map_or("–".to_string(), |v| format_value(v, Unit::Degrees));
            c.number(&t, "°", &VALUE, w, base, alert, 2.0);
        }
    }
    let dim = if state == State::Unavailable { DIM } else { 1.0 };
    let alpha = c.pm.pixels().iter().map(|p| (f32::from(p.alpha()) * dim).round() as u8).collect();
    Template {
        width: pw,
        height: ph,
        scale,
        alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::text::{BoxText, TextRaster};

    fn kinds() -> Vec<Kind> {
        vec![
            Kind::Value {
                label: "cpu".into(),
                unit: Unit::Percent,
            },
            Kind::Stacked {
                label: "mem".into(),
                unit: Unit::Percent,
            },
            Kind::Meter { label: "gpu".into() },
            Kind::Bar { label: "mem".into() },
            Kind::Cores { cores: 10 },
            Kind::Graph { label: "dsk".into() },
            Kind::GraphValue {
                label: "cpu".into(),
                unit: Unit::Percent,
            },
            Kind::Network(crate::tray::NetDir::Up),
            Kind::Network(crate::tray::NetDir::Down),
            Kind::Network(crate::tray::NetDir::Both),
            Kind::Temperature,
        ]
    }

    #[test]
    fn the_track_has_3_to_1_contrast_on_light_and_dark_menu_bars() {
        let lum = |v: f32| {
            let v = v / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        // The template is drawn black on a light bar (about #e5e5e5 to white) and white on a dark one (about #2b2b2b and darker).
        for (ink, bg) in [
            (0.0f32, 229.0f32),
            (0.0, 243.0),
            (0.0, 255.0),
            (255.0, 0.0),
            (255.0, 43.0),
            (255.0, 60.0),
        ] {
            let mix = ink * TRACK + bg * (1.0 - TRACK);
            let (hi, lo) = (lum(mix).max(lum(bg)), lum(mix).min(lum(bg)));
            assert!((hi + 0.05) / (lo + 0.05) >= 3.0, "ink {ink} on {bg}");
        }
    }

    #[test]
    fn the_baseline_rounds_down_so_1x_sits_on_the_clock() {
        assert_eq!(baseline(1.0), 13.0, "13.5 pt at 1x: 13 px, not 14");
        assert_eq!(baseline(2.0), 13.5, "@2x: 27 px, unchanged");
    }

    #[test]
    fn the_width_never_follows_the_value_or_the_state_and_1x_matches_2x() {
        for k in kinds() {
            let one = render(&k, &Reading::default(), State::Normal, &BoxText, 1);
            for v in [0.0, 0.4, 1.0, 9.6, 50.0, 99.6, 100.0, 180.0, f32::NAN] {
                let r = Reading {
                    value: Some(v),
                    history: vec![v; 40],
                    cores: vec![v; 10],
                    up: f64::from(v) * 1e7,
                    down: f64::from(v) * 3e5,
                };
                for st in [State::Normal, State::Alert, State::Unavailable] {
                    let two = render(&k, &r, st, &BoxText, 2);
                    assert_eq!(two.width, 2 * one.width, "{k:?} {v} {st:?}");
                    assert_eq!(two.height, 36);
                }
            }
        }
    }

    #[test]
    fn a_little_load_shows_at_least_one_device_pixel() {
        let r = Reading {
            value: Some(1.0),
            ..Reading::default()
        };
        for scale in [1, 2] {
            let t = render(&Kind::Bar { label: "m".into() }, &r, State::Normal, &BoxText, scale);
            let bottom = 16 * scale - 1;
            assert!((0..t.width).any(|x| t.at(x, bottom) == 255), "scale {scale}");
        }
    }

    #[test]
    fn alert_is_a_plate_not_a_colour_and_unavailable_dims() {
        let k = Kind::Value {
            label: "cpu".into(),
            unit: Unit::Percent,
        };
        let r = Reading {
            value: Some(93.0),
            ..Reading::default()
        };
        let normal = render(&k, &r, State::Normal, &BoxText, 2);
        let alert = render(&k, &r, State::Alert, &BoxText, 2);
        // Above the caps, near the right edge: inside the plate in alert, empty otherwise.
        let (x, y) = (
            normal.width - 6,
            ((BASELINE - BoxText.cap_height(&VALUE) - 1.0) * 2.0).round() as u32,
        );
        assert_eq!((normal.at(x, y), alert.at(x, y)), (0, 255));
        assert!(
            alert.rgba().chunks(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0),
            "a template is black"
        );
        let off = render(&k, &r, State::Unavailable, &BoxText, 2);
        assert!(off.alpha.iter().all(|a| f32::from(*a) <= 255.0 * DIM + 1.0));
    }

    #[test]
    fn rates_keep_three_figures() {
        assert_eq!(format_rate(999.4), ("999".into(), "B/s"));
        assert_eq!(format_rate(999.6), ("1.0".into(), "KB/s"));
        assert_eq!(format_rate(24_300_000.0), ("24".into(), "MB/s"));
        assert_eq!(format_rate(f64::NAN), ("0".into(), "B/s"));
        assert_eq!(format_rate(5e15).1, "GB/s");
    }

    #[test]
    fn a_network_item_draws_only_its_own_direction() {
        use super::super::NetDir;
        let at = |up: f64, down: f64| Reading {
            value: Some(1.0),
            up,
            down,
            ..Reading::default()
        };
        for scale in [1, 2] {
            let draw = |d: NetDir, r: &Reading| render(&Kind::Network(d), r, State::Normal, &BoxText, scale).alpha;
            // Changing the other direction's rate leaves the item unchanged.
            assert_eq!(
                draw(NetDir::Up, &at(5e3, 1.0)),
                draw(NetDir::Up, &at(5e3, 7e7)),
                "up ignores down at {scale}x"
            );
            assert_eq!(
                draw(NetDir::Down, &at(1.0, 5e3)),
                draw(NetDir::Down, &at(7e7, 5e3)),
                "down ignores up at {scale}x"
            );
            // Its own rate and its arrow do show.
            assert_ne!(draw(NetDir::Up, &at(5e3, 0.0)), draw(NetDir::Up, &at(7e7, 0.0)));
            assert_ne!(draw(NetDir::Up, &at(5e3, 5e3)), draw(NetDir::Down, &at(5e3, 5e3)));
            // The combined item changes when either rate changes, and differs from both single items.
            let both = |r: &Reading| draw(NetDir::Both, r);
            assert_ne!(both(&at(5e3, 5e3)), both(&at(7e7, 5e3)), "both follows up at {scale}x");
            assert_ne!(both(&at(5e3, 5e3)), both(&at(5e3, 7e7)), "both follows down at {scale}x");
            assert_ne!(both(&at(5e3, 5e3)), draw(NetDir::Up, &at(5e3, 5e3)));
            assert_ne!(both(&at(5e3, 5e3)), draw(NetDir::Down, &at(5e3, 5e3)));
        }
    }
}
