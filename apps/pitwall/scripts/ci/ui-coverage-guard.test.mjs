import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lineCoverage } from './ui-coverage-guard.mjs';

test('sums lines found and hit over all files', () => {
  assert.equal(lineCoverage('SF:a\nLF:10\nLH:8\nend_of_record\nSF:b\nLF:10\nLH:10\nend_of_record\n'), 90);
});

test('a report without lines has no coverage figure', () => {
  assert.equal(lineCoverage('TN:\n'), null);
});
