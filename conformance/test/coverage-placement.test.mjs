// ADR 0116 D4 (amended 2026-11-26, user): coverage runs only in the nightly (os_full), a release's gates and a tag. A train, a branch and a
// push to main run plain nextest, so the Linux tests of the `crate` job are cacheable and the coverage run (which is not) is out of the way.
// The same three conditions are plan_ci::coverage_run (paddock-dev), which decides whether the plan's Linux crate leg tests are cacheable.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { expr, job, loadWorkflow, stepBy } from './lib/workflow.mjs';

const COVERAGE = 'Coverage (>= 90 % lines; 99 % security target report-only)';

/** What the coverage condition lets through, as the three facts the workflows can set. */
function covers(condition, facts) {
  const c = expr(condition);
  const v = {
    'inputs.os_full == true': facts.os_full === true,
    "inputs.os_full == 'true'": facts.os_full === 'true',
    "github.event.inputs.os_full == 'true'": facts.os_full === 'true',
    'inputs.release == true': facts.release === true,
    "startsWith(github.ref, 'refs/tags/')": String(facts.ref ?? '').startsWith('refs/tags/'),
  };
  // The condition is one parenthesised `||` list of those atoms; anything else is not what the plan's coverage_run mirrors.
  const body = c.replace(/^\(|\)$/g, '');
  const atoms = body.split(' || ');
  assert.ok(atoms.every((a) => a in v), `coverage condition has an atom the placement test does not know: ${c}`);
  return atoms.some((a) => v[a]);
}

test('the coverage step of the crates job runs in the nightly, a release and a tag, and on no train, branch or push to main', () => {
  const step = stepBy(job(loadWorkflow('ci.yml'), 'crates'), COVERAGE);
  assert.ok(step, 'the crates job has the coverage step');
  assert.ok(step.if, 'the coverage step has a condition: unguarded it runs on every train');
  assert.equal(covers(step.if, { os_full: true }), true, 'the nightly passes os_full');
  assert.equal(covers(step.if, { release: true }), true, "a release's gates");
  assert.equal(covers(step.if, { ref: 'refs/tags/v1.0.0' }), true, 'a tag');
  for (const ref of ['refs/heads/train/2026-11-26-p', 'refs/heads/main', 'refs/heads/feat/x']) {
    assert.equal(covers(step.if, { ref }), false, `no coverage on ${ref}`);
  }
});

test('the nightly calls ci.yml with os_full, so it runs the coverage step', () => {
  const nightly = loadWorkflow('nightly.yml');
  const call = job(nightly, 'ci');
  assert.equal(call.uses, './.gitea/workflows/ci.yml');
  assert.ok([true, 'true'].includes(call.with?.os_full), 'nightly.yml passes os_full: true');
});

test('the crate job runs the Linux tests plainly exactly when coverage does not (the leg never runs them twice, and never none)', () => {
  const wf = loadWorkflow('ci.yml');
  const tests = stepBy(job(wf, 'crate'), 'Tests (workspace, every feature)');
  const record = stepBy(job(wf, 'crate'), 'Record test results (test cache)');
  const cov = stepBy(job(wf, 'crates'), COVERAGE);
  for (const [what, step] of [['the plain test step', tests], ['the record step', record]]) {
    assert.ok(step, `${what} exists`);
    const c = expr(step.if);
    assert.ok(c.includes("runner.os != 'Linux' ||"), `${what} runs on Linux when coverage does not`);
    assert.ok(c.includes(expr(cov.if).replace(/^\(|\)$/g, '')) || c.includes(expr(cov.if)), `${what} excludes Linux with the coverage condition itself`);
  }
});

test('the placement check sees an unguarded coverage step and a condition with an unknown atom (positive controls)', () => {
  assert.throws(() => covers("${{ github.event_name == 'push' }}", {}), /does not know/);
  assert.equal(covers("${{ (inputs.os_full == true || startsWith(github.ref, 'refs/tags/')) }}", { ref: 'refs/heads/train/x' }), false);
});
