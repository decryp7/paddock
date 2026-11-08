//! The grid arrangement of ADR 0044: placements as rectangles of cells, settled by gravity, swap and push; the keyboard
//! gestures; the narrower arrangements derived from the design one.
//!
//! Everything here is pure and works on [`Item`]s (an id, a rectangle and the kind's [`SizeRule`]); the layout types,
//! catalogue and edits sit on top of it. All arithmetic is checked: no input panics or overflows.

mod derive;
mod keys;
mod model;
mod settle;
mod steps;

pub use derive::derive;
pub(crate) use derive::first_free;
pub use keys::{Moves, moves, moves_all};
pub use model::{Block, Dir, GridError, GridGesture, Item, MAX_ROWS, Rect, Size};
pub use settle::{arrange, compact, reading_order};
pub use steps::{SizeRule, Steps};
