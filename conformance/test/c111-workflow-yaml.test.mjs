import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { findingsOf, makeApp } from './helpers.mjs';

const GOOD = 'name: ci\non: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo hi\n        if: ${{ !cancelled() }}\n';
// The 2026-11-12 incident: an unquoted `!cancelled()` is a YAML tag, not the expression.
const BANG = 'name: ci\non: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo hi\n        if: !cancelled()\n';

test('C111: workflows and actions that parse pass', async () => {
  const root = makeApp({
    '.gitea/workflows/ci.yml': GOOD,
    '.gitea/actions/x/action.yml': "name: x\nruns:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      if: '!cancelled()'\n",
  });
  assert.deepEqual(await findingsOf('C111', root), []);
});

test('C111: an unquoted `if: !cancelled()` fails with file, line, message and the next step', async () => {
  const root = makeApp({ '.gitea/workflows/ci.yml': BANG });
  const f = await findingsOf('C111', root);
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].file, '.gitea/workflows/ci.yml');
  assert.equal(f[0].line, 8);
  assert.match(f[0].message, /Unresolved tag: !cancelled\(\)/);
  assert.match(f[0].fix, /quoted/);
  assert.match(f[0].fix, /\$\{\{ !cancelled\(\) \}\}/);
});

test('C111: one error per problem, in workflows and in nested actions, and a duplicate key counts', async () => {
  const root = makeApp({
    '.gitea/workflows/a.yml': 'a: [1\n',
    '.gitea/workflows/b.yaml': BANG,
    '.gitea/actions/deep/er/action.yml': 'x: 1\nx: 2\n',
    '.gitea/workflows/ok.yml': GOOD,
  });
  const f = await findingsOf('C111', root);
  assert.deepEqual(f.map((x) => x.file).sort(), ['.gitea/actions/deep/er/action.yml', '.gitea/workflows/a.yml', '.gitea/workflows/b.yaml']);
  assert.equal(f.every((x) => x.severity === 'error' && x.line >= 1), true);
});

test('C111: a tree with nothing to parse is "check broken", an error no waiver covers, never a pass', async () => {
  const f = await findingsOf('C111', makeApp({ 'README.md': 'x' }));
  assert.equal(f.length, 1);
  assert.equal(f[0].unwaivable, true);
  assert.match(f[0].message, /check broken/);
});

test('C111: every workflow and action of this very repository parses', async () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
  const f = await findingsOf('C111', root);
  assert.deepEqual(f.map((x) => `${x.file}:${x.line} ${x.message}`), []);
});
