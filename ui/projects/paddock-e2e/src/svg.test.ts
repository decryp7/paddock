import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pickProbe, type ShapeProbe } from './svg.ts';

const probe = (x: number, y: number, cushioned: boolean): ShapeProbe => ({ x, y, cushioned });

test('the nearest probe with room around it wins over a nearer one on the very edge of the fill', () => {
  // The half ring of Dyno's disk map: the probe nearest the box centre sat on the edge and a WebKit click missed the shape.
  assert.deepEqual(pickProbe([probe(641.6, 407, false), probe(650, 420, true), probe(660, 430, true)]), { x: 650, y: 420 });
});

test('with no probe that has room, the nearest probe on the shape is used (a thin shape is still clickable)', () => {
  assert.deepEqual(pickProbe([probe(1, 2, false), probe(3, 4, false)]), { x: 1, y: 2 });
});

test('no probe on the shape gives no point', () => {
  assert.equal(pickProbe([]), null);
});
