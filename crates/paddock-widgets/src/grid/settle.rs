//! Gravity, swap and push (ADR 0044 §1): how an arrangement settles after a placement is put on a target rectangle.

use std::cmp::Ordering;

use super::keys::{self, Move};
use super::model::{Block, GridError, GridGesture, Item, MAX_ROWS, Rect};
use super::steps::SizeRule;
use crate::id::PlacementId;

/// Reading order: row, then column, then id.
pub(crate) fn order(a: &Item, b: &Item) -> Ordering {
    (a.rect.y, a.rect.x, a.id.as_str()).cmp(&(b.rect.y, b.rect.x, b.id.as_str()))
}

/// The items sorted in reading order (`y`, then `x`, then id).
pub fn reading_order(mut items: Vec<Item>) -> Vec<Item> {
    items.sort_by(order);
    items
}

/// Raises `rect` to the highest row where it overlaps none of `placed`: first below whatever it overlaps, then up while
/// the row above is free.
fn rise(mut rect: Rect, placed: &[Rect]) -> Result<Rect, GridError> {
    while let Some(bottom) = placed
        .iter()
        .filter(|p| p.overlaps(&rect))
        .map(Rect::bottom)
        .max()
    {
        rect = rect.at_row(bottom).ok_or(GridError::NoRoom)?;
    }
    while rect.y > 0 {
        let above = Rect {
            y: rect.y - 1,
            ..rect
        };
        if placed.iter().any(|p| p.overlaps(&above)) {
            break;
        }
        rect = above;
    }
    Ok(rect)
}

/// Gravity: each item, in reading order, moves up to the highest free row. The result is in reading order and
/// compacting it again changes nothing.
pub fn compact(items: Vec<Item>) -> Result<Vec<Item>, GridError> {
    let mut placed: Vec<Rect> = Vec::with_capacity(items.len());
    let mut out = Vec::with_capacity(items.len());
    for mut item in reading_order(items) {
        item.rect = rise(item.rect, &placed)?;
        placed.push(item.rect);
        out.push(item);
    }
    Ok(reading_order(out))
}

fn index_of(items: &[Item], id: &PlacementId) -> Result<usize, GridError> {
    items
        .iter()
        .position(|i| &i.id == id)
        .ok_or(GridError::Unknown)
}

/// A valid arrangement: every rectangle inside the grid, no overlap, no repeated id.
pub(crate) fn validate(items: &[Item], columns: u8) -> Result<(), GridError> {
    for (n, a) in items.iter().enumerate() {
        if !a.rect.fits(columns) {
            return Err(GridError::Invalid);
        }
        for b in &items[n + 1..] {
            if a.id == b.id || a.rect.overlaps(&b.rect) {
                return Err(GridError::Invalid);
            }
        }
    }
    Ok(())
}

/// Clamps a requested rectangle to the kind's rule and the grid.
fn clamp(rect: Rect, rule: &SizeRule, columns: u8) -> Rect {
    let w = rule.widths.nearest(rect.w).clamp(1, columns.max(1));
    let h = rule.heights.nearest(rect.h).max(1);
    let x = rect.x.min(columns.saturating_sub(w));
    let y = rect.y.min(MAX_ROWS.saturating_sub(u16::from(h)));
    Rect { x, y, w, h }
}

/// Swap, else push down, then gravity. `items[i]` is put on `target`.
fn put(mut items: Vec<Item>, i: usize, target: Rect) -> Result<Vec<Item>, GridError> {
    let old = items[i].rect;
    let overlapped: Vec<usize> = (0..items.len())
        .filter(|&j| j != i && items[j].rect.overlaps(&target))
        .collect();
    if let [j] = overlapped[..] {
        let other = items[j].rect;
        if other == target && target.size() == old.size() {
            items[j].rect = old;
            items[i].rect = target;
            return compact(items);
        }
    }
    items[i].rect = target;
    let mut placed = Vec::with_capacity(items.len());
    placed.push(target);
    let mut order: Vec<usize> = (0..items.len()).filter(|&j| j != i).collect();
    order.sort_by(|&a, &b| self::order(&items[a], &items[b]));
    for j in order {
        let mut rect = items[j].rect;
        while let Some(bottom) = placed
            .iter()
            .filter(|p| p.overlaps(&rect))
            .map(Rect::bottom)
            .max()
        {
            rect = rect.at_row(bottom).ok_or(GridError::NoRoom)?;
        }
        placed.push(rect);
        items[j].rect = rect;
    }
    compact(items)
}

/// Exchanges the rectangles of two same-sized items, then gravity.
fn exchange(mut items: Vec<Item>, i: usize, j: usize) -> Result<Vec<Item>, GridError> {
    let (a, b) = (items[i].rect, items[j].rect);
    items[i].rect = b;
    items[j].rect = a;
    compact(items)
}

/// Whether both lists hold the same rectangles per id. Linear when both are in the same order (the usual case: the
/// arrangement is kept in reading order).
fn same_rects(a: &[Item], b: &[Item]) -> bool {
    a.len() == b.len()
        && (a.iter().zip(b).all(|(x, y)| x.id == y.id && x.rect == y.rect)
            // Not in the same order: the first rectangle that differs ends the search, the usual case of a change.
            || a.iter().all(|x| b.iter().any(|y| y.id == x.id && y.rect == x.rect)))
}

/// Applies one gesture to a valid arrangement of `columns` columns and returns the new one, compact and in reading
/// order. Keyboard gestures that change nothing are refused with the reason ([`GridError::Blocked`]); a
/// [`GridGesture::Place`] on its own place is accepted and changes nothing.
pub fn arrange(items: &[Item], columns: u8, gesture: &GridGesture) -> Result<Vec<Item>, GridError> {
    validate(items, columns)?;
    arrange_valid(items, columns, gesture)
}

/// [`arrange`] for an arrangement already known to be valid. Works on the slice and clones only when the gesture does
/// something, so the many probes of [`super::moves`] stay cheap.
pub(crate) fn arrange_valid(
    items: &[Item],
    columns: u8,
    gesture: &GridGesture,
) -> Result<Vec<Item>, GridError> {
    match gesture {
        GridGesture::Place { id, rect } => {
            let i = index_of(items, id)?;
            let target = clamp(*rect, &items[i].rule, columns);
            put(items.to_vec(), i, target)
        }
        GridGesture::Nudge { id, dir } => {
            let i = index_of(items, id)?;
            let next = match keys::nudge(items, i, *dir, columns).map_err(GridError::Blocked)? {
                Move::To(rect) => put(items.to_vec(), i, rect)?,
                Move::Exchange(j) => exchange(items.to_vec(), i, j)?,
            };
            changed(items, next)
        }
        GridGesture::Grow { id, dir } => {
            let i = index_of(items, id)?;
            let rect = keys::grow(&items[i], *dir, columns).map_err(GridError::Blocked)?;
            changed(items, put(items.to_vec(), i, rect)?)
        }
        GridGesture::ToFirst { id } => {
            let i = index_of(items, id)?;
            let rect = keys::first(&items[i]);
            changed(items, put(items.to_vec(), i, rect)?)
        }
        GridGesture::ToLast { id } => {
            let i = index_of(items, id)?;
            changed(items, to_last(items.to_vec(), i, columns)?)
        }
    }
}

/// End: the placement goes after the last one in reading order, in the first free place there, and rises only while it
/// stays after that one in reading order (ADR 0044 amendment). The others are compacted as usual.
fn to_last(mut items: Vec<Item>, i: usize, columns: u8) -> Result<Vec<Item>, GridError> {
    let mut moved = items.remove(i);
    let others = compact(items)?;
    // The first free place after the last placement is already the highest one that keeps End last: every place
    // before it in the scan is earlier in reading order or taken.
    let rect = match others.last() {
        Some(last) => {
            let from = (u32::from(last.rect.y), u32::from(last.rect.x) + 1);
            super::derive::first_free(&others, from, moved.rect.w, moved.rect.h, columns)?
        }
        None => Rect {
            x: 0,
            y: 0,
            ..moved.rect
        },
    };
    moved.rect = rect;
    let mut out = others;
    out.push(moved);
    Ok(out)
}

fn changed(before: &[Item], next: Vec<Item>) -> Result<Vec<Item>, GridError> {
    if same_rects(before, &next) {
        Err(GridError::Blocked(Block::Unchanged))
    } else {
        Ok(next)
    }
}
