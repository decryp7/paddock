//! The keyboard gestures of ADR 0044 §5 as targets for the settle step, and which of them would change anything.

use serde::{Deserialize, Serialize};

use super::model::{Block, Dir, GridError, GridGesture, Item, Rect};
use super::settle::arrange;

/// Where a nudge sends the placement.
pub(crate) enum Move {
    /// To this rectangle, pushing what is there.
    To(Rect),
    /// Exchange places with the item at this index (same size).
    Exchange(usize),
}

/// One arrow key: one cell if free; the same-sized neighbour that blocks is swapped with; otherwise just past it.
/// Down always passes the next widget below (gravity leaves no free cell below).
pub(crate) fn nudge(items: &[Item], i: usize, dir: Dir, columns: u8) -> Result<Move, Block> {
    let r = items[i].rect;
    let others = || items.iter().enumerate().filter(move |(j, _)| *j != i);
    if dir == Dir::Down {
        let below = others()
            .filter(|(_, o)| o.rect.shares_columns(&r) && u32::from(o.rect.y) >= r.bottom())
            .min_by_key(|(_, o)| (o.rect.y, o.rect.x))
            .ok_or(Block::Edge(Dir::Down))?;
        if below.1.rect.size() == r.size() {
            return Ok(Move::Exchange(below.0));
        }
        let y = below.1.rect.bottom();
        return r.at_row(y).map(Move::To).ok_or(Block::Edge(Dir::Down));
    }
    let cell = match dir {
        Dir::Left => r.x.checked_sub(1).map(|x| Rect { x, ..r }),
        Dir::Right => (r.right() < u16::from(columns)).then(|| Rect { x: r.x + 1, ..r }),
        // Down returned above.
        _ => r.y.checked_sub(1).map(|y| Rect { y, ..r }),
    }
    .ok_or(Block::Edge(dir))?;
    let blockers: Vec<(usize, &Item)> = others().filter(|(_, o)| o.rect.overlaps(&cell)).collect();
    let Some(first) = blockers.first() else {
        return Ok(Move::To(cell));
    };
    if blockers.len() == 1 && first.1.rect.size() == r.size() {
        return Ok(Move::Exchange(first.0));
    }
    let past = match dir {
        Dir::Left => {
            let edge = blockers.iter().map(|(_, o)| o.rect.x).min().unwrap_or(0);
            Rect {
                x: edge.saturating_sub(r.w),
                ..r
            }
        }
        Dir::Right => {
            let edge = blockers
                .iter()
                .map(|(_, o)| o.rect.right())
                .max()
                .unwrap_or(0);
            let x = edge.min(u16::from(columns.saturating_sub(r.w)));
            Rect {
                x: u8::try_from(x).unwrap_or(0),
                ..r
            }
        }
        _ => {
            let edge = blockers.iter().map(|(_, o)| o.rect.y).min().unwrap_or(0);
            Rect {
                y: edge.saturating_sub(u16::from(r.h)),
                ..r
            }
        }
    };
    // Always a different rectangle (the blocker lies on that side); a no-op result is refused by the caller.
    Ok(Move::To(past))
}

/// Shift and an arrow key: one step of the kind's rule; Right and Left change the width, Down and Up the height.
pub(crate) fn grow(item: &Item, dir: Dir, columns: u8) -> Result<Rect, Block> {
    let r = item.rect;
    let next = match dir {
        Dir::Right => Rect {
            w: item.rule.widths.up(r.w).ok_or(Block::Largest)?,
            ..r
        },
        Dir::Left => Rect {
            w: item.rule.widths.down(r.w).ok_or(Block::Smallest)?,
            ..r
        },
        Dir::Down => Rect {
            h: item.rule.heights.up(r.h).ok_or(Block::Largest)?,
            ..r
        },
        Dir::Up => Rect {
            h: item.rule.heights.down(r.h).ok_or(Block::Smallest)?,
            ..r
        },
    };
    if next.fits(columns) {
        Ok(next)
    } else {
        Err(Block::Edge(dir))
    }
}

/// Home: the top-left corner.
pub(crate) fn first(item: &Item) -> Rect {
    Rect {
        x: 0,
        y: 0,
        ..item.rect
    }
}

/// End: below everything else, at the left (gravity may then lift it into a hole beside the others).
pub(crate) fn last(items: &[Item], i: usize) -> Result<Rect, GridError> {
    let bottom = items
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, o)| o.rect.bottom())
        .max()
        .unwrap_or(0);
    Rect {
        x: 0,
        ..items[i].rect
    }
    .at_row(bottom)
    .ok_or(GridError::NoRoom)
}

ts! {
/// Which keyboard and menu gestures would change the arrangement for one placement; the UI enables its keys and menu
/// items from this, never from its own arithmetic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moves {
    /// Move left.
    pub left: bool,
    /// Move right.
    pub right: bool,
    /// Move up.
    pub up: bool,
    /// Move down.
    pub down: bool,
    /// Move to first.
    pub first: bool,
    /// Move to last.
    pub last: bool,
    /// Wider.
    pub wider: bool,
    /// Narrower.
    pub narrower: bool,
    /// Taller.
    pub taller: bool,
    /// Shorter.
    pub shorter: bool,
}
}

/// The [`Moves`] of one placement: each gesture is applied and kept if it works, so the answer is the edit's own.
/// An unknown id or an invalid arrangement gives all `false`.
pub fn moves(items: &[Item], columns: u8, id: &crate::PlacementId) -> Moves {
    let ok = |g: GridGesture| arrange(items, columns, &g).is_ok();
    let id = || id.clone();
    Moves {
        left: ok(GridGesture::Nudge {
            id: id(),
            dir: Dir::Left,
        }),
        right: ok(GridGesture::Nudge {
            id: id(),
            dir: Dir::Right,
        }),
        up: ok(GridGesture::Nudge {
            id: id(),
            dir: Dir::Up,
        }),
        down: ok(GridGesture::Nudge {
            id: id(),
            dir: Dir::Down,
        }),
        first: ok(GridGesture::ToFirst { id: id() }),
        last: ok(GridGesture::ToLast { id: id() }),
        wider: ok(GridGesture::Grow {
            id: id(),
            dir: Dir::Right,
        }),
        narrower: ok(GridGesture::Grow {
            id: id(),
            dir: Dir::Left,
        }),
        taller: ok(GridGesture::Grow {
            id: id(),
            dir: Dir::Down,
        }),
        shorter: ok(GridGesture::Grow {
            id: id(),
            dir: Dir::Up,
        }),
    }
}
