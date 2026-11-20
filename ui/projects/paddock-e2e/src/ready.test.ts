import assert from 'node:assert/strict';
import { test } from 'node:test';
import { menuGap, overlayOffset, streamGrew } from './ready.ts';

test('overlayOffset is the larger of the left-edge offset and the width difference', () => {
  assert.equal(overlayOffset({ x: 536, width: 212 }, { x: 536, width: 212 }), 0);
  assert.equal(overlayOffset({ x: 0, width: 131 }, { x: 536.09375, width: 212 }), 536.09375, 'the panel before the overlay positioned it (sweep of 2026-11-17)');
  assert.equal(overlayOffset({ x: 536, width: 150 }, { x: 536, width: 212 }), 62);
});

test('overlayOffset is null while either box is not drawn', () => {
  assert.equal(overlayOffset(null, { x: 0, width: 1 }), null);
  assert.equal(overlayOffset({ x: 0, width: 1 }, null), null);
});

test('menuGap is 0 for a menu right below, above or beside its trigger', () => {
  const file = { x: 115, y: 4, width: 36, height: 26 };
  assert.equal(menuGap({ x: 115, y: 30, width: 270, height: 540 }, file), 0, 'below a menu-bar item');
  assert.equal(menuGap({ x: 115, y: -300, width: 270, height: 304 }, file), 0, 'above it');
  const item = { x: 120, y: 200, width: 260, height: 26 };
  assert.equal(menuGap({ x: 380, y: 196, width: 240, height: 300 }, item), 0, 'a submenu beside its item');
  assert.equal(menuGap({ x: 115, y: 33, width: 270, height: 540 }, file), 3);
});

test('menuGap is Infinity for a menu that covers its trigger or touches none of its sides, null while one is not drawn', () => {
  // Run 8297: the File menu at the top-left before the overlay was positioned, over its own trigger.
  assert.equal(menuGap({ x: 0, y: 0, width: 274, height: 540 }, { x: 115, y: 4, width: 36, height: 26 }), Number.POSITIVE_INFINITY);
  assert.equal(menuGap({ x: 600, y: 600, width: 100, height: 100 }, { x: 115, y: 4, width: 36, height: 26 }), Number.POSITIVE_INFINITY);
  assert.equal(menuGap(null, { x: 0, y: 0, width: 1, height: 1 }), null);
  assert.equal(menuGap({ x: 0, y: 0, width: 1, height: 1 }, null), null);
});


test('streamGrew is true when text or elements arrived between two reads, false when nothing changed', () => {
  assert.equal(streamGrew({ chars: 10, elements: 2 }, { chars: 22, elements: 2 }), true, 'a piece of text');
  assert.equal(streamGrew({ chars: 10, elements: 2 }, { chars: 10, elements: 3 }), true, 'a tool chip with no text yet');
  assert.equal(streamGrew({ chars: 10, elements: 2 }, { chars: 10, elements: 2 }), false, 'stuck');
});
