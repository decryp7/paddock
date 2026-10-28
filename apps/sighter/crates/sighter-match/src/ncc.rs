//! The NCC engine: integral images for window mean and variance, a coarse scan, and a full-resolution refinement.

use crate::MatchError;
use sighter_core::{Frame, Rect};

/// Matching limits and the lowest raw score worth reporting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Lowest NCC reported.
    pub min_raw: f32,
    /// Most matches returned.
    pub max_hits: usize,
    /// Most multiply-adds one search may need (bounded work for a big region or template).
    pub work_budget: u64,
    /// Most raw positions collected per scan before it stops (the best survive suppression).
    pub max_raw_hits: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            min_raw: 0.5,
            max_hits: 16,
            work_budget: DEFAULT_WORK_BUDGET,
            max_raw_hits: DEFAULT_MAX_RAW_HITS,
        }
    }
}

/// About a second of single-core work.
pub const DEFAULT_WORK_BUDGET: u64 = 2_000_000_000;
/// Default for [`Params::max_raw_hits`].
pub const DEFAULT_MAX_RAW_HITS: usize = 65_536;

/// One match: where the template's top-left corner lands, and its NCC.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawMatch {
    /// The matched area, the template's size.
    pub rect: Rect,
    /// NCC in -1..=1.
    pub ncc: f32,
}

struct Gray {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

impl Gray {
    fn of(f: &Frame) -> Gray {
        Gray {
            w: f.width() as usize,
            h: f.height() as usize,
            px: f.gray(),
        }
    }

    /// Box-filter downscale by an integer factor.
    fn down(&self, s: usize) -> Gray {
        let (w, h) = (self.w / s, self.h / s);
        let mut px = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0;
                for dy in 0..s {
                    let row = (y * s + dy) * self.w + x * s;
                    acc += self.px[row..row + s].iter().sum::<f32>();
                }
                px[y * w + x] = acc / (s * s) as f32;
            }
        }
        Gray { w, h, px }
    }
}

/// Integral images of I and I^2 for O(1) window mean and variance.
struct Img<'a> {
    g: &'a Gray,
    w: usize,
    s: Vec<f64>,
    s2: Vec<f64>,
}

impl<'a> Img<'a> {
    fn of(g: &'a Gray) -> Img<'a> {
        let w = g.w + 1;
        let mut s = vec![0.0; w * (g.h + 1)];
        let mut s2 = s.clone();
        for y in 0..g.h {
            let (mut r, mut r2) = (0.0, 0.0);
            for x in 0..g.w {
                let v = f64::from(g.px[y * g.w + x]);
                r += v;
                r2 += v * v;
                s[(y + 1) * w + x + 1] = s[y * w + x + 1] + r;
                s2[(y + 1) * w + x + 1] = s2[y * w + x + 1] + r2;
            }
        }
        Img { g, w, s, s2 }
    }

    fn sums(&self, x: usize, y: usize, tw: usize, th: usize) -> (f64, f64) {
        let f = |v: &[f64]| v[(y + th) * self.w + x + tw] - v[(y + th) * self.w + x] - v[y * self.w + x + tw] + v[y * self.w + x];
        (f(&self.s), f(&self.s2))
    }
}

/// A zero-mean template ready for matching.
struct Tpl {
    z: Vec<f32>,
    w: usize,
    h: usize,
    norm: f64,
}

impl Tpl {
    fn of(t: &Gray) -> Result<Tpl, MatchError> {
        let mean = t.px.iter().sum::<f32>() / t.px.len() as f32;
        let z: Vec<f32> = t.px.iter().map(|v| v - mean).collect();
        let norm = z.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>().sqrt();
        if norm < 1e-3 * (t.px.len() as f64).sqrt() {
            return Err(MatchError::TemplateFlat);
        }
        Ok(Tpl { z, w: t.w, h: t.h, norm })
    }
}

/// NCC of the template with its top-left corner at (x, y); 0 on a flat window.
fn ncc_at(img: &Img, t: &Tpl, x: usize, y: usize) -> f32 {
    let n = (t.w * t.h) as f64;
    let (s, s2) = img.sums(x, y, t.w, t.h);
    let var = s2 - s * s / n;
    if var <= 1e-6 * n {
        return 0.0;
    }
    let mut num = 0.0f64;
    for ty in 0..t.h {
        let start = (y + ty) * img.g.w + x;
        let row = &img.g.px[start..start + t.w];
        let trow = &t.z[ty * t.w..(ty + 1) * t.w];
        num += f64::from(row.iter().zip(trow).map(|(a, b)| a * b).sum::<f32>());
    }
    (num / (var.sqrt() * t.norm)) as f32
}

type Hit = (f32, usize, usize);

fn scan(img: &Img, t: &Tpl, xs: std::ops::Range<usize>, ys: std::ops::Range<usize>, min: f32, cap: usize, out: &mut Vec<Hit>) {
    for y in ys {
        for x in xs.clone() {
            let s = ncc_at(img, t, x, y);
            if s >= min {
                if out.len() >= cap {
                    return;
                }
                out.push((s, x, y));
            }
        }
    }
}

/// Keeps the best of overlapping hits (corners closer than half the template), at most `max` of them.
fn suppress(mut hits: Vec<Hit>, tw: usize, th: usize, max: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
    let mut kept: Vec<Hit> = Vec::new();
    for h in hits {
        if kept.len() >= max {
            break;
        }
        if kept.iter().all(|k| k.1.abs_diff(h.1) * 2 >= tw || k.2.abs_diff(h.2) * 2 >= th) {
            kept.push(h);
        }
    }
    kept
}

pub(crate) fn find(frame: &Frame, template: &Frame, params: &Params) -> Result<Vec<RawMatch>, MatchError> {
    let (w, h) = (frame.width(), frame.height());
    let (tw, th) = (template.width(), template.height());
    if tw > w || th > h {
        return Err(MatchError::TemplateTooLarge { tw, th, w, h });
    }
    let img = Gray::of(frame);
    let tpl = Gray::of(template);
    let (tw, th) = (tpl.w, tpl.h);
    // Coarse level: shrink so the template keeps at least ~12 px on its short side.
    let s = [4usize, 2].into_iter().find(|s| tw.min(th) / s >= 12).unwrap_or(1);
    let full_positions = ((img.w - tw + 1) * (img.h - th + 1)) as u64;
    let area = (tw * th) as u64;
    let coarse_template = if s > 1 { Tpl::of(&tpl.down(s)).ok() } else { None };
    let needed = match coarse_template {
        // Coarse scan over s^2 fewer positions with an s^2 smaller template, then up to 16 refinements of (4s+1)^2 positions.
        Some(_) => full_positions / (s * s) as u64 * (area / (s * s) as u64) + 16 * (4 * s as u64 + 1).pow(2) * area,
        None => full_positions * area,
    };
    if needed > params.work_budget {
        return Err(MatchError::TooMuchWork {
            needed,
            budget: params.work_budget,
        });
    }
    let t = Tpl::of(&tpl)?;
    let full = Img::of(&img);
    let (maxx, maxy) = (img.w - tw, img.h - th);
    let mut hits = Vec::new();
    match coarse_template {
        None => scan(&full, &t, 0..maxx + 1, 0..maxy + 1, params.min_raw, params.max_raw_hits, &mut hits),
        Some(ct) => {
            let small = img.down(s);
            let ci = Img::of(&small);
            let mut coarse = Vec::new();
            scan(
                &ci,
                &ct,
                0..small.w - ct.w + 1,
                0..small.h - ct.h + 1,
                (params.min_raw - 0.25).max(0.3),
                params.max_raw_hits,
                &mut coarse,
            );
            let r = 2 * s;
            for (_, cx, cy) in suppress(coarse, ct.w, ct.h, 16) {
                let (cx, cy) = (cx * s, cy * s);
                scan(
                    &full,
                    &t,
                    cx.saturating_sub(r)..(cx + r).min(maxx) + 1,
                    cy.saturating_sub(r)..(cy + r).min(maxy) + 1,
                    params.min_raw,
                    params.max_raw_hits,
                    &mut hits,
                );
            }
        }
    }
    Ok(suppress(hits, tw, th, params.max_hits)
        .into_iter()
        .map(|(ncc, x, y)| RawMatch {
            rect: Rect::new(x as u32, y as u32, tw as u32, th as u32),
            ncc,
        })
        .collect())
}
