import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import { sitesOf, summaryTable } from './e2e-ready-report.mjs';

test('sitesOf keeps the sites of the ready-reads rule only', () => {
  const results = [{ filePath: '/r/a.ts', messages: [{ ruleId: 'paddock/e2e-ready-reads', line: 4, message: 'x' }, { ruleId: 'other', line: 9, message: 'y' }] }, { filePath: '/r/b.ts', messages: [] }];
  assert.deepEqual(sitesOf(results), [{ file: '/r/a.ts', line: 4, message: 'x' }]);
});

test('summaryTable lists each project and the total', () => {
  const t = summaryTable({ 'apps/debrief/app': 3, ui: 2 });
  assert.match(t, /\| apps\/debrief\/app \| 3 \|/);
  assert.match(t, /\| \*\*total \(measured projects\)\*\* \| \*\*5\*\* \|/);
  assert.match(t, /projects not measured\*\* \| \*\*0\*\*/);
});

test('summaryTable shows a project eslint failed in as not measured, never as zero sites (ER-33)', () => {
  const t = summaryTable({ ui: 2 }, ['apps/dyno/app', 'apps/pitwall/app']);
  assert.match(t, /\| apps\/dyno\/app \| not measured \|/);
  assert.match(t, /\| apps\/pitwall\/app \| not measured \|/);
  assert.match(t, /projects not measured\*\* \| \*\*2\*\*/);
  assert.doesNotMatch(t, /apps\/dyno\/app \| 0/);
});

test('the script measures a project whose plugin is registered only inside the e2e block (a --rule override cannot find it: "could not find plugin paddock")', () => {
  const out = execFileSync('node', ['scripts/e2e-ready-report.mjs', 'scripts/fixtures/e2e-ready'], { encoding: 'utf8', cwd: new URL('..', import.meta.url) });
  assert.match(out, /scripts\/fixtures\/e2e-ready\/e2e\/bad\.ts:6/, out);
  assert.match(out, /\| scripts\/fixtures\/e2e-ready \| 1 \|/);
  assert.match(out, /projects not measured\*\* \| \*\*0\*\*/);
});
