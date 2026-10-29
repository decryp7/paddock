/**
 * The narrowest the message column of the events grid may get, px. The Explorer and the other docked panes give way before
 * the message does (the Events view declares its minimum width from this), so a narrow window never squeezes the message
 * to a few words; when even that does not fit, the layout plan closes the Explorer.
 */
export const MESSAGE_COLUMN_MIN = 360;

/** The narrowest the grid's other columns get, px: level mark, time, severity, source (the time and source give way before the message). */
export const GRID_COLUMN_MINS = { mark: 16, time: 100, severity: 52, source: 88 } as const;

/**
 * The narrowest the Events view may be, px: the message column's minimum plus the grid's other columns at their minimum, the
 * gaps between the columns, the row padding and the 8 px scroll bar. Small enough that the Explorer at its 240 px minimum, the
 * Events view and a Registry or Home column fit side by side in a 1440 px window. A grid with extra columns shows them within
 * this width (they give way first).
 */
export const EVENTS_VIEW_MIN_WIDTH =
  MESSAGE_COLUMN_MIN + GRID_COLUMN_MINS.mark + GRID_COLUMN_MINS.time + GRID_COLUMN_MINS.severity + GRID_COLUMN_MINS.source + 4 * 10 + 2 * 14 + 8;
