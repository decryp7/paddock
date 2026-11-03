//! Sheet composition: wallpapers, bars, system items drawn as vector stand-ins, captions, close-ups.

use crate::coretext::CoreText;
use dyno_core::tray::text::{TextRaster, TextStyle as Style};
use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, Rect,
    SpreadMode, Stroke, Transform,
};

/// Sheet text through the CoreText adapter.
pub struct Font {
    ct: CoreText,
}

/// A CSS-like weight (400, 600, 700) on AppKit's scale.
pub fn weight(w: f32) -> f32 {
    match w as u32 {
        0..=449 => 0.0,
        450..=549 => 0.23,
        550..=649 => 0.3,
        _ => 0.4,
    }
}

impl Font {
    pub fn system() -> Font {
        Font { ct: CoreText }
    }
    pub fn raster(&self) -> &dyn TextRaster {
        &self.ct
    }
    pub fn measure(&self, t: &str, s: &Style) -> f32 {
        self.ct.advance(t, s)
    }
    /// Draws `t` in `c` with its pen at (`x`, `y`) points, `scale` pixels per point; returns the advance in points.
    pub fn draw(&self, pm: &mut Pixmap, t: &str, s: &Style, x: f32, y: f32, scale: f32, c: Color) -> f32 {
        let m = self.ct.mask(t, s, scale);
        let ox = (x * scale).round() as i32 - m.origin as i32;
        let oy = (y * scale).round() as i32 - m.baseline as i32;
        let mut src = Pixmap::new(m.width.max(1), m.height.max(1)).unwrap();
        for (k, p) in src.pixels_mut().iter_mut().enumerate() {
            let a = f32::from(m.alpha[k]) / 255.0 * c.alpha();
            *p = tiny_skia::ColorU8::from_rgba(
                (c.red() * 255.0) as u8,
                (c.green() * 255.0) as u8,
                (c.blue() * 255.0) as u8,
                (a * 255.0).round() as u8,
            )
            .premultiply();
        }
        pm.draw_pixmap(ox, oy, src.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        self.measure(t, s)
    }
}

pub fn rgba(hex: u32, a: f32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, (a * 255.0).round() as u8)
}

pub fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

pub fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &solid(c), Transform::identity(), None);
    }
}

pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let mut pb = PathBuilder::new();
    let k = 0.5523 * r;
    let (x1, y1) = (x + w, y + h);
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - r + k, y, x1, y + r - k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + r - k, y1, x, y1 - r + k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

pub fn fill_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    pm.fill_path(
        &rrect_path(x, y, w, h, r),
        &solid(c),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A diagonal gradient wallpaper with a second soft glow.
pub fn wallpaper(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, a: u32, b: u32, glow: u32) {
    let shader = LinearGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x + w, y + h),
        vec![GradientStop::new(0.0, rgba(a, 1.0)), GradientStop::new(1.0, rgba(b, 1.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    let mut p = Paint::default();
    p.shader = shader;
    pm.fill_rect(Rect::from_xywh(x, y, w, h).unwrap(), &p, Transform::identity(), None);
    let g = tiny_skia::RadialGradient::new(
        Point::from_xy(x + w * 0.62, y + h * 0.1),
        Point::from_xy(x + w * 0.62, y + h * 0.1),
        w * 0.35,
        vec![GradientStop::new(0.0, rgba(glow, 0.55)), GradientStop::new(1.0, rgba(glow, 0.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    let mut p = Paint::default();
    p.shader = g;
    pm.fill_rect(Rect::from_xywh(x, y, w, h).unwrap(), &p, Transform::identity(), None);
}

/// Places a template image tinted with `c` (the menu bar's own tint for its mode).
pub fn template(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32, c: Color) {
    let mut t = src.clone();
    for p in t.pixels_mut() {
        let a = p.alpha() as f32 / 255.0 * c.alpha();
        *p = tiny_skia::ColorU8::from_rgba(
            (c.red() * 255.0) as u8,
            (c.green() * 255.0) as u8,
            (c.blue() * 255.0) as u8,
            (a * 255.0).round() as u8,
        )
        .premultiply();
    }
    dst.draw_pixmap(x, y, t.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

pub fn place(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    dst.draw_pixmap(x, y, src.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Places `src` enlarged `k` times with nearest-neighbour sampling, so every device pixel shows.
pub fn zoom(dst: &mut Pixmap, src: &Pixmap, x: f32, y: f32, k: f32) {
    let paint = PixmapPaint {
        quality: FilterQuality::Nearest,
        ..PixmapPaint::default()
    };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_row(k, 0.0, 0.0, k, x, y), None);
}

/// A pixel grid over a zoomed area (one line per source pixel), for close-ups.
pub fn grid(dst: &mut Pixmap, x: f32, y: f32, w: u32, h: u32, k: f32, c: Color) {
    for i in 0..=w {
        fill_rect(dst, x + i as f32 * k, y, 1.0, h as f32 * k, c);
    }
    for j in 0..=h {
        fill_rect(dst, x, y + j as f32 * k, w as f32 * k, 1.0, c);
    }
}

/// Area-average resampling to `w` x `h` (how an OS shrinks a larger icon).
pub fn resample(src: &Pixmap, w: u32, h: u32) -> Pixmap {
    let mut out = Pixmap::new(w, h).unwrap();
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let (fx, fy) = (sw / w as f32, sh / h as f32);
    let px = src.pixels();
    for oy in 0..h {
        for ox in 0..w {
            let (x0, x1) = (ox as f32 * fx, (ox + 1) as f32 * fx);
            let (y0, y1) = (oy as f32 * fy, (oy + 1) as f32 * fy);
            let mut acc = [0f32; 4];
            let mut tot = 0.0;
            let mut sy = y0.floor() as u32;
            while (sy as f32) < y1 && sy < src.height() {
                let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                let mut sx = x0.floor() as u32;
                while (sx as f32) < x1 && sx < src.width() {
                    let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let p = px[(sy * src.width() + sx) as usize];
                    let wgt = wx * wy;
                    acc[0] += p.red() as f32 * wgt;
                    acc[1] += p.green() as f32 * wgt;
                    acc[2] += p.blue() as f32 * wgt;
                    acc[3] += p.alpha() as f32 * wgt;
                    tot += wgt;
                    sx += 1;
                }
                sy += 1;
            }
            let c = |v: f32| (v / tot).round().clamp(0.0, 255.0) as u8;
            let a = c(acc[3]);
            let p = tiny_skia::PremultipliedColorU8::from_rgba(c(acc[0]).min(a), c(acc[1]).min(a), c(acc[2]).min(a), a).unwrap();
            out.pixels_mut()[(oy * w + ox) as usize] = p;
        }
    }
    out
}

/// Caption text in sheet pixels.
pub fn text(pm: &mut Pixmap, font: &Font, s: &str, size: f32, weight: f32, x: f32, y: f32, c: Color) -> f32 {
    font.draw(pm, s, &Style::new(size).weight(crate::sheet::weight(weight)), x, y, 1.0, c)
}

// ------------------------------------------------------------------------------------- macOS system items (stand-ins)

/// The system items right of Dyno's, drawn as simple vector stand-ins at `scale`: battery, Wi-Fi, Spotlight, Control
/// Centre and the clock. Returns their left edge. `cy` is the bar's centre in pixels.
pub fn mac_system(pm: &mut Pixmap, font: &Font, right: f32, cy: f32, scale: f32, c: Color, clock: &str) -> f32 {
    let s = scale;
    let p = solid(c);
    // Clock: 13 pt, same baseline rule as Dyno's one-line text (the image's 13.5 pt baseline is 16.5 pt from the bar's top).
    let cs = Style::new(13.0);
    let cw = font.measure(clock, &cs);
    let mut x = right - cw * s;
    let base = (cy / s - 12.0 + 16.5) * s;
    font.draw(pm, clock, &cs, x / s, ((base / s) * s).round() / s, s, c);
    x -= 16.0 * s;
    // Control Centre: two capsule toggles.
    let w = 15.0 * s;
    x -= w;
    let st = Stroke {
        width: 1.3 * s,
        ..Stroke::default()
    };
    for (i, on_left) in [(0, true), (1, false)] {
        let y = cy - 5.5 * s + i as f32 * 6.5 * s;
        pm.stroke_path(&rrect_path(x, y, w, 4.5 * s, 2.25 * s), &p, &st, Transform::identity(), None);
        let kx = if on_left { x + 2.25 * s } else { x + w - 2.25 * s };
        let mut pb = PathBuilder::new();
        pb.push_circle(kx, y + 2.25 * s, 1.6 * s);
        pm.fill_path(&pb.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
    }
    x -= 16.0 * s;
    // Spotlight: a magnifier.
    let r = 5.0 * s;
    x -= 14.0 * s;
    let mut pb = PathBuilder::new();
    pb.push_circle(x + r + 1.0 * s, cy - 1.5 * s, r);
    pm.stroke_path(
        &pb.finish().unwrap(),
        &p,
        &Stroke {
            width: 1.6 * s,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
    let mut pb = PathBuilder::new();
    pb.move_to(x + 2.0 * r - 0.5 * s, cy + 2.5 * s);
    pb.line_to(x + 13.5 * s, cy + 6.5 * s);
    pm.stroke_path(
        &pb.finish().unwrap(),
        &p,
        &Stroke {
            width: 2.0 * s,
            line_cap: LineCap::Round,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
    x -= 16.0 * s;
    // Wi-Fi: three arcs and a dot.
    let ww = 17.0 * s;
    x -= ww;
    let (wx, wy) = (x + ww / 2.0, cy + 5.5 * s);
    for k in 1..=3 {
        let rr = k as f32 * 3.7 * s;
        let mut pb = PathBuilder::new();
        for i in 0..=24 {
            let t = (225.0 + 90.0 * i as f32 / 24.0f32).to_radians();
            let (px, py) = (wx + rr * t.cos(), wy + rr * t.sin());
            if i == 0 {
                pb.move_to(px, py)
            } else {
                pb.line_to(px, py)
            }
        }
        pm.stroke_path(
            &pb.finish().unwrap(),
            &p,
            &Stroke {
                width: 1.9 * s,
                line_cap: LineCap::Round,
                ..Stroke::default()
            },
            Transform::identity(),
            None,
        );
    }
    let mut pb = PathBuilder::new();
    pb.push_circle(wx, wy - 0.4 * s, 1.4 * s);
    pm.fill_path(&pb.finish().unwrap(), &p, FillRule::Winding, Transform::identity(), None);
    x -= 16.0 * s;
    // Battery.
    let (bw, bh) = (25.0 * s, 12.0 * s);
    x -= bw + 2.5 * s;
    let by = cy - bh / 2.0;
    let mut dim = c;
    dim.set_alpha(c.alpha() * 0.45);
    pm.stroke_path(
        &rrect_path(x, by, bw, bh, 3.5 * s),
        &solid(dim),
        &Stroke {
            width: 1.0 * s,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
    pm.fill_path(
        &rrect_path(x + 2.0 * s, by + 2.0 * s, (bw - 4.0 * s) * 0.78, bh - 4.0 * s, 2.0 * s),
        &p,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    pm.fill_path(
        &rrect_path(x + bw + 1.0 * s, cy - 2.0 * s, 1.5 * s, 4.0 * s, 0.75 * s),
        &solid(dim),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    x
}

/// App menus at the bar's left (stand-in text).
pub fn mac_menus(pm: &mut Pixmap, font: &Font, x: f32, cy: f32, scale: f32, c: Color) {
    let base = (cy / scale - 12.0 + 16.5) * scale;
    let mut x = x;
    for (i, m) in ["Finder", "File", "Edit", "View", "Go", "Window", "Help"].iter().enumerate() {
        let st = Style::new(13.0).weight(if i == 0 { 0.4 } else { 0.0 }).proportional();
        let w = font.draw(pm, m, &st, x / scale, base / scale, scale, c);
        x += (w + 20.0) * scale;
    }
}
