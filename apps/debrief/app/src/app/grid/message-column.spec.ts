import { describe, expect, it } from 'vitest';
import { BUILTIN_VIEWS } from '../builtin/builtin-views';
import { EVENTS_VIEW_MIN_WIDTH, eventsViewMinWidth, GRID, GRID_COLUMN_MINS, MESSAGE_COLUMN_MIN, timeFullWidth, timeMinWidth } from './message-column';

describe('the events grid geometry', () => {
  it('the Events view declares a minimum width that holds the message column at its minimum and the other columns', () => {
    const events = BUILTIN_VIEWS.find((v) => v.id === 'events');
    expect(events?.layout?.min.width).toBe(EVENTS_VIEW_MIN_WIDTH);
    const columns = GRID_COLUMN_MINS.mark + timeMinWidth() + GRID_COLUMN_MINS.severity + GRID_COLUMN_MINS.source;
    expect(EVENTS_VIEW_MIN_WIDTH).toBe(MESSAGE_COLUMN_MIN + columns + 4 * GRID.gap + 2 * GRID.padX + GRID.scrollbar);
  });

  it('each extra column adds its minimum and one gap', () => {
    expect(eventsViewMinWidth(2) - eventsViewMinWidth(0)).toBe(2 * (GRID_COLUMN_MINS.extra + GRID.gap));
  });

  it('the time column is sized in characters of the cell\'s font: the compact time at the least, the full format at the most', () => {
    for (const charPx of [7.2, 7.8, 8.4]) {
      expect(timeMinWidth(charPx)).toBeGreaterThanOrEqual(12 * charPx);
      expect(timeFullWidth('readable', charPx)).toBeGreaterThanOrEqual(19 * charPx);
      expect(timeFullWidth('iso', charPx)).toBeGreaterThanOrEqual(23 * charPx);
      expect(timeMinWidth(charPx)).toBeLessThan(timeFullWidth('readable', charPx));
      expect(timeFullWidth('readable', charPx)).toBeLessThan(timeFullWidth('iso', charPx));
    }
  });

  it('a wider font asks the plan for a wider Events view', () => {
    expect(eventsViewMinWidth(0, 7.8)).toBeGreaterThan(eventsViewMinWidth(0, 7.2));
  });
});
