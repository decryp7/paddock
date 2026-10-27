import { test } from 'node:test';
import assert from 'node:assert/strict';
import { inlineElements } from './check-index.mjs';

test('inline styles and scripts are found', () => {
  const html = '<head><style>:root{}</style><link rel="stylesheet" href="s.css"></head><body><script>x()</script></body>';
  assert.deepEqual(inlineElements(html), ['<style>', '<script>']);
});

test('external scripts and stylesheets pass', () => {
  const html = '<link rel="stylesheet" href="s.css"><script src="main.js" type="module"></script>';
  assert.deepEqual(inlineElements(html), []);
});
