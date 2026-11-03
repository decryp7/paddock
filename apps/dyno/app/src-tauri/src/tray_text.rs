//! The macOS adapter of `dyno_core::tray::text::TextRaster`: CoreText draws the system font into an alpha-only `CGBitmapContext`.
//! The font comes from `CTFontCreateUIFontForLanguage` (the system UI font; the system picks the optical size; no font file is
//! read), given the style's weight and, for tabular figures, the monospaced-numbers feature. Fonts and lines are CoreText
//! objects, immutable and thread-safe, so nothing here needs the main thread (AppKit's `NSFont` is only used to check these
//! fonts in the tests). Fonts are cached per style and lines per text and style: the items repeat the same few labels and numbers
//! every second. This is the app's edge (the port is `dyno_core::tray::text::TextRaster`); `examples/tray_sheets` includes this
//! file by path, so there is one implementation. The only unsafe code is the CoreText and CoreGraphics calls.
#![allow(unsafe_code)]

use dyno_core::tray::text::{TextMask, TextRaster, TextStyle};
use objc2_core_foundation::{CFArray, CFAttributedString, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo};
use objc2_core_text::{
    kCTFontAttributeName, kCTFontFeatureSelectorIdentifierKey, kCTFontFeatureSettingsAttribute, kCTFontFeatureTypeIdentifierKey,
    kCTFontTraitsAttribute, kCTFontWeightTrait, kCTForegroundColorFromContextAttributeName, kCTKernAttributeName, CTFont, CTFontDescriptor,
    CTFontUIFontType, CTLine,
};
use std::collections::HashMap;
use std::sync::Mutex;

/// A CoreText object shared between threads. CTFont and CTLine are immutable once made, which Apple documents as thread-safe.
struct Shared<T>(CFRetained<T>);

// Safety: CTFont and CTLine are immutable CoreText objects, safe to use from any thread (CoreText threading notes).
unsafe impl<T> Send for Shared<T> {}
// Safety: as above; only shared references are handed out.
unsafe impl<T> Sync for Shared<T> {}

/// Most cached lines before the cache starts over (a runaway label must not grow it without bound).
const MAX_LINES: usize = 512;

type FontKey = (u32, u32, bool);
type LineKey = (String, FontKey, u32);

/// CoreText over the system font, with caches.
#[derive(Default)]
pub struct CoreText {
    fonts: Mutex<HashMap<FontKey, Shared<CTFont>>>,
    lines: Mutex<HashMap<LineKey, Shared<CTLine>>>,
    /// Measurements only: when set nothing is cached.
    uncached: bool,
}

fn key(style: &TextStyle) -> FontKey {
    (style.size.to_bits(), style.weight.to_bits(), style.tabular)
}

/// The system UI font for `style`: its size, weight and, for tabular figures, monospaced numbers (type 6, selector 0).
fn make_font(style: &TextStyle) -> Option<CFRetained<CTFont>> {
    let base = unsafe { CTFont::new_ui_font_for_language(CTFontUIFontType::System, f64::from(style.size), None) }?;
    let weight = CFNumber::new_f64(f64::from(style.weight));
    let traits = CFDictionary::<CFString, CFType>::from_slices(&[unsafe { kCTFontWeightTrait }], &[weight.as_ref()]);
    let mut keys: Vec<&CFString> = vec![unsafe { kCTFontTraitsAttribute }];
    let mut values: Vec<&CFType> = vec![traits.as_ref()];
    let kind = CFNumber::new_i32(6);
    let selector = CFNumber::new_i32(0);
    let setting = CFDictionary::<CFString, CFType>::from_slices(
        &[unsafe { kCTFontFeatureTypeIdentifierKey }, unsafe {
            kCTFontFeatureSelectorIdentifierKey
        }],
        &[kind.as_ref(), selector.as_ref()],
    );
    let settings = CFArray::<CFType>::from_objects(&[setting.as_ref()]);
    if style.tabular {
        keys.push(unsafe { kCTFontFeatureSettingsAttribute });
        values.push(settings.as_ref());
    }
    let attributes = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    let descriptor = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
    Some(unsafe { base.copy_with_attributes(f64::from(style.size), std::ptr::null(), Some(&descriptor)) })
}

fn make_line(text: &str, font: &CTFont, style: &TextStyle) -> Option<CFRetained<CTLine>> {
    let kern = CFNumber::new_f64(f64::from(style.tracking));
    let yes: &CFBoolean = unsafe { objc2_core_foundation::kCFBooleanTrue }?;
    let keys: [&CFString; 3] = unsafe {
        [
            kCTFontAttributeName,
            kCTKernAttributeName,
            kCTForegroundColorFromContextAttributeName,
        ]
    };
    let values: [&CFType; 3] = [font.as_ref(), kern.as_ref(), yes.as_ref()];
    let dict = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    let s = CFString::from_str(text);
    let attr = unsafe { CFAttributedString::new(None, Some(&s), Some(dict.as_opaque())) }?;
    Some(unsafe { CTLine::with_attributed_string(&attr) })
}

impl CoreText {
    /// A text port with caches.
    pub fn new() -> CoreText {
        CoreText::default()
    }

    /// A text port that caches nothing (to measure what the caches save; the sheets example includes this file and does not use it).
    #[allow(dead_code)]
    pub fn uncached() -> CoreText {
        CoreText {
            uncached: true,
            ..CoreText::default()
        }
    }

    fn font(&self, style: &TextStyle) -> Option<Shared<CTFont>> {
        if self.uncached {
            return make_font(style).map(Shared);
        }
        let mut fonts = self.fonts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = fonts.get(&key(style)) {
            return Some(Shared(f.0.clone()));
        }
        let font = make_font(style)?;
        fonts.insert(key(style), Shared(font.clone()));
        Some(Shared(font))
    }

    /// The line for `text` in `style`, made once per text and style (the tracking is part of the style's identity here).
    fn line(&self, text: &str, style: &TextStyle) -> Option<Shared<CTLine>> {
        let k: LineKey = (text.to_string(), key(style), style.tracking.to_bits());
        if !self.uncached {
            if let Some(l) = self.lines.lock().unwrap_or_else(|e| e.into_inner()).get(&k) {
                return Some(Shared(l.0.clone()));
            }
        }
        let font = self.font(style)?;
        let line = make_line(text, &font.0, style)?;
        if !self.uncached {
            let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
            if lines.len() >= MAX_LINES {
                lines.clear();
            }
            lines.insert(k, Shared(line.clone()));
        }
        Some(Shared(line))
    }
}

/// (width, ascent, descent) of a line.
fn bounds(line: &CTLine) -> (f64, f64, f64) {
    let (mut ascent, mut descent, mut leading) = (0.0, 0.0, 0.0);
    let w = unsafe { line.typographic_bounds(&mut ascent, &mut descent, &mut leading) };
    (w, ascent, descent)
}

impl TextRaster for CoreText {
    fn advance(&self, text: &str, style: &TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(line) = self.line(text, style) else { return 0.0 };
        // CoreText adds the kern after the last character too; the port's advance ends at the last glyph.
        bounds(&line.0).0 as f32 - style.tracking
    }

    fn cap_height(&self, style: &TextStyle) -> f32 {
        self.font(style).map_or(0.0, |f| unsafe { f.0.cap_height() } as f32)
    }

    fn mask(&self, text: &str, style: &TextStyle, scale: f32) -> TextMask {
        let s = f64::from(scale);
        // One line, measured once, drawn once.
        let line = if text.is_empty() { None } else { self.line(text, style) };
        let (w, ascent, descent) = line.as_ref().map_or((0.0, 0.0, 0.0), |l| bounds(&l.0));
        let pad = 2u32;
        let width = ((w * s).ceil() as u32 + 2 * pad).max(1);
        let baseline = (ascent * s).ceil() as u32 + pad;
        let height = baseline + (descent * s).ceil() as u32 + pad;
        let mut alpha = vec![0u8; (width * height) as usize];
        if let Some(line) = &line {
            let ctx = unsafe {
                CGBitmapContextCreate(
                    alpha.as_mut_ptr().cast(),
                    width as usize,
                    height as usize,
                    8,
                    width as usize,
                    None,
                    CGImageAlphaInfo::Only.0,
                )
            };
            if let Some(ctx) = ctx {
                CGContext::set_should_smooth_fonts(Some(&ctx), false);
                CGContext::scale_ctm(Some(&ctx), s, s);
                CGContext::set_gray_fill_color(Some(&ctx), 0.0, 1.0);
                // Bitmap memory is top-down, drawing coordinates bottom-up: the baseline is (height - baseline) from the bottom.
                CGContext::set_text_position(Some(&ctx), f64::from(pad) / s, f64::from(height - baseline) / s);
                unsafe { line.0.draw(&ctx) };
            }
        }
        TextMask {
            width,
            height,
            baseline,
            origin: pad,
            alpha,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::tray::menu_bar::{LABEL, PAIR, STACK_LABEL, STACK_VALUE, VALUE};
    use objc2_app_kit::NSFont;

    /// The reference: what AppKit makes for the same style (the first version of this adapter used it).
    fn reference(style: &TextStyle) -> objc2::rc::Retained<NSFont> {
        let (size, weight) = (f64::from(style.size), f64::from(style.weight));
        if style.tabular {
            NSFont::monospacedDigitSystemFontOfSize_weight(size, weight)
        } else {
            NSFont::systemFontOfSize_weight(size, weight)
        }
    }

    #[test]
    fn the_core_text_fonts_measure_like_appkits_for_every_style_the_items_use() {
        let ct = CoreText::new();
        for style in [VALUE, LABEL, STACK_LABEL, STACK_VALUE, PAIR] {
            let nf = reference(&style);
            let cap = ct.cap_height(&style);
            assert!(
                (cap - nf.capHeight() as f32).abs() < 0.02,
                "cap height {cap} vs {} for {style:?}",
                nf.capHeight()
            );
            // The width of digits and of capitals: the tabular figures and the tracking give the same advances.
            for text in ["000", "88%", "CPU", "MEM"] {
                let ours = ct.advance(text, &style);
                let theirs = ct_reference_width(&nf, text, &style) as f32;
                assert!((ours - theirs).abs() < 0.05, "{text:?} {ours} vs {theirs} in {style:?}");
            }
        }
    }

    /// The width AppKit's font gives a line: through the same CoreText line code, with its font (NSFont is toll-free bridged).
    fn ct_reference_width(nf: &objc2::rc::Retained<NSFont>, text: &str, style: &TextStyle) -> f64 {
        // Safety: an NSFont is a CTFont (toll-free bridging); the reference lives as long as `nf`.
        let font: &CTFont = unsafe { &*objc2::rc::Retained::as_ptr(nf).cast::<CTFont>() };
        let line = make_line(text, font, style).unwrap();
        bounds(&line).0 - f64::from(style.tracking)
    }

    #[test]
    fn tabular_figures_are_equal_width_and_proportional_ones_are_not() {
        let ct = CoreText::new();
        let tab = VALUE;
        assert!((ct.advance("111", &tab) - ct.advance("888", &tab)).abs() < 0.01);
        let prop = tab.proportional();
        assert!(ct.advance("111", &prop) < ct.advance("888", &prop) - 1.0);
    }

    #[test]
    fn the_caches_change_nothing_but_the_cost() {
        let (cached, plain) = (CoreText::new(), CoreText::uncached());
        for text in ["23%", "CPU", "", "100°"] {
            for _ in 0..2 {
                assert_eq!(cached.mask(text, &VALUE, 2.0), plain.mask(text, &VALUE, 2.0), "{text:?}");
                assert_eq!(cached.advance(text, &VALUE), plain.advance(text, &VALUE));
            }
        }
    }

    /// ER-20 measurement, not a check: `cargo test -p dyno-app --lib measure_text -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement that prints the cost of rasterising 32 items' text; it asserts nothing about time"]
    fn measure_text() {
        for (name, ct) in [("uncached", CoreText::uncached()), ("cached", CoreText::new())] {
            let start = std::time::Instant::now();
            for tick in 0..100 {
                for item in 0..32 {
                    let v = format!("{}%", (tick + item) % 100);
                    ct.mask("CPU", &dyno_core::tray::menu_bar::STACK_LABEL, 2.0);
                    ct.mask(&v, &dyno_core::tray::menu_bar::STACK_VALUE, 2.0);
                    ct.advance("000", &dyno_core::tray::menu_bar::STACK_VALUE);
                }
            }
            println!("{name}: {:?} per tick for 32 stacked items (two masks each)", start.elapsed() / 100);
        }
    }
}
