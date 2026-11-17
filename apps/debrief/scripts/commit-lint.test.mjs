// Wiring only: the rules are tested once, in conformance/test/c23-commit-lint.test.mjs (rule 55, ER-22).
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lint as canonical } from '../../../conformance/src/commit-lint.mjs';
import { lint } from './commit-lint.mjs';

test('is the shared lint, not a copy', () => {
  assert.equal(lint, canonical);
});

test('judges a message the way the shared lint does (Conventional Commits, attribution, re-land line)', () => {
  assert.deepEqual(lint('feat: add calculated fields'), []);
  assert.match(lint('added stuff')[0], /does not start with/);
  assert.match(lint('fix: x\n\nCo-Authored-By: A <a@b.c>').join(), /Co-Authored-By/);
  assert.match(lint('revert: reapply x').join(), /This reverts commit/);
});
