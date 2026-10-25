import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scanSeams } from './scan.mjs';

const image = (w, h, f) => {
  const pixels = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) pixels.set([...f(x, y), 255], (y * w + x) * 4);
  return { pixels, width: w, height: h };
};

test('a smooth ramp passes', () => assert.deepEqual(scanSeams(image(40, 60, (x, y) => [y, y, y])), []));
test('a step of 3 passes, a step of 4 fails and names the pixel and channel', () => {
  assert.deepEqual(scanSeams(image(40, 60, (x, y) => [y < 30 ? 10 : 13, 0, 0])), []);
  const bad = scanSeams(image(40, 60, (x, y) => [0, y < 30 ? 10 : 14, 0]));
  assert.ok(bad.length > 0);
  assert.deepEqual(bad[0], { x: 8, y: 30, channel: 'g', from: 10, to: 14 });
});
test('the edge zone and transparent pixels are ignored', () => {
  const img = image(40, 60, (x, y) => [y < 5 ? 0 : 100, 0, 0]);
  assert.deepEqual(scanSeams(img), []);
  img.pixels[(30 * 40 + 20) * 4 + 3] = 0;
  assert.deepEqual(scanSeams(img), []);
});
