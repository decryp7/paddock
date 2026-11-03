//! The text port of the menu-bar items: the core lays out and composites, an adapter rasterises. On macOS the adapter is
//! CoreText drawing the system font into an alpha-only bitmap context (the app's edge and `examples/tray_sheets.rs`), so the
//! optical sizes and tracking are the system's own and no font file is read or bundled.

/// How a run of text is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Size in points.
    pub size: f32,
    /// Weight on AppKit's scale (`NSFontWeight`): 0 regular, 0.23 medium, 0.3 semibold, 0.4 bold.
    pub weight: f32,
    /// Tabular (equal-width) figures.
    pub tabular: bool,
    /// Extra space after each character, in points.
    pub tracking: f32,
}

impl TextStyle {
    /// Regular text at `size` points with tabular figures.
    pub const fn new(size: f32) -> TextStyle {
        TextStyle {
            size,
            weight: 0.0,
            tabular: true,
            tracking: 0.0,
        }
    }

    /// The same style at `weight`.
    pub const fn weight(mut self, weight: f32) -> TextStyle {
        self.weight = weight;
        self
    }

    /// The same style with `tracking` points after each character.
    pub const fn tracking(mut self, tracking: f32) -> TextStyle {
        self.tracking = tracking;
        self
    }

    /// The same style with proportional figures.
    pub const fn proportional(mut self) -> TextStyle {
        self.tabular = false;
        self
    }
}

/// Coverage of one run of text, in device pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMask {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Rows from the mask's top to the baseline.
    pub baseline: u32,
    /// Columns from the mask's left to the pen's start.
    pub origin: u32,
    /// Row-major coverage, 0 to 255.
    pub alpha: Vec<u8>,
}

/// Lays out and rasterises text (a port: CoreText on macOS, a box font in tests).
pub trait TextRaster {
    /// The advance of `text` in points.
    fn advance(&self, text: &str, style: &TextStyle) -> f32;
    /// The cap height in points.
    fn cap_height(&self, style: &TextStyle) -> f32;
    /// The coverage of `text` at `scale` pixels per point.
    fn mask(&self, text: &str, style: &TextStyle, scale: f32) -> TextMask;
}

/// A box font for tests: every character is a solid box, digits 0.6 em wide, other characters 0.7 em (0.45 em for "1"
/// without tabular figures); the cap height is 0.7 em.
#[derive(Debug, Default, Clone, Copy)]
pub struct BoxText;

impl BoxText {
    fn width(c: char, style: &TextStyle) -> f32 {
        let em = style.size;
        let w = match c {
            '1' if !style.tabular => 0.45,
            '0'..='9' => 0.6,
            ' ' => 0.3,
            _ => 0.7,
        };
        w * em + style.tracking
    }
}

impl TextRaster for BoxText {
    fn advance(&self, text: &str, style: &TextStyle) -> f32 {
        let n = text.chars().count();
        text.chars().map(|c| BoxText::width(c, style)).sum::<f32>() - if n > 0 { style.tracking } else { 0.0 }
    }

    fn cap_height(&self, style: &TextStyle) -> f32 {
        0.7 * style.size
    }

    fn mask(&self, text: &str, style: &TextStyle, scale: f32) -> TextMask {
        let w = (self.advance(text, style) * scale).ceil().max(0.0) as u32;
        let h = (self.cap_height(style) * scale).round().max(1.0) as u32;
        let mut alpha = vec![0u8; (w * h) as usize];
        let mut x = 0.0f32;
        for c in text.chars() {
            let cw = BoxText::width(c, style) - style.tracking;
            if c != ' ' {
                let (x0, x1) = ((x * scale).round() as u32, (((x + cw) * scale).round() as u32).min(w));
                for row in 0..h {
                    for col in x0..x1 {
                        alpha[(row * w + col) as usize] = 255;
                    }
                }
            }
            x += cw + style.tracking;
        }
        TextMask {
            width: w,
            height: h,
            baseline: h,
            origin: 0,
            alpha,
        }
    }
}
