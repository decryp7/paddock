//! The proposed macOS menu-bar items: template images (black plus alpha), 18 pt high, one fixed width per format.
//!
//! Every length is in points; `scale` is pixels per point (1 or 2). Shapes snap to whole device pixels at each scale;
//! text keeps its outlines (macOS does not hint) on a baseline that sits on a whole device pixel.

use crate::font::{Font, Style};
use tiny_skia::{BlendMode, Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Image height in points (tray-icon sets every status-item image to 18 pt high).
pub const H: f32 = 18.0;
/// Baseline of one-line text, in points from the image top: the caps of 13 pt text centred on the 24 pt menu bar, like
/// the clock's (the image is centred, so its top is 3 pt below the bar's).
pub const BASELINE: f32 = 13.5;
/// Alpha of tracks and empty parts.
pub const TRACK: f32 = 0.28;
/// Alpha of an area graph's fill.
pub const AREA: f32 = 0.38;
/// Alpha of the whole item while its value is unavailable or the agent is away.
pub const DIM: f32 = 0.45;
/// The space between a label and what follows it.
pub const GAP: f32 = 4.0;

/// The type of one-line values (the menu bar's own font and size).
pub fn value_style() -> Style {
    Style::new(13.0)
}
/// The type of labels before a value or a mark: small caps-height label, semibold, slightly tracked.
pub fn label_style() -> Style {
    Style::new(9.5).weight(620.0).tracking(0.35)
}
/// The two-line stack: the label line.
pub fn stack_label_style() -> Style {
    Style::new(8.0).weight(620.0).tracking(0.45)
}
/// The two-line stack: the value line.
pub fn stack_value_style() -> Style {
    Style::new(11.0).weight(510.0)
}
/// The network pair's lines.
pub fn pair_style() -> Style {
    Style::new(9.0).weight(510.0)
}

/// What a value means, for formatting and the fixed field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    /// 0 to 100 with "%".
    Percent,
    /// Degrees, 0 to 199, with "°".
    Degrees,
}

/// The formats of a menu-bar item.
#[derive(Clone, Debug)]
pub enum Kind {
    /// "CPU 23%": a label and a value on one line.
    Value { label: String, unit: Unit },
    /// The label over the value.
    Stacked { label: String, unit: Unit },
    /// A label and a horizontal meter.
    Meter { label: String },
    /// A label and one vertical bar.
    Bar { label: String },
    /// One column per core.
    Cores { cores: usize },
    /// A label and an area graph of the recent values.
    Graph { label: String },
    /// An area graph with the label-over-value stack beside it.
    GraphValue { label: String, unit: Unit },
    /// A 270-degree gauge arc with the metric's letter inside.
    Ring { letter: String },
    /// Upload over download, with arrows and units.
    Network,
    /// A thermometer and the degrees.
    Temperature,
}

/// What an item shows now.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    /// The value (0 to 100, or degrees); `None` when unavailable.
    pub value: Option<f32>,
    /// Recent values, oldest first (graphs).
    pub history: Vec<f32>,
    /// Per-core loads.
    pub cores: Vec<f32>,
    /// Bytes per second up and down.
    pub up: f64,
    pub down: f64,
}

/// The item's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Normal,
    /// Over its alert threshold: the number (or the label, or the letter) sits on a solid plate, knocked out. A shape
    /// cue, so it reads in a template and never by colour alone.
    Alert,
    /// No value (not measurable, or the agent is away): an en dash in the field and the whole item dimmed.
    Unavailable,
}

fn ink(a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba(0.0, 0.0, 0.0, a.clamp(0.0, 1.0)).unwrap());
    p.anti_alias = true;
    p
}

fn knock() -> Paint<'static> {
    let mut p = ink(1.0);
    p.blend_mode = BlendMode::DestinationOut;
    p
}

/// `v` points snapped to whole device pixels.
pub fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

fn rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, scale: f32, p: &Paint) {
    let (x0, y0) = (snap(x, scale) * scale, snap(y, scale) * scale);
    let (x1, y1) = (snap(x + w, scale) * scale, snap(y + h, scale) * scale);
    if let Some(r) = Rect::from_ltrb(x0, y0, x1.max(x0 + 1.0), y1.max(y0 + 1.0)) {
        pm.fill_rect(r, p, Transform::identity(), None);
    }
}

fn rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, scale: f32, p: &Paint) {
    let (x0, y0) = (snap(x, scale) * scale, snap(y, scale) * scale);
    let (x1, y1) = (snap(x + w, scale) * scale, snap(y + h, scale) * scale);
    let r = (r * scale).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
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
        pm.fill_path(&path, p, FillRule::Winding, Transform::identity(), None);
    }
}

/// The baseline of one-line text at `scale`, on a whole device pixel.
pub fn baseline(scale: f32) -> f32 {
    snap(BASELINE, scale)
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

/// A rate as up to three figures and a unit: "0", "9.4", "12", "512" then the next unit ("1.2" MB/s).
pub fn format_rate(bps: f64) -> (String, &'static str) {
    let units = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut v = bps.max(0.0);
    let mut i = 0;
    // Switch up at 999.5 so "1000" never shows (four figures would not fit the field).
    while v >= 999.5 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    let text = if v < 9.95 && i > 0 { format!("{v:.1}") } else { format!("{}", v.round() as u64) };
    (text, units[i])
}

/// The fixed width of a number field: three tabular digits (all digits share one advance).
fn field(font: &Font, s: &Style) -> f32 {
    font.measure("000", s)
}

/// The fixed width of an item in points (whole points, so the 1x and 2x images have the same point size). It depends
/// only on the format, never on the value.
pub fn width(kind: &Kind, font: &Font) -> f32 {
    let w = match kind {
        Kind::Value { label, unit } => {
            LABEL_INSET + font.measure(&label.to_uppercase(), &label_style()) + GAP + field(font, &value_style()) + font.measure(unit_text(*unit), &value_style())
        }
        Kind::Stacked { label, unit } => stack_width(font, label, *unit),
        Kind::Meter { label } => LABEL_INSET + font.measure(&label.to_uppercase(), &label_style()) + GAP + 24.0,
        Kind::Bar { label } => LABEL_INSET + font.measure(&label.to_uppercase(), &label_style()) + GAP - 1.0 + 5.0,
        Kind::Cores { cores } => (*cores as f32) * 3.0 - 1.0,
        Kind::Graph { label } => LABEL_INSET + font.measure(&label.to_uppercase(), &label_style()) + GAP + 30.0,
        Kind::GraphValue { label, unit } => 24.0 + GAP + stack_width(font, label, *unit),
        Kind::Ring { .. } => 17.0,
        Kind::Network => net_layout(font).total,
        Kind::Temperature => 6.0 + 3.0 + field(font, &value_style()) + font.measure("°", &value_style()),
    };
    w.ceil()
}

fn stack_width(font: &Font, label: &str, unit: Unit) -> f32 {
    let l = font.measure(&label.to_uppercase(), &stack_label_style());
    let v = field(font, &stack_value_style()) + font.measure(unit_text(unit), &stack_value_style());
    l.max(v)
}

struct NetLayout {
    arrow: f32,
    num: f32,
    unit: f32,
    total: f32,
}

fn net_layout(font: &Font) -> NetLayout {
    let s = pair_style();
    let arrow = 5.0;
    let num = field(font, &s);
    let unit = ["B/s", "KB/s", "MB/s", "GB/s"].iter().map(|u| font.measure(u, &s)).fold(0.0, f32::max);
    NetLayout {
        arrow,
        num,
        unit,
        total: arrow + 2.0 + num + 2.0 + unit,
    }
}

/// Inset of a label from the item's left edge, so its alert plate fits inside the image.
pub const LABEL_INSET: f32 = 1.5;

/// Draws a number right-aligned in a field ending at `right`; in alert, on a plate flush with `right`, the digits knocked
/// out and moved in by the plate's padding.
fn number(pm: &mut Pixmap, font: &Font, text: &str, unit: &str, s: &Style, right: f32, base: f32, scale: f32, alert: bool) {
    let uw = font.measure(unit, s);
    let tw = font.measure(text, s);
    if alert {
        let cap = font.cap_height(s);
        let (pad_x, pad_y) = (1.5, if s.size < 12.0 { 0.75 } else { (s.size * 0.12).clamp(1.25, 2.0) });
        let r = right - pad_x;
        rrect(pm, r - uw - tw - pad_x, base - cap - pad_y, tw + uw + 2.0 * pad_x, cap + 2.0 * pad_y, 2.5, scale, &ink(1.0));
        font.draw(pm, text, s, r - uw - tw, base, scale, &knock());
        font.draw(pm, unit, s, r - uw, base, scale, &knock());
    } else {
        font.draw(pm, text, s, right - uw - tw, base, scale, &ink(1.0));
        font.draw(pm, unit, s, right - uw, base, scale, &ink(1.0));
    }
}

/// Draws a label at the inset; in alert, knocked out of a plate. Returns where what follows may start (before the gap).
fn label(pm: &mut Pixmap, font: &Font, text: &str, s: &Style, _x: f32, base: f32, scale: f32, alert: bool) -> f32 {
    let t = text.to_uppercase();
    let w = font.measure(&t, s);
    let x = LABEL_INSET;
    if alert {
        let cap = font.cap_height(s);
        rrect(pm, 0.0, base - cap - 2.0, w + 2.0 * LABEL_INSET, cap + 4.0, 2.0, scale, &ink(1.0));
        font.draw(pm, &t, s, x, base, scale, &knock());
    } else {
        font.draw(pm, &t, s, x, base, scale, &ink(1.0));
    }
    w + LABEL_INSET
}

fn graph(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, history: &[f32], scale: f32) {
    // The plate gives the graph a defined size when it is empty.
    rrect(pm, x, y, w, h, 2.0, scale, &ink(0.14));
    let n = w.round() as usize; // one sample per point
    let recent: Vec<f32> = history.iter().rev().take(n).rev().copied().collect();
    if recent.is_empty() {
        return;
    }
    let step = w / n as f32;
    let x0 = x + w - recent.len() as f32 * step;
    let line_w = (0.75 * scale).max(1.0);
    let inset = line_w / 2.0 / scale;
    let yv = |v: f32| y + h - inset - (h - 2.0 * inset) * v.clamp(0.0, 100.0) / 100.0;
    let mut area = PathBuilder::new();
    let mut line = PathBuilder::new();
    area.move_to(x0 * scale, (y + h) * scale);
    for (i, v) in recent.iter().enumerate() {
        let (px, py) = ((x0 + (i as f32 + 0.5) * step) * scale, yv(*v) * scale);
        if i == 0 {
            area.line_to(x0 * scale, py);
            line.move_to(x0 * scale, py);
        }
        area.line_to(px, py);
        line.line_to(px, py);
    }
    let last = yv(*recent.last().unwrap()) * scale;
    area.line_to((x + w) * scale, last);
    line.line_to((x + w) * scale, last);
    area.line_to((x + w) * scale, (y + h) * scale);
    area.close();
    let mut clip = tiny_skia::Mask::new(pm.width(), pm.height()).unwrap();
    {
        let mut pb = PathBuilder::new();
        let r = Rect::from_xywh(snap(x, scale) * scale, snap(y, scale) * scale, w * scale, h * scale).unwrap();
        pb.push_rect(r);
        clip.fill_path(&pb.finish().unwrap(), FillRule::Winding, true, Transform::identity());
    }
    if let Some(p) = area.finish() {
        pm.fill_path(&p, &ink(AREA), FillRule::Winding, Transform::identity(), Some(&clip));
    }
    if let Some(p) = line.finish() {
        let st = Stroke {
            width: line_w,
            line_cap: LineCap::Butt,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pm.stroke_path(&p, &ink(1.0), &st, Transform::identity(), Some(&clip));
    }
}

fn arc(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, from: f32, sweep: f32, w: f32, a: f32, scale: f32) {
    if sweep <= 0.0 {
        return;
    }
    let mut pb = PathBuilder::new();
    let steps = ((sweep / 4.0).ceil() as usize).max(2);
    for i in 0..=steps {
        let t = (from + sweep * i as f32 / steps as f32).to_radians();
        let (x, y) = ((cx + r * t.cos()) * scale, (cy + r * t.sin()) * scale);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    let st = Stroke {
        width: w * scale,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    if let Some(p) = pb.finish() {
        pm.stroke_path(&p, &ink(a), &st, Transform::identity(), None);
    }
}

/// Renders one item as a template image at `scale`.
pub fn render(kind: &Kind, r: &Reading, state: State, font: &Font, scale: f32) -> Pixmap {
    let w = width(kind, font);
    let mut pm = Pixmap::new((w * scale) as u32, (H * scale) as u32).unwrap();
    let alert = state == State::Alert;
    let base = baseline(scale);
    let value = if state == State::Unavailable { None } else { r.value };
    match kind {
        Kind::Value { label: l, unit } => {
            label(&mut pm, font, l, &label_style(), 0.0, base, scale, false);
            let text = value.map_or("–".to_string(), |v| format_value(v, *unit));
            number(&mut pm, font, &text, unit_text(*unit), &value_style(), w, base, scale, alert);
        }
        Kind::Stacked { label: l, unit } => draw_stack(&mut pm, font, l, *unit, value, 0.0, w, scale, alert),
        Kind::Meter { label: l } => {
            let lw = label(&mut pm, font, l, &label_style(), 0.0, base, scale, alert);
            let cap = font.cap_height(&label_style());
            let (mh, mx) = (6.0, lw + GAP);
            let my = snap(base - cap / 2.0 - mh / 2.0, scale);
            let mw = w - mx;
            rrect(&mut pm, mx, my, mw, mh, mh / 2.0, scale, &ink(TRACK));
            if let Some(v) = value {
                let fw = snap(mw * v.clamp(0.0, 100.0) / 100.0, scale).max(if v > 0.5 { mh } else { 0.0 });
                if fw > 0.0 {
                    rrect(&mut pm, mx, my, fw, mh, mh / 2.0, scale, &ink(1.0));
                }
            }
        }
        Kind::Bar { label: l } => {
            let lw = label(&mut pm, font, l, &label_style(), 0.0, base, scale, alert);
            let (bx, bw, by, bh) = (lw + GAP - 1.0, 5.0, 2.0, 14.0);
            rrect(&mut pm, bx, by, bw, bh, 1.5, scale, &ink(TRACK));
            if let Some(v) = value {
                let fh = snap(bh * v.clamp(0.0, 100.0) / 100.0, scale).max(if v > 0.5 { 1.0 / scale } else { 0.0 });
                rrect(&mut pm, bx, by + bh - fh, bw, fh, 1.5_f32.min(fh / 2.0), scale, &ink(1.0));
            }
        }
        Kind::Cores { cores } => {
            for i in 0..*cores {
                let x = i as f32 * 3.0;
                rect(&mut pm, x, 2.0, 2.0, 14.0, scale, &ink(TRACK));
                if state != State::Unavailable {
                    let v = r.cores.get(i).copied().unwrap_or(0.0);
                    let fh = snap(14.0 * v.clamp(0.0, 100.0) / 100.0, scale).max(if v > 0.5 { 1.0 / scale } else { 0.0 });
                    if fh > 0.0 {
                        rect(&mut pm, x, 16.0 - fh, 2.0, fh, scale, &ink(1.0));
                    }
                }
            }
        }
        Kind::Graph { label: l } => {
            let lw = label(&mut pm, font, l, &label_style(), 0.0, base, scale, alert);
            let hist = if state == State::Unavailable { &[][..] } else { &r.history[..] };
            graph(&mut pm, lw + GAP, 2.0, w - lw - GAP, 14.0, hist, scale);
        }
        Kind::GraphValue { label: l, unit } => {
            let hist = if state == State::Unavailable { &[][..] } else { &r.history[..] };
            graph(&mut pm, 0.0, 2.0, 24.0, 14.0, hist, scale);
            draw_stack(&mut pm, font, l, *unit, value, 24.0 + GAP, w, scale, alert);
        }
        Kind::Ring { letter } => {
            let (cx, cy, rad, sw) = (8.5, 9.0, 7.0, 2.0);
            arc(&mut pm, cx, cy, rad, 135.0, 270.0, sw, TRACK, scale);
            if let Some(v) = value {
                arc(&mut pm, cx, cy, rad, 135.0, 270.0 * v.clamp(0.0, 100.0) / 100.0, sw, 1.0, scale);
            }
            let s = Style::new(7.5).weight(700.0);
            let t = letter.to_uppercase();
            let lw = font.measure(&t, &s);
            let cap = font.cap_height(&s);
            let lb = snap(cy + cap / 2.0, scale);
            if alert {
                let mut p = ink(1.0);
                p.anti_alias = true;
                let mut pb = PathBuilder::new();
                pb.push_circle(cx * scale, cy * scale, 4.6 * scale);
                pm.fill_path(&pb.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
                font.draw(&mut pm, &t, &s, cx - lw / 2.0, lb, scale, &knock());
            } else {
                font.draw(&mut pm, &t, &s, cx - lw / 2.0, lb, scale, &ink(1.0));
            }
        }
        Kind::Network => {
            let s = pair_style();
            let l = net_layout(font);
            let cap = font.cap_height(&s);
            for (row, bps, up) in [(0, r.up, true), (1, r.down, false)] {
                let b = snap(if row == 0 { 7.5 } else { 16.5 }, scale);
                // A solid triangle on the caps' height, pointing the way the data goes.
                let (top, bot) = (b - cap, b);
                let mid = (top + bot) / 2.0;
                let (ax0, ax1) = (0.0, l.arrow);
                let mut pb = PathBuilder::new();
                if up {
                    pb.move_to((ax0 + ax1) / 2.0 * scale, (mid - 2.4) * scale);
                    pb.line_to(ax1 * scale, (mid + 1.8) * scale);
                    pb.line_to(ax0 * scale, (mid + 1.8) * scale);
                } else {
                    pb.move_to(ax0 * scale, (mid - 1.8) * scale);
                    pb.line_to(ax1 * scale, (mid - 1.8) * scale);
                    pb.line_to((ax0 + ax1) / 2.0 * scale, (mid + 2.4) * scale);
                }
                pb.close();
                pm.fill_path(&pb.finish().unwrap(), &ink(1.0), FillRule::Winding, Transform::identity(), None);
                let (text, unit) = if state == State::Unavailable { ("–".to_string(), "") } else { format_rate(bps) };
                let right = l.arrow + 2.0 + l.num;
                let tw = font.measure(&text, &s);
                font.draw(&mut pm, &text, &s, right - tw, b, scale, &ink(1.0));
                font.draw(&mut pm, unit, &s, right + 2.0, b, scale, &ink(1.0));
            }
        }
        Kind::Temperature => {
            // Thermometer: a 1 pt outlined stem and a solid bulb; the stem fills from 20 to 110 degrees.
            let (sx, sw, top, bulb_c, bulb_r) = (1.5, 3.0, 2.5, 13.0, 2.75);
            let px = 1.0 / scale;
            let line = 1.0_f32.max(px);
            rrect(&mut pm, sx, top, sw, bulb_c - top, sw / 2.0, scale, &ink(1.0));
            rrect(&mut pm, sx + line, top + line, sw - 2.0 * line, bulb_c - top - line, (sw - 2.0 * line) / 2.0, scale, &knock());
            if let Some(v) = value {
                let span = bulb_c - top - line;
                let f = snap(span * ((v - 20.0) / 90.0).clamp(0.0, 1.0), scale);
                if f > 0.0 {
                    rect(&mut pm, sx + line, bulb_c - f, sw - 2.0 * line, f, scale, &ink(1.0));
                }
            }
            let mut pb = PathBuilder::new();
            pb.push_circle((sx + sw / 2.0) * scale, bulb_c * scale, bulb_r * scale);
            pm.fill_path(&pb.finish().unwrap(), &ink(1.0), FillRule::Winding, Transform::identity(), None);
            let text = value.map_or("–".to_string(), |v| format_value(v, Unit::Degrees));
            number(&mut pm, font, &text, "°", &value_style(), w, base, scale, alert);
        }
    }
    if state == State::Unavailable {
        dim(&mut pm, DIM);
    }
    pm
}

fn draw_stack(pm: &mut Pixmap, font: &Font, l: &str, unit: Unit, value: Option<f32>, x: f32, right: f32, scale: f32, alert: bool) {
    let ls = stack_label_style();
    let vs = stack_value_style();
    let lb = snap(7.0, scale);
    let vb = snap(16.5, scale);
    let t = l.to_uppercase();
    let lw = font.measure(&t, &ls);
    let _ = x;
    font.draw(pm, &t, &ls, right - lw, lb, scale, &ink(1.0));
    let text = value.map_or("–".to_string(), |v| format_value(v, unit));
    number(pm, font, &text, unit_text(unit), &vs, right, vb, scale, alert);
}

/// Multiplies every pixel's alpha by `a`.
pub fn dim(pm: &mut Pixmap, a: f32) {
    for p in pm.pixels_mut() {
        let c = p.demultiply();
        let na = (c.alpha() as f32 * a).round() as u8;
        *p = tiny_skia::ColorU8::from_rgba(c.red(), c.green(), c.blue(), na).premultiply();
    }
}
