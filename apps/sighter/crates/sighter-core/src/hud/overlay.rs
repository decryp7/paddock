//! The overlay item model: every detection as a box with its confidence band, label text and "chosen" flag. Pure data;
//! drawing (borders on the live view and the hidden local overlay) waits for the UI phase, which maps bands to `--pd-*`
//! status tokens. A band is always text as well ([`Band::text`]), never colour alone.
//!
//! Masking applies: a box over a redacted region or a password field shows no OCR text, and OCR text elsewhere goes
//! through [`mask`].

use super::mask::{HudText, Secret, mask};
use crate::detect::{ClassId, DetectError, Detection, MAX_DETECTIONS, Score};
use crate::geometry::{Logical, Map, Rect};

/// Longest class name kept in a label.
pub const MAX_CLASS_NAME: usize = 32;

/// A confidence band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Band {
    /// At or above the high threshold.
    High,
    /// At or above the medium threshold.
    Medium,
    /// Below the medium threshold.
    Low,
}

impl Band {
    /// The band as words (shown next to the score; the colour is a rendering detail).
    pub fn text(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

/// Why overlay settings or input were refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OverlayError {
    /// The thresholds are not `0 < medium < high <= 1`.
    #[error("band thresholds need 0 < medium < high <= 1, got medium {medium} and high {high}; fix them in Settings")]
    Thresholds {
        /// The medium value given.
        medium: f64,
        /// The high value given.
        high: f64,
    },
    /// Too many detections or a bad score.
    #[error(transparent)]
    Detect(#[from] DetectError),
}

/// The confidence band thresholds. A Setting (rule 21): validated here, defaults below, never hard-coded elsewhere.
/// The defaults follow the resolver's `tau` of 0.8 for high; medium starts at 0.5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandThresholds {
    high: f64,
    medium: f64,
}

impl Default for BandThresholds {
    fn default() -> Self {
        Self { high: 0.8, medium: 0.5 }
    }
}

impl BandThresholds {
    /// Validates the two settings.
    pub fn new(medium: f64, high: f64) -> Result<Self, OverlayError> {
        if medium.is_nan() || high.is_nan() || medium <= 0.0 || high > 1.0 || medium >= high {
            return Err(OverlayError::Thresholds { medium, high });
        }
        Ok(Self { high, medium })
    }
    /// The band of a score.
    pub fn band(&self, score: Score) -> Band {
        match score.get() {
            s if s >= self.high => Band::High,
            s if s >= self.medium => Band::Medium,
            _ => Band::Low,
        }
    }
    /// The high threshold.
    pub fn high(&self) -> f64 {
        self.high
    }
    /// The medium threshold.
    pub fn medium(&self) -> f64 {
        self.medium
    }
}

/// How the score is written in a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreStyle {
    /// `0.92`.
    Decimal,
    /// `92%`.
    Percent,
}

/// Class names from the model card, by class id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassNames(Vec<String>);

impl ClassNames {
    /// Names by index; control characters are dropped and long names cut.
    pub fn new(names: &[&str]) -> Self {
        Self(
            names
                .iter()
                .map(|n| n.chars().filter(|c| !c.is_control()).take(MAX_CLASS_NAME).collect())
                .collect(),
        )
    }
    /// The name of a class, or `class N` for an id the model card does not know.
    pub fn name(&self, c: ClassId) -> String {
        self.0
            .get(c.0 as usize)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("class {}", c.0))
    }
}

/// The label text: `Button 0.92 high`, or `Button 92% high`, with `, target` for the chosen box. The band is always
/// spelled out.
pub fn label(name: &str, score: Score, band: Band, style: ScoreStyle, chosen: bool) -> String {
    let value = match style {
        ScoreStyle::Decimal => format!("{:.2}", score.get()),
        ScoreStyle::Percent => format!("{:.0}%", score.get() * 100.0),
    };
    let target = if chosen { ", target" } else { "" };
    format!("{name} {value} {}{target}", band.text())
}

/// One box to draw, in the output space `S` (capture pixels for the live view, host input for the local overlay).
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayItem<S> {
    /// The box in output coordinates.
    pub rect: Rect<S>,
    /// The class.
    pub class: ClassId,
    /// The raw score.
    pub score: Score,
    /// Its confidence band.
    pub band: Band,
    /// Whether this is the chosen target.
    pub chosen: bool,
    /// The full label, band included.
    pub label: String,
    /// OCR text to show, or `None` when there is none or the box is masked.
    pub text: Option<HudText>,
    /// True when the box lies over a redacted or password region, so no text is shown.
    pub masked: bool,
}

/// What the overlay needs besides the detections.
#[derive(Debug, Clone)]
pub struct OverlayContext<'a> {
    /// Band thresholds (Settings).
    pub thresholds: BandThresholds,
    /// Score style.
    pub style: ScoreStyle,
    /// Class names.
    pub names: &'a ClassNames,
    /// The class the detector uses for masked password fields, if the model has one.
    pub password_class: Option<ClassId>,
    /// Redaction regions in logical points (frames, video and HUD all hide what lies here).
    pub redactions: &'a [Rect<Logical>],
    /// Secrets of the run, for masking OCR text.
    pub secrets: &'a [Secret],
}

/// One detection to show with its optional OCR text.
#[derive(Debug, Clone)]
pub struct OverlayInput<'a> {
    /// The detection (logical points).
    pub detection: Detection,
    /// Whether it is the chosen target.
    pub chosen: bool,
    /// OCR text read inside the box, if any.
    pub ocr_text: Option<&'a str>,
}

/// Builds the overlay items for a frame: maps each box through `to_output`, assigns band and label, and applies masking.
/// Every item has a band and a non-empty label; none of the output depends on the order of the input beyond its own
/// position (the result keeps the input order).
pub fn build_items<S>(
    inputs: &[OverlayInput<'_>],
    to_output: &Map<Logical, S>,
    ctx: &OverlayContext<'_>,
) -> Result<Vec<OverlayItem<S>>, OverlayError> {
    if inputs.len() > MAX_DETECTIONS {
        return Err(DetectError::TooMany {
            got: inputs.len(),
            max: MAX_DETECTIONS,
        }
        .into());
    }
    Ok(inputs
        .iter()
        .map(|i| {
            let d = &i.detection;
            let band = ctx.thresholds.band(d.score);
            let masked = ctx.password_class == Some(d.class)
                || ctx
                    .redactions
                    .iter()
                    .any(|r| r.touches(&d.rect, 0.0) && r.intersection(&d.rect).is_some());
            let text = if masked { None } else { i.ocr_text.map(|t| mask(t, ctx.secrets)) };
            OverlayItem {
                rect: to_output.rect(d.rect),
                class: d.class,
                score: d.score,
                band,
                chosen: i.chosen,
                label: label(&ctx.names.name(d.class), d.score, band, ctx.style, i.chosen),
                text,
                masked,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::mask::SecretName;
    use super::*;
    use crate::geometry::{CapturePx, Scale};
    use proptest::prelude::*;

    fn s(v: f64) -> Score {
        Score::new(v).unwrap()
    }
    fn det(x: f64, class: u32, score: f64) -> Detection {
        Detection {
            rect: Rect::new(x, 10.0, 40.0, 20.0).unwrap(),
            class: ClassId(class),
            score: s(score),
            tile: 0,
        }
    }
    fn to_capture() -> Map<Logical, CapturePx> {
        Map::new(Scale::uniform(2.0).unwrap(), 5.0, 0.0).unwrap()
    }

    #[test]
    fn bands_follow_the_default_thresholds_and_the_settings() {
        let t = BandThresholds::default();
        assert_eq!(
            (
                t.band(s(0.92)),
                t.band(s(0.8)),
                t.band(s(0.6)),
                t.band(s(0.5)),
                t.band(s(0.49)),
                t.band(s(0.0))
            ),
            (Band::High, Band::High, Band::Medium, Band::Medium, Band::Low, Band::Low)
        );
        let custom = BandThresholds::new(0.3, 0.6).unwrap();
        assert_eq!(
            (custom.band(s(0.6)), custom.band(s(0.4)), custom.high(), custom.medium()),
            (Band::High, Band::Medium, 0.6, 0.3)
        );
        for (m, h) in [
            (0.8, 0.5),
            (0.5, 0.5),
            (0.0, 0.5),
            (-0.1, 0.5),
            (0.5, 1.1),
            (f64::NAN, 0.5),
            (0.2, f64::NAN),
        ] {
            assert!(matches!(BandThresholds::new(m, h), Err(OverlayError::Thresholds { .. })), "{m} {h}");
        }
        assert!(BandThresholds::new(0.5, 1.0).is_ok());
        assert_eq!(
            [Band::High.text(), Band::Medium.text(), Band::Low.text()],
            ["high", "medium", "low"]
        );
    }

    #[test]
    fn labels_carry_name_score_band_and_target() {
        assert_eq!(label("Button", s(0.92), Band::High, ScoreStyle::Decimal, false), "Button 0.92 high");
        assert_eq!(
            label("Button", s(0.92), Band::High, ScoreStyle::Percent, true),
            "Button 92% high, target"
        );
        assert_eq!(label("Tab", s(0.5), Band::Medium, ScoreStyle::Percent, false), "Tab 50% medium");
        let names = ClassNames::new(&["Button\u{7}", "", &"n".repeat(100)]);
        assert_eq!(names.name(ClassId(0)), "Button");
        assert_eq!(names.name(ClassId(1)), "class 1");
        assert_eq!(names.name(ClassId(2)).len(), MAX_CLASS_NAME);
        assert_eq!(names.name(ClassId(9)), "class 9");
    }

    #[test]
    fn items_are_mapped_banded_labelled_and_masked() {
        let names = ClassNames::new(&["Button", "Text input", "Password field"]);
        let secret = Secret::new(SecretName::new("PW").unwrap(), "hunter2".into()).unwrap();
        let redactions = [Rect::new(300.0, 0.0, 100.0, 100.0).unwrap()];
        let ctx = OverlayContext {
            thresholds: BandThresholds::default(),
            style: ScoreStyle::Decimal,
            names: &names,
            password_class: Some(ClassId(2)),
            redactions: &redactions,
            secrets: std::slice::from_ref(&secret),
        };
        let inputs = [
            OverlayInput {
                detection: det(0.0, 0, 0.92),
                chosen: true,
                ocr_text: Some("Save"),
            },
            OverlayInput {
                detection: det(100.0, 1, 0.55),
                chosen: false,
                ocr_text: Some("pass hunter2 ok"),
            },
            OverlayInput {
                detection: det(200.0, 2, 0.7),
                chosen: false,
                ocr_text: Some("•••••"),
            },
            OverlayInput {
                detection: det(310.0, 1, 0.3),
                chosen: false,
                ocr_text: Some("account 4111"),
            },
        ];
        let items = build_items(&inputs, &to_capture(), &ctx).unwrap();
        assert_eq!(items[0].label, "Button 0.92 high, target");
        assert_eq!(
            (items[0].rect.x(), items[0].rect.w(), items[0].text.clone()),
            (5.0, 80.0, Some(HudText::plain("Save")))
        );
        assert_eq!(items[1].band, Band::Medium);
        assert_eq!(
            items[1].text.as_ref().map(HudText::render).as_deref(),
            Some("pass •••• secret PW ok")
        );
        // Password-class box and a box over a redaction region show no OCR text at all.
        assert!(items[2].masked && items[2].text.is_none());
        assert!(items[3].masked && items[3].text.is_none() && items[3].band == Band::Low && items[3].label.ends_with("low"));
        assert!(!items[3].label.contains("4111") && !items[2].label.contains('•'));
        let many = vec![inputs[0].clone(); MAX_DETECTIONS + 1];
        assert!(matches!(
            build_items(&many, &to_capture(), &ctx),
            Err(OverlayError::Detect(DetectError::TooMany { .. }))
        ));
    }

    proptest! {
        /// Every item gets a band, and its label names the band in words; a masked item never carries text.
        #[test]
        fn every_item_has_a_band_label_and_safe_text(
            specs in prop::collection::vec((0.0f64..600.0, 0u32..5, 0.0f64..=1.0, any::<bool>(), "[a-z0-9 ]{0,12}"), 0..30),
            medium in 0.05f64..0.5, high in 0.55f64..=1.0, percent in any::<bool>(),
        ) {
            let names = ClassNames::new(&["A", "B", "C"]);
            let redactions = [Rect::new(100.0, 0.0, 120.0, 100.0).unwrap()];
            let ctx = OverlayContext {
                thresholds: BandThresholds::new(medium, high).unwrap(),
                style: if percent { ScoreStyle::Percent } else { ScoreStyle::Decimal },
                names: &names, password_class: Some(ClassId(1)), redactions: &redactions, secrets: &[],
            };
            let inputs: Vec<OverlayInput<'_>> = specs.iter().map(|(x, c, sc, ch, t)| OverlayInput { detection: det(*x, *c, *sc), chosen: *ch, ocr_text: Some(t.as_str()) }).collect();
            let items = build_items(&inputs, &to_capture(), &ctx).unwrap();
            prop_assert_eq!(items.len(), inputs.len());
            for it in &items {
                prop_assert!(!it.label.is_empty() && it.label.contains(it.band.text()));
                prop_assert_eq!(it.band, ctx.thresholds.band(it.score));
                prop_assert!(!it.masked || it.text.is_none());
                prop_assert_eq!(it.label.contains("target"), it.chosen);
            }
            // The label never carries OCR text, masked or not: it is class, score, band and target only.
            prop_assert!(items.iter().all(|it| it.label.split(' ').count() >= 3));
        }

        /// Bands are monotone in the score.
        #[test]
        fn bands_are_monotone(a in 0.0f64..=1.0, b in 0.0f64..=1.0, medium in 0.05f64..0.5, high in 0.55f64..=1.0) {
            let t = BandThresholds::new(medium, high).unwrap();
            let rank = |x: Band| match x { Band::Low => 0, Band::Medium => 1, Band::High => 2 };
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(rank(t.band(Score::new(lo).unwrap())) <= rank(t.band(Score::new(hi).unwrap())));
        }
    }
}
