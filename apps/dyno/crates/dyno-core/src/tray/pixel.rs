//! Windows notification-area and Linux appindicator items: square icons drawn for each exact pixel size (16, 20, 24, 32 on
//! Windows at 100 to 200 %; 22 on Linux) in Dyno's dash numerals (design: `docs/design/dyno-tray/README.md`). Straight
//! strokes sit on whole pixels; only arcs are anti-aliased. No font is needed.

use super::Icon;
use tiny_skia::{Color, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// A colour.
pub type Rgba = [u8; 4];

/// Taskbar or panel theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// A dark taskbar or panel.
    pub dark: bool,
}

impl Theme {
    /// Numerals and labels: white on a dark taskbar, near-black on a light one.
    pub fn ink(&self) -> Rgba {
        if self.dark {
            [0xff, 0xff, 0xff, 0xff]
        } else {
            [0x1b, 0x1b, 0x1f, 0xff]
        }
    }
    /// Meter and graph fill: Dyno's accent (design guide: dark #F063A8, light #B01E6E).
    pub fn accent(&self) -> Rgba {
        if self.dark {
            [0xf0, 0x63, 0xa8, 0xff]
        } else {
            [0xb0, 0x1e, 0x6e, 0xff]
        }
    }
    /// Tracks: the ink at 55 % on a light taskbar, 40 % on a dark one (3:1 non-text contrast against either).
    pub fn track(&self) -> Rgba {
        let mut c = self.ink();
        c[3] = if self.dark { 0x66 } else { 0x8c };
        c
    }
    /// The track of an alert bar: the neutral track, so the danger fill stays distinct from it.
    pub fn alert_track(&self) -> Rgba {
        self.track()
    }
    /// The alert plate (the family's danger colour) and the ink on it.
    pub fn alert(&self) -> (Rgba, Rgba) {
        if self.dark {
            ([0xff, 0x6b, 0x6b, 0xff], [0x14, 0x17, 0x1c, 0xff])
        } else {
            ([0xc4, 0x2b, 0x2b, 0xff], [0xff, 0xff, 0xff, 0xff])
        }
    }
}

/// The dash numerals at one size: height, width, stroke.
#[derive(Clone, Copy, Debug)]
pub struct Numerals {
    /// Height in pixels.
    pub h: i32,
    /// Width in pixels.
    pub w: i32,
    /// Stroke in pixels.
    pub s: i32,
}

/// A 1-bit glyph.
pub struct Glyph {
    /// Width in pixels.
    pub w: i32,
    /// Height in pixels.
    pub h: i32,
    /// Row-major: set or not.
    pub bits: Vec<bool>,
}

impl Glyph {
    fn new(w: i32, h: i32) -> Glyph {
        Glyph {
            w,
            h,
            bits: vec![false; (w * h) as usize],
        }
    }
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                self.bits[(y * self.w + x) as usize] = true;
            }
        }
    }
    fn clear(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.bits[(y * self.w + x) as usize] = false;
        }
    }
    /// Whether the pixel at (x, y) is set (false outside).
    pub fn at(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.bits[(y * self.w + x) as usize]
    }
}

/// One numeral (or ".", "°", "-") drawn from segments with chamfered outer corners.
pub fn numeral(ch: char, n: Numerals) -> Glyph {
    numeral_with(ch, n, true)
}

/// A numeral; `flag` false draws the 1 as a bare stem (only where three digits would not fit otherwise: 100 at 16 px).
pub fn numeral_with(ch: char, n: Numerals, flag: bool) -> Glyph {
    let Numerals { h, w, s } = n;
    let mid = (h - s) / 2;
    let k = (s - 1).max(1); // chamfer: pixels with dx + dy < k at the corner go
    let seg = |g: &mut Glyph, c: char| match c {
        'a' => g.fill(0, 0, w, s),
        'b' => g.fill(w - s, 0, w, mid + s),
        'c' => g.fill(w - s, mid, w, h),
        'd' => g.fill(0, h - s, w, h),
        'e' => g.fill(0, mid, s, h),
        'f' => g.fill(0, 0, s, mid + s),
        'g' => g.fill(0, mid, w, mid + s),
        _ => {}
    };
    let chamfer = |g: &mut Glyph, corner: &str| {
        let (cx, cy, dx, dy) = match corner {
            "tl" => (0, 0, 1, 1),
            "tr" => (w - 1, 0, -1, 1),
            "bl" => (0, h - 1, 1, -1),
            "br" => (w - 1, h - 1, -1, -1),
            "mr" => (w - 1, mid, -1, 1),
            "ml" => (0, mid + s - 1, 1, -1),
            _ => return,
        };
        for i in 0..k {
            for j in 0..k - i {
                g.clear(cx + i * dx, cy + j * dy);
            }
        }
    };
    let (segs, corners): (&str, &[&str]) = match ch {
        '0' => ("abcdef", &["tl", "tr", "bl", "br"]),
        '2' => ("abged", &["tl", "tr"]),
        '3' => ("abgcd", &["tr", "br"]),
        '4' => ("fgbc", &[]),
        '5' => ("afgcd", &["mr", "br"]),
        '6' => ("afgcde", &["tl", "mr", "bl", "br"]),
        '7' => ("abc", &[]),
        '8' => ("abcdefg", &["tl", "tr", "bl", "br"]),
        '9' => ("abcdfg", &["tl", "tr", "br", "ml"]),
        '-' => ("g", &[]),
        _ => ("", &[]),
    };
    match ch {
        '1' if !flag => {
            let mut g = Glyph::new(s, h);
            g.fill(0, 0, s, h);
            g
        }
        '1' => {
            let gw = s + 1;
            let mut g = Glyph::new(gw, h);
            g.fill(gw - s, 0, gw, h);
            g.fill(0, 1, gw - s, 1 + s);
            g
        }
        '.' => {
            let mut g = Glyph::new(s, h);
            g.fill(0, h - s, s, h);
            g
        }
        '°' => {
            let d = s * 2 + 1 + i32::from(s > 1);
            let mut g = Glyph::new(d, h);
            g.fill(0, 0, d, d);
            for y in s..d - s {
                for x in s..d - s {
                    g.clear(x, y);
                }
            }
            for (cx, cy) in [(0, 0), (d - 1, 0), (0, d - 1), (d - 1, d - 1)] {
                g.clear(cx, cy);
            }
            g
        }
        _ => {
            let mut g = Glyph::new(w, h);
            for c in segs.chars() {
                seg(&mut g, c);
            }
            for c in corners {
                chamfer(&mut g, c);
            }
            g
        }
    }
}

const CAPS5: &[(char, [&str; 5])] = &[
    ('A', [" # ", "# #", "###", "# #", "# #"]),
    ('B', ["## ", "# #", "## ", "# #", "## "]),
    ('C', [" ##", "#  ", "#  ", "#  ", " ##"]),
    ('D', ["## ", "# #", "# #", "# #", "## "]),
    ('E', ["###", "#  ", "## ", "#  ", "###"]),
    ('F', ["###", "#  ", "## ", "#  ", "#  "]),
    ('G', [" ##", "#  ", "# #", "# #", " ##"]),
    ('K', ["# #", "# #", "## ", "# #", "# #"]),
    ('M', ["#   #", "## ##", "# # #", "#   #", "#   #"]),
    ('N', ["#  #", "## #", "# ##", "#  #", "#  #"]),
    ('P', ["## ", "# #", "## ", "#  ", "#  "]),
    ('S', [" ##", "#  ", " # ", "  #", "## "]),
    ('T', ["###", " # ", " # ", " # ", " # "]),
    ('U', ["# #", "# #", "# #", "# #", "###"]),
    ('W', ["#   #", "#   #", "# # #", "## ##", "#   #"]),
];

const CAPS7: &[(char, [&str; 7])] = &[
    ('A', [" ## ", "#  #", "#  #", "####", "#  #", "#  #", "#  #"]),
    ('B', ["### ", "#  #", "#  #", "### ", "#  #", "#  #", "### "]),
    ('C', [" ###", "#   ", "#   ", "#   ", "#   ", "#   ", " ###"]),
    ('D', ["### ", "#  #", "#  #", "#  #", "#  #", "#  #", "### "]),
    ('E', ["####", "#   ", "#   ", "### ", "#   ", "#   ", "####"]),
    ('F', ["####", "#   ", "#   ", "### ", "#   ", "#   ", "#   "]),
    ('G', [" ###", "#   ", "#   ", "# ##", "#  #", "#  #", " ###"]),
    ('K', ["#  #", "#  #", "# # ", "##  ", "# # ", "#  #", "#  #"]),
    ('M', ["#   #", "## ##", "# # #", "#   #", "#   #", "#   #", "#   #"]),
    ('N', ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"]),
    ('P', ["### ", "#  #", "#  #", "### ", "#   ", "#   ", "#   "]),
    ('S', [" ###", "#   ", "#   ", " ## ", "   #", "   #", "### "]),
    ('T', ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('U', ["#  #", "#  #", "#  #", "#  #", "#  #", "#  #", " ## "]),
    ('W', ["#   #", "#   #", "#   #", "#   #", "# # #", "## ##", "#   #"]),
];

/// A capital from the 5- or 7-row label alphabet.
pub fn cap(ch: char, rows: i32) -> Glyph {
    let rows_of: Vec<&str> = if rows >= 7 {
        CAPS7.iter().find(|(c, _)| *c == ch).map(|(_, r)| r.to_vec()).unwrap_or_default()
    } else {
        CAPS5.iter().find(|(c, _)| *c == ch).map(|(_, r)| r.to_vec()).unwrap_or_default()
    };
    let h = if rows >= 7 { 7 } else { 5 };
    let w = rows_of.first().map_or(3, |r| r.len() as i32);
    let mut g = Glyph::new(w, h);
    for (y, r) in rows_of.iter().enumerate() {
        for (x, c) in r.chars().enumerate() {
            if c == '#' {
                g.fill(x as i32, y as i32, x as i32 + 1, y as i32 + 1);
            }
        }
    }
    g
}

/// A square icon being drawn.
pub struct Canvas {
    /// The pixels.
    pub pm: Pixmap,
    /// Width and height.
    pub size: i32,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], c[3]));
    p.anti_alias = true;
    p
}

impl Canvas {
    /// A transparent canvas, `None` for a size of zero.
    pub fn new(size: i32) -> Option<Canvas> {
        Some(Canvas {
            pm: Pixmap::new(size.max(0) as u32, size.max(0) as u32)?,
            size,
        })
    }
    /// Fills whole pixels from (x0, y0) to (x1, y1), exclusive.
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let Some(r) = tiny_skia::Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32) else {
            return;
        };
        let mut p = paint(c);
        p.anti_alias = false;
        self.pm.fill_rect(r, &p, Transform::identity(), None);
    }
    /// A rectangle with its four corner pixels left out (a 1 px rounding on whole pixels).
    pub fn rounded(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        if y1 - y0 >= 3 && x1 - x0 >= 3 {
            self.rect(x0 + 1, y0, x1 - 1, y0 + 1, c);
            self.rect(x0, y0 + 1, x1, y1 - 1, c);
            self.rect(x0 + 1, y1 - 1, x1 - 1, y1, c);
        } else {
            self.rect(x0, y0, x1, y1, c);
        }
    }
    /// Draws a glyph with its top left at (x, y).
    pub fn glyph(&mut self, g: &Glyph, x: i32, y: i32, c: Rgba) {
        for gy in 0..g.h {
            for gx in 0..g.w {
                if g.at(gx, gy) {
                    self.rect(x + gx, y + gy, x + gx + 1, y + gy + 1, c);
                }
            }
        }
    }
    /// Strokes an arc with round caps (angles in degrees, clockwise from +x).
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, from: f32, sweep: f32, w: f32, c: Rgba) {
        if sweep <= 0.0 {
            return;
        }
        let mut pb = PathBuilder::new();
        let steps = ((sweep / 4.0).ceil() as usize).max(2);
        for i in 0..=steps {
            let t = (from + sweep * i as f32 / steps as f32).to_radians();
            let (x, y) = (cx + r * t.cos(), cy + r * t.sin());
            if i == 0 {
                pb.move_to(x, y)
            } else {
                pb.line_to(x, y)
            }
        }
        let st = Stroke {
            width: w,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        if let Some(p) = pb.finish() {
            self.pm.stroke_path(&p, &paint(c), &st, Transform::identity(), None);
        }
    }
    /// Fills a polygon (anti-aliased).
    pub fn poly(&mut self, pts: &[(f32, f32)], c: Rgba) {
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(*x, *y)
            } else {
                pb.line_to(*x, *y)
            }
        }
        pb.close();
        if let Some(p) = pb.finish() {
            self.pm
                .fill_path(&p, &paint(c), tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// The alert notch: a right triangle in the top left corner, `k` px on its first row (rows of k, k-1, .. 1; 5 when the digits
/// leave room, 4 at the least). Always whole.
fn draw_notch(cv: &mut Canvas, plate: Rgba, k: i32) {
    for y in 0..k {
        cv.rect(0, y, k - y, y + 1, plate);
    }
}

/// Whether (x, y) is on or touches (also diagonally) the notch of size `k`: the pixels glyphs must stay out of.
fn in_notch_zone(k: i32, x: i32, y: i32) -> bool {
    (x - 1).max(0) + (y - 1).max(0) < k && x >= 0 && y >= 0 && x < k + 1 && y < k + 1
}

/// The numerals to try at one size, widest first: as laid out, one pixel narrower, and a condensed hairline set.
pub fn ladder(n: Numerals) -> [Numerals; 3] {
    [n, Numerals { w: n.w - 1, ..n }, Numerals { w: n.w - 2, s: 1, ..n }]
}

/// The layout of one size.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The icon's size in pixels.
    pub size: i32,
    /// Label caps rows (0: no label at this size).
    pub label_rows: i32,
    /// Top row of the label.
    pub label_y: i32,
    /// The numerals.
    pub digits: Numerals,
    /// Top row of the numerals.
    pub digits_y: i32,
    /// Gap between numerals.
    pub tracking: i32,
    /// Top row of the meter.
    pub meter_y: i32,
    /// Height of the meter.
    pub meter_h: i32,
    /// Small numerals for the network pair.
    pub small: Numerals,
}

/// The layouts per pixel size (Windows 100, 125, 150, 200 %; Linux 22).
pub fn layout(size: i32) -> Layout {
    match size {
        ..=17 => Layout {
            size: 16,
            label_rows: 0,
            label_y: 0,
            digits: Numerals { h: 10, w: 6, s: 2 },
            digits_y: 2,
            tracking: 1,
            meter_y: 13,
            meter_h: 2,
            small: Numerals { h: 5, w: 3, s: 1 },
        },
        18..=21 => Layout {
            size: 20,
            label_rows: 0,
            label_y: 0,
            digits: Numerals { h: 11, w: 6, s: 2 },
            digits_y: 2,
            tracking: 1,
            meter_y: 15,
            meter_h: 3,
            small: Numerals { h: 7, w: 4, s: 1 },
        },
        22..=23 => Layout {
            size: 22,
            label_rows: 5,
            label_y: 0,
            digits: Numerals { h: 11, w: 6, s: 2 },
            digits_y: 7,
            tracking: 1,
            meter_y: 20,
            meter_h: 2,
            small: Numerals { h: 7, w: 4, s: 1 },
        },
        24..=31 => Layout {
            size: 24,
            label_rows: 5,
            label_y: 1,
            digits: Numerals { h: 12, w: 7, s: 2 },
            digits_y: 8,
            tracking: 1,
            meter_y: 21,
            meter_h: 2,
            small: Numerals { h: 7, w: 4, s: 1 },
        },
        _ => Layout {
            size: 32,
            label_rows: 7,
            label_y: 1,
            digits: Numerals { h: 16, w: 9, s: 3 },
            digits_y: 10,
            tracking: 2,
            meter_y: 28,
            meter_h: 3,
            small: Numerals { h: 9, w: 5, s: 1 },
        },
    }
}

fn glyphs(text: &str, n: Numerals, tracking: i32, room: i32) -> Vec<Glyph> {
    let with_flag: Vec<Glyph> = text.chars().map(|c| numeral(c, n)).collect();
    let w = with_flag.iter().map(|g| g.w).sum::<i32>() + tracking * (with_flag.len() as i32 - 1).max(0);
    if w <= room {
        with_flag
    } else {
        text.chars().map(|c| numeral_with(c, n, false)).collect()
    }
}

fn text_width(text: &str, n: Numerals, tracking: i32) -> i32 {
    let gs = glyphs(text, n, tracking, i32::MAX);
    gs.iter().map(|g| g.w).sum::<i32>() + tracking * (gs.len() as i32 - 1).max(0)
}

/// Draws numerals right-aligned at `right` (exclusive), within `right` pixels. Returns the left edge.
pub fn digits(cv: &mut Canvas, text: &str, n: Numerals, tracking: i32, right: i32, y: i32, c: Rgba) -> i32 {
    let placed = place(text, n, tracking, right);
    for (g, x) in &placed {
        cv.glyph(g, *x, y, c);
    }
    placed.first().map_or(right, |(_, x)| *x)
}

/// The glyphs of `text` with their left edges, right-aligned at `right`.
fn place(text: &str, n: Numerals, tracking: i32, right: i32) -> Vec<(Glyph, i32)> {
    let gs = glyphs(text, n, tracking, right);
    let w = gs.iter().map(|g| g.w).sum::<i32>() + tracking * (gs.len() as i32 - 1).max(0);
    let mut x = right - w;
    let mut out = Vec::new();
    for g in gs {
        let gw = g.w;
        out.push((g, x));
        x += gw + tracking;
    }
    out
}

/// How the numbers fit beside an alert notch.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    /// The numerals.
    pub n: Numerals,
    /// Top row of the numerals.
    pub y: i32,
    /// The notch size.
    pub k: i32,
}

/// The numerals and notch size that keep a 1 px gutter between the notch and every glyph pixel: the full 5 px notch first, with
/// the digits narrowing (see [`ladder`]) before the notch shrinks to 4 px. The row never moves, so an alert does not make the
/// digits jump.
pub fn fit(text: &str, l: &Layout, right: i32) -> Fit {
    let y = l.digits_y;
    for k in [5, 4] {
        for n in ladder(l.digits) {
            let clear = place(text, n, l.tracking, right)
                .iter()
                .all(|(g, x)| (0..g.h).all(|gy| (0..g.w).all(|gx| !g.at(gx, gy) || !in_notch_zone(k, x + gx, y + gy))));
            if clear {
                return Fit { n, y, k };
            }
        }
    }
    Fit { n: l.digits, y, k: 4 }
}

/// Draws a label right-aligned at `right`.
pub fn label(cv: &mut Canvas, text: &str, rows: i32, right: i32, y: i32, c: Rgba) {
    let gs: Vec<Glyph> = text.chars().map(|ch| cap(ch, rows)).collect();
    let w: i32 = gs.iter().map(|g| g.w).sum::<i32>() + gs.len() as i32 - 1;
    let mut x = right - w;
    for g in gs {
        cv.glyph(&g, x, y, c);
        x += g.w + 1;
    }
}

/// The kinds on Windows and Linux.
#[derive(Clone, Debug)]
pub enum Kind {
    /// A percentage and its meter, with a label from 22 px.
    Percent {
        /// Up to three capitals.
        label: String,
    },
    /// A meter, with a label from 22 px.
    Meter {
        /// Up to three capitals.
        label: String,
    },
    /// One vertical column filled from the bottom, with a label above it from 22 px.
    Bar {
        /// Up to three capitals.
        label: String,
    },
    /// One column per core.
    Cores,
    /// An area graph, with a label from 22 px.
    Graph {
        /// Up to three capitals.
        label: String,
    },
    /// The number over a small area graph of the recent values (the label is in the tooltip).
    GraphValue,
    /// A 270-degree gauge with the metric's letter.
    Ring {
        /// The letter.
        letter: char,
    },
    /// One direction's rate.
    Network(super::NetDir),
    /// Degrees and a meter from 20 to 110.
    Temperature,
}

/// What an item shows now (same meaning as on macOS).
#[derive(Clone, Debug, Default)]
pub struct Reading {
    /// The value; `None` when unavailable.
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

/// The item's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// As measured.
    Normal,
    /// Over the threshold: below 24 px a corner notch and a danger bar under solid digits; from 24 px the number on a
    /// danger plate with a 1 px margin.
    Alert,
    /// No value: a dash, the icon at 45 %.
    Unavailable,
}

#[allow(clippy::too_many_arguments)]
fn meter(cv: &mut Canvas, l: &Layout, v: Option<f32>, t: Theme, x0: i32, x1: i32, y: i32, h: i32) {
    meter_in(cv, l, v, t.track(), t.accent(), x0, x1, y, h);
}

#[allow(clippy::too_many_arguments)]
fn meter_in(cv: &mut Canvas, l: &Layout, v: Option<f32>, track: Rgba, fill: Rgba, x0: i32, x1: i32, y: i32, h: i32) {
    // Square ends on whole pixels: at 2 to 4 px high a rounded end only notches the corners.
    cv.rect(x0, y, x1, y + h, track);
    if let Some(v) = v {
        let w = ((x1 - x0) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
        let w = if v > 0.5 { w.max(1) } else { w };
        cv.rect(x0, y, x0 + w, y + h, fill);
    }
    let _ = l;
}

/// An area graph of the newest values on whole pixels: a faint plate from `top` to above the baseline track at row `bot - 1`,
/// one column per value (newest at the right), each value above 0.5 % at least one pixel above the track.
#[allow(clippy::too_many_arguments)]
fn area_graph(cv: &mut Canvas, history: &[f32], x0: i32, x1: i32, top: i32, bot: i32, t: Theme, fill: Option<Rgba>) {
    cv.rect(x0, top, x1, bot - 1, {
        let mut c = t.ink();
        c[3] = 0x18;
        c
    });
    cv.rect(x0, bot - 1, x1, bot, t.track());
    let Some(fill) = fill else {
        return;
    };
    let cols = (x1 - x0).max(0) as usize;
    let recent: Vec<f32> = history.iter().rev().take(cols).rev().copied().collect();
    let start = x1 - recent.len() as i32;
    for (i, v) in recent.iter().enumerate() {
        let h = ((bot - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
        // A sample above zero shows at least one pixel above the baseline track.
        let h = if *v > 0.5 { h.max(2) } else { h };
        cv.rect(start + i as i32, bot - h, start + i as i32 + 1, bot, fill);
    }
}

/// The width of the single column of [`Kind::Bar`]: half the icon, even, so it centres on whole pixels (8 at 16 px, 16 at 32).
fn bar_width(size: i32) -> i32 {
    (size / 2) & !1
}

/// The unavailable mark (design: "an en dash in the field"): a bar of ink from `x0` to `x1`, `h` rows high and centred on row
/// `cy`. With it, an item whose agent is away never looks like a reading of 0 that is only dimmer.
fn dash(cv: &mut Canvas, x0: i32, x1: i32, cy: i32, h: i32, c: Rgba) {
    let y0 = cy - h / 2;
    cv.rect(x0, y0, x1, y0 + h, c);
}

/// The dash's thickness at each size.
fn dash_h(size: i32) -> i32 {
    if size >= 32 {
        3
    } else {
        2
    }
}

/// A rate as up to three figures and a unit letter at the small sizes.
pub fn short_rate(bps: f64) -> (String, char) {
    let units = ['B', 'K', 'M', 'G'];
    let mut v = bps.max(0.0);
    let mut i = 0;
    while v >= 999.5 && i < 3 {
        v /= 1000.0;
        i += 1;
    }
    let t = if v < 9.95 && i > 0 {
        format!("{v:.1}")
    } else {
        format!("{}", v.round() as u64)
    };
    (t, units[i])
}

/// Renders one item at `size` pixels for a light or dark taskbar.
pub fn render(kind: &Kind, r: &Reading, state: State, size: i32, t: Theme) -> Icon {
    let l = layout(size);
    let Some(mut cv) = Canvas::new(l.size) else {
        return Icon::new(l.size.max(0) as u32);
    };
    let s = l.size;
    let value = if state == State::Unavailable { None } else { r.value };
    let ink = t.ink();
    let right = if l.size <= 16 { s } else { s - 1 };
    let (plate, plate_ink) = t.alert();
    let small_alert = l.size < 24;
    let danger_track = t.alert_track();
    // The notch is drawn last, whole; its size comes from the digits' fit.
    let notch_k = std::cell::Cell::new(0);
    let notch = |_: &mut Canvas| notch_k.set(5);
    // From 24 px the number sits on a tile of one fixed width (that of "100") and the bar spans the same box in both states.
    let tile_w = (text_width("100", l.digits, l.tracking) + 2).min(s);
    let tile_x0 = s - tile_w;
    // One alert encoding at every size: a danger bar.
    let bar = |cv: &mut Canvas, v: Option<f32>, y: i32, h: i32, tiled: bool| {
        let (x0, x1) = if tiled && !small_alert { (tile_x0, s) } else { (1, right) };
        if state == State::Alert {
            meter_in(cv, &l, v, danger_track, plate, x0, x1, y, h);
        } else {
            meter_in(cv, &l, v, t.track(), t.accent(), x0, x1, y, h);
        }
    };
    // From 24 px the digits are centred in the tile, in both states (the right edge of the box they are drawn against).
    let centred = |text: &str| (tile_x0 + (tile_w + text_width(text, l.digits, l.tracking)) / 2).min(s);
    // The number with its alert notch (small) or plate (from 24 px).
    let num = |cv: &mut Canvas, text: &str| {
        let n = l.digits;
        if state == State::Alert && small_alert {
            // Below 24 px a knocked-out plate smears the digits: they stay solid ink and the alert is the corner notch and
            // the danger bar under them. The digits yield to the notch (`fit`).
            let f = fit(text, &l, right);
            notch_k.set(f.k);
            digits(cv, text, f.n, l.tracking, right, f.y, ink);
        } else if state == State::Alert {
            cv.rounded(tile_x0, l.digits_y - 1, s, l.digits_y + n.h + 1, plate);
            digits(cv, text, n, l.tracking, centred(text), l.digits_y, plate_ink);
        } else if small_alert {
            digits(cv, text, n, l.tracking, right, l.digits_y, ink);
        } else {
            digits(cv, text, n, l.tracking, centred(text), l.digits_y, ink);
        }
    };
    match kind {
        Kind::Percent { label: lb } => {
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            let text = value.map_or("-".to_string(), |v| format!("{}", v.clamp(0.0, 100.0).round() as u32));
            num(&mut cv, &text);
            bar(&mut cv, value, l.meter_y, l.meter_h, true);
        }
        Kind::Temperature => {
            if l.label_rows > 0 {
                label(&mut cv, "TMP", l.label_rows, right, l.label_y, ink);
            }
            if l.size <= 20 {
                // 16 and 20 px: two bold digits and a 3 px degree ring tucked into the top right (no gap would fit).
                let text = value.map_or("-".to_string(), |v| format!("{}", v.round().min(99.0) as u32));
                let f = if state == State::Alert {
                    fit(&text, &l, s - 3)
                } else {
                    Fit {
                        n: l.digits,
                        y: l.digits_y,
                        k: 5,
                    }
                };
                if state == State::Alert {
                    notch_k.set(f.k);
                }
                let c = ink;
                digits(&mut cv, &text, f.n, l.tracking, s - 3, f.y, c);
                if value.is_some() {
                    cv.glyph(&numeral('°', Numerals { h: 3, w: 3, s: 1 }), s - 3, f.y, c);
                }
            } else {
                let text = match value {
                    None => "-".to_string(),
                    Some(v) if v >= 99.5 => format!("{}", v.round() as u32),
                    Some(v) => format!("{}°", v.round() as u32),
                };
                num(&mut cv, &text);
            }
            bar(&mut cv, value.map(|v| (v - 20.0) / 0.9), l.meter_y, l.meter_h, true);
        }
        Kind::Meter { label: lb } => {
            // Three eighths of the icon high (6 rows at 16 px), so the mark has the weight of the other styles.
            let mh = (s * 3 + 4) / 8;
            let my = if l.label_rows > 0 {
                l.label_y + l.label_rows + (s - l.label_rows - mh) / 2
            } else {
                (s - mh) / 2
            };
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            bar(&mut cv, value, my, mh, false);
            if state == State::Unavailable {
                let w = (right - 1) / 2;
                dash(
                    &mut cv,
                    1 + (right - 1 - w) / 2,
                    1 + (right - 1 + w) / 2,
                    my + mh / 2,
                    dash_h(s),
                    ink,
                );
            }
            if state == State::Alert {
                notch(&mut cv);
            }
        }
        Kind::Cores => {
            let n = r.cores.len().max(1) as i32;
            let avail = s - 2;
            let gap = i32::from(n * 2 <= avail);
            let w = ((avail + gap) / n - gap).max(1);
            let total = n * w + (n - 1) * gap;
            let x0 = (s - total) / 2;
            let (top, bot) = (1, s - 1);
            for (i, v) in r.cores.iter().enumerate() {
                let x = x0 + i as i32 * (w + gap);
                cv.rect(x, top, x + w, bot, t.track());
                if state != State::Unavailable {
                    let h = ((bot - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
                    let h = if *v > 0.5 { h.max(1) } else { h };
                    cv.rect(x, bot - h, x + w, bot, t.accent());
                }
            }
            if state == State::Unavailable {
                dash(&mut cv, x0, x0 + total, s / 2, dash_h(s), ink);
            }
        }
        Kind::Graph { label: lb } => {
            let top = if l.label_rows > 0 { l.label_y + l.label_rows + 2 } else { 1 };
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            let fill = match state {
                State::Unavailable => None,
                State::Alert => Some(plate),
                State::Normal => Some(t.accent()),
            };
            area_graph(&mut cv, &r.history, 1, right, top, s - 1, t, fill);
            match state {
                // The alert as on the meter and the bar: the corner notch (a shape) and the danger fill.
                State::Alert => notch(&mut cv),
                State::Unavailable => {
                    let w = (right - 1) / 2;
                    dash(
                        &mut cv,
                        1 + (right - 1 - w) / 2,
                        1 + (right - 1 + w) / 2,
                        (top + s - 2) / 2,
                        dash_h(s),
                        ink,
                    );
                }
                State::Normal => {}
            }
        }
        Kind::Bar { label: lb } => {
            // One column, a level standing up, never a meter on its side. Below 22 px it is centred; from 22 px the label sits at
            // the right as on every other style, and the column stands right-aligned under it.
            let w = bar_width(s);
            let (x0, top) = if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
                (right - w, l.label_y + l.label_rows + 2)
            } else {
                ((s - w) / 2, 1)
            };
            let bot = s - 1;
            cv.rect(x0, top, x0 + w, bot, t.track());
            if let Some(v) = value {
                let h = ((bot - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
                let h = if v > 0.5 { h.max(1) } else { h };
                let fill = if state == State::Alert { plate } else { t.accent() };
                cv.rect(x0, bot - h, x0 + w, bot, fill);
            }
            if state == State::Unavailable {
                dash(&mut cv, x0 + 1, x0 + w - 1, (top + bot) / 2, dash_h(s), ink);
            }
            if state == State::Alert {
                notch(&mut cv);
            }
        }
        Kind::GraphValue => {
            // The number at the top right (the newest value, above the graph's newest column), the graph under it. No label:
            // the tooltip names the metric. At 16 and 20 px the number is in the 7-row numerals, so the graph gets 7 and 10
            // rows; 1 px margins all round, as on the other styles.
            let n = if s <= 20 { Numerals { h: 7, w: 4, s: 1 } } else { l.digits };
            let lg = Layout {
                digits_y: 1,
                digits: n,
                ..l
            };
            let text = value.map_or("-".to_string(), |v| format!("{}", v.clamp(0.0, 100.0).round() as u32));
            if state == State::Alert && small_alert {
                let f = fit(&text, &lg, right);
                notch_k.set(f.k);
                digits(&mut cv, &text, f.n, l.tracking, right, f.y, ink);
            } else if state == State::Alert {
                let w = text_width(&text, n, l.tracking) + 2;
                cv.rounded(right - w - 1, lg.digits_y - 1, right + 1, lg.digits_y + n.h + 1, plate);
                digits(&mut cv, &text, n, l.tracking, right, lg.digits_y, plate_ink);
            } else {
                digits(&mut cv, &text, n, l.tracking, right, lg.digits_y, ink);
            }
            // At 16 px the plate starts right under the digits (7 rows of graph); larger icons keep a row between them.
            let top = lg.digits_y + n.h + if s <= 16 { 0 } else { 1 };
            let bot = s - 1;
            let fill = match state {
                State::Unavailable => None,
                State::Alert => Some(plate),
                State::Normal => Some(t.accent()),
            };
            area_graph(&mut cv, &r.history, 1, right, top, bot, t, fill);
        }
        Kind::Ring { letter } => {
            let c = s as f32 / 2.0;
            let sw = (s as f32 / 8.0).round().max(2.0);
            let rad = c - sw / 2.0 - 0.5;
            cv.arc(c, c, rad, 135.0, 270.0, sw, t.track());
            if let Some(v) = value {
                cv.arc(c, c, rad, 135.0, 270.0 * v.clamp(0.0, 100.0) / 100.0, sw, t.accent());
            }
            let rows = if s >= 32 { 7 } else { 5 };
            let g = cap(*letter, rows);
            let (gx, gy) = ((s - g.w) / 2, (s - g.h) / 2);
            if state == State::Alert && small_alert {
                notch(&mut cv);
                cv.glyph(&g, gx, gy, ink);
            } else if state == State::Alert {
                cv.rounded(gx - 2, gy - 2, gx + g.w + 2, gy + g.h + 2, plate);
                cv.glyph(&g, gx, gy, plate_ink);
            } else {
                cv.glyph(&g, gx, gy, ink);
            }
            if state == State::Unavailable {
                // In the gauge's open quarter at the bottom, under the letter.
                // Below 22 px, 6 px wide; from 22 px a third of the icon less 2 px at each end, 3 rows high, so it floats in the
                // gap and never touches the arc's ends (which would close the gauge into a cup).
                let (w, h) = if s <= 20 { (6, dash_h(s)) } else { (((s / 2) & !1) - 4, 3) };
                dash(&mut cv, (s - w) / 2, (s + w) / 2, s - 1 - h / 2 - 1, h, ink);
            }
        }
        Kind::Network(dir) => {
            let rows = dir.rows();
            // 16 and 20: a log meter with an arrow; 22 and up: a line of small numerals with a unit letter.
            let arrow = |cv: &mut Canvas, x: f32, y: f32, w: f32, up: bool| {
                let h = (w / 2.0).ceil();
                if up {
                    cv.poly(&[(x + w / 2.0, y), (x + w, y + h), (x, y + h)], ink);
                } else {
                    cv.poly(&[(x, y), (x + w, y), (x + w / 2.0, y + h)], ink);
                }
            };
            let level = |bps: f64| -> f32 { ((bps.max(1.0).log10() / 9.0) * 100.0) as f32 };
            if l.size <= 20 {
                let aw: f32 = if s <= 16 { 5.0 } else { 6.0 };
                let mh = if s <= 16 { 3 } else { 4 };
                for (row, &up) in rows.iter().enumerate() {
                    let bps = if up { r.up } else { r.down };
                    let y = match (rows.len(), row) {
                        (1, _) => s / 2 - mh / 2,
                        (_, 0) => s / 4 - mh / 2,
                        _ => s * 3 / 4 - mh / 2,
                    };
                    let ah = (aw / 2.0).ceil();
                    arrow(&mut cv, 0.0, y as f32 + (mh as f32 - ah) / 2.0, aw, up);
                    let v = if state == State::Unavailable { None } else { Some(level(bps)) };
                    meter(&mut cv, &l, v, t, aw as i32 + 1, right, y, mh);
                    if state == State::Unavailable {
                        let (x0, x1) = (aw as i32 + 1, right);
                        let w = (x1 - x0) / 2;
                        dash(&mut cv, x0 + (x1 - x0 - w) / 2, x0 + (x1 - x0 + w) / 2, y + mh / 2, mh, ink);
                    }
                }
            } else {
                let n = l.small;
                for (row, &up) in rows.iter().enumerate() {
                    let bps = if up { r.up } else { r.down };
                    let y = match (rows.len(), row) {
                        (1, _) => (s - n.h) / 2,
                        (_, 0) => (s / 2 - n.h) / 2,
                        _ => s / 2 + (s / 2 - n.h) / 2,
                    };
                    let aw = (n.w + 1) as f32;
                    arrow(&mut cv, 0.0, y as f32 + (n.h as f32 - aw / 2.0) / 2.0, aw, up);
                    let (text, unit) = if state == State::Unavailable {
                        ("-".to_string(), ' ')
                    } else {
                        short_rate(bps)
                    };
                    let ug = cap(if unit == 'B' { 'B' } else { unit }, if n.h >= 7 { 7 } else { 5 });
                    let ux = right - ug.w;
                    let uy = y + n.h - ug.h;
                    if unit != ' ' {
                        cv.glyph(&ug, ux, uy, ink);
                    }
                    digits(&mut cv, &text, n, 1, ux - 1, y, ink);
                }
            }
        }
    }
    if notch_k.get() > 0 {
        draw_notch(&mut cv, plate, notch_k.get());
    }
    let dim = if state == State::Unavailable { 0.45 } else { 1.0 };
    let size = l.size as u32;
    let rgba = cv
        .pm
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), (f32::from(c.alpha()) * dim).round() as u8]
        })
        .collect();
    Icon { size, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::NetDir;

    fn px(i: &Icon, x: u32, y: u32) -> [u8; 4] {
        let k = ((y * i.size + x) * 4) as usize;
        [i.rgba[k], i.rgba[k + 1], i.rgba[k + 2], i.rgba[k + 3]]
    }

    #[test]
    fn every_size_draws_its_own_square_and_three_digits_fit_16_px() {
        for size in [16, 20, 22, 24, 32] {
            let i = render(
                &Kind::Percent { label: "CPU".into() },
                &Reading {
                    value: Some(100.0),
                    ..Reading::default()
                },
                State::Normal,
                size,
                Theme { dark: true },
            );
            assert_eq!(i.size, size as u32);
            assert_eq!(i.rgba.len(), (size * size * 4) as usize);
        }
        let n = layout(16);
        assert!(text_width("100", n.digits, n.tracking) > 16, "with flags it would not fit");
        assert_eq!(
            glyphs("100", n.digits, n.tracking, 16).iter().map(|g| g.w).sum::<i32>() + 2 * n.tracking,
            16
        );
    }

    #[test]
    fn below_24_px_an_alert_keeps_solid_digits_and_adds_a_notch_and_a_danger_bar() {
        for dark in [false, true] {
            let t = Theme { dark };
            let (plate, _) = t.alert();
            for size in [16, 20, 22] {
                let i = render(
                    &Kind::Percent { label: "MEM".into() },
                    &Reading {
                        value: Some(93.0),
                        ..Reading::default()
                    },
                    State::Alert,
                    size,
                    t,
                );
                assert_eq!(px(&i, 0, 0), plate, "the notch, {size}");
                let l = layout(size);
                assert_eq!(px(&i, 1, l.meter_y as u32), plate, "the danger bar, {size}");
                let ink = (0..i.size * i.size)
                    .filter(|k| i.rgba[(*k * 4) as usize..(*k * 4 + 4) as usize] == t.ink())
                    .count();
                assert!(ink > 20, "the digits stay in ink, {size}");
            }
        }
    }

    #[test]
    fn straight_parts_are_whole_pixels() {
        for size in [16, 20, 24, 32] {
            for k in [
                Kind::Percent { label: "CPU".into() },
                Kind::Cores,
                Kind::Graph { label: "DSK".into() },
                Kind::Temperature,
            ] {
                let r = Reading {
                    value: Some(37.0),
                    history: vec![12.0, 60.0, 3.0],
                    cores: vec![10.0, 90.0, 40.0, 0.0],
                    ..Reading::default()
                };
                let i = render(&k, &r, State::Normal, size, Theme { dark: false });
                // Tracks are translucent by design; nothing else is partial.
                let partial = i
                    .rgba
                    .chunks(4)
                    .filter(|p| p[3] != 0 && p[3] != 0xff && p[3] != 0x8c && p[3] != 0x18)
                    .count();
                assert_eq!(partial, 0, "{k:?} at {size}");
            }
        }
    }

    #[test]
    fn odd_input_draws_a_defined_state() {
        for v in [f32::NAN, -5.0, 1e9] {
            let i = render(
                &Kind::Percent { label: "CPU".into() },
                &Reading {
                    value: Some(v),
                    ..Reading::default()
                },
                State::Normal,
                16,
                Theme { dark: true },
            );
            assert_eq!(i.size, 16);
        }
        let i = render(&Kind::Cores, &Reading::default(), State::Normal, 16, Theme { dark: true });
        assert_eq!(i.size, 16);
    }

    fn alert_icon(kind: &Kind, v: f32, size: i32, dark: bool) -> Icon {
        let r = Reading {
            value: Some(v),
            ..Reading::default()
        };
        render(kind, &r, State::Alert, size, Theme { dark })
    }

    fn near(i: &Icon, x: i32, y: i32, f: &dyn Fn([u8; 4]) -> bool) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < i.size as i32 && ny < i.size as i32 && f(px(i, nx as u32, ny as u32))
            })
        })
    }

    #[test]
    fn the_notch_never_touches_a_glyph_at_any_value_size_or_theme() {
        for dark in [false, true] {
            let t = Theme { dark };
            let (plate, _) = t.alert();
            for size in [16, 20, 22] {
                let l = layout(size);
                for v in 0..=100 {
                    let i = alert_icon(&Kind::Percent { label: "CPU".into() }, v as f32, size, dark);
                    let mut notch = 0;
                    for y in 0..l.meter_y {
                        for x in 0..size {
                            if px(&i, x as u32, y as u32) == plate {
                                notch += 1;
                                assert!(!near(&i, x, y, &|p| p == t.ink()), "notch ({x},{y}) touches a glyph: {v} at {size}");
                            }
                        }
                    }
                    assert_eq!(notch, 15, "the notch is whole: {v} at {size}");
                }
            }
        }
    }

    #[test]
    fn the_notch_has_the_same_shape_for_every_value() {
        let (plate, _) = Theme { dark: false }.alert();
        for size in [16, 20, 22] {
            for v in [0.0, 7.0, 17.0, 33.0, 88.0, 100.0] {
                let i = alert_icon(&Kind::Percent { label: "CPU".into() }, v, size, false);
                for (y, w) in [(0, 5), (1, 4), (2, 3), (3, 2), (4, 1)] {
                    for x in 0..5 {
                        assert_eq!(px(&i, x, y) == plate, x < w, "({x},{y}) {v} at {size}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_digit_stays_distinct_in_every_numeral_set_of_the_ladder() {
        for size in [16, 20, 22, 24, 32] {
            for n in ladder(layout(size).digits) {
                let gs: Vec<Glyph> = "0123456789".chars().map(|c| numeral_with(c, n, false)).collect();
                for a in 0..10 {
                    for b in a + 1..10 {
                        assert!(
                            gs[a].w != gs[b].w || gs[a].bits != gs[b].bits,
                            "{a} and {b} differ at {size} ({n:?})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_bar_spans_the_same_box_normal_or_alert() {
        let cols = |i: &Icon, y: u32| (0..i.size).filter(|x| px(i, *x, y)[3] != 0).collect::<Vec<_>>();
        for size in [16, 20, 22, 24, 32] {
            let l = layout(size);
            for dark in [false, true] {
                let r = Reading {
                    value: Some(93.0),
                    ..Reading::default()
                };
                let k = Kind::Percent { label: "CPU".into() };
                let a = render(&k, &r, State::Alert, size, Theme { dark });
                let n = render(&k, &r, State::Normal, size, Theme { dark });
                assert_eq!(cols(&a, l.meter_y as u32), cols(&n, l.meter_y as u32), "{size}");
            }
        }
    }

    fn lum(c: [f32; 3]) -> f32 {
        let f = |v: f32| {
            let v = v / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
    }

    #[test]
    fn tracks_have_3_to_1_contrast_on_light_and_dark_taskbars() {
        // Windows 11's taskbars are about #f3f3f3 (light) and #202020 (dark); test lighter and darker as well.
        for (dark, bgs) in [(false, [0xe5u8, 0xf3, 0xff]), (true, [0x00, 0x18, 0x2b])] {
            let t = Theme { dark };
            for bgv in bgs.into_iter().chain(if dark { Some(0x20) } else { None }) {
                let bg = [f32::from(bgv); 3];
                let lb = lum(bg);
                for c in [t.track(), t.alert_track()] {
                    let a = f32::from(c[3]) / 255.0;
                    let mix = [0, 1, 2].map(|k| f32::from(c[k]) * a + bg[k] * (1.0 - a));
                    let (hi, lo) = (lum(mix).max(lb), lum(mix).min(lb));
                    let ratio = (hi + 0.05) / (lo + 0.05);
                    assert!(ratio >= 3.0, "dark {dark}: {c:?} on #{bgv:02x}: {ratio:.2}");
                }
            }
        }
    }
    #[test]
    fn a_hundred_is_three_distinct_digits_at_16_and_20_px() {
        for size in [16, 20] {
            let i = alert_icon(&Kind::Percent { label: "CPU".into() }, 100.0, size, false);
            let l = layout(size);
            let ink = Theme { dark: false }.ink();
            let cols: Vec<bool> = (0..size as u32)
                .map(|x| (l.digits_y as u32..(l.digits_y + l.digits.h) as u32).any(|y| px(&i, x, y) == ink))
                .collect();
            let runs = cols.windows(2).filter(|w| !w[0] && w[1]).count() + usize::from(cols[0]);
            assert_eq!(runs, 3, "1, 0 and 0 are separated by gaps at {size} px");
        }
    }

    #[test]
    fn the_alert_bar_is_danger_at_every_size_and_aligned_with_the_plate_from_24_px() {
        for dark in [false, true] {
            let t = Theme { dark };
            let (plate, _) = t.alert();
            for size in [16, 20, 22, 24, 32] {
                let l = layout(size);
                let i = alert_icon(&Kind::Percent { label: "CPU".into() }, 93.0, size, dark);
                let by = l.meter_y as u32;
                assert_eq!(px(&i, size as u32 / 2, by), plate, "bar fill is danger at {size}");
                assert_ne!(px(&i, size as u32 / 2, by), t.accent());
                if size >= 24 {
                    // The plate's left edge is where its first column has plate pixels at the digits' middle row.
                    let my = (l.digits_y + l.digits.h / 2) as u32;
                    let left = (0..size as u32).find(|x| px(&i, *x, my)[3] != 0).unwrap();
                    assert_eq!(px(&i, left, by), plate, "bar starts at the plate, {size}");
                    assert_ne!(px(&i, size as u32 - 1, by)[3], 0, "bar ends at the plate, {size}");
                    assert!(left < 2 || px(&i, left - 1, by)[3] == 0, "nothing left of the plate, {size}");
                }
            }
        }
    }

    #[test]
    fn digits_follow_the_taskbar_theme() {
        for size in [16, 20, 24, 32] {
            let r = Reading {
                value: Some(42.0),
                ..Reading::default()
            };
            let has = |dark: bool, c: Rgba| {
                render(&Kind::Percent { label: "CPU".into() }, &r, State::Normal, size, Theme { dark })
                    .rgba
                    .chunks(4)
                    .any(|p| p == c)
            };
            assert!(
                has(false, [0x1b, 0x1b, 0x1f, 0xff]) && !has(false, [0xff, 0xff, 0xff, 0xff]),
                "light taskbar: dark ink, {size}"
            );
            assert!(
                has(true, [0xff, 0xff, 0xff, 0xff]) && !has(true, [0x1b, 0x1b, 0x1f, 0xff]),
                "dark taskbar: white ink, {size}"
            );
        }
    }

    /// First and last column and row of the pixels of colour `c` at or below row `from`.
    fn extent(i: &Icon, c: Rgba, from: u32) -> Option<(u32, u32, u32, u32)> {
        let pts: Vec<(u32, u32)> = (from..i.size)
            .flat_map(|y| (0..i.size).map(move |x| (x, y)))
            .filter(|&(x, y)| px(i, x, y) == c)
            .collect();
        let f = |g: fn(&(u32, u32)) -> u32| (pts.iter().map(g).min(), pts.iter().map(g).max());
        let ((x0, x1), (y0, y1)) = (f(|p| p.0), f(|p| p.1));
        Some((x0?, x1?, y0?, y1?))
    }

    #[test]
    fn the_digits_keep_their_rows_when_an_alert_starts_below_24_px() {
        for size in [16, 20, 22] {
            let l = layout(size);
            let t = Theme { dark: false };
            for v in 0..=100 {
                let r = Reading {
                    value: Some(v as f32),
                    ..Reading::default()
                };
                let k = Kind::Percent { label: "CPU".into() };
                let from = (l.label_rows + 1) as u32;
                let rows = |st| {
                    let e = extent(&render(&k, &r, st, size, t), t.ink(), from).unwrap();
                    (e.2, e.3)
                };
                assert_eq!(rows(State::Normal), rows(State::Alert), "{v} at {size}");
            }
        }
    }

    #[test]
    fn from_24_px_the_digits_are_centred_in_the_fixed_tile() {
        for size in [24, 32] {
            let l = layout(size);
            let t = Theme { dark: false };
            let (plate, plate_ink) = t.alert();
            for v in [0.0, 7.0, 17.0, 33.0, 88.0, 100.0] {
                let r = Reading {
                    value: Some(v),
                    ..Reading::default()
                };
                let k = Kind::Percent { label: "CPU".into() };
                let a = render(&k, &r, State::Alert, size, t);
                let (_, _, y0, _) = extent(&a, plate, 0).unwrap();
                let tile = extent(&a, plate, y0).unwrap();
                let ink = extent(&a, plate_ink, l.digits_y as u32 - 1).unwrap();
                let (left, right) = (ink.0 - tile.0, tile.1 - ink.1);
                assert!(left.abs_diff(right) <= 1, "alert {v} at {size}: gaps {left} and {right}");
                // The same digits sit in the same place when there is no alert.
                let n = render(&k, &r, State::Normal, size, t);
                let ni = extent(&n, t.ink(), l.digits_y as u32 - 1).unwrap();
                assert_eq!((ni.0, ni.1), (ink.0, ink.1), "normal {v} at {size}");
            }
        }
    }

    #[test]
    fn a_network_item_draws_only_its_own_direction() {
        let at = |up: f64, down: f64| Reading {
            value: Some(1.0),
            up,
            down,
            ..Reading::default()
        };
        for size in [16, 20, 22, 24, 32] {
            let draw = |d: NetDir, r: &Reading| render(&Kind::Network(d), r, State::Normal, size, Theme { dark: true }).rgba;
            assert_eq!(
                draw(NetDir::Up, &at(5e3, 1.0)),
                draw(NetDir::Up, &at(5e3, 7e7)),
                "up ignores down at {size}"
            );
            assert_eq!(
                draw(NetDir::Down, &at(1.0, 5e3)),
                draw(NetDir::Down, &at(7e7, 5e3)),
                "down ignores up at {size}"
            );
            assert_ne!(
                draw(NetDir::Up, &at(5e3, 0.0)),
                draw(NetDir::Up, &at(7e7, 0.0)),
                "up shows up at {size}"
            );
            assert_ne!(
                draw(NetDir::Down, &at(0.0, 5e3)),
                draw(NetDir::Down, &at(0.0, 7e7)),
                "down shows down at {size}"
            );
            let both = |r: &Reading| draw(NetDir::Both, r);
            assert_ne!(both(&at(5e3, 5e3)), both(&at(7e7, 5e3)), "both follows up at {size}");
            assert_ne!(both(&at(5e3, 5e3)), both(&at(5e3, 7e7)), "both follows down at {size}");
            assert_ne!(both(&at(5e3, 5e3)), draw(NetDir::Up, &at(5e3, 5e3)), "both is not up at {size}");
        }
    }

    /// Every kind on Windows and Linux, with the reading each needs.
    fn every_kind() -> Vec<Kind> {
        vec![
            Kind::Percent { label: "CPU".into() },
            Kind::Meter { label: "CPU".into() },
            Kind::Bar { label: "CPU".into() },
            Kind::Graph { label: "CPU".into() },
            Kind::GraphValue,
            Kind::Ring { letter: 'C' },
            Kind::Temperature,
            Kind::Cores,
            Kind::Network(NetDir::Up),
            Kind::Network(NetDir::Down),
            Kind::Network(NetDir::Both),
        ]
    }

    fn reading(v: f32) -> Reading {
        Reading {
            value: Some(v),
            history: (0..64).map(|i| (v - 20.0 + (i % 7) as f32 * 6.0).clamp(0.0, 100.0)).collect(),
            cores: vec![v; 8],
            up: f64::from(v) * 1e4,
            down: f64::from(v) * 2e4,
        }
    }

    /// The share of the icon's pixels that differ by more than a faint step (40 of 255 in a channel); `undim` first takes the
    /// unavailable 45 % off `a`, so what is left is the shape, not the tone.
    fn differing(a: &Icon, b: &Icon, undim: bool) -> f32 {
        let n = a.rgba.len() / 4;
        let d = a
            .rgba
            .chunks(4)
            .zip(b.rgba.chunks(4))
            .filter(|(p, q)| {
                let pa = if undim {
                    (f32::from(p[3]) / 0.45).min(255.0)
                } else {
                    f32::from(p[3])
                };
                let (pa, qa) = (pa / 255.0, f32::from(q[3]) / 255.0);
                (0..3).any(|c| (f32::from(p[c]) * pa - f32::from(q[c]) * qa).abs() > 40.0) || (pa - qa).abs() * 255.0 > 40.0
            })
            .count();
        d as f32 / n as f32
    }

    /// The alert is a change of shape and colour on every kind that can alert (a load, a graph, the temperature), at every size
    /// and in both themes: at least 3 % of the icon differs from the same reading without it.
    #[test]
    fn every_kind_that_can_alert_shows_it_at_every_size_and_theme() {
        for kind in every_kind().into_iter().filter(|k| !matches!(k, Kind::Network(_) | Kind::Cores)) {
            for size in [16, 20, 22, 24, 32] {
                for dark in [true, false] {
                    let r = reading(93.0);
                    let normal = render(&kind, &r, State::Normal, size, Theme { dark });
                    let alert = render(&kind, &r, State::Alert, size, Theme { dark });
                    let share = differing(&alert, &normal, false);
                    assert!(
                        share >= 0.03,
                        "{kind:?} {size} px dark={dark}: alert differs in {:.1} % of the icon",
                        share * 100.0
                    );
                }
            }
        }
    }

    /// Unavailable is a shape (an en dash), not only a dimmer tone: with the 45 % taken off, at least 3 % of the icon still
    /// differs from a reading of 0, for every kind at every size and in both themes.
    #[test]
    fn unavailable_differs_from_zero_by_its_dash_not_only_its_tone() {
        for kind in every_kind() {
            for size in [16, 20, 22, 24, 32] {
                for dark in [true, false] {
                    let zero = render(&kind, &reading(0.0), State::Normal, size, Theme { dark });
                    let away = render(&kind, &reading(0.0), State::Unavailable, size, Theme { dark });
                    let share = differing(&away, &zero, true);
                    assert!(
                        share >= 0.03,
                        "{kind:?} {size} px dark={dark}: unavailable differs in {:.1} % of the icon",
                        share * 100.0
                    );
                }
            }
        }
    }

    /// The gauge's unavailable dash floats in its open quarter: from 22 px no dash pixel touches (also diagonally) a pixel of the
    /// arc's track.
    #[test]
    fn the_ring_dash_stays_clear_of_the_arc() {
        for size in [22, 24, 32] {
            for dark in [true, false] {
                let t = Theme { dark };
                let away = render(&Kind::Ring { letter: 'C' }, &Reading::default(), State::Unavailable, size, t);
                let s = size as u32;
                let ink = t.ink();
                let at = |x: i64, y: i64| -> Option<[u8; 4]> {
                    (x >= 0 && y >= 0 && x < i64::from(s) && y < i64::from(s)).then(|| px(&away, x as u32, y as u32))
                };
                // Dash pixels: the bottom rows' ink-coloured pixels (the letter sits in the middle).
                let dash: Vec<(i64, i64)> = (i64::from(s) * 3 / 4..i64::from(s))
                    .flat_map(|y| (0..i64::from(s)).map(move |x| (x, y)))
                    .filter(|(x, y)| at(*x, *y).is_some_and(|p| p[..3] == ink[..3] && p[3] >= 100))
                    .collect();
                assert!(!dash.is_empty(), "{size} px: a dash is drawn");
                for (x, y) in &dash {
                    for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                        let n = at(x + dx, y + dy);
                        let is_dash = dash.contains(&(x + dx, y + dy));
                        assert!(
                            is_dash || n.is_none_or(|p| p[3] == 0),
                            "{size} px dark={dark}: the dash at ({x}, {y}) touches the arc"
                        );
                    }
                }
            }
        }
    }
}
