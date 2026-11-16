import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PINNED_NOW, PINNED_NOW_ENV, pinnedInstant } from './pinned-clock.ts';

test('the pin is the default, the option, or the environment, in that order of strength', () => {
  assert.equal(pinnedInstant({}, {}).toISOString(), '2026-10-22T14:30:00.000Z');
  assert.equal(pinnedInstant({ now: PINNED_NOW }, {}).toISOString(), '2026-10-22T14:30:00.000Z');
  assert.equal(pinnedInstant({ now: Date.UTC(2026, 0, 2, 3) }, {}).toISOString(), '2026-01-02T03:00:00.000Z');
  assert.equal(pinnedInstant({ now: '2026-01-02T03:00:00Z' }, { [PINNED_NOW_ENV]: '2026-11-16T00:19:00Z' }).toISOString(), '2026-11-16T00:19:00.000Z');
  assert.equal(pinnedInstant({}, { [PINNED_NOW_ENV]: '' }).toISOString(), '2026-10-22T14:30:00.000Z');
});

test('an unreadable instant names its source and the format', () => {
  assert.throws(() => pinnedInstant({}, { [PINNED_NOW_ENV]: 'noon' }), /PADDOCK_E2E_NOW: "noon" is not a date/);
  assert.throws(() => pinnedInstant({ now: 'x' }, {}), /pinned clock now: "x" is not a date/);
});
