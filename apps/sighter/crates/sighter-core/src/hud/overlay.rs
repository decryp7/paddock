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
    /// Whole hundredths (1 to 100), so a threshold can be written exactly in a label.
    high: u32,
    medium: u32,
}

impl Default for BandThresholds {
    fn default() -> Self {
        Self { high: 80, medium: 50 }
    }
}

/// `x` as whole hundredths, if it is one (within float noise).
fn as_hundredths(x: f64) -> Option<u32> {
    let h = (x * 100.0).round();
    ((x * 100.0 - h).abs() < 1e-9 && (0.0..=100.0).contains(&h)).then_some(h as u32)
}

impl BandThresholds {
    /// Validates the two settings: `0 < medium < high <= 1`, each a whole number of hundredths (0.5, 0.8, 0.85; not 0.855),
    /// so the label (which shows hundredths) and the band can never disagree.
    pub fn new(medium: f64, high: f64) -> Result<Self, OverlayError> {
        match (as_hundredths(medium), as_hundredths(high)) {
            (Some(m), Some(h)) if m >= 1 && m < h => Ok(Self { high: h, medium: m }),
            _ => Err(OverlayError::Thresholds { medium, high }),
        }
    }
    /// The band of a score, taken on the score rounded **down** to hundredths (what [`label`] shows). Because thresholds
    /// are whole hundredths this equals comparing the raw score with the threshold (up to 1e-9 of float noise, so 0.8 is
    /// high and 0.29 is 0.29), and a label can never read `0.80` for a score below 0.8 or `0.50` for one below 0.5.
    pub fn band(&self, score: Score) -> Band {
        match hundredths(score) {
            h if h >= self.high => Band::High,
            h if h >= self.medium => Band::Medium,
            _ => Band::Low,
        }
    }
    /// The high threshold.
    pub fn high(&self) -> f64 {
        f64::from(self.high) / 100.0
    }
    /// The medium threshold.
    pub fn medium(&self) -> f64 {
        f64::from(self.medium) / 100.0
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

/// The score rounded **down** to whole hundredths (0 to 100): the one value a label shows, in either style. Rounding down
/// (with 1e-9 for float noise, so 0.29 is 29) means a displayed value never exceeds the score: 0.7999 shows `0.79`.
fn hundredths(score: Score) -> u32 {
    (score.get() * 100.0 + 1e-9).floor() as u32
}

/// The label text: `Button 0.92 high`, or `Button 92% high`, with `, target` for the chosen box. The band is always
/// spelled out. Both styles show the score rounded down to hundredths, formatted from integers (no float formatting ties).
pub fn label(name: &str, score: Score, band: Band, style: ScoreStyle, chosen: bool) -> String {
    let h = hundredths(score);
    let value = match style {
        ScoreStyle::Decimal => format!("{}.{:02}", h / 100, h % 100),
        ScoreStyle::Percent => format!("{h}%"),
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

    /// Regression: high is the resolver's acting threshold (0.8), so 0.7999 must never read "high" or "0.80", and a
    /// label must never show a value above the score.
    #[test]
    fn the_band_follows_the_raw_score_and_the_label_rounds_down() {
        let t = BandThresholds::default();
        for (raw, band, shown) in [
            (0.7999, Band::Medium, "0.79"),
            (0.795, Band::Medium, "0.79"),
            (0.8, Band::High, "0.80"),
            (0.29, Band::Low, "0.29"),
            (0.4999, Band::Low, "0.49"),
            (0.5, Band::Medium, "0.50"),
            (1.0, Band::High, "1.00"),
            (0.0, Band::Low, "0.00"),
        ] {
            let sc = s(raw);
            assert_eq!(t.band(sc), band, "{raw}");
            let text = label("X", sc, t.band(sc), ScoreStyle::Decimal, false);
            assert_eq!(text, format!("X {shown} {}", band.text()), "{raw}");
        }
        assert_eq!(label("X", s(0.7999), Band::Medium, ScoreStyle::Percent, false), "X 79% medium");
        assert_eq!(label("X", s(0.125), Band::Low, ScoreStyle::Decimal, false), "X 0.12 low");
    }

    #[test]
    fn thresholds_must_be_whole_hundredths() {
        assert!(BandThresholds::new(0.5, 0.85).is_ok());
        for (m, h) in [(0.5, 0.855), (0.505, 0.8), (0.0, 0.8), (0.01, 1.0)] {
            assert_eq!(BandThresholds::new(m, h).is_ok(), m == 0.01, "{m} {h}");
        }
        assert!(matches!(BandThresholds::new(0.5, 1.01), Err(OverlayError::Thresholds { .. })));
        // With whole-hundredth thresholds, band and displayed value agree for every score on the 0.01 grid and just below.
        let t = BandThresholds::new(0.35, 0.8).unwrap();
        for i in 0..=1000u32 {
            let sc = s(f64::from(i) / 1000.0);
            let shown = hundredths(sc);
            let by_raw = if sc.get() + 1e-9 >= 0.8 {
                Band::High
            } else if sc.get() + 1e-9 >= 0.35 {
                Band::Medium
            } else {
                Band::Low
            };
            assert_eq!(t.band(sc), by_raw, "{i}");
            assert!(shown <= i / 10);
        }
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
            medium_h in 5u32..50, high_h in 55u32..=100, percent in any::<bool>(),
        ) {
            let names = ClassNames::new(&["A", "B", "C"]);
            let redactions = [Rect::new(100.0, 0.0, 120.0, 100.0).unwrap()];
            let ctx = OverlayContext {
                thresholds: BandThresholds::new(f64::from(medium_h) / 100.0, f64::from(high_h) / 100.0).unwrap(),
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
            // The label is exactly class, score, band and target: it never carries OCR text, masked or not.
            for (it, i) in items.iter().zip(&inputs) {
                let expected = label(&names.name(i.detection.class), i.detection.score, it.band, ctx.style, i.chosen);
                prop_assert_eq!(&it.label, &expected);
            }
        }

        /// Bands are monotone in the score.
        #[test]
        fn bands_are_monotone(a in 0.0f64..=1.0, b in 0.0f64..=1.0, medium_h in 5u32..50, high_h in 55u32..=100) {
            let t = BandThresholds::new(f64::from(medium_h) / 100.0, f64::from(high_h) / 100.0).unwrap();
            let rank = |x: Band| match x { Band::Low => 0, Band::Medium => 1, Band::High => 2 };
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(rank(t.band(Score::new(lo).unwrap())) <= rank(t.band(Score::new(hi).unwrap())));
        }
    }
}
