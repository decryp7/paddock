import assert from 'node:assert/strict';
import { test } from 'node:test';
import { overlayOffset } from './ready.ts';

test('overlayOffset is the larger of the left-edge offset and the width difference', () => {
  assert.equal(overlayOffset({ x: 536, width: 212 }, { x: 536, width: 212 }), 0);
  assert.equal(overlayOffset({ x: 0, width: 131 }, { x: 536.09375, width: 212 }), 536.09375, 'the panel before the overlay positioned it (sweep of 2026-11-17)');
  assert.equal(overlayOffset({ x: 536, width: 150 }, { x: 536, width: 212 }), 62);
});

test('overlayOffset is null while either box is not drawn', () => {
  assert.equal(overlayOffset(null, { x: 0, width: 1 }), null);
  assert.equal(overlayOffset({ x: 0, width: 1 }, null), null);
});
