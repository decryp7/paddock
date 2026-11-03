//! The proposed Windows notification-area and Linux appindicator items: square icons drawn for each exact pixel size
//! (16, 20, 24, 32 on Windows at 100 to 200 %; 22 on Linux), in Dyno's dash numerals. Straight strokes sit on whole pixels;
//! only arcs are anti-aliased.

use tiny_skia::{Color, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// A colour.
pub type Rgba = [u8; 4];

/// Taskbar or panel theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
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
    /// Tracks: the ink at 30 %.
    pub fn track(&self) -> Rgba {
        let mut c = self.ink();
        c[3] = 0x4d;
        c
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
    pub h: i32,
    pub w: i32,
    pub s: i32,
}

/// A 1-bit glyph.
pub struct Glyph {
    pub w: i32,
    pub h: i32,
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
    pub pm: Pixmap,
    pub size: i32,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], c[3]));
    p.anti_alias = true;
    p
}

impl Canvas {
    pub fn new(size: i32) -> Canvas {
        Canvas {
            pm: Pixmap::new(size as u32, size as u32).unwrap(),
            size,
        }
    }
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let r = tiny_skia::Rect::from_ltrb(x0 as f32, y0 as f32, x1 as f32, y1 as f32).unwrap();
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
    pub fn glyph(&mut self, g: &Glyph, x: i32, y: i32, c: Rgba) {
        for gy in 0..g.h {
            for gx in 0..g.w {
                if g.at(gx, gy) {
                    self.rect(x + gx, y + gy, x + gx + 1, y + gy + 1, c);
                }
            }
        }
    }
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
            self.pm.fill_path(&p, &paint(c), tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }
}

/// The layout of one size.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub size: i32,
    /// Label caps rows (0: no label at this size).
    pub label_rows: i32,
    pub label_y: i32,
    pub digits: Numerals,
    pub digits_y: i32,
    /// Gap between numerals.
    pub tracking: i32,
    pub meter_y: i32,
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
            digits_y: 1,
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
    let gs = glyphs(text, n, tracking, right);
    let w = gs.iter().map(|g| g.w).sum::<i32>() + tracking * (gs.len() as i32 - 1).max(0);
    let mut x = right - w;
    let left = x;
    for g in gs {
        cv.glyph(&g, x, y, c);
        x += g.w + tracking;
    }
    left
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
    Percent { label: String },
    Meter { label: String },
    Cores,
    Graph { label: String },
    Ring { letter: char },
    Network,
    Temperature,
}

/// What an item shows now (same meaning as on macOS).
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub value: Option<f32>,
    pub history: Vec<f32>,
    pub cores: Vec<f32>,
    pub up: f64,
    pub down: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Normal,
    Alert,
    Unavailable,
}

fn meter(cv: &mut Canvas, l: &Layout, v: Option<f32>, t: Theme, x0: i32, x1: i32, y: i32, h: i32) {
    // Square ends on whole pixels: at 2 to 4 px high a rounded end only notches the corners.
    cv.rect(x0, y, x1, y + h, t.track());
    if let Some(v) = v {
        let w = ((x1 - x0) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
        let w = if v > 0.5 { w.max(1) } else { w };
        cv.rect(x0, y, x0 + w, y + h, t.accent());
    }
    let _ = l;
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
    let t = if v < 9.95 && i > 0 { format!("{v:.1}") } else { format!("{}", v.round() as u64) };
    (t, units[i])
}

/// Renders one item at `size` pixels for a light or dark taskbar.
pub fn render(kind: &Kind, r: &Reading, state: State, size: i32, t: Theme) -> Pixmap {
    let l = layout(size);
    let mut cv = Canvas::new(l.size);
    let s = l.size;
    let value = if state == State::Unavailable { None } else { r.value };
    let ink = t.ink();
    let right = if l.size <= 16 { s } else { s - 1 };
    let (plate, plate_ink) = t.alert();
    // The number with its alert plate.
    let mut num = |cv: &mut Canvas, text: &str| {
        let n = l.digits;
        if state == State::Alert {
            let w = text_width(text, n, l.tracking);
            let (x0, x1) = ((right - w - 1).max(0), s);
            cv.rounded(x0, l.digits_y - 1, x1, l.digits_y + n.h + 1, plate);
            digits(cv, text, n, l.tracking, right, l.digits_y, plate_ink);
        } else {
            digits(cv, text, n, l.tracking, right, l.digits_y, ink);
        }
    };
    match kind {
        Kind::Percent { label: lb } => {
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            let text = value.map_or("-".to_string(), |v| format!("{}", v.clamp(0.0, 100.0).round() as u32));
            num(&mut cv, &text);
            meter(&mut cv, &l, value, t, 1, right, l.meter_y, l.meter_h);
        }
        Kind::Temperature => {
            if l.label_rows > 0 {
                label(&mut cv, "TMP", l.label_rows, right, l.label_y, ink);
            }
            if l.size <= 20 {
                // 16 and 20 px: two bold digits and a 3 px degree ring tucked into the top right (no gap would fit).
                let text = value.map_or("-".to_string(), |v| format!("{}", v.round().min(99.0) as u32));
                let c = if state == State::Alert {
                    cv.rect(0, l.digits_y - 1, s, l.digits_y + l.digits.h + 1, plate);
                    plate_ink
                } else {
                    ink
                };
                digits(&mut cv, &text, l.digits, l.tracking, s - 3, l.digits_y, c);
                if value.is_some() {
                    cv.glyph(&numeral('°', Numerals { h: 3, w: 3, s: 1 }), s - 3, l.digits_y, c);
                }
            } else {
                let text = match value {
                    None => "-".to_string(),
                    Some(v) if v >= 99.5 => format!("{}", v.round() as u32),
                    Some(v) => format!("{}°", v.round() as u32),
                };
                num(&mut cv, &text);
            }
            meter(&mut cv, &l, value.map(|v| (v - 20.0) / 0.9), t, 1, right, l.meter_y, l.meter_h);
        }
        Kind::Meter { label: lb } => {
            let mh = (s / 4).max(4);
            let my = if l.label_rows > 0 { l.label_y + l.label_rows + (s - l.label_rows - mh) / 2 } else { (s - mh) / 2 };
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            meter(&mut cv, &l, value, t, 1, right, my, mh);
            if state == State::Alert {
                cv.rounded(right - 3, my - 3, s, my, plate);
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
        }
        Kind::Graph { label: lb } => {
            let top = if l.label_rows > 0 { l.label_y + l.label_rows + 2 } else { 1 };
            if l.label_rows > 0 {
                label(&mut cv, lb, l.label_rows, right, l.label_y, ink);
            }
            let (x0, x1, bot) = (1, right, s - 1);
            cv.rect(x0, top, x1, bot, {
                let mut c = t.ink();
                c[3] = 0x18;
                c
            });
            cv.rect(x0, bot - 1, x1, bot, t.track());
            if state != State::Unavailable {
                let cols = (x1 - x0) as usize;
                let recent: Vec<f32> = r.history.iter().rev().take(cols).rev().copied().collect();
                let start = x1 - recent.len() as i32;
                for (i, v) in recent.iter().enumerate() {
                    let h = ((bot - top) as f32 * v.clamp(0.0, 100.0) / 100.0).round() as i32;
                    cv.rect(start + i as i32, bot - h, start + i as i32 + 1, bot, t.accent());
                }
            }
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
            if state == State::Alert {
                cv.rounded(gx - 2, gy - 2, gx + g.w + 2, gy + g.h + 2, plate);
                cv.glyph(&g, gx, gy, plate_ink);
            } else {
                cv.glyph(&g, gx, gy, ink);
            }
        }
        Kind::Network => {
            // 16 and 20: two log meters with arrows; 22 and up: two lines of small numerals with a unit letter.
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
                for (row, bps, up) in [(0, r.up, true), (1, r.down, false)] {
                    let y = if row == 0 { s / 4 - mh / 2 } else { s * 3 / 4 - mh / 2 };
                    let ah = (aw / 2.0).ceil();
                    arrow(&mut cv, 0.0, y as f32 + (mh as f32 - ah) / 2.0, aw, up);
                    let v = if state == State::Unavailable { None } else { Some(level(bps)) };
                    meter(&mut cv, &l, v, t, aw as i32 + 1, right, y, mh);
                }
            } else {
                let n = l.small;
                for (row, bps, up) in [(0, r.up, true), (1, r.down, false)] {
                    let y = if row == 0 { (s / 2 - n.h) / 2 } else { s / 2 + (s / 2 - n.h) / 2 };
                    let aw = (n.w + 1) as f32;
                    arrow(&mut cv, 0.0, y as f32 + (n.h as f32 - aw / 2.0) / 2.0, aw, up);
                    let (text, unit) = if state == State::Unavailable { ("-".to_string(), ' ') } else { short_rate(bps) };
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
    if state == State::Unavailable {
        crate::mac::dim(&mut cv.pm, 0.45);
    }
    cv.pm
}
