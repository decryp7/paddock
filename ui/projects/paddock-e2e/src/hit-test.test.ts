import assert from 'node:assert/strict';
import { test } from 'node:test';
import { formatCovered, INTERACTIVE_SELECTOR } from './hit-test.ts';

test('an empty list reads as the passing state', () => {
  assert.equal(formatCovered([]), 'nothing covers an interactive element');
});

test('each finding gets a line naming the element, its cover and where it was seen', () => {
  const text = formatCovered([
    { name: 'Save', selector: 'button.btn', coveredBy: 'div.tip', at: 'rest' },
    { name: '', selector: 'a', coveredBy: 'span', at: 'div.scroll scrolled to 40/0' },
  ]);
  assert.match(text, /^2 interactive elements are covered:/);
  assert.match(text, /- "Save" button\.btn is covered by div\.tip \(rest\)/);
  assert.match(text, /- a is covered by span \(div\.scroll scrolled to 40\/0\)/);
  assert.match(formatCovered([{ name: 'x', selector: 'b', coveredBy: 'c', at: 'rest' }]), /^1 interactive element is covered:/);
});

test('the candidate list names every interactive role of the brief', () => {
  for (const part of ['button', 'a[href]', 'input', '[role="tab"]', '[role="menuitem"]', '[role="option"]', '[role="checkbox"]', '[role="switch"]', '[role="slider"]', '[tabindex]']) {
    assert.ok(INTERACTIVE_SELECTOR.includes(part), part);
  }
});
