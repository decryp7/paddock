// ER-49 (test a commit once): a push to main of a commit that a complete green run already tested ends early. `./dev ci reuse`
// (tools/paddock-dev/src/ci_runs/reuse.rs) decides; these tests keep the workflows that use it wired so that a reused run runs no
// job and never counts as a complete run (the reused run decides the commit for `./dev ci verdict`).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';
import { codeLines, jobText } from './ci-verdict-reader.test.mjs';
import { loadWorkflow, workflowText } from './lib/workflow.mjs';

const text = workflowText;
const PUSH_TO_MAIN = "if: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}";

/** Why a workflow's plan job does not ask `./dev ci reuse` for a push to main only, or null. */
export function reuseProblem(workflowText, file) {
  const plan = jobText(workflowText, 'plan') ?? '';
  const at = plan.indexOf(`run: ./dev ci reuse --workflow ${file}`);
  if (at < 0) return `its plan job does not run \`./dev ci reuse --workflow ${file}\``;
  const step = plan.slice(plan.lastIndexOf('- name:', at), at);
  if (!step.includes('id: reuse')) return 'the reuse step has no `id: reuse` for the decision to read';
  if (!step.includes(PUSH_TO_MAIN)) return `the reuse step is not limited to a push to main (${PUSH_TO_MAIN})`;
  return null;
}

test('ci.yml and debrief.yml ask ./dev ci reuse for a push to main, and only for that', () => {
  for (const w of ['ci.yml', 'debrief.yml']) assert.equal(reuseProblem(text(w), w), null, w);
});

test('the reuse check sees a missing, unguarded or unnamed step (positive control)', () => {
  const ci = text('ci.yml');
  assert.match(reuseProblem(ci.replace('run: ./dev ci reuse --workflow ci.yml', 'run: true'), 'ci.yml'), /does not run/);
  assert.match(reuseProblem(ci.replace(PUSH_TO_MAIN, ''), 'ci.yml'), /not limited to a push to main/);
  assert.match(reuseProblem(ci.replace('id: reuse', 'id: other'), 'ci.yml'), /no `id: reuse`/);
  assert.match(reuseProblem(text('debrief.yml'), 'ci.yml'), /does not run/);
});

test('a reused ci.yml run is declared by the planner: PLAN_REUSE reaches `./dev plan --ci`, every job reads rerun_jobs', () => {
  const ci = text('ci.yml');
  const plan = jobText(ci, 'plan');
  // The reuse decision is an environment variable of the plan step; the planner (plan_cmd.rs) turns it into `rerun_jobs=[]` and an
  // empty `ran` in the scope line (tested in Rust: golden.rs ci_plan_declares_a_scope...). No shell line decides it any more.
  assert.match(plan, /PLAN_REUSE: \$\{\{ steps\.reuse\.outputs\.reuse \}\}/);
  assert.match(plan, /\n\s+\.\/dev plan --ci\n/);
  assert.doesNotMatch(plan, /rerun_jobs=\[\]/, 'the planner writes rerun_jobs, not the shell');
  // Every job reads `rerun_jobs`: an empty list is none of them, so a reused run runs nothing.
  const jobs = [...ci.slice(ci.indexOf('\njobs:\n')).matchAll(/^ {2}([\w-]+):\n/gm)].map((m) => m[1]).filter((j) => j !== 'plan');
  assert.ok(jobs.length >= 15, `the jobs of ci.yml were read (${jobs.length})`);
  for (const j of jobs) {
    const body = jobText(ci, j);
    assert.match(body, /rerun_jobs \|\| '\[/, `${j}: its if reads rerun_jobs, so an empty list ends it`);
  }
});

// ADR 0113: the engines a run declares and the engines it runs are one value. The `e2e-webkit` job reads the planner's `webkit` output,
// which `./dev plan --ci` also uses for the declared engines, so a workflow expression cannot run WebKit when the scope says it did not,
// or the reverse.
test('the e2e-webkit job and the declared engines read the planner\'s one webkit value', () => {
  const wf = loadWorkflow('ci.yml');
  assert.match(String(wf.jobs.plan.outputs.webkit), /steps\.plan\.outputs\.webkit/, 'the plan job exposes the planner\'s webkit output');
  const cond = String(wf.jobs['e2e-webkit'].if);
  assert.match(cond, /needs\.plan\.outputs\.webkit == 'true'/);
  assert.doesNotMatch(cond, /inputs\.webkit|github\.ref/, 'no second expression for WebKit: the planner decides (plan_ci::wants_webkit)');
  // Positive control of the matcher: the old expression is refused.
  assert.match("inputs.webkit == true || (github.event_name == 'push'", /inputs\.webkit|github\.ref/);
});

test('a reused debrief.yml run plans nothing: REUSE turns run off before it is written, and the scope call comes after', () => {
  const plan = jobText(text('debrief.yml'), 'plan');
  assert.match(plan, /if \[ "\$REUSE" = true \]; then\n\s+run=false/);
  const code = codeLines(plan);
  assert.ok(code.indexOf('echo "run=$run" >> "$GITHUB_OUTPUT"') < code.findIndex((l) => l.startsWith('./dev ci scope ')), 'the scope is declared after run');
});
