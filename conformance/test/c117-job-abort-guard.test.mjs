import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf as run, makeApp } from './helpers.mjs';

const findingsOf = (id, root) => run(id, root, { app: 'paddock' });
const G = "env.PADDOCK_JOB_ABORTED != '1'";
const job = (name, steps) => `  ${name}:\n    runs-on: x\n    steps:\n${steps}`;
const SETUP = '      - uses: ./.gitea/actions/build-jobs\n      - uses: ./.gitea/actions/job-isolation\n';
const wf = (...jobs) => `name: ci\non: push\njobs:\n${jobs.join('')}`;
const ACTION = (cond) => `name: x\nruns:\n  using: composite\n  steps:\n    - run: echo\n      shell: bash\n      if: \${{ ${cond} }}\n`;

test('C117: guarded steps and steps without !cancelled() pass', async () => {
  const root = makeApp({
    '.gitea/workflows/ci.yml': wf(
      job('a', `${SETUP}      - run: echo a\n        if: \${{ !cancelled() && ${G} }}\n      - run: echo b\n      - run: echo c\n        if: always()\n`),
    ),
  });
  assert.deepEqual(await findingsOf('C117', root), []);
});

test('C117: an unguarded !cancelled() step in a job with build-jobs or job-isolation is a warning with file, line and next step', async () => {
  const root = makeApp({
    '.gitea/workflows/ci.yml': wf(
      job('a', `${SETUP}      - run: echo a\n        if: \${{ !cancelled() }}\n`),
      job('b', `      - uses: ./.gitea/actions/job-isolation\n      - run: echo b\n        if: \${{ !cancelled() && runner.os == 'Linux' }}\n`),
    ),
  });
  const f = await findingsOf('C117', root);
  assert.equal(f.length, 2);
  assert.ok(f.every((x) => x.severity === 'warn' && x.file === '.gitea/workflows/ci.yml'), JSON.stringify(f));
  assert.deepEqual(f.map((x) => x.line), [10, 16]);
  assert.match(f[0].message, /job a/);
  assert.match(f[0].fix, /PADDOCK_JOB_ABORTED != '1'/);
});

test('C117: a job without the setup steps, and a job-level if, are not read', async () => {
  const root = makeApp({
    '.gitea/workflows/ci.yml': wf(
      job('a', `${SETUP}`),
      job('plain', '      - run: echo p\n        if: ${{ !cancelled() }}\n'),
    ).replace('  plain:\n', "  plain:\n    if: ${{ !cancelled() }}\n"),
  });
  assert.deepEqual(await findingsOf('C117', root), []);
});

test('C117: the composite action an isolated job uses is held to the rule, another action is not', async () => {
  const root = makeApp({
    '.gitea/workflows/ci.yml': wf(job('a', `${SETUP}      - uses: ./.gitea/actions/gates\n`)),
    '.gitea/actions/gates/action.yml': ACTION('!cancelled()'),
    '.gitea/actions/other/action.yml': ACTION('!cancelled()'),
  });
  const f = await findingsOf('C117', root);
  assert.equal(f.length, 1);
  assert.equal(f[0].file, '.gitea/actions/gates/action.yml');
  const ok = makeApp({
    '.gitea/workflows/ci.yml': wf(job('a', `${SETUP}      - uses: ./.gitea/actions/gates\n`)),
    '.gitea/actions/gates/action.yml': ACTION(`!cancelled() && ${G}`),
  });
  assert.deepEqual(await findingsOf('C117', ok), []);
});

test('C117: no workflow, or no isolated job at all, is check broken and never a pass', async () => {
  const none = await findingsOf('C117', makeApp({ 'README.md': 'x' }));
  assert.equal(none.length, 1);
  assert.equal(none[0].unwaivable, true);
  assert.match(none[0].message, /check broken/);
  const blind = await findingsOf('C117', makeApp({ '.gitea/workflows/ci.yml': wf(job('a', '      - run: echo\n')) }));
  assert.equal(blind.length, 1);
  assert.match(blind[0].message, /check broken/);
});
