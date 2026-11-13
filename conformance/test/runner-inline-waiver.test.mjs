import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runChecks } from '../src/runner.mjs';
import { error, unwaivable, warn } from '../src/finding.mjs';
import { makeApp } from './helpers.mjs';

const check = (id, found) => ({ id, rule: 1, title: id, scope: 'both', mode: 'report-only', run: () => found });

test('an inline-waived finding is counted as waived with its reason; an unwaivable one is never waived by a comment', async () => {
  const root = makeApp({});
  const kept = { ...warn('X1', 1, 'a.rs', 1, 'm', 'f'), inlineWaiver: 'a reason here' };
  const broken = { ...unwaivable(error('X2', 1, 'b.rs', 1, 'it could not look', 'fix')), inlineWaiver: 'a comment cannot cover this' };
  const r = await runChecks({ root, app: 'x', registry: [check('X1', [kept]), check('X2', [broken])] });
  assert.deepEqual(r.waived.map((w) => [w.check, w.waiver]), [['X1', { reason: 'a reason here', expires: 'inline' }]]);
  assert.ok(!('inlineWaiver' in r.waived[0]));
  assert.deepEqual(r.findings.map((f) => f.check), ['X2'], 'the unwaivable finding stays a finding');
  assert.equal(r.errors, 1);
  assert.equal(r.warnings, 0);
});
