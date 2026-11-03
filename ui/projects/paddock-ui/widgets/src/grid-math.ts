import type { Rect } from '../generated/widgets';

/** The grid element's measures in px, read at the moment of a gesture. */
export interface GridGeometry {
  /** Left of the grid in the viewport. */
  readonly left: number;
  /** Top of the grid in the viewport. */
  readonly top: number;
  /** Width of one column. */
  readonly colWidth: number;
  /** Height of one row unit. */
  readonly rowHeight: number;
  /** The gap between cells, both ways. */
  readonly gap: number;
  /** Columns of the arrangement on screen. */
  readonly columns: number;
}

/**
 * The cell a dragged frame's top-left corner is nearest to, for a frame `w` by `h` cells. Pointer-to-cell rounding is the
 * UI's only arithmetic (ADR 0044 §2): the engine decides what the drop means. `left` and `top` are relative to the grid.
 */
export function moveTarget(g: GridGeometry, left: number, top: number, w: number, h: number): Rect {
  const x = Math.round(left / (g.colWidth + g.gap));
  const y = Math.round(top / (g.rowHeight + g.gap));
  return { x: clamp(x, 0, Math.max(0, g.columns - w)), y: Math.max(0, y), w, h };
}

/** The cell size nearest to a frame of `width` by `height` px whose top-left cell is `(x, y)`; at least 1 by 1 and inside the grid. */
export function resizeTarget(g: GridGeometry, x: number, y: number, width: number, height: number): Rect {
  const w = Math.round((width + g.gap) / (g.colWidth + g.gap));
  const h = Math.round((height + g.gap) / (g.rowHeight + g.gap));
  return { x, y, w: clamp(w, 1, Math.max(1, g.columns - x)), h: Math.max(1, h) };
}

/** The CSS grid lines of a rectangle: `x + 1 / span w`. */
export const columnLines = (r: Rect): string => `${r.x + 1} / span ${r.w}`;
/** The CSS grid lines of a rectangle: `y + 1 / span h`. */
export const rowLines = (r: Rect): string => `${r.y + 1} / span ${r.h}`;

/** True when two rectangles are the same cells. */
export const sameRect = (a: Rect, b: Rect): boolean => a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}
