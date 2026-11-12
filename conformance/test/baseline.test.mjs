import assert from 'node:assert/strict';
import { test } from 'node:test';
import { baselineGrowth, enforceShrinkOnly } from '../src/util/baseline.mjs';

const gitWith = (base) => (args) => {
  if (args[0] === 'rev-parse') return args[1] === 'HEAD' ? 'def\n' : 'abc\n';
  if (args[0] === 'merge-base') return 'abc\n';
  if (args[0] === 'show') {
    if (base === null) throw Object.assign(new Error('fatal: path does not exist'), { stderr: 'fatal: path does not exist' });
    return base;
  }
  throw new Error(`unexpected git ${args.join(' ')}`);
};
const opts = (base, current) => ({ root: '.', path: 'b.json', current, keyOf: (k) => k, entriesOf: (j) => j.jobs, git: gitWith(base) });

test('growth: entries the merge base lacks are added; a second copy of a known entry is added; shrinking and equal are fine', () => {
  const base = JSON.stringify({ jobs: ['a', 'b'] });
  assert.deepEqual(baselineGrowth(opts(base, ['a', 'b'])).added, []);
  assert.deepEqual(baselineGrowth(opts(base, ['a'])).added, []);
  assert.deepEqual(baselineGrowth(opts(base, ['a', 'b', 'c'])).added, ['c']);
  assert.deepEqual(baselineGrowth(opts(base, ['a', 'a', 'b'])).added, ['a']);
});

test('growth: no baseline at the merge base is the first introduction; no merge base or a broken file is not measured, never a pass', () => {
  assert.deepEqual(baselineGrowth(opts(null, ['x'])).added, []);
  const noGit = baselineGrowth({ ...opts('{}', []), git: () => { throw new Error('not a git repository'); } });
  assert.match(noGit.reason, /no merge base/);
  assert.match(baselineGrowth(opts('not json', [])).reason, /cannot be parsed/);
});

test('enforceShrinkOnly: added entries throw; fixed entries only warn', () => {
  const base = JSON.stringify({ jobs: ['a'] });
  assert.throws(() => enforceShrinkOnly(opts(base, ['a', 'z'])), /grew.*z/);
  assert.doesNotThrow(() => enforceShrinkOnly({ ...opts(base, []), gone: ['a'] }));
  assert.doesNotThrow(() => enforceShrinkOnly({ ...opts(base, ['a']), git: () => { throw new Error('x'); } }));
});
