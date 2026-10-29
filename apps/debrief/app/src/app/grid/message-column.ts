/**
 * The narrowest the message column of the events grid may get, px. The Explorer and the other docked panes give way before
 * the message does (the Events view declares its minimum width from this), so a narrow window never squeezes the message
 * to a few words; when even that does not fit, the layout plan closes the Explorer.
 */
export const MESSAGE_COLUMN_MIN = 360;

/**
 * The narrowest the Events view may be, px: the message column's minimum plus the grid's other columns at their minimum
 * (level mark, time, severity, source), the gaps between the columns, the row padding and the vertical scroll bar. A grid
 * with extra columns shows them within this width (they give way first).
 */
export const EVENTS_VIEW_MIN_WIDTH = MESSAGE_COLUMN_MIN + (16 + 110 + 52 + 96) + 4 * 10 + 2 * 14 + 16;
