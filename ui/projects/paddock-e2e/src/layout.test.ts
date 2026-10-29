import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isAcceptedEllipsis, isSilentlyClipped, unacceptedTextClips, type ClipProbe } from './layout.ts';

const base: ClipProbe = { clientWidth: 100, scrollWidth: 160, hasOwnText: true, overflowX: 'hidden', textOverflow: 'clip', positionAbsolute: false, says: true, never: false };

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

test('an ellipsis counts only when the element says its full text and is not a control, tab, link or number', () => {
  const ok = { textOverflow: 'ellipsis', says: true, never: false };
  assert.equal(isAcceptedEllipsis(ok), true);
  assert.equal(isAcceptedEllipsis({ ...ok, says: false }), false);
  assert.equal(isAcceptedEllipsis({ ...ok, never: true }), false);
  assert.equal(isAcceptedEllipsis({ ...ok, textOverflow: 'clip' }), false);
  assert.equal(isSilentlyClipped({ ...base, textOverflow: 'ellipsis', says: false }), true);
  assert.equal(isSilentlyClipped({ ...base, textOverflow: 'ellipsis', never: true }), true);
});

test('texts beyond their box or the window are reported unless they are an accepted ellipsis', () => {
  const clip = { description: '"x" in span', pastWindowEdge: false, beyondClipper: true, textOverflow: 'ellipsis', says: true, never: false };
  assert.deepEqual(unacceptedTextClips([clip]), []);
  assert.equal(unacceptedTextClips([{ ...clip, says: false }]).length, 1);
  assert.equal(unacceptedTextClips([{ ...clip, never: true }]).length, 1);
  assert.equal(unacceptedTextClips([{ ...clip, beyondClipper: false, pastWindowEdge: true, textOverflow: 'clip' }]).length, 1);
  assert.equal(unacceptedTextClips([{ ...clip, pastWindowEdge: true, textOverflow: 'clip', beyondClipper: true }]).length, 2);
});
