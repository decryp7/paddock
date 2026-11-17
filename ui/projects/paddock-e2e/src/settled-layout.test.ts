import assert from 'node:assert/strict';
import { test } from 'node:test';
import { layoutSettled, type LayoutReading } from './settled-layout.ts';

const r = (count: number, widths: number[], fits = true): LayoutReading => ({ count, widths, fits });

test('two agreeing readings with boxes that fit settle the layout', () => {
  assert.equal(layoutSettled(r(2, [100, 200]), r(2, [100, 200])), true);
});

test('the first reading never settles, and a changed count or width does not', () => {
  assert.equal(layoutSettled(null, r(2, [100, 200])), false);
  assert.equal(layoutSettled(r(2, [100, 200]), r(3, [100, 200, 50])), false);
  assert.equal(layoutSettled(r(2, [100, 200]), r(2, [100, 201])), false);
});

test('a reading of no boxes is never settled, however much it agrees (the vacuous every() of the old waits)', () => {
  assert.equal(layoutSettled(r(0, []), r(0, [])), false);
  assert.equal(layoutSettled(r(0, []), r(0, []), 0), true, 'minCount 0 is the page that draws no boxes by design');
  assert.equal(layoutSettled(r(0, []), r(1, [5]), 0), false, 'but a box appearing is still a change');
  assert.equal(layoutSettled(r(1, [5]), r(1, [5]), 3), false, 'fewer than the expected boxes');
  assert.equal(layoutSettled(r(3, [5, 5, 5]), r(3, [5, 5, 5]), 3), true);
});

test('a box that does not fit is not settled', () => {
  assert.equal(layoutSettled(r(1, [5], false), r(1, [5], false)), false);
  assert.equal(layoutSettled(r(1, [5]), r(1, [5], false)), false);
});

test('settledLayout compares a read with the one just before it, not with the one before that (A, B, A is not settled)', async () => {
  const { settledLayout } = await import('./settled-layout.ts');
  const a = r(1, [100]);
  const b = r(1, [120]);
  const reads = [a, b, a, a];
  let n = 0;
  const settled = await settledLayout(async () => reads[Math.min(n++, reads.length - 1)]!, { timeout: 10_000 });
  assert.deepEqual(settled, a);
  assert.equal(n, 4, 'A, B, A agrees only with the read two before it; it settles on the fourth read, A, A');
});
