//! The template layer of the resolver (SG3): zero-mean normalised cross-correlation (NCC) of a saved crop against the
//! frame, on a coarse level first and refined at full resolution. Raw scores are in -1..=1 (1 is identical up to brightness
//! and contrast); they only become decisions through [`locate`], which calibrates them and applies the margin rule.
//!
//! Scale is 1:1 by design (design review 1.1): the crop must come from the same pixel scale as the frame. A crop taken at
//! another scale will not match; take a new crop. Large regions use an integral-image coarse pass, not an FFT; the FFT
//! variant is left for when a profile shows the need.

mod ncc;

use sighter_core::{Calibrator, Candidate, Frame, Layer, RawScore, Rect, Resolution, ScoreError, Thresholds, decide};

pub use ncc::{Params, RawMatch};

/// Why a template could not be matched.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MatchError {
    /// The template is larger than the region it is searched in.
    #[error("the template ({tw}x{th}) is larger than the search area ({w}x{h})")]
    TemplateTooLarge {
        /// Template width.
        tw: u32,
        /// Template height.
        th: u32,
        /// Search width.
        w: u32,
        /// Search height.
        h: u32,
    },
    /// The template has no detail, so correlation is undefined.
    #[error("the template is a flat colour; crop something with detail (an icon with its edges)")]
    TemplateFlat,
    /// The search region lies outside the frame.
    #[error("the search region {0:?} is outside the frame")]
    RegionOutside(Rect),
    /// The search would exceed the work budget.
    #[error("the search needs about {needed} operations; the budget is {budget}: use a smaller region or template")]
    TooMuchWork {
        /// Estimated operations.
        needed: u64,
        /// The budget.
        budget: u64,
    },
    /// The score could not be calibrated (a wrong calibrator).
    #[error("calibration failed: {0}")]
    Calibration(#[from] ScoreError),
}

/// Finds `template` in `region` of `frame` (the whole frame when `None`); raw matches with NCC at least
/// `params.min_raw`, best first, in frame coordinates.
pub fn find(frame: &Frame, template: &Frame, region: Option<Rect>, params: &Params) -> Result<Vec<RawMatch>, MatchError> {
    let (area, origin) = match region {
        None => (None, Rect::new(0, 0, 0, 0)),
        Some(r) => {
            let cropped = frame.crop(r).map_err(|_| MatchError::RegionOutside(r))?;
            (Some(cropped), r.clamp_to(frame.width(), frame.height()).unwrap_or(r))
        }
    };
    let hay = area.as_ref().unwrap_or(frame);
    let mut hits = ncc::find(hay, template, params)?;
    for h in &mut hits {
        h.rect.x += origin.x;
        h.rect.y += origin.y;
    }
    Ok(hits)
}

/// Finds `template`, calibrates every hit with the template calibrator and applies the margin rule: two equal icons are
/// `Ambiguous`, a theme-inverted crop is `NotFound` rather than a low-score hit.
pub fn locate(
    frame: &Frame,
    template: &Frame,
    region: Option<Rect>,
    params: &Params,
    calibrator: &Calibrator,
    thresholds: &Thresholds,
) -> Result<Resolution, MatchError> {
    let mut candidates = Vec::new();
    for m in find(frame, template, region, params)? {
        let score = calibrator.calibrate(RawScore::new(Layer::Template, m.ncc)?)?;
        candidates.push(Candidate { rect: m.rect, score });
    }
    Ok(decide(&candidates, thresholds))
}

#[cfg(test)]
mod tests;
