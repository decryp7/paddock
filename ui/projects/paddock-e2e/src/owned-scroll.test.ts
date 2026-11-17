import assert from 'node:assert/strict';
import { test } from 'node:test';
import { anchoringVerdict } from './owned-scroll.ts';

test('anchoringVerdict: held when the scroller kept its place, moved when it did not, not measured when the control saw no anchoring', () => {
  assert.equal(anchoringVerdict({ control: 100, shift: 0 }), 'held');
  assert.equal(anchoringVerdict({ control: 100, shift: 100 }), 'moved');
  assert.equal(anchoringVerdict({ control: 100, shift: -100 }), 'moved');
  // ER-33: with no anchoring even forced on, a 0 shift proves nothing.
  assert.equal(anchoringVerdict({ control: 0, shift: 0 }), 'not-measured');
  assert.equal(anchoringVerdict({ control: 40, shift: 0 }), 'not-measured');
  // Sub-pixel rounding is no move.
  assert.equal(anchoringVerdict({ control: 99.5, shift: 0.5 }), 'held');
  assert.equal(anchoringVerdict({ control: 50, shift: 0 }, 50), 'held');
});
