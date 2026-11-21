// ER-49 (test a commit once): a push to main of a commit that a complete green run already tested ends early. `./dev ci reuse`
// (tools/paddock-dev/src/ci_runs/reuse.rs) decides; these tests keep the workflows that use it wired so that a reused run runs no
// job and never counts as a complete run (the reused run decides the commit for `./dev ci verdict`).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';
import { jobText, verdictSnippet } from './ci-verdict-reader.test.mjs';

const text = (w) => readFileSync(join(PADDOCK, '.gitea', 'workflows', w), 'utf8');
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

test('a reused ci.yml run runs no job and is never complete', () => {
  const ci = text('ci.yml');
  const plan = jobText(ci, 'plan');
  // Every job reads `rerun_jobs`: an empty list is none of them, so a reused run runs nothing.
  assert.match(plan, /if \[ "\$PLAN_REUSE" = true \]; then echo 'rerun_jobs=\[\]' >> "\$GITHUB_OUTPUT"; else \.\/dev plan --ci; fi/);
  const say = (env) => spawnSync('bash', ['-c', verdictSnippet(ci)], { encoding: 'utf8', env: { PATH: process.env.PATH, PLAN_SINCE_RUN: '', PLAN_MODE_FULL: 'true', EVENT: 'push', ...env } }).stdout.trim();
  assert.equal(say({}), 'plan verdict: complete=true');
  assert.equal(say({ PLAN_REUSE: 'false' }), 'plan verdict: complete=true');
  assert.equal(say({ PLAN_REUSE: 'true' }), 'plan verdict: complete=false');
  const jobs = [...ci.slice(ci.indexOf('\njobs:\n')).matchAll(/^ {2}([\w-]+):\n/gm)].map((m) => m[1]).filter((j) => j !== 'plan');
  assert.ok(jobs.length >= 15, `the jobs of ci.yml were read (${jobs.length})`);
  for (const j of jobs) {
    const body = jobText(ci, j);
    assert.match(body, /rerun_jobs \|\| '\[/, `${j}: its if reads rerun_jobs, so an empty list ends it`);
  }
});

test('a reused debrief.yml run plans nothing and is never complete', () => {
  const debrief = text('debrief.yml');
  const plan = jobText(debrief, 'plan');
  assert.match(plan, /if \[ "\$REUSE" = true \]; then\n\s+run=false/);
  const snippet = verdictSnippet(debrief);
  const say = (run) => spawnSync('bash', ['-c', snippet], { encoding: 'utf8', env: { PATH: process.env.PATH, run, PLAN_SINCE_RUN: '', PLAN_MODE_FULL: 'true' } }).stdout.trim();
  assert.equal(say('false'), 'plan verdict: complete=false');
  assert.equal(say('true'), 'plan verdict: complete=true');
});

// P1 (2026-11-21): "complete" has one meaning for ci.yml, the scope of a push to main, WebKit included (plan_ci::wants_webkit). A full
// dispatch without the webkit input logged complete=true, so reuse could skip the main push run for it and the tag gate would read a
// green verdict that never ran WebKit. The marker now needs the push, the webkit input, or an app's own gates.
test('a full ci.yml dispatch without WebKit is not complete; with WebKit, a push or an app gate it is', () => {
  const ci = text('ci.yml');
  const say = (env) =>
    spawnSync('bash', ['-c', verdictSnippet(ci)], {
      encoding: 'utf8',
      env: { PATH: process.env.PATH, PLAN_SINCE_RUN: '', PLAN_MODE_FULL: 'true', PLAN_REUSE: '', PLAN_WEBKIT: '', PLAN_APP: '', EVENT: 'workflow_dispatch', ...env },
    }).stdout.trim();
  // The train dispatch with no base and no webkit input: a smaller scope than a push to main.
  assert.equal(say({}), 'plan verdict: complete=false');
  // Positive controls: the same run with the webkit input, a push to main, an app's own gates.
  assert.equal(say({ PLAN_WEBKIT: '1' }), 'plan verdict: complete=true');
  assert.equal(say({ EVENT: 'push' }), 'plan verdict: complete=true');
  assert.equal(say({ PLAN_APP: 'dyno' }), 'plan verdict: complete=true');
  // A rerun or a reused run is never complete, with WebKit or not.
  assert.equal(say({ PLAN_WEBKIT: '1', PLAN_SINCE_RUN: '8001' }), 'plan verdict: complete=false');
  assert.equal(say({ EVENT: 'push', PLAN_REUSE: 'true' }), 'plan verdict: complete=false');
});
