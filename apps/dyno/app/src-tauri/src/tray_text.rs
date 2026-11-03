//! The macOS adapter of `dyno_core::tray::text::TextRaster`: CoreText draws the system font (AppKit's
//! `systemFontOfSize:weight:` or, for tabular figures, `monospacedDigitSystemFontOfSize:weight:`) into an alpha-only
//! `CGBitmapContext`. The system picks the optical size; no font file is read. This is the app's edge (the port is
//! `dyno_core::tray::text::TextRaster`); `examples/tray_sheets` includes this file by path, so there is one implementation.
//! The only unsafe code is the CoreText and CoreGraphics calls, plain message sends on objects made here.
#![allow(unsafe_code)]

use dyno_core::tray::text::{TextMask, TextRaster, TextStyle};
use objc2::rc::Retained;
use objc2_app_kit::NSFont;
use objc2_core_foundation::{CFAttributedString, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo};
use objc2_core_text::{kCTFontAttributeName, kCTForegroundColorFromContextAttributeName, kCTKernAttributeName, CTFont, CTLine};

/// CoreText over the system font.
pub struct CoreText;

fn font(style: &TextStyle) -> Retained<NSFont> {
    let (size, weight) = (f64::from(style.size), f64::from(style.weight));
    if style.tabular {
        NSFont::monospacedDigitSystemFontOfSize_weight(size, weight)
    } else {
        NSFont::systemFontOfSize_weight(size, weight)
    }
}

/// The line, or `None` when CoreText refuses (the text then measures and draws as nothing, and the item shows no label).
fn line(text: &str, style: &TextStyle) -> Option<CFRetained<CTLine>> {
    let f = font(style);
    // NSFont is toll-free bridged to CTFont.
    let ct: &CTFont = unsafe { &*(Retained::as_ptr(&f).cast::<CTFont>()) };
    let kern = CFNumber::new_f64(f64::from(style.tracking));
    let yes: &CFBoolean = unsafe { objc2_core_foundation::kCFBooleanTrue }?;
    let keys: [&CFString; 3] = unsafe {
        [
            kCTFontAttributeName,
            kCTKernAttributeName,
            kCTForegroundColorFromContextAttributeName,
        ]
    };
    let values: [&CFType; 3] = [ct.as_ref(), kern.as_ref(), yes.as_ref()];
    let dict = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
    let s = CFString::from_str(text);
    let attr = unsafe { CFAttributedString::new(None, Some(&s), Some(dict.as_opaque())) }?;
    Some(unsafe { CTLine::with_attributed_string(&attr) })
}

fn bounds(text: &str, style: &TextStyle) -> (f64, f64, f64) {
    let Some(l) = line(text, style) else { return (0.0, 0.0, 0.0) };
    let (mut ascent, mut descent, mut leading) = (0.0, 0.0, 0.0);
    let w = unsafe { l.typographic_bounds(&mut ascent, &mut descent, &mut leading) };
    (w, ascent, descent)
}

impl TextRaster for CoreText {
    fn advance(&self, text: &str, style: &TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        // CoreText adds the kern after the last character too; the port's advance ends at the last glyph.
        bounds(text, style).0 as f32 - style.tracking
    }

    fn cap_height(&self, style: &TextStyle) -> f32 {
        font(style).capHeight() as f32
    }

    fn mask(&self, text: &str, style: &TextStyle, scale: f32) -> TextMask {
        let s = f64::from(scale);
        let (w, ascent, descent) = bounds(text, style);
        let pad = 2u32;
        let width = ((w * s).ceil() as u32 + 2 * pad).max(1);
        let baseline = (ascent * s).ceil() as u32 + pad;
        let height = baseline + (descent * s).ceil() as u32 + pad;
        let mut alpha = vec![0u8; (width * height) as usize];
        if !text.is_empty() {
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
            if let (Some(ctx), Some(l)) = (ctx, line(text, style)) {
                CGContext::set_should_smooth_fonts(Some(&ctx), false);
                CGContext::scale_ctm(Some(&ctx), s, s);
                CGContext::set_gray_fill_color(Some(&ctx), 0.0, 1.0);
                // Bitmap memory is top-down, drawing coordinates bottom-up: the baseline is (height - baseline) from the bottom.
                CGContext::set_text_position(Some(&ctx), f64::from(pad) / s, f64::from(height - baseline) / s);
                unsafe { l.draw(&ctx) };
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
