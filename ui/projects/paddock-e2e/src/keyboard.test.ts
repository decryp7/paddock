import assert from 'node:assert/strict';
import { test } from 'node:test';
import { walkStep } from './keyboard.ts';

test('a walk goes on while every stop is new', () => {
  assert.equal(walkStep([], 'a'), 'continue');
  assert.equal(walkStep(['a'], 'b'), 'continue');
  assert.equal(walkStep(['a', 'b', 'c'], 'd'), 'continue');
});

test('the focus leaving the page’s elements ends the walk cleanly', () => {
  assert.equal(walkStep([], null), 'left');
  assert.equal(walkStep(['a', 'b'], null), 'left');
});

test('coming back to the first stop is a cycle, as in a dialog or a menu', () => {
  assert.equal(walkStep(['a', 'b'], 'a'), 'cycled');
  assert.equal(walkStep(['a', 'b', 'c', 'd'], 'a'), 'cycled');
});

test('a stop that does not move, or a loop that skips the first stop, is a trap', () => {
  assert.equal(walkStep(['a'], 'a'), 'trapped');
  assert.equal(walkStep(['a', 'b'], 'b'), 'trapped');
  assert.equal(walkStep(['a', 'b', 'c'], 'b'), 'trapped');
  assert.equal(walkStep(['a', 'b', 'c', 'd'], 'c'), 'trapped');
});
