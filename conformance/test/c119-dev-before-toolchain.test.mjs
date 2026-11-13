import assert from 'node:assert/strict';
import { test } from 'node:test';
import { devBeforeToolchain, toolchainActions } from '../src/checks/C119-dev-before-toolchain.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const setup = toolchainActions([['kept', 'run: bash conformance/ci/kit.sh host x'], ['plain', 'run: echo']]);
const lines = (text) => devBeforeToolchain(text, setup).map((e) => e.line);

test('C119 bad: a job that runs ./dev before any toolchain setup, once per job, in a one-line or a block run', () => {
  assert.deepEqual(lines('jobs:\n  a:\n    steps:\n      - uses: actions/checkout@v4\n      - run: ./dev x\n      - run: ./dev y\n'), [5]);
  assert.deepEqual(lines('jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/plain\n      - name: n\n        run: |\n          set -e\n          ./dev x\n'), [8]);
});

test('C119 good: setup-rust, or an action that goes through kit.sh, or kit.sh itself, comes first', () => {
  assert.deepEqual(lines('jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n      - run: ./dev x\n'), []);
  assert.deepEqual(lines('jobs:\n  a:\n    steps:\n      - uses: ./.gitea/actions/kept\n      - run: ./dev x\n'), []);
  assert.deepEqual(lines('jobs:\n  a:\n    steps:\n      - run: bash conformance/ci/kit.sh which\n      - run: ./dev x\n'), []);
});

test('C119: a waiver needs a reason; the bare comment is a finding that says so', async () => {
  const wf = (c) => `name: x\njobs:\n  a:\n    steps:\n      # ${c}\n      - run: ./dev x\n`;
  const root = makeApp({ '.gitea/workflows/ci.yml': wf('conformance: dev-before-toolchain — the image has the only compiler'), '.gitea/workflows/b.yml': wf('conformance: dev-before-toolchain') });
  const f = await findingsOf('C119', root, { app: 'paddock' });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn', 'report-only (ER-41)');
  assert.equal(f[0].file, '.gitea/workflows/b.yml');
  assert.match(f[0].message, /waiver comment has no reason/);
});

test('C119: no workflow file is check broken, never a pass', async () => {
  const f = await findingsOf('C119', makeApp({ 'README.md': 'x' }), { app: 'paddock' });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].unwaivable, true);
});
