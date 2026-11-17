import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sitesOf, summaryTable } from './e2e-ready-report.mjs';

test('sitesOf keeps the sites of the ready-reads rule only', () => {
  const results = [{ filePath: '/r/a.ts', messages: [{ ruleId: 'paddock/e2e-ready-reads', line: 4, message: 'x' }, { ruleId: 'other', line: 9, message: 'y' }] }, { filePath: '/r/b.ts', messages: [] }];
  assert.deepEqual(sitesOf(results), [{ file: '/r/a.ts', line: 4, message: 'x' }]);
});

test('summaryTable lists each project and the total', () => {
  const t = summaryTable({ 'apps/debrief/app': 3, ui: 2 });
  assert.match(t, /\| apps\/debrief\/app \| 3 \|/);
  assert.match(t, /\| \*\*total\*\* \| \*\*5\*\* \|/);
});
