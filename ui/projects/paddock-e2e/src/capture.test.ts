import assert from 'node:assert/strict';
import { test } from 'node:test';
import { problemsOf, type PageProbe } from './capture.ts';

const calm: PageProbe = { busy: [], undecoded: [], animating: [], frameScrolled: false, cutOff: [], overlays: [], layout: '', hiddenHeight: 0 };

test('a calm page has no problems', () => {
  assert.deepEqual(problemsOf(calm, 'viewport'), []);
});

test('every finding of the reviews is named', () => {
  const p = problemsOf({ ...calm, busy: ['<pd-skeleton>'], undecoded: ['<img>'], animating: ['CSSAnimation'], frameScrolled: true, cutOff: ['<p>'], overlays: ['<div>'] }, 'viewport');
  assert.equal(p.length, 6);
  assert.match(p.join('\n'), /still loading[^]*not decoded[^]*animations[^]*frame was scrolled[^]*cut off[^]*overlay/);
});

test('a whole-main shot does not mind the bottom edge, but still refuses the rest', () => {
  assert.deepEqual(problemsOf({ ...calm, cutOff: ['<p>'] }, 'main'), []);
  assert.equal(problemsOf({ ...calm, cutOff: ['<p>'], overlays: ['<div>'] }, 'main').length, 1);
});
