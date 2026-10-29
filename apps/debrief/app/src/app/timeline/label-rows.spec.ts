import { describe, expect, it } from 'vitest';
import { placeLabels } from './label-rows';

describe('placeLabels', () => {
  it('stacks labels that touch and reuses a row once the label before it has ended', () => {
    expect(placeLabels([{ x: 0, w: 50 }, { x: 10, w: 50 }, { x: 54, w: 20 }], 1000)).toEqual([
      { row: 0, flip: false },
      { row: 1, flip: false },
      { row: 0, flip: false },
    ]);
  });

  it('hangs a label that would pass the right edge left of its anchor, and stacks it against what it now covers', () => {
    const slots = placeLabels([{ x: 900, w: 60 }, { x: 980, w: 60 }], 1000);
    expect(slots).toEqual([
      { row: 0, flip: false },
      { row: 1, flip: true },
    ]);
  });

  it('leaves a label that has no room on its left where it is', () => {
    expect(placeLabels([{ x: 10, w: 60 }], 50)).toEqual([{ row: 0, flip: false }]);
  });
});
