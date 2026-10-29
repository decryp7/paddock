import type { DateFormat } from '../core/format';

/**
 * The events grid's geometry, px, in one place: the grid's CSS reads the frame values through custom properties, its column
 * template is built from the column minimums, and the Events view declares its minimum width to the layout plan from the same
 * numbers (ADR 0056). The time column is sized in characters of the time cell's own font, measured per theme (`GridMetrics`):
 * the themes differ in the monospace font and its size, so no fixed pixel count per character is right for all of them.
 */
export const GRID = {
  /** The narrowest the message column may get. The Explorer and the other docked panes give way before it does. */
  message: 360,
  /** The narrowest each column may get (the time and source give way before the message). */
  min: {
    /** Marker. */
    mark: 16,
    /** Level. */
    severity: 52,
    /** Instance. */
    source: 88,
    /** Each extra column. */
    extra: 90,
  },
  /** Characters of the time cell: the compact format `09:00:00.000`, and the full date and time (`14 Oct 09:00:00.000`, `2026-10-14 09:00:00.000`). */
  timeChars: { compact: 12, readable: 19, iso: 23 },
  /** The widest an extra column grows, and the source column. */
  max: { extra: 150, source: 240 },
  /** Between columns. */
  gap: 10,
  /** Left and right padding of header and rows. */
  padX: 14,
  /** The thin scroll bar (styles.scss). */
  scrollbar: 8,
} as const;

/** The narrowest the message column of the events grid may get, px. */
export const MESSAGE_COLUMN_MIN = GRID.message;

/** The narrowest each column of the events grid may get, px. */
export const GRID_COLUMN_MINS = GRID.min;

/** Used until the grid has measured its font: a little over the widest advance of the usual monospace fonts at 12 px. */
export const DEFAULT_CHAR_PX = 7.3;

/** Rounding slack of the time column, px. */
const TIME_SLACK = 2;

/** The narrowest the time column may get: the compact time in the cell's font. */
export function timeMinWidth(charPx: number = DEFAULT_CHAR_PX): number {
  return Math.ceil(GRID.timeChars.compact * charPx) + TIME_SLACK;
}

/** The full width of the time column for a date format: below it the grid shows the compact time. */
export function timeFullWidth(format: DateFormat, charPx: number = DEFAULT_CHAR_PX): number {
  return Math.ceil(GRID.timeChars[format] * charPx) + TIME_SLACK;
}

/**
 * The narrowest the Events view may be, px, with `extraColumns` extra grid columns: the message column at its minimum, the
 * other columns at theirs, the gaps, the row padding and the scroll bar. The layout plan accounts for it; at this width the
 * time column shows the compact time.
 */
export function eventsViewMinWidth(extraColumns = 0, charPx: number = DEFAULT_CHAR_PX): number {
  const m = GRID.min;
  const columns = 4 + extraColumns + 1;
  return GRID.message + m.mark + timeMinWidth(charPx) + m.severity + m.source + extraColumns * m.extra + (columns - 1) * GRID.gap + 2 * GRID.padX + GRID.scrollbar;
}

/** The Events view's minimum width with no extra columns at the default character width (the declaration in the built-in views). */
export const EVENTS_VIEW_MIN_WIDTH = eventsViewMinWidth(0);
