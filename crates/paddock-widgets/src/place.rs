//! How a placement sits on its surface (ADR 0044 §1a): a span on a flow surface (popup, menu bar), a rectangle of
//! cells on a grid surface (the dashboard). The placement type follows from the surface, so a span on a grid surface
//! or a rectangle on the popup cannot be written.

use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::WidgetError;
use crate::grid::{Rect, SizeRule, Steps};

ts! {
/// The place of a placement on a flow surface: columns it spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    /// Columns it spans; one of its kind's spans.
    pub span: u8,
}

/// The place of a placement on a grid surface: a rectangle of cells (on the wire `x`, `y`, `w`, `h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cells {
    /// Where it is.
    #[serde(flatten)]
    #[cfg_attr(any(test, feature = "ts"), ts(flatten))]
    pub rect: Rect,
}

/// Allowed widths or heights of a kind, as the chooser and the UI need them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "steps")]
pub enum StepsInfo {
    /// Every value from `min` to `max`.
    Range {
        /// Smallest.
        min: u8,
        /// Largest.
        max: u8,
    },
    /// Only these values, ascending.
    Only {
        /// The values.
        values: Vec<u8>,
    },
}

/// How a kind may be sized, on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "arrangement")]
pub enum KindSize {
    /// Flow surface: allowed spans, the first is the default.
    Spans {
        /// The spans.
        spans: Vec<u8>,
    },
    /// Grid surface: allowed widths and heights and the default size.
    Cells {
        /// Allowed widths.
        widths: StepsInfo,
        /// Allowed heights.
        heights: StepsInfo,
        /// Default width.
        default_w: u8,
        /// Default height.
        default_h: u8,
    },
}
}

impl From<Steps> for StepsInfo {
    fn from(s: Steps) -> Self {
        match s {
            Steps::Range { min, max } => StepsInfo::Range { min, max },
            Steps::Only(v) => StepsInfo::Only { values: v.to_vec() },
        }
    }
}

/// How a placement sits on its surface; implemented by [`Span`] and [`Cells`] only.
pub trait Place: Clone + Debug + PartialEq + Serialize + DeserializeOwned + 'static {
    /// What a kind declares: the allowed spans, or a [`SizeRule`].
    type Rule: Copy + Debug + PartialEq + 'static;

    /// Whether the rule is usable on a surface of `columns` columns.
    fn rule_valid(rule: &Self::Rule, columns: u8) -> bool;

    /// Whether this place is allowed by `rule` on a surface of `columns` columns.
    fn check(&self, rule: &Self::Rule, columns: u8) -> Result<(), WidgetError>;

    /// A new placement's place by default (a grid surface places it afterwards).
    fn initial(rule: &Self::Rule) -> Option<Self>;

    /// The rule as wire data.
    fn size_info(rule: &Self::Rule) -> KindSize;

    /// The span, on a flow surface.
    fn span(&self) -> Option<u8> {
        None
    }

    /// The same place with another span, on a flow surface.
    fn with_span(&self, _span: u8) -> Option<Self> {
        None
    }

    /// The rectangle, on a grid surface.
    fn rect(&self) -> Option<Rect> {
        None
    }

    /// A place for a rectangle, on a grid surface.
    fn from_rect(_rect: Rect) -> Option<Self> {
        None
    }

    /// The size rule, on a grid surface.
    fn size_rule(_rule: &Self::Rule) -> Option<SizeRule> {
        None
    }
}

impl Place for Span {
    type Rule = &'static [u8];

    fn rule_valid(rule: &&'static [u8], columns: u8) -> bool {
        !rule.is_empty() && rule.iter().all(|&s| s >= 1 && s <= columns)
    }

    fn check(&self, rule: &&'static [u8], _columns: u8) -> Result<(), WidgetError> {
        if rule.contains(&self.span) {
            Ok(())
        } else {
            Err(WidgetError::SpanNotAllowed {
                allowed: rule.to_vec(),
            })
        }
    }

    fn initial(rule: &&'static [u8]) -> Option<Self> {
        rule.first().map(|&span| Span { span })
    }

    fn size_info(rule: &&'static [u8]) -> KindSize {
        KindSize::Spans {
            spans: rule.to_vec(),
        }
    }

    fn span(&self) -> Option<u8> {
        Some(self.span)
    }

    fn with_span(&self, span: u8) -> Option<Self> {
        Some(Span { span })
    }
}

impl Place for Cells {
    type Rule = SizeRule;

    fn rule_valid(rule: &SizeRule, columns: u8) -> bool {
        rule.is_valid(columns)
    }

    fn check(&self, rule: &SizeRule, columns: u8) -> Result<(), WidgetError> {
        let r = self.rect;
        if !r.fits(columns) {
            return Err(WidgetError::Invalid(
                "This widget is outside the grid.".into(),
            ));
        }
        if !rule.allows(r.size()) {
            return Err(WidgetError::SizeNotAllowed {
                widths: rule.widths.into(),
                heights: rule.heights.into(),
            });
        }
        Ok(())
    }

    fn initial(rule: &SizeRule) -> Option<Self> {
        Some(Cells {
            rect: Rect::new(0, 0, rule.default.w, rule.default.h),
        })
    }

    fn size_info(rule: &SizeRule) -> KindSize {
        KindSize::Cells {
            widths: rule.widths.into(),
            heights: rule.heights.into(),
            default_w: rule.default.w,
            default_h: rule.default.h,
        }
    }

    fn rect(&self) -> Option<Rect> {
        Some(self.rect)
    }

    fn from_rect(rect: Rect) -> Option<Self> {
        Some(Cells { rect })
    }

    fn size_rule(rule: &SizeRule) -> Option<SizeRule> {
        Some(*rule)
    }
}
