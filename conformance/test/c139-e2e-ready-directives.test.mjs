import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { entriesOf } from '../src/checks/C139-e2e-ready-directives.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const R = 'paddock/e2e-ready-reads';
const supp = (n, file = 'e2e/a.spec.ts') => JSON.stringify({ [file]: { [R]: { count: n } } });
const SPEC = { 'ui/e2e/a.spec.ts': 'const a = 1;\n' };
const sh = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: root, stdio: 'ignore' });
/** A repository whose merge base (`main`) has the ui suppressions `base` and whose `topic` commit has `now`. */
function repo(base, now) {
  const root = makeApp({ ...SPEC, 'ui/eslint-suppressions.json': base });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'base');
  sh(root, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(root, 'ui/eslint-suppressions.json'), now);
  sh(root, 'commit', '-q', '-a', '--allow-empty', '-m', 'topic');
  return root;
}
const run = (root, extra = {}) => findingsOf('C139', root, { options: { baseRef: 'main', ...extra } });

test('C139 entries: ESLint bulk suppressions, each entry counted `count` times; anything else is not that shape', () => {
  assert.deepEqual(entriesOf({ 'e2e/a.ts': { [R]: { count: 2 }, 'no-debugger': { count: 1 } } }).length, 3);
  assert.deepEqual(entriesOf({}), []);
  for (const bad of [[], null, 'x', { a: [] }, { a: { r: {} } }, { a: { r: { count: 0 } } }, { a: { r: { count: 1.5 } } }, { a: { r: { count: '1' } } }]) assert.equal(entriesOf(bad), null, JSON.stringify(bad));
});

test('C139 growth: an unchanged or shrunk list says nothing, a raised count, a new rule and a new file are one unwaivable finding at the file', async () => {
  assert.deepEqual(await run(repo(supp(2), supp(2))), []);
  assert.deepEqual(await run(repo(supp(2), supp(1))), [], 'a lowered count is shrinking');
  const raised = await run(repo(supp(1), supp(2)));
  assert.deepEqual(raised.map((f) => [f.file, f.line, f.severity, f.unwaivable]), [['ui/eslint-suppressions.json', 1, 'error', true]]);
  assert.match(raised[0].message, /the exception list grew: 1 entry is \(counting each `count`\) not in ui\/eslint-suppressions\.json at the merge base [0-9a-f]{9}; it may only shrink/);
  assert.match(raised[0].fix, /user's approval/);
  const added = await run(repo(supp(1), JSON.stringify({ 'e2e/a.spec.ts': { [R]: { count: 1 } }, 'e2e/b.spec.ts': { [R]: { count: 2 } } })));
  assert.match(added[0].message, /grew: 2 entries are/);
});

test('C139 growth: a file new at the merge base, and a checkout where git cannot give a base, are said out loud and never a pass (ER-33)', async () => {
  const first = makeApp(SPEC);
  sh(first, 'init', '-q', '-b', 'main');
  sh(first, 'add', '.');
  sh(first, 'commit', '-q', '-m', 'base: no list yet');
  sh(first, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(first, 'ui/eslint-suppressions.json'), supp(3));
  sh(first, 'add', '.');
  sh(first, 'commit', '-q', '-m', 'the list is introduced');
  const intro = await run(first);
  assert.deepEqual(intro.map((f) => [f.file, f.severity, f.unwaivable]), [['ui/eslint-suppressions.json', 'error', true]], 'a list new at the merge base is an empty base: its 3 entries are growth');
  assert.match(intro[0].message, /grew: 3 entries are/);
  // The same for a list in another app (a new apps/pitwall/app/eslint-suppressions.json with count 5 gave no finding).
  const other = makeApp(SPEC);
  sh(other, 'init', '-q', '-b', 'main');
  sh(other, 'add', '.');
  sh(other, 'commit', '-q', '-m', 'base');
  sh(other, 'checkout', '-q', '-b', 'topic');
  mkdirSync(join(other, 'apps/pitwall/app'), { recursive: true });
  writeFileSync(join(other, 'apps/pitwall/app/eslint-suppressions.json'), supp(5));
  sh(other, 'add', '.');
  sh(other, 'commit', '-q', '-m', 'a new exception list');
  const pw = await run(other);
  assert.match(pw[0].message, /grew: 5 entries are/);
  const w = await run(repo(supp(1), supp(1)), { baseRef: 'no-such-ref' });
  assert.equal(w.length, 1);
  assert.match(w[0].message, /shrink-only rule is not enforced here/);
  const inCi = await run(repo(supp(1), supp(1)), { baseRef: 'no-such-ref', ci: true });
  assert.deepEqual(inCi.map((f) => [f.severity, f.outcome, f.unwaivable]), [['error', 'not-measured', true]], 'in CI an unusable target ref is a not-measured error (ADR 0089)');
});

test('C139 root run: no e2e spec read is not measured, never a pass (ER-33)', async () => {
  const found = await run(makeApp({ 'ui/eslint-suppressions.json': supp(1) }));
  assert.deepEqual(found.map((f) => [f.severity, f.outcome, f.unwaivable]), [['error', 'not-measured', true]]);
  assert.match(found[0].message, /read no e2e spec/);
  for (const hidden of ['ui/e2e/x-snapshots/a.ts', 'ui/e2e/node_modules/a.ts', 'ui/src/a.ts', 'ui/e2e/a.js']) {
    assert.equal((await run(makeApp({ [hidden]: 'x\n', 'ui/eslint-suppressions.json': supp(1) }))).length, 1, `${hidden} is not an e2e spec`);
  }
});

test('C139 files: a malformed suppressions file is one check-broken finding at it; a file outside the app and ui folders is not read', async () => {
  const found = await run(makeApp({ ...SPEC, 'ui/eslint-suppressions.json': '{"e2e/a.spec.ts":{"r":{"count":0}}}', 'apps/dyno/app/eslint-suppressions.json': 'nope', 'ui/src/eslint-suppressions.json': 'nope' }));
  assert.deepEqual(found.map((f) => [f.file, f.outcome, f.unwaivable]), [['apps/dyno/app/eslint-suppressions.json', 'check-broken', true], ['ui/eslint-suppressions.json', 'check-broken', true]]);
});
