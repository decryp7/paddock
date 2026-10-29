import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isSilentlyClipped, type ClipProbe } from './layout.ts';

const base: ClipProbe = { clientWidth: 100, scrollWidth: 160, hasOwnText: true, overflowX: 'hidden', textOverflow: 'clip', positionAbsolute: false };

test('text cut without an ellipsis is clipped silently', () => {
  assert.equal(isSilentlyClipped(base), true);
  assert.equal(isSilentlyClipped({ ...base, overflowX: 'clip' }), true);
});

test('an ellipsis, enough room, scrolling, no text or a hidden box is fine', () => {
  assert.equal(isSilentlyClipped({ ...base, textOverflow: 'ellipsis' }), false);
  assert.equal(isSilentlyClipped({ ...base, scrollWidth: 101 }), false);
  assert.equal(isSilentlyClipped({ ...base, overflowX: 'auto' }), false);
  assert.equal(isSilentlyClipped({ ...base, overflowX: 'visible' }), false);
  assert.equal(isSilentlyClipped({ ...base, hasOwnText: false }), false);
  assert.equal(isSilentlyClipped({ ...base, clientWidth: 0 }), false);
});

test('screen-reader-only text is clipped on purpose', () => {
  assert.equal(isSilentlyClipped({ ...base, clientWidth: 1, positionAbsolute: true }), false);
});
