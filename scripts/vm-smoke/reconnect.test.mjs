import assert from 'node:assert/strict';
import { test } from 'node:test';
import { retryOnClosed } from './reconnect.mjs';

test('a closed page is replaced by a fresh handle and the call runs again', async () => {
  let reconnects = 0;
  const r = await retryOnClosed(async (h) => { if (h === 'stale') throw new Error('page.evaluate: Target page, context or browser has been closed'); return `ok with ${h}`; }, 'stale', async () => { reconnects++; return 'fresh'; });
  assert.deepEqual([r.value, r.handle, reconnects], ['ok with fresh', 'fresh', 1]);
});
test('any other error passes through without a reconnect', async () => {
  let reconnects = 0;
  await assert.rejects(retryOnClosed(async () => { throw new Error('no such button'); }, 'h', async () => { reconnects++; return 'x'; }), /no such button/);
  assert.equal(reconnects, 0);
});
test('a page that stays closed fails after one retry', async () => {
  let reconnects = 0;
  await assert.rejects(retryOnClosed(async () => { throw new Error('Target page, context or browser has been closed'); }, 'h', async () => { reconnects++; return 'h2'; }), /closed/);
  assert.equal(reconnects, 1);
});
