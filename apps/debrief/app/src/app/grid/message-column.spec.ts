import { describe, expect, it } from 'vitest';
import { BUILTIN_VIEWS } from '../builtin/builtin-views';
import { EVENTS_VIEW_MIN_WIDTH, eventsViewMinWidth, GRID, GRID_COLUMN_MINS, MESSAGE_COLUMN_MIN, timeFullWidth } from './message-column';

describe('the events grid geometry', () => {
  it('the Events view declares a minimum width that holds the message column at its minimum and the other columns', () => {
    const events = BUILTIN_VIEWS.find((v) => v.id === 'events');
    expect(events?.layout?.min.width).toBe(EVENTS_VIEW_MIN_WIDTH);
    const columns = GRID_COLUMN_MINS.mark + GRID_COLUMN_MINS.time + GRID_COLUMN_MINS.severity + GRID_COLUMN_MINS.source;
    expect(EVENTS_VIEW_MIN_WIDTH).toBe(MESSAGE_COLUMN_MIN + columns + 4 * GRID.gap + 2 * GRID.padX + GRID.scrollbar);
  });

  it('each extra column adds its minimum and one gap', () => {
    expect(eventsViewMinWidth(2) - eventsViewMinWidth(0)).toBe(2 * (GRID_COLUMN_MINS.extra + GRID.gap));
  });

  it('the time column grows to the full date and time of the format, and its minimum holds the compact time', () => {
    expect(timeFullWidth('readable')).toBeLessThan(timeFullWidth('iso'));
    expect(GRID_COLUMN_MINS.time).toBeLessThan(timeFullWidth('readable'));
    // 12 characters ("09:00:00.000") at 7.3 px; the full formats are 19 and 23 characters.
    expect(GRID_COLUMN_MINS.time).toBeGreaterThanOrEqual(Math.ceil(12 * 7.3));
    expect(timeFullWidth('readable')).toBeGreaterThanOrEqual(Math.ceil(19 * 7.3));
    expect(timeFullWidth('iso')).toBeGreaterThanOrEqual(Math.ceil(23 * 7.3));
  });
});
