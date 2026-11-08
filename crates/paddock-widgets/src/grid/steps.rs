//! The sizes a kind allows (ADR 0044 §1): a range or a list of widths and heights.

use super::model::Size;

/// Allowed widths or heights, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steps {
    /// Every value from `min` to `max`, both included.
    Range {
        /// Smallest.
        min: u8,
        /// Largest.
        max: u8,
    },
    /// Only these values, in ascending order.
    Only(&'static [u8]),
}

/// Allowed widths and heights of a kind, and its default size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeRule {
    /// Allowed widths.
    pub widths: Steps,
    /// Allowed heights.
    pub heights: Steps,
    /// The size a new placement has; inside both.
    pub default: Size,
}

impl Steps {
    /// The smallest allowed value (1 for an empty rule, which [`Steps::is_valid`] refuses).
    pub fn min(&self) -> u8 {
        match self {
            Steps::Range { min, .. } => *min,
            Steps::Only(list) => list.first().copied().unwrap_or(1),
        }
    }

    /// The largest allowed value.
    pub fn max(&self) -> u8 {
        match self {
            Steps::Range { max, .. } => *max,
            Steps::Only(list) => list.last().copied().unwrap_or(1),
        }
    }

    /// Whether `v` is allowed.
    pub fn allows(&self, v: u8) -> bool {
        match self {
            Steps::Range { min, max } => (*min..=*max).contains(&v),
            Steps::Only(list) => list.contains(&v),
        }
    }

    /// Whether the rule is usable: not empty, ascending, from 1 up.
    pub fn is_valid(&self) -> bool {
        match self {
            Steps::Range { min, max } => *min >= 1 && min <= max,
            Steps::Only(list) => {
                !list.is_empty() && list[0] >= 1 && list.windows(2).all(|w| w[0] < w[1])
            }
        }
    }

    /// The allowed value nearest to `v`; the smaller of two equally near ones.
    pub fn nearest(&self, v: u8) -> u8 {
        match self {
            Steps::Range { min, max } => v.clamp(*min, *max.max(min)),
            Steps::Only(list) => list
                .iter()
                .copied()
                .min_by_key(|c| (c.abs_diff(v), *c))
                .unwrap_or(1),
        }
    }

    /// The next allowed value above `v`, if any.
    pub fn up(&self, v: u8) -> Option<u8> {
        match self {
            Steps::Range { min, max } => {
                let next = v.checked_add(1)?.max(*min);
                (next <= *max).then_some(next)
            }
            Steps::Only(list) => list.iter().copied().find(|c| *c > v),
        }
    }

    /// The next allowed value below `v`, if any.
    pub fn down(&self, v: u8) -> Option<u8> {
        match self {
            Steps::Range { min, max } => {
                let next = v.checked_sub(1)?.min(*max);
                (next >= *min).then_some(next)
            }
            Steps::Only(list) => list.iter().copied().rev().find(|c| *c < v),
        }
    }
}

impl SizeRule {
    /// A rule is usable when both step lists are and the default is inside both.
    pub fn is_valid(&self, columns: u8) -> bool {
        self.widths.is_valid()
            && self.heights.is_valid()
            && self.widths.max() <= columns
            && self.widths.allows(self.default.w)
            && self.heights.allows(self.default.h)
    }

    /// Whether `size` is allowed.
    pub fn allows(&self, size: Size) -> bool {
        self.widths.allows(size.w) && self.heights.allows(size.h)
    }
}
