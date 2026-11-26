// Both CI audits (ci.yml's `quick` job: `dev audit compare`, nightly.yml's `audit` job: `dev audit nightly`) are report-only until the repository variable
// PADDOCK_AUDIT_ENFORCE is 1 (ER-41), and then they must pass `--enforce`. A step that stops adding it leaves the gate report-only for
// ever, with every job green (ER-33). The workflows are read through the line reader every conformance check uses (util/workflows.mjs,
// ER-42), and the step's own script is run with the variable set and unset, with `./dev` replaced by a function that prints its
// arguments, so the test judges what the step does, not how it is spelled.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { parseJobs, parseSteps } from '../src/util/workflows.mjs';
import { PADDOCK } from './helpers.mjs';

/** The step that runs `./dev audit` in the job `id` of a workflow: { job, run, env }, through the shared reader. */
export function auditStep(text, id = 'audit') {
  const job = parseJobs(text).find((j) => j.id === id);
  if (!job) return null;
  const step = parseSteps(job.text).find((s) => /\.\/dev audit\b/.test(s.run ?? ''));
  if (!step) return { job, run: null };
  return { job, run: step.run, jobEnv: /^\s*PADDOCK_AUDIT_ENFORCE:\s*\$\{\{\s*vars\.PADDOCK_AUDIT_ENFORCE\s*\}\}\s*$/m.test(job.text) };
}

/** The arguments `./dev` receives when the step's script runs with PADDOCK_AUDIT_ENFORCE = `value` (undefined: unset), or throws why it could not run. */
export function devArgs(run, value) {
  const env = { ...process.env, PADDOCK_BASE_REF: 'origin/main' };
  delete env.PADDOCK_AUDIT_ENFORCE;
  if (value !== undefined) env.PADDOCK_AUDIT_ENFORCE = value;
  const r = spawnSync('bash', ['-c', `./dev() { printf '%s\\n' "$@"; }\n${run}`], { encoding: 'utf8', env });
  if (r.error || r.status !== 0) throw new Error(`the audit step's script did not run: ${r.error?.message ?? r.stderr}`);
  return r.stdout.split('\n').filter(Boolean);
}

const WORKFLOWS = [
  ['.gitea/workflows/ci.yml', 'compare', 'quick'],
  ['.gitea/workflows/nightly.yml', 'nightly', 'audit'],
];

for (const [file, sub, id] of WORKFLOWS) {
  test(`${file}: the job ${id} runs \`./dev audit ${sub}\` and adds --enforce exactly when PADDOCK_AUDIT_ENFORCE is 1`, () => {
    const s = auditStep(readFileSync(join(PADDOCK, file), 'utf8'), id);
    assert.ok(s?.run, `${file} has a \`${id}\` job with a step that runs \`./dev audit\` (read through parseJobs and parseSteps)`);
    assert.ok(s.jobEnv, `${file}: the job ${id} passes the repository variable as PADDOCK_AUDIT_ENFORCE`);
    const on = devArgs(s.run, '1');
    assert.equal(on[0], 'audit');
    assert.equal(on[1], sub);
    assert.ok(on.includes('--enforce'), `enforced when the variable is 1: ${on}`);
    for (const off of [undefined, '', '0']) assert.ok(!devArgs(s.run, off).includes('--enforce'), `report-only when the variable is ${JSON.stringify(off)}`);
  });
}

test('control: a step that never adds --enforce, or adds it always, is seen', () => {
  const never = './dev audit nightly';
  assert.ok(!devArgs(never, '1').includes('--enforce'), 'the probe sees a step that ignores the variable');
  const always = './dev audit nightly --enforce';
  assert.ok(devArgs(always, undefined).includes('--enforce'), 'the probe sees a step that enforces always');
  assert.equal(auditStep('jobs:\n  other:\n    steps:\n      - run: ./dev audit nightly\n'), null, 'no audit job is none, never a pass');
});
