//! Narrower arrangements, derived and never stored (ADR 0044 §3).

use super::model::{GridError, Item, Rect};
use super::settle::reading_order;

/// The narrower arrangement of a design arrangement of `design` columns, for a grid of `columns` columns.
///
/// In the design's reading order each item gets the half or the full width of the narrow grid (the smaller one not
/// below its share `w * columns / design`, and not below its kind's minimum scaled the same way), keeps its height, and
/// goes to the first free place after the previous one (CSS grid's sparse auto-placement), without gravity. A half-width
/// item left alone in its rows, with nothing beside it, takes the whole row when its kind allows that wide (no hole beside
/// it, which an odd number of half-width items before a full-width one would leave). At one
/// column every item is full width. The result is in the design's reading order, so focus order is the same at every
/// width.
pub fn derive(items: &[Item], design: u8, columns: u8) -> Result<Vec<Item>, GridError> {
    let columns = columns.max(1);
    let design = design.max(1);
    let half = columns.div_ceil(2);
    let scale_up = |w: u8| (u32::from(w) * u32::from(columns)).div_ceil(u32::from(design));
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut cursor = (0u32, 0u32);
    for item in reading_order(items.to_vec()) {
        let need = scale_up(item.rect.w).max(scale_up(item.rule.widths.min()));
        let w = if need <= u32::from(half) {
            half
        } else {
            columns
        };
        let h = item.rect.h.max(1);
        let rect = first_free(&out, cursor, w, h, columns)?;
        cursor = (u32::from(rect.y), u32::from(rect.right()));
        out.push(Item { rect, ..item });
    }
    fill_lone_halves(&mut out, design, columns, half);
    Ok(out)
}

/// Widens each half-width item at the left edge whose right half is empty in all of its rows to the whole width, when the
/// kind's maximum width (scaled to the narrow grid) reaches it. Only widens, so nothing placed overlaps afterwards.
fn fill_lone_halves(out: &mut [Item], design: u8, columns: u8, half: u8) {
    if half >= columns {
        return;
    }
    for i in 0..out.len() {
        let r = out[i].rect;
        let scaled_max =
            (u32::from(out[i].rule.widths.max()) * u32::from(columns)).div_ceil(u32::from(design));
        if r.x != 0 || r.w != half || scaled_max < u32::from(columns) {
            continue;
        }
        let hole = Rect {
            x: half,
            w: columns - half,
            ..r
        };
        if !out.iter().any(|o| o.rect.overlaps(&hole)) {
            out[i].rect.w = columns;
        }
    }
}

/// The first place at or after `cursor` (row, column), scanning rows then columns, where a `w` by `h` rectangle fits
/// and overlaps none of `placed`.
pub(crate) fn first_free(
    placed: &[Item],
    cursor: (u32, u32),
    w: u8,
    h: u8,
    columns: u8,
) -> Result<Rect, GridError> {
    let (start_row, start_col) = cursor;
    let last_row = super::MAX_ROWS.saturating_sub(u16::from(h));
    for y in u16::try_from(start_row).unwrap_or(u16::MAX)..=last_row {
        let from = if u32::from(y) == start_row {
            start_col
        } else {
            0
        };
        for x in u8::try_from(from).unwrap_or(u8::MAX)..=columns.saturating_sub(w) {
            let rect = Rect { x, y, w, h };
            if !placed.iter().any(|p| p.rect.overlaps(&rect)) {
                return Ok(rect);
            }
        }
    }
    Err(GridError::NoRoom)
}
