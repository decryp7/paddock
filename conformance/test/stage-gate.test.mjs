import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { final, gateIds, gateIsReportOnly, notPassed, parseNeeds, stage1, STAGE_LINE } from '../ci/stage-gate.mjs';
import { job as workflowJob, loadWorkflow, needsOf } from './lib/workflow.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, '..', 'ci', 'stage-gate.mjs');
const TABLE = JSON.parse(readFileSync(join(here, '..', 'ci', 'stages.json'), 'utf8'));

const ctx = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, { result: v }]));
/** A needs context with gates: `gates` maps a gate id to [result, its `result` output]. */
const withGates = (gates, rest = {}) => ({
  plan: { result: 'success' },
  ...Object.fromEntries(Object.entries(gates).map(([g, [r, out]]) => [g, { result: r, outputs: { result: out } }])),
  ...ctx(rest),
});
const PASSED = { 'stage1-linux': ['success', 'passed'], 'stage1-macos': ['success', 'passed'] };
const run = (mode, needs, more = {}) =>
  spawnSync('node', [script, mode], { encoding: 'utf8', env: { ...process.env, NEEDS: needs, WORKFLOW: 'ci.yml', GATE: 'stage1-linux', EXPECTED: '[]', STAGE: '', ...more } });
const gatesFile = (text) => {
  const f = join(mkdtempSync(join(tmpdir(), 'stage-gate-')), 'gates.toml');
  writeFileSync(f, text);
  return f;
};

test('stage 1 passes when every job passed or was left out by the plan', () => {
  const r = stage1(ctx({ plan: 'success', ui: 'success', c99: 'skipped' }));
  assert.equal(r.ok, true);
  assert.equal(r.result, 'passed');
  assert.match(r.lines[0], /2 job\(s\) ran, 1 left out/);
});

test('stage 1 fails on a failed job and on a cancelled one, and names them', () => {
  for (const bad of ['failure', 'cancelled']) {
    const r = stage1(ctx({ plan: 'success', conformance: bad, ui: 'success' }), true);
    assert.equal(r.ok, false);
    assert.equal(r.result, 'failed');
    assert.match(r.lines[0], new RegExp(`::error::stage 1 failed: conformance \\(${bad}\\)`));
  }
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

// ---- the mode comes from the register, not a variable (ER-41) ----
const GATES = '[[report_only]]\nid = "C122"\nreason = "x"\n\n[[report_only]]\nid = "gate:ci.yml/stage1-linux"\nreason = "ER-41"\n';

test('a gate is report-only exactly while docs/gates.toml registers it', () => {
  assert.equal(gateIsReportOnly(GATES, 'ci.yml', 'stage1-linux'), true);
  assert.equal(gateIsReportOnly(GATES, 'ci.yml', 'stage1-macos'), false, 'another gate of the same workflow is its own entry');
  assert.equal(gateIsReportOnly(GATES, 'debrief.yml', 'stage1-linux'), false);
  assert.equal(gateIsReportOnly('[[report_only]]\nid = "C122"\n', 'ci.yml', 'stage1-linux'), false, 'entry removed: blocking');
  assert.equal(gateIsReportOnly('[[not_gated]]\nid = "gate:ci.yml/stage1-linux"\n', 'ci.yml', 'stage1-linux'), false, 'only a report_only table counts');
});

test('report-only: the CLI warns for a registered gate and blocks for an unregistered one; an unreadable register is broken', () => {
  const needs = JSON.stringify(ctx({ plan: 'success', ui: 'failure' }));
  const out = join(mkdtempSync(join(tmpdir(), 'stage-out-')), 'out');
  const soft = run('stage1', needs, { GATES_FILE: gatesFile(GATES), GITHUB_OUTPUT: out });
  assert.equal(soft.status, 0);
  assert.match(soft.stdout, /::warning::stage 1 failed: ui \(failure\)\. Report-only/);
  assert.equal(readFileSync(out, 'utf8'), 'result=failed(report-only)\n');
  assert.equal(run('stage1', needs, { GATES_FILE: gatesFile('[[report_only]]\nid = "C122"\n') }).status, 1);
  assert.equal(run('stage1', needs, { GATES_FILE: '/nonexistent/gates.toml' }).status, 2);
});

// ---- the verdict ----
test('the final job prints stage2=complete only after passed gates and every expected stage-2 job a success', () => {
  const ok = final(withGates(PASSED, { e2e: 'success', 'e2e-webkit': 'success' }), '', '["e2e","e2e-webkit"]');
  assert.equal(ok.ok, true);
  assert.deepEqual(ok.lines, [`${STAGE_LINE}stage1=passed stage2=complete`]);
});

test('positive control: a failing fast job on one OS stops only that OS (its gate fails, the other gate passes)', () => {
  const linux = stage1(ctx({ plan: 'success', ui: 'success' }));
  const mac = stage1(ctx({ plan: 'success', 'tools-macos': 'failure' }), true);
  assert.equal(linux.result, 'passed', 'the Linux gate is not touched by a macOS failure');
  assert.equal(mac.result, 'failed');
  // In the run, the macOS gate failed and its stage-2 job was skipped; the Linux stage-2 job ran. The run is never complete.
  const r = final(withGates({ 'stage1-linux': ['success', 'passed'], 'stage1-macos': ['failure', 'failed'] }, { e2e: 'success', 'e2e-webkit': 'skipped' }), '', '["e2e","e2e-webkit"]');
  assert.equal(r.ok, false);
  assert.equal(r.lines[0], `${STAGE_LINE}stage1=failed stage2=not-run`);
});

test('a stage-2 job the plan expects must be success: skipped or missing is never complete', () => {
  for (const rest of [{ e2e: 'success', 'app-e2e': 'skipped' }, { e2e: 'success' }]) {
    const r = final(withGates(PASSED, rest), '', '["e2e","app-e2e"]');
    assert.equal(r.ok, false);
    assert.equal(r.lines[0], `${STAGE_LINE}stage1=passed stage2=incomplete`);
    assert.match(r.lines[1], /expected stage-2 job\(s\) that did not succeed: app-e2e/);
  }
  assert.equal(final(withGates(PASSED, { e2e: 'skipped' }), '', '[]').ok, true, 'nothing expected: nothing required');
  assert.throws(() => final(withGates(PASSED), '', 'nope'), /stage2_jobs/);
  assert.throws(() => final(ctx({ plan: 'success', e2e: 'success' }), '', '[]'), /no stage1-<os> gate/);
});

test('report-only: a failed gate says failed(report-only) in the verdict, never passed', () => {
  const needs = withGates({ 'stage1-linux': ['success', 'failed(report-only)'], 'stage1-macos': ['success', 'passed'] }, { e2e: 'success' });
  const r = final(needs, '', '["e2e"]');
  assert.equal(r.ok, false);
  assert.equal(r.lines[0], `${STAGE_LINE}stage1=failed(report-only) stage2=complete`);
  // A gate that cannot say (no output) is a failure, not a pass.
  const unknown = final({ plan: { result: 'success' }, 'stage1-linux': { result: 'success' }, e2e: { result: 'success' } }, '', '["e2e"]');
  assert.equal(unknown.lines[0], `${STAGE_LINE}stage1=failed stage2=complete`);
});

test('a failed or cancelled stage-2 job is stage2=failed', () => {
  for (const bad of ['failure', 'cancelled']) {
    const r = final(withGates(PASSED, { e2e: bad }), '', '["e2e"]');
    assert.equal(r.ok, false);
    assert.equal(r.lines[0], `${STAGE_LINE}stage1=passed stage2=failed`);
  }
});

test('stage=fast: stage 2 is not-run, never complete; the CLI reads STAGE and rejects anything unknown', () => {
  const r = final(withGates(PASSED, { e2e: 'skipped' }), 'fast', '["e2e"]');
  assert.equal(r.ok, true);
  assert.equal(r.lines[0], `${STAGE_LINE}stage1=passed stage2=not-run`);
  const needs = JSON.stringify(withGates(PASSED, { e2e: 'success' }));
  const cli = (stage) => run('final', needs, { STAGE: stage, EXPECTED: '["e2e"]' });
  assert.match(cli('fast').stdout, /^stage verdict: stage1=passed stage2=not-run$/m);
  for (const s of ['', 'all']) assert.match(cli(s).stdout, /^stage verdict: stage1=passed stage2=complete$/m, s);
  assert.equal(cli('fsat').status, 2);
  assert.match(cli('fsat').stdout, /stage input/);
});

test('gateIds finds the gates by name', () => {
  assert.deepEqual(gateIds(withGates(PASSED, { e2e: 'success' })), ['stage1-linux', 'stage1-macos']);
});

// ---- the workflow files hold to the table (read through the YAML accessor, ER-42) ----
for (const file of ['ci.yml', 'debrief.yml']) {
  test(`${file}: every job is in the table exactly once, and the gates, stage-2 jobs and final job need what the table says`, () => {
    const wf = loadWorkflow(file);
    const t = TABLE[file];
    const ids = Object.keys(wf.jobs);
    const gates = Object.keys(t.gates);
    const placed = [...Object.values(t.gates).flat(), ...Object.keys(t.stage2), ...t.parallel, ...gates, 'plan', 'stages'];
    for (const id of ids) {
      const n = placed.filter((p) => p === id).length;
      if (['release', 'nightly-mark'].includes(id)) continue; // later jobs: they need the stage-2 jobs themselves
      assert.equal(n, 1, `${file}: job ${id} must be in conformance/ci/stages.json exactly once (found ${n}). Fix: add it to a gate's list (fast), to stage2 (long) or to parallel`);
    }
    for (const id of placed) assert.ok(ids.includes(id), `stages.json names ${id}, which is no job of ${file}`);
    for (const [g, jobs] of Object.entries(t.gates)) {
      assert.deepEqual([...needsOf(workflowJob(wf, g))].sort(), ['plan', ...jobs].sort(), `${g} needs exactly its jobs of the table`);
      assert.match(String(workflowJob(wf, g).if), /!cancelled\(\)/, `${g} runs after a failure too`);
      const step = workflowJob(wf, g).steps.find((s) => s.id === 'gate');
      assert.equal(step.env.GATE, g);
      assert.equal(step.env.WORKFLOW, file);
    }
    for (const [id, needGates] of Object.entries(t.stage2)) {
      const j = workflowJob(wf, id);
      for (const g of needGates) {
        assert.ok(needsOf(j).includes(g), `${id} needs ${g}`);
        assert.match(String(j.if), new RegExp(`needs\\.${g}\\.result == 'success'`), `${id} runs only after a passed ${g}`);
      }
      assert.match(String(j.if), /github\.event\.inputs\.stage != 'fast'/, `${id} is skipped by stage=fast`);
    }
    assert.deepEqual([...needsOf(workflowJob(wf, 'stages'))].sort(), ['plan', ...gates, ...Object.keys(t.stage2)].sort(), 'stages needs the gates and every stage-2 job');
    const env = workflowJob(wf, 'stages').steps.find((s) => s.name === 'Stage verdict').env;
    assert.match(String(env.EXPECTED), /needs\.plan\.outputs\.stage2_jobs/);
    assert.match(String(env.STAGE), /github\.event\.inputs\.stage/);
    const plan = workflowJob(wf, 'plan').steps.map((s) => s.run ?? '').join('\n');
    assert.match(plan, /"\$PLAN_STAGE" != fast/);
    assert.match(plan, /the stage input is/, 'the stage input is validated');
    assert.match(plan, /stage2_jobs=/);
    assert.ok(wf.on.workflow_dispatch.inputs.stage, 'the stage dispatch input');
    for (const id of Object.keys(t.stage2)) assert.ok(plan.includes(id), `the plan's expected list names ${id}`);
  });

  test(`${file}: every gate:<workflow>/<job> entry of docs/gates.toml names a real gate of the table (a flipped gate has no entry)`, () => {
    const text = readFileSync(join(here, '..', '..', 'docs', 'gates.toml'), 'utf8');
    const ids = [...text.matchAll(/^id = "gate:([^/"]+)\/([^"]+)"$/gm)].filter((m) => m[1] === file).map((m) => m[2]);
    for (const g of ids) assert.ok(Object.keys(TABLE[file].gates).includes(g), `docs/gates.toml registers gate:${file}/${g}, which is no gate of conformance/ci/stages.json`);
    // Positive control: the reader sees an entry that is there and none that is not.
    assert.equal(gateIsReportOnly(text, file, 'stage1-nonexistent'), false);
  });
}

test('ci.yml: the plan job logs the staged layout the verdict reader keys on, and the app legs are split per OS, with the end-to-end legs a matrix of their own', () => {
  const wf = loadWorkflow('ci.yml');
  const plan = workflowJob(wf, 'plan');
  const run = plan.steps.map((s) => s.run ?? '').join('\n');
  assert.match(run, /^\s*echo "plan layout: staged"$/m);
  assert.match(String(workflowJob(wf, 'app-linux').with.legs), /plan\.outputs\.apps_linux/);
  assert.match(String(workflowJob(wf, 'app-e2e').with.legs), /plan\.outputs\.apps_e2e/);
});

// ---- ci.yml's crate, paddock-dev and app jobs: one call of a reusable workflow per operating system (user decisions 2026-11-20) ----

const OSES = [['linux', 'ubuntu-latest'], ['windows', 'windows-latest'], ['macos', 'macos-latest']];
const FAMILIES = { crate: ['ci-crate.yml', 'crate_legs'], 'paddock-dev': ['ci-paddock-dev.yml', 'paddock_dev_legs'], app: ['ci-app.yml', 'apps'] };

test('ci.yml: each of crate, paddock-dev and app is one call per OS of one reusable workflow, in the table once, in its own OS gate, skipped when the plan has no leg for that OS', () => {
  const wf = loadWorkflow('ci.yml');
  const gates = TABLE['ci.yml'].gates;
  assert.deepEqual(Object.keys(gates).sort(), ['stage1-linux', 'stage1-macos', 'stage1-windows'], 'a gate per OS');
  for (const [family, [callee, output]] of Object.entries(FAMILIES)) {
    assert.equal(wf.jobs[family], undefined, `the matrix job ${family} is gone (a job cannot need one matrix leg)`);
    for (const [short] of OSES) {
      const id = `${family}-${short}`;
      const j = workflowJob(wf, id);
      assert.ok(gates[`stage1-${short}`].includes(id), `${id} is in the table's stage1-${short}`);
      assert.equal(j.uses, `./.gitea/workflows/${callee}`, `${id} calls ${callee}: the steps exist once`);
      assert.match(String(j.with.legs), new RegExp(`needs\\.plan\\.outputs\\.${output}_${short}\\b`), `${id} passes only its OS's legs`);
      assert.match(String(j.secrets.BUILD_INPUTS_TOKEN), /secrets\.BUILD_INPUTS_TOKEN/, `${id} passes the read credential`);
      assert.match(String(j.if), new RegExp(`needs\\.plan\\.outputs\\.[a-z_]+_${short} != '\\[\\]'`), `${id} is skipped when the plan has no leg for its OS`);
      assert.match(String(j.if), new RegExp(`'\\["${id}"\\]'\\), '${id}'\\)`), `${id} asks rerun_jobs for itself`);
      assert.ok(String(wf.jobs.plan.outputs[`${output}_${short}`]).includes(`steps.plan.outputs.${output}_${short}`), `the plan publishes ${output}_${short}`);
    }
  }
  assert.equal(workflowJob(wf, 'app-e2e').uses, './.gitea/workflows/ci-app.yml', 'the end-to-end legs are a call of the same reusable workflow');
  const plan = workflowJob(wf, 'plan').steps.map((s) => s.run ?? '').join('\n');
  for (const [, o] of Object.values(FAMILIES)) assert.match(plan, new RegExp(`\\b${o}\\b`), `the plan step splits ${o} by OS`);
  assert.match(plan, /select\(\.os == \$os\)/);
});

for (const [family, [callee]] of Object.entries(FAMILIES)) {
  test(`${callee}: a reusable workflow with one job ${family}, a matrix fed by its legs input, the credential as a declared secret, and the kit variable`, () => {
    const wf = loadWorkflow(callee);
    assert.deepEqual(Object.keys(wf.jobs), [family], 'one job: the id is the family, so github.job names the build folder as before');
    const j = workflowJob(wf, family);
    assert.match(String(j.strategy.matrix.include), /fromJSON\(inputs\.legs\)/);
    assert.ok(wf.on.workflow_call.inputs.legs && wf.on.workflow_call.inputs.release, 'the inputs legs and release');
    assert.ok(wf.on.workflow_call.secrets.BUILD_INPUTS_TOKEN, 'the secret is declared');
    assert.match(String(wf.env.PADDOCK_CI_KIT), /vars\.PADDOCK_CI_KIT/);
    assert.equal(needsOf(j).length, 0, 'a called workflow reads no needs: its caller decides whether it runs');
    for (const s of j.steps ?? []) assert.doesNotMatch(JSON.stringify(s), /needs\.plan/, 'no step reads the caller\'s plan');
  });
}

test('ci.yml: the plan job logs the staged layout the verdict reader keys on, and the app legs are split per OS, with the end-to-end legs a matrix of their own', () => {
  const wf = loadWorkflow('ci.yml');
  const plan = workflowJob(wf, 'plan');
  const run = plan.steps.map((s) => s.run ?? '').join('\n');
  assert.match(run, /^\s*echo "plan layout: staged"$/m);
  assert.match(String(workflowJob(wf, 'app-linux').with.legs), /plan\.outputs\.apps_linux/);
  assert.match(String(workflowJob(wf, 'app-e2e').with.legs), /plan\.outputs\.apps_e2e/);
});
