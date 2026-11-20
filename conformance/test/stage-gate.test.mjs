import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { final, notPassed, parseNeeds, stage1, STAGE_LINE } from '../ci/stage-gate.mjs';

const script = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci', 'stage-gate.mjs');
const ctx = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, { result: v }]));
const run = (mode, needs) => spawnSync('node', [script, mode], { encoding: 'utf8', env: { ...process.env, NEEDS: needs } });

test('stage 1 passes when every job passed or was left out by the plan', () => {
  const r = stage1(ctx({ plan: 'success', ui: 'success', c99: 'skipped' }));
  assert.equal(r.ok, true);
  assert.match(r.lines[0], /2 job\(s\) ran, 1 left out/);
});

test('stage 1 fails on a failed job and on a cancelled one, and names them', () => {
  for (const bad of ['failure', 'cancelled']) {
    const r = stage1(ctx({ plan: 'success', crate: bad, ui: 'success' }), true);
    assert.equal(r.ok, false);
    assert.match(r.lines[0], new RegExp(`::error::stage 1 failed: crate \\(${bad}\\)`));
    assert.match(r.lines[0], /Stage 2 .* does not run/);
  }
});

test('report-only (ER-41): without ENFORCE a failed stage 1 only warns; with it, the CLI blocks', () => {
  const needs = JSON.stringify(ctx({ plan: 'success', ui: 'failure' }));
  const cli = (enforce) => spawnSync('node', [script, 'stage1'], { encoding: 'utf8', env: { ...process.env, NEEDS: needs, ENFORCE: enforce } });
  const soft = cli('');
  assert.equal(soft.status, 0);
  assert.match(soft.stdout, /::warning::stage 1 failed: ui \(failure\)\. Report-only/);
  assert.equal(cli('1').status, 1);
});

test('a job with no result is not a pass (positive control: the check can see a missing result)', () => {
  assert.deepEqual(notPassed({ a: {}, b: { result: 'success' } }), ['a (no result)']);
});

test('a gate that saw no jobs, or no JSON, is broken and fails with exit 2', () => {
  for (const text of ['{}', '[]', 'null', 'not json', '']) {
    assert.throws(() => parseNeeds(text), /needs context/);
    assert.equal(run('stage1', text).status, 2, text);
  }
  assert.equal(run('nonsense', '{"a":{"result":"success"}}').status, 2);
});

test('the final job prints stage2=complete only after a passed stage 1 and a stage 2 with no failure', () => {
  const ok = final(ctx({ plan: 'success', stage1: 'success', e2e: 'success', 'e2e-webkit': 'skipped' }));
  assert.equal(ok.ok, true);
  assert.deepEqual(ok.lines, [`${STAGE_LINE}stage1=passed stage2=complete`]);
});

test('a stage 2 that did not run because stage 1 failed is never complete', () => {
  for (const s1 of ['failure', 'skipped', 'cancelled', undefined]) {
    const r = final(ctx({ plan: 'success', stage1: s1, e2e: 'skipped' }));
    assert.equal(r.ok, false, String(s1));
    assert.equal(r.lines[0], `${STAGE_LINE}stage1=failed stage2=not-run`);
  }
});

test('a failed or cancelled stage-2 job is stage2=failed', () => {
  for (const bad of ['failure', 'cancelled']) {
    const r = final(ctx({ plan: 'success', stage1: 'success', e2e: bad }));
    assert.equal(r.ok, false);
    assert.equal(r.lines[0], `${STAGE_LINE}stage1=passed stage2=failed`);
  }
});

test('stage=fast: a passed stage 1 passes the run but stage 2 is not-run, never complete', () => {
  const r = final(ctx({ plan: 'success', stage1: 'success', e2e: 'skipped' }), 'fast');
  assert.equal(r.ok, true);
  assert.equal(r.lines[0], `${STAGE_LINE}stage1=passed stage2=not-run`);
  // A failed stage 1 is still a failure under fast.
  assert.equal(final(ctx({ plan: 'success', stage1: 'failure' }), 'fast').ok, false);
  // The CLI reads STAGE from the environment (the workflows' `stage` input); empty and `all` are the full run.
  const needs = JSON.stringify(ctx({ stage1: 'success', e2e: 'success' }));
  const cli = (stage) => spawnSync('node', [script, 'final'], { encoding: 'utf8', env: { ...process.env, NEEDS: needs, STAGE: stage } });
  assert.match(cli('fast').stdout, /^stage verdict: stage1=passed stage2=not-run$/m);
  for (const s of ['', 'all']) assert.match(cli(s).stdout, /^stage verdict: stage1=passed stage2=complete$/m, s);
});

test('the final job without a stage1 need is broken', () => {
  assert.throws(() => final(ctx({ plan: 'success', e2e: 'success' })), /no `stage1` job/);
});

test('the CLI exit codes: 0 passed, 1 failed, and the line is printed on stdout', () => {
  const pass = run('final', JSON.stringify(ctx({ stage1: 'success', e2e: 'success' })));
  assert.equal(pass.status, 0);
  assert.match(pass.stdout, /^stage verdict: stage1=passed stage2=complete$/m);
  const fail = spawnSync('node', [script, 'stage1'], { encoding: 'utf8', env: { ...process.env, ENFORCE: '1', NEEDS: JSON.stringify(ctx({ plan: 'success', ui: 'failure' })) } });
  assert.equal(fail.status, 1);
  assert.match(fail.stdout, /::error::stage 1 failed: ui \(failure\)/);
});

// ---- the workflows hold to the stages (read through the YAML accessor, ER-42) ----
import { job as workflowJob, loadWorkflow, needsOf } from './lib/workflow.mjs';

for (const [file, stage2, last] of [
  ['ci.yml', ['e2e', 'e2e-webkit', 'app-e2e'], ['stage1', 'stages']],
  ['debrief.yml', ['e2e', 'bundle', 'tauri-smoke'], ['stage1', 'stages']],
]) {
  test(`${file}: every job is in stage 1 (a need of the gate), in stage 2 (needs the gate) or is a gate or a later job`, () => {
    const wf = loadWorkflow(file);
    const ids = Object.keys(wf.jobs);
    const gate = needsOf(workflowJob(wf, 'stage1'));
    assert.ok(gate.length > 3, `the stage1 gate needs the stage-1 jobs (${gate.join(', ')})`);
    for (const id of stage2) {
      assert.ok(ids.includes(id), `${file} has the stage-2 job ${id}`);
      assert.ok(needsOf(workflowJob(wf, id)).includes('stage1'), `${id} needs stage1`);
      assert.ok(!gate.includes(id), `${id} is stage 2: the gate must not wait for it`);
      assert.match(String(workflowJob(wf, id).if), /needs\.stage1\.result == 'success'/, `${id} runs only after a passed stage 1`);
    }
    // A job outside both stages would run in neither the gate's nor stage 2's order: name it here when it is a gate or a release job.
    const later = (id) => needsOf(workflowJob(wf, id)).some((n) => stage2.includes(n) || n === 'stage1') && !stage2.includes(id);
    for (const id of ids) {
      if (id === 'plan' || last.includes(id) || stage2.includes(id) || gate.includes(id) || later(id)) continue;
      assert.fail(`${file}: job ${id} is in no stage. Fix: add it to the stage1 job's needs (a fast check) or make it need stage1 (a long job)`);
    }
    for (const id of gate) assert.ok(ids.includes(id), `the stage1 gate needs ${id}, which is no job of ${file}`);
    // The final job sees every stage-2 job and the gate, so its line cannot be printed over a stage that did not run.
    const needs = needsOf(workflowJob(wf, 'stages'));
    for (const id of ['stage1', ...stage2]) assert.ok(needs.includes(id), `stages needs ${id}`);
    for (const id of ['stage1', 'stages']) assert.match(String(workflowJob(wf, id).if), /always\(\)/, `${id} runs after a failure too`);
  });
}

for (const file of ['ci.yml', 'debrief.yml']) {
  test(`${file}: the stage input exists, every stage-2 job honours stage=fast, and the plan is never complete under it`, () => {
    const wf = loadWorkflow(file);
    assert.ok(wf.on.workflow_dispatch.inputs.stage, 'the stage dispatch input');
    const stage2 = file === 'ci.yml' ? ['e2e', 'e2e-webkit', 'app-e2e'] : ['e2e', 'bundle', 'tauri-smoke'];
    for (const id of stage2) assert.match(String(workflowJob(wf, id).if), /github\.event\.inputs\.stage != 'fast'/, `${id} is skipped by stage=fast`);
    const plan = workflowJob(wf, 'plan').steps.map((s) => s.run ?? '').join('\n');
    assert.match(plan, /"\$PLAN_STAGE" != fast/);
    const env = workflowJob(wf, 'stages').steps.find((s) => s.name === 'Stage verdict').env;
    assert.match(String(env.STAGE), /github\.event\.inputs\.stage/);
  });
}

test('ci.yml: the plan job logs the staged layout the verdict reader keys on, and the app legs are split in two matrices', () => {
  const wf = loadWorkflow('ci.yml');
  const plan = workflowJob(wf, 'plan');
  const run = plan.steps.map((s) => s.run ?? '').join('\n');
  assert.match(run, /^\s*echo "plan layout: staged"$/m);
  assert.match(String(workflowJob(wf, 'app').strategy.matrix.include), /plan\.outputs\.apps\b(?!_)/);
  assert.match(String(workflowJob(wf, 'app-e2e').strategy.matrix.include), /plan\.outputs\.apps_e2e/);
});
