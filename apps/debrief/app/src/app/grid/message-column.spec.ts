import { describe, expect, it } from 'vitest';
import { BUILTIN_VIEWS } from '../builtin/builtin-views';
import { EVENTS_VIEW_MIN_WIDTH, MESSAGE_COLUMN_MIN } from './message-column';

describe('the message column minimum', () => {
  it('the Events view declares a minimum width that holds the message column at its minimum and the other columns', () => {
    const events = BUILTIN_VIEWS.find((v) => v.id === 'events');
    expect(events?.layout?.min.width).toBe(EVENTS_VIEW_MIN_WIDTH);
    expect(EVENTS_VIEW_MIN_WIDTH).toBeGreaterThan(MESSAGE_COLUMN_MIN + 16 + 110 + 52 + 96);
  });
});
