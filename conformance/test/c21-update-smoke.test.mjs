import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const GOOD = {
  'scripts/shell-demo/smoke-service-update.sh': '#!/bin/bash\n',
  'tools/vm-smoke.sh': '#!/bin/bash\n',
  'docs/blocks/releasing.md': '2. Before tagging: run `tools/vm-smoke.sh macos`.\n',
  '.gitea/workflows/ci.yml': 'name: ci\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n',
};

test('C21 good: runner, checklist step and no workflow; repos without the smoke are not asked', async () => {
  assert.deepEqual(await findingsOf('C21', makeApp(GOOD)), []);
  assert.deepEqual(await findingsOf('C21', makeApp({ 'README.md': 'x' })), []);
});

test('C21 bad: a missing runner, a missing or silent checklist', async () => {
  const { 'tools/vm-smoke.sh': _, ...noRunner } = GOOD;
  assert.match(messages(await findingsOf('C21', makeApp(noRunner))), /vm-smoke\.sh is missing/);
  const { 'docs/blocks/releasing.md': __, ...noList } = GOOD;
  assert.match(messages(await findingsOf('C21', makeApp(noList))), /releasing\.md is missing/);
  const silent = { ...GOOD, 'docs/blocks/releasing.md': '2. tag\n' };
  assert.match(messages(await findingsOf('C21', makeApp(silent))), /does not ask for the local VM update smoke/);
});

test('C21 bad: a workflow that runs the VM smoke (a comment mentioning it is fine)', async () => {
  const wf = '# see tools/vm-smoke.sh\nname: x\njobs:\n  a:\n    runs-on: macos-latest\n    steps:\n      - run: bash tools/vm-smoke.sh macos\n';
  const f = await findingsOf('C21', makeApp({ ...GOOD, '.gitea/workflows/service-smoke.yml': wf }));
  assert.equal(f.length, 1);
  assert.match(messages(f), /CI has no VMs/);
  for (const run of ['bash tools/macos-vm-smoke.sh --old a --new b', 'tart clone img vm']) {
    const direct = `name: x\njobs:\n  a:\n    runs-on: macos-latest\n    steps:\n      - run: ${run}\n`;
    assert.equal((await findingsOf('C21', makeApp({ ...GOOD, '.gitea/workflows/z.yml': direct }))).length, 1, run);
  }
  const commentOnly = '# tools/vm-smoke.sh runs locally\nname: x\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n';
  assert.deepEqual(await findingsOf('C21', makeApp({ ...GOOD, '.gitea/workflows/y.yml': commentOnly })), []);
});
