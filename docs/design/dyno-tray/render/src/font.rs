//! Text through the OS's own font file: shaped with rustybuzz (tabular figures, the variation axes) and filled as vector
//! outlines with tiny-skia. Nothing is bundled and nothing is fetched: on macOS the system font is
//! `/System/Library/Fonts/SFNS.ttf` (a variable font: wght, wdth, opsz, GRAD), which is the font `NSFont.menuBarFont`
//! resolves to (measured 2026-11-03: 13 pt, opsz 17, wght 400).

use rustybuzz::{Face, Feature, UnicodeBuffer, Variation};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use ttf_parser::Tag;

/// A loaded font file.
pub struct Font {
    data: Vec<u8>,
}

/// How a run of text is set.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Size in points.
    pub size: f32,
    /// The wght axis (400 regular, 510 medium, 590 semibold, 700 bold).
    pub weight: f32,
    /// The wdth axis (100 normal; lower is condensed).
    pub width: f32,
    /// Tabular (equal-width) figures.
    pub tnum: bool,
    /// Extra space after each glyph, in points.
    pub tracking: f32,
}

impl Style {
    /// Regular text at `size` with tabular figures.
    pub fn new(size: f32) -> Style {
        Style {
            size,
            weight: 400.0,
            width: 100.0,
            tnum: true,
            tracking: 0.0,
        }
    }
    pub fn weight(mut self, w: f32) -> Style {
        self.weight = w;
        self
    }
    pub fn width(mut self, w: f32) -> Style {
        self.width = w;
        self
    }
    pub fn tnum(mut self, on: bool) -> Style {
        self.tnum = on;
        self
    }
    pub fn tracking(mut self, t: f32) -> Style {
        self.tracking = t;
        self
    }
}

struct Outline<'a> {
    pb: &'a mut PathBuilder,
    k: f32,
    x: f32,
    y: f32,
}

impl OutlinePen for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x * self.k, self.y - y * self.k);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x * self.k, self.y - y * self.k);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(self.x + x1 * self.k, self.y - y1 * self.k, self.x + x * self.k, self.y - y * self.k);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + x1 * self.k,
            self.y - y1 * self.k,
            self.x + x2 * self.k,
            self.y - y2 * self.k,
            self.x + x * self.k,
            self.y - y * self.k,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Font {
    /// The macOS system font, read from the OS at run time.
    pub fn system() -> Font {
        let data = std::fs::read("/System/Library/Fonts/SFNS.ttf").expect("the macOS system font (SFNS.ttf)");
        Font { data }
    }

    fn face(&self, s: &Style) -> Face<'_> {
        let mut face = Face::from_slice(&self.data, 0).expect("a font face");
        // opsz follows the size in points, clamped to the font's 17 to 96 (the menu bar itself uses 17 at 13 pt).
        face.set_variations(&[
            Variation {
                tag: Tag::from_bytes(b"wght"),
                value: s.weight,
            },
            Variation {
                tag: Tag::from_bytes(b"wdth"),
                value: s.width,
            },
            Variation {
                tag: Tag::from_bytes(b"opsz"),
                value: s.size.clamp(17.0, 96.0),
            },
        ]);
        face
    }

    fn shape(&self, text: &str, s: &Style) -> (Face<'_>, Vec<(u16, f32)>) {
        let face = self.face(s);
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        let features: Vec<Feature> = if s.tnum {
            vec![Feature::new(Tag::from_bytes(b"tnum"), 1, ..)]
        } else {
            vec![]
        };
        let out = rustybuzz::shape(&face, &features, buf);
        let k = s.size / face.units_per_em() as f32;
        let glyphs = out
            .glyph_infos()
            .iter()
            .zip(out.glyph_positions())
            .map(|(i, p)| (i.glyph_id as u16, p.x_advance as f32 * k + s.tracking))
            .collect();
        (face, glyphs)
    }

    /// The advance of `text` in points.
    pub fn measure(&self, text: &str, s: &Style) -> f32 {
        let (_, g) = self.shape(text, s);
        let w: f32 = g.iter().map(|(_, a)| a).sum();
        w - if g.is_empty() { 0.0 } else { s.tracking }
    }

    /// Cap height in points.
    pub fn cap_height(&self, s: &Style) -> f32 {
        let face = self.face(s);
        face.capital_height().unwrap_or(1400) as f32 * s.size / face.units_per_em() as f32
    }

    /// Fills `text` with its baseline at (`x`, `y`) points, at `scale` pixels per point. Returns the advance in points.
    pub fn draw(&self, pm: &mut Pixmap, text: &str, s: &Style, x: f32, y: f32, scale: f32, paint: &Paint) -> f32 {
        let (_face, glyphs) = self.shape(text, s);
        let font = FontRef::new(&self.data).expect("a font");
        let loc = font.axes().location([("wght", s.weight), ("wdth", s.width), ("opsz", s.size.clamp(17.0, 96.0))]);
        let outlines = font.outline_glyphs();
        let mut pb = PathBuilder::new();
        let mut pen = x * scale;
        for (gid, adv) in &glyphs {
            let mut o = Outline {
                pb: &mut pb,
                k: 1.0,
                x: pen,
                y: y * scale,
            };
            if let Some(g) = outlines.get(skrifa::GlyphId::new(u32::from(*gid))) {
                let _ = g.draw(DrawSettings::unhinted(Size::new(s.size * scale), LocationRef::from(&loc)), &mut o);
            }
            pen += adv * scale;
        }
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
        }
        pen / scale - x
    }
}
