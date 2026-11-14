import { test } from 'node:test';
import assert from 'node:assert/strict';
import { testArgs } from './run-tests.mjs';

// The push gate runs these tests beside Clippy rows; without a cap `node --test` starts one process per core (ER-24). The cap is
// PADDOCK_TEST_CONCURRENCY, and it must come before the file patterns: after them, node ignores the flag (measured 2026-11-15).
const ARGV = ['--test', 'test/a.test.mjs', 'test/b.test.mjs'];

test('no PADDOCK_TEST_CONCURRENCY: node chooses, as before', () => {
  assert.deepEqual(testArgs(ARGV, {}), ARGV);
  assert.deepEqual(testArgs(ARGV, { PADDOCK_TEST_CONCURRENCY: '' }), ARGV);
});

test('PADDOCK_TEST_CONCURRENCY caps the test processes, before the patterns', () => {
  assert.deepEqual(testArgs(ARGV, { PADDOCK_TEST_CONCURRENCY: '4' }), ['--test', '--test-concurrency=4', 'test/a.test.mjs', 'test/b.test.mjs']);
});

test('a value that is not a positive whole number fails with what to set, never runs uncapped', () => {
  for (const v of ['0', '-1', 'abc', '2.5', '4 ']) {
    assert.throws(() => testArgs(ARGV, { PADDOCK_TEST_CONCURRENCY: v }), /PADDOCK_TEST_CONCURRENCY must be a positive whole number/, v);
  }
});

test('the arguments start with --test (the patterns stay in package.json, where test-files-run reads them)', () => {
  assert.throws(() => testArgs(['test/a.test.mjs'], {}), /usage: node test\/run-tests.mjs --test/);
});
