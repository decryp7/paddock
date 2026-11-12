import assert from 'node:assert/strict';
import { test } from 'node:test';
import { checks } from '../src/checks/index.mjs';

// Two branches that each add a check with the same number meet in a train: fail there, not in a review.
test('every registered check has its own id', () => {
  const seen = new Map();
  for (const c of checks) {
    assert.ok(!seen.has(c.id), `check id ${c.id} is registered twice (${seen.get(c.id)} and ${c.title}): renumber one, the next free number across all open branches`);
    seen.set(c.id, c.title);
  }
});
