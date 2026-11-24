import assert from 'node:assert/strict';
import { test } from 'node:test';
import C167 from '../src/checks/C167-growth-test-history.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const wf = (job) => `name: x\non: push\njobs:\n${job}`;
const CHECKOUT = (extra = '') => `      - uses: actions/checkout@v4\n        with:\n          persist-credentials: false\n${extra}`;
const job = ({ checkout = CHECKOUT('          fetch-depth: 0\n'), env = '    env:\n      PADDOCK_BASE_REF: ${{ inputs.base || \'origin/main\' }}\n', run = 'npm run test:scripts' } = {}) =>
  `  gates:\n    runs-on: x\n${env}    steps:\n${checkout}      - name: t\n        run: ${run}\n`;
const at = (text) => ({ '.gitea/workflows/demo.yml': wf(text) });

test('C167 good: full history and PADDOCK_BASE_REF in the job env', async () => {
  assert.deepEqual(await C167.run(ctxOf(at(job()))), []);
});

test('C167 good: PADDOCK_BASE_REF on the step that runs the tests', async () => {
  const text = `  gates:\n    runs-on: x\n    steps:\n${CHECKOUT('          fetch-depth: 0\n')}      - name: t\n        env:\n          PADDOCK_BASE_REF: origin/main\n        run: npm run test:scripts\n`;
  assert.deepEqual(await C167.run(ctxOf(at(text))), []);
});

test('C167 bad (positive control, run 8807): a one-commit checkout', async () => {
  const f = await C167.run(ctxOf(at(job({ checkout: CHECKOUT() }))));
  assert.equal(f.length, 1);
  assert.equal(f[0].file, '.gitea/workflows/demo.yml');
  assert.match(f[0].message, /fetch-depth: 0/);
  assert.equal((await C167.run(ctxOf(at(job({ checkout: CHECKOUT('          fetch-depth: 1\n') }))))).length, 1);
});

test('C167 bad (positive control, run 8807): no PADDOCK_BASE_REF', async () => {
  const f = await C167.run(ctxOf(at(job({ env: '' }))));
  assert.equal(f.length, 1);
  assert.match(f[0].message, /PADDOCK_BASE_REF/);
});

test('C167 bad: both missing is one finding naming both', async () => {
  const f = await C167.run(ctxOf(at(job({ env: '', checkout: CHECKOUT() }))));
  assert.equal(f.length, 1);
  assert.match(f[0].message, /fetch-depth: 0.*PADDOCK_BASE_REF|PADDOCK_BASE_REF.*fetch-depth: 0/);
});

test('C167 bad: a job with no checkout step cannot have the history', async () => {
  assert.equal((await C167.run(ctxOf(at(job({ checkout: '' }))))).length, 1);
});

test('C167 the tests run through ./dev ci run: the project contract says which phase runs them', async () => {
  const files = {
    'ui/paddock.ci.json': '{"scripts":["cd ui","npm run test:scripts"],"build":["cd ui","npm run build"]}',
    ...at(job({ env: '', run: './dev ci run ui scripts' })),
  };
  assert.equal((await C167.run(ctxOf(files))).length, 1);
  assert.deepEqual(await C167.run(ctxOf({ ...files, '.gitea/workflows/demo.yml': wf(job({ env: '', run: './dev ci run ui build' })) })), [], 'a phase that does not run them');
  assert.deepEqual(await C167.run(ctxOf({ ...files, '.gitea/workflows/demo.yml': wf(job({ run: './dev ci run ui scripts' })) })), []);
});

test('C167 a comment that names the script is not a run of it', async () => {
  assert.deepEqual(await C167.run(ctxOf(at(job({ env: '', checkout: CHECKOUT(), run: '|\n          # npm run test:scripts\n          npm run build' })))), []);
});

test('C167 check broken, not a pass, when no workflow was read or one does not parse', async () => {
  const none = await C167.run(ctxOf({}));
  assert.equal(none.length, 1);
  assert.equal(none[0].unwaivable, true);
  const broken = await C167.run(ctxOf({ '.gitea/workflows/a.yml': 'jobs: [\n' }));
  assert.equal(broken.length, 1);
  assert.equal(broken[0].unwaivable, true);
});
