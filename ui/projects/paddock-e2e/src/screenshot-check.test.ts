import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rawTextProblems, statusChipProblems, type VisibleText } from './screenshot-check.ts';

const at = (text: string): VisibleText => ({ text, where: 'span.x' });
const flagged = (text: string, allow?: (string | RegExp)[]) => rawTextProblems([at(text)], allow).length > 0;

test('raw enum and identifier text is flagged', () => {
  for (const t of ['READ_ONLY', 'Status: IN_PROGRESS', 'pending_review', 'pdfViewer', 'Mode: isEnabled', 'undefined', 'Size: NaN MB', 'null', 'Name: [object Object]']) {
    assert.equal(flagged(t), true, t);
  }
});

test('words, product names, units and sentences are not flagged', () => {
  for (const t of ['Read only', 'In progress', 'iPhone', 'macOS 15', 'JavaScript', 'OK', 'AB', 'C:\\Users', 'file_name.txt is open', 'Nothing to show', 'Strokes use currentColor in a 24 px grid']) {
    assert.equal(flagged(t), false, t);
  }
});

test('mock, lorem, TODO and debug strings are flagged', () => {
  for (const t of ['Mock data', 'Lorem ipsum dolor', 'TODO: wire this', 'FIXME', '[debug] state', 'DEBUG', 'debug: on']) assert.equal(flagged(t), true, t);
  assert.equal(flagged('Debug logging'), false, 'a setting called Debug logging is fine');
  assert.equal(flagged('Mockingbird'), false);
});

test('an allowed text is not flagged, by string or pattern', () => {
  assert.equal(flagged('READ_ONLY', ['READ_ONLY']), false);
  assert.equal(flagged('Mock data', [/^Mock data$/]), false);
});

test('messages name the kind, the text and where', () => {
  assert.deepEqual(rawTextProblems([at('IN_PROGRESS'), at('IN_PROGRESS')]), ['raw identifier (SNAKE_CASE) shown: "IN_PROGRESS" in span.x']);
});

test('a status chip needs a word or an icon', () => {
  const chips = [
    { description: 'a', text: 'Healthy', hasIcon: true },
    { description: 'b', text: '', hasIcon: true },
    { description: 'c', text: 'Down', hasIcon: false },
    { description: 'd', text: '', hasIcon: false },
  ];
  assert.deepEqual(statusChipProblems(chips), ['status chip without a word or icon (colour alone): d']);
});
