import assert from 'node:assert/strict';
import { test } from 'node:test';
import { baselineGrowth, enforceShrinkOnly, growthDigest, reconcile } from '../src/util/baseline.mjs';

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

test('growth: a baseline absent at the merge base is an empty base, so every entry is growth (ER-46); no merge base or a broken file is not measured, never a pass', () => {
  assert.deepEqual(baselineGrowth(opts(null, ['x', 'x', 'y'])).added, ['x', 'x', 'y'], 'a new list with entries is growth, however many');
  assert.deepEqual(baselineGrowth(opts(null, [])).added, [], 'a new empty list adds nothing');
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

test('reconcile: the exact key first, then the loose key; counts per key', () => {
  const exact = (e) => `${e.f}:${e.h}`;
  const loose = (e) => e.h;
  const a = { f: 'a', h: 1 };
  assert.deepEqual(reconcile([{ f: 'b', h: 1 }], [a], exact, loose), { fresh: [], stale: [], moved: [{ item: { f: 'b', h: 1 }, from: a }] });
  assert.equal(reconcile([{ f: 'b', h: 1 }], [a], exact).fresh.length, 1, 'without a loose key a move is new');
  assert.equal(reconcile([{ f: 'a', h: 1 }, { f: 'b', h: 1 }], [a], exact, loose).fresh.length, 1, 'a copy is new');
});

const gates = (check, by, count, digest) => `version = 1\n[[register_growth]]\ncheck = "${check}"\nby = "${by}"\ndate = "2026-11-27"\ncount = ${count}\ndigest = "${digest}"\n`;

test('growth approval: the digest is the SHA-256 of the sorted keys joined by a newline (the oracle is shasum of "a\\nb")', () => {
  assert.equal(growthDigest(['b', 'a']), '7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78');
});

test('growth approval: the user\'s matching entry drops the additions; any other entry keeps them and prints the line to add', () => {
  const base = JSON.stringify({ jobs: ['a'] });
  const run = (register) => baselineGrowth({ ...opts(base, ['a', 'b', 'c']), check: 'CX', register });
  const keys = ['b', 'c'];
  const right = growthDigest(keys);
  assert.deepEqual(run(gates('CX', 'user', 2, right)).added, [], 'approved');
  for (const [name, register] of [
    ['no register', ''],
    ['other additions', gates('CX', 'user', 2, growthDigest(['b', 'z']))],
    ['wrong count', gates('CX', 'user', 3, right)],
    ['not the user', gates('CX', 'coordinator', 2, right)],
    ['another check', gates('CY', 'user', 2, right)],
  ]) {
    const g = run(register);
    assert.deepEqual(g.added, keys, name);
    assert.match(g.ask, new RegExp(`check = "CX", by = "user", date = "<day>", count = 2, digest = "${right}"`), name);
  }
  // Without a check id nothing is read: the growth is returned as measured.
  assert.deepEqual(baselineGrowth({ ...opts(base, ['a', 'b']), register: gates('CX', 'user', 1, growthDigest(['b'])) }).added, ['b']);
});
