import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isOpaqueColor } from './grid.ts';

test('a header colour is opaque only at full alpha', () => {
  assert.equal(isOpaqueColor('rgb(240, 242, 245)'), true);
  assert.equal(isOpaqueColor('rgba(240, 242, 245, 1)'), true);
  assert.equal(isOpaqueColor('rgba(240, 242, 245, 0.6)'), false);
  assert.equal(isOpaqueColor('color(srgb 0.9 0.9 0.9 / 0.5)'), false);
  assert.equal(isOpaqueColor('color(srgb 0.9 0.9 0.9 / 100%)'), true);
  assert.equal(isOpaqueColor('oklch(0.5 0.1 200 / 40%)'), false);
  assert.equal(isOpaqueColor('transparent'), false);
  assert.equal(isOpaqueColor(''), false);
  assert.equal(isOpaqueColor('#1b1e23'), true);
});
