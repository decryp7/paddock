//! The vocabulary of the grid: rectangles of cells, directions, gestures, items and why a gesture changes nothing.

use serde::{Deserialize, Serialize};

use super::steps::SizeRule;
use crate::id::PlacementId;

/// Most rows a grid surface has (ADR 0044 §1).
pub const MAX_ROWS: u16 = 512;

ts! {
/// A placement's rectangle in cells: `x + w <= columns`, `y + h <= MAX_ROWS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    /// Left column, 0-based.
    pub x: u8,
    /// Top row, 0-based.
    pub y: u16,
    /// Width in columns.
    pub w: u8,
    /// Height in rows.
    pub h: u8,
}

/// A direction on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dir {
    /// Towards column 0.
    Left,
    /// Away from column 0.
    Right,
    /// Towards row 0.
    Up,
    /// Away from row 0.
    Down,
}

/// One arrangement gesture on a grid surface: what a drop, a resize or a key asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "gesture")]
pub enum GridGesture {
    /// A drop or a pointer resize: the target rectangle, clamped to the kind's rule and the grid.
    Place {
        /// The placement.
        id: PlacementId,
        /// The target.
        rect: Rect,
    },
    /// Arrow keys: one cell, or past the neighbour that blocks.
    Nudge {
        /// The placement.
        id: PlacementId,
        /// The direction.
        dir: Dir,
    },
    /// Shift and arrow keys: Right wider, Left narrower, Down taller, Up shorter, by one step of the kind's rule.
    Grow {
        /// The placement.
        id: PlacementId,
        /// The direction.
        dir: Dir,
    },
    /// Home: first in reading order.
    ToFirst {
        /// The placement.
        id: PlacementId,
    },
    /// End: last in reading order.
    ToLast {
        /// The placement.
        id: PlacementId,
    },
}
}

/// A width and height in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Columns.
    pub w: u8,
    /// Rows.
    pub h: u8,
}

impl Rect {
    /// A rectangle.
    pub const fn new(x: u8, y: u16, w: u8, h: u8) -> Rect {
        Rect { x, y, w, h }
    }

    /// Its size.
    pub const fn size(&self) -> Size {
        Size {
            w: self.w,
            h: self.h,
        }
    }

    /// The column after its last one.
    pub fn right(&self) -> u16 {
        u16::from(self.x) + u16::from(self.w)
    }

    /// The row after its last one.
    pub fn bottom(&self) -> u32 {
        u32::from(self.y) + u32::from(self.h)
    }

    /// Whether the two share at least one cell.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.w > 0
            && self.h > 0
            && other.w > 0
            && other.h > 0
            && u16::from(self.x) < other.right()
            && u16::from(other.x) < self.right()
            && u32::from(self.y) < other.bottom()
            && u32::from(other.y) < self.bottom()
    }

    /// Whether the columns of the two intersect.
    pub(crate) fn shares_columns(&self, other: &Rect) -> bool {
        u16::from(self.x) < other.right() && u16::from(other.x) < self.right()
    }

    /// Whether the rectangle lies inside a grid of `columns` and [`MAX_ROWS`] and has cells.
    pub fn fits(&self, columns: u8) -> bool {
        self.w > 0
            && self.h > 0
            && self.right() <= u16::from(columns)
            && self.bottom() <= u32::from(MAX_ROWS)
    }

    /// The same rectangle `y` rows down, or `None` past the last row.
    pub(crate) fn at_row(self, y: u32) -> Option<Rect> {
        let y = u16::try_from(y).ok()?;
        let moved = Rect { y, ..self };
        (moved.bottom() <= u32::from(MAX_ROWS)).then_some(moved)
    }
}

/// One placement as the grid sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The placement's id.
    pub id: PlacementId,
    /// Where it is.
    pub rect: Rect,
    /// The sizes its kind allows.
    pub rule: SizeRule,
}

/// Why a gesture changes nothing; the UI says so instead of staying silent (ADR 0044 §5, announcements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// The placement is at the edge of the grid in this direction.
    Edge(Dir),
    /// The width or height is at the kind's largest size.
    Largest,
    /// The width or height is at the kind's smallest size.
    Smallest,
    /// The gesture leaves the arrangement as it is (already first, already last, dropped where it was).
    Unchanged,
}

/// Why a gesture could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GridError {
    /// No item has this id.
    #[error("No widget has this id.")]
    Unknown,
    /// The arrangement would need more rows than the grid has.
    #[error("There is no room below: the dashboard has at most {MAX_ROWS} rows.")]
    NoRoom,
    /// The arrangement given is not valid (outside the grid, overlapping, a size outside the rule, duplicate ids).
    #[error("The arrangement is not valid.")]
    Invalid,
    /// The gesture changes nothing.
    #[error("Nothing to change: {0:?}.")]
    Blocked(Block),
}
