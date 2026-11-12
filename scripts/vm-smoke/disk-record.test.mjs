import assert from 'node:assert/strict';
import { test } from 'node:test';
import { recordScratch } from './disk-record.mjs';

test('recordScratch passes a timeout, and a failure or a timeout logs one ::warning:: and does not throw', () => {
  const seen = [];
  const log = (m) => seen.push(m);
  let opts;
  assert.equal(recordScratch('/s', '/r', { log, spawn: (c, a, o) => ((opts = o), { status: 0 }) }), true);
  assert.equal(opts.timeout, 20_000);
  assert.deepEqual(seen, []);
  assert.equal(recordScratch('/s', '/r', { log, spawn: () => ({ status: 3 }) }), false);
  assert.equal(recordScratch('/s', '/r', { log, spawn: () => ({ status: null, error: Object.assign(new Error('x'), { code: 'ETIMEDOUT' }) }) }), false);
  assert.equal(seen.length, 2);
  assert.match(seen[0], /::warning::vm-smoke: owner record for \/s failed \(exit 3\)/);
  assert.match(seen[1], /ETIMEDOUT after 20 s/);
});
