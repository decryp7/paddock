// Rule 54, "green that proves something": a release run that built no installers says so, and a nightly, manual or tag run that was
// meant to build them and did not fails. The nightly builds every released app's installers, with the same inputs as the app's own
// release workflow.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { failedJobNames, verdict } from '../ci/release-verdict.mjs';
import { PADDOCK } from './helpers.mjs';
import { conjuncts, expr, job, loadAction, loadWorkflow, needsOf, stepsOf, stepsUsing, withs } from './lib/workflow.mjs';

const read = (...p) => readFileSync(join(PADDOCK, ...p), 'utf8');
const APPS = ['dyno', 'spotter', 'pitcrew', 'pitwall'];

test('built installers are a notice; a failed build fails; a cancelled run is not blamed', () => {
  assert.deepEqual(verdict({ app: 'dyno', event: 'workflow_dispatch', planRun: 'true', releaseResult: 'success' }).ok, true);
  assert.match(verdict({ app: 'dyno', event: 'push', releaseResult: 'success' }).line, /^installers: built \(dyno/);
  const failed = verdict({ app: 'dyno', event: 'push', releaseResult: 'failure' });
  assert.equal(failed.ok, false);
  assert.match(failed.line, /build FAILED/);
  assert.equal(verdict({ app: 'dyno', event: 'push', releaseResult: 'cancelled' }).ok, true);
});

test('a push the change does not reach may build nothing and says "installers: not built"', () => {
  const v = verdict({ app: 'spotter', event: 'push', ref: 'refs/heads/main', planRun: 'false', releaseResult: 'skipped' });
  assert.equal(v.ok, true);
  assert.equal(v.level, 'notice');
  assert.match(v.line, /^installers: not built \(spotter\): the change does not reach spotter/);
});

test('a nightly, a manual run and a tag that built nothing fail, whatever the plan said', () => {
  for (const [event, ref, why] of [['schedule', 'refs/heads/main', /nightly run builds them/], ['workflow_dispatch', 'refs/heads/x', /manual run builds them/], ['push', 'refs/tags/dyno-v1.0.0', /tag builds them/]]) {
    for (const planRun of event === 'schedule' ? ['true', ''] : ['true', 'false', '']) {
      const v = verdict({ app: 'dyno', event, ref, planRun, releaseResult: 'skipped' });
      assert.equal(v.ok, false, `${event} plan=${planRun}`);
      assert.match(v.line, /^installers: not built \(dyno\) although/);
      assert.match(v.line, why);
    }
  }
  assert.equal(verdict({ app: 'dyno', event: 'schedule', releaseResult: '' }).ok, false, 'a missing job is not a pass');
});

test('a nightly the change gate skipped (main and the image unchanged) says so and passes', () => {
  const v = verdict({ app: 'dyno', event: 'schedule', ref: 'refs/heads/main', planRun: 'false', releaseResult: 'skipped' });
  assert.deepEqual([v.ok, v.level], [true, 'notice']);
  assert.match(v.line, /^installers: not built \(dyno\): main and the toolchain image are unchanged/);
});

test('a tag whose gates failed names the failed gates and says the tag was not released', async () => {
  const v = verdict({ app: 'dyno', event: 'push', ref: 'refs/tags/dyno-v1.0.0', planRun: 'true', releaseResult: 'skipped', gatesResult: 'failure', failedGates: ['app dyno (windows-latest)', 'app dyno (ubuntu-latest, e2e-1)'] });
  assert.equal(v.ok, false);
  assert.equal(v.line, 'installers: not built (dyno): the gates were failure: app dyno (windows-latest), app dyno (ubuntu-latest, e2e-1), so the tag was not released');
  assert.match(verdict({ app: 'dyno', event: 'push', ref: 'refs/tags/dyno-v1', releaseResult: 'skipped', gatesResult: 'failure' }).line, /the gates were failure, so the tag/);
  const jobs = { jobs: [{ name: 'plan', conclusion: 'success' }, { name: 'app dyno (windows-latest)', conclusion: 'failure' }, { name: 'installers verdict', conclusion: null }, { name: 'bundle (macos-latest)', conclusion: 'success' }] };
  assert.deepEqual(await failedJobNames({ SERVER_URL: 'https://x', REPOSITORY: 'a/b', RUN_ID: '1', TOKEN: 't' }, async () => ({ ok: true, json: async () => jobs })), ['app dyno (windows-latest)']);
  assert.deepEqual(await failedJobNames({}, async () => { throw new Error('down'); }), [], 'an unreachable API still gives a verdict');
});

test('the verdict of every release workflow also gets the gates result, and none of them runs on a push to main', () => {
  for (const app of APPS) {
    const wf = loadWorkflow(`${app}-release.yml`);
    const verdictJob = job(wf, 'verdict');
    assert.deepEqual([...needsOf(verdictJob)].sort(), ['gates', 'installers', 'plan', 'release'], app);
    assert.equal(expr(verdictJob.if), 'always()', app);
    const [call, ...more] = stepsUsing(verdictJob, './.gitea/actions/release-verdict');
    assert.equal(more.length, 0, `${app}: one verdict step`);
    assert.equal(call.with.app, app);
    assert.equal(expr(call.with['plan-run']), 'needs.plan.outputs.run', app);
    assert.equal(expr(call.with['skip-reason']), "needs.plan.outputs.inactive == 'true' && 'inactive' || ''", `${app}: an inactive app is said`);
    assert.equal(expr(call.with['release-result']), "needs.release.result != 'skipped' && needs.release.result || needs.installers.result", app);
    assert.equal(expr(call.with['gates-result']), 'needs.gates.result', app);
    // A tag push only: the nightly builds the installers, and a scheduled run here skipped them.
    assert.ok(wf.on.push?.tags?.length > 0 && wf.on.push.branches === undefined, `${app}: a tag push only`);
    assert.equal(wf.on.schedule, undefined, `${app}: the nightly installers are nightly.yml's (a scheduled run here skipped them)`);
  }
});

test('a push the plan said to build, with no installers, is a warning that says the run proves nothing', () => {
  const v = verdict({ app: 'dyno', event: 'push', ref: 'refs/heads/main', planRun: 'true', releaseResult: 'skipped' });
  assert.deepEqual([v.ok, v.level], [true, 'warning']);
  assert.match(v.line, /proves nothing about the release path/);
});

test('the nightly builds every app\'s installers with the app\'s own release inputs and secrets, and its verdict needs them all', () => {
  const nightly = loadWorkflow('nightly.yml');
  for (const app of APPS) {
    const own = job(loadWorkflow(`${app}-release.yml`), 'installers');
    const mine = job(nightly, `installers-${app}`);
    assert.deepEqual(mine.with, own.with, `${app}: the nightly's inputs equal ${app}-release.yml's`);
    assert.deepEqual(mine.secrets, own.secrets, `${app}: the same secrets`);
    assert.deepEqual(needsOf(mine), ['installers-gate'], `${app}: only the gate, a job that always runs`);
    assert.equal(expr(mine.if), "needs.installers-gate.outputs.run == 'true'", `${app}: skipped with the gate`);
  }
  const verdictJob = job(nightly, 'installers-verdict');
  for (const app of APPS) assert.ok(needsOf(verdictJob).includes(`installers-${app}`), `${app}: in the verdict's needs`);
  assert.equal(expr(verdictJob.if), 'always()');
  const calls = stepsUsing(verdictJob, './.gitea/actions/release-verdict');
  for (const app of APPS) {
    const call = calls.find((c) => c.with.app === app);
    assert.ok(call, `${app}: has a verdict step`);
    assert.equal(expr(call.with['plan-run']), 'needs.installers-gate.outputs.run', `${app}: the verdict knows the gate's answer`);
  }
  const gate = job(nightly, 'installers-gate');
  const key = JSON.stringify(gate);
  assert.ok(key.includes("nightly-installers-${{ github.sha }}-${{ hashFiles('ci-image/**', 'rust-toolchain*', '.nvmrc') }}"), 'the gate keys on the commit, the toolchain image and the pinned Rust and Node');
  const mark = job(nightly, 'installers-mark');
  assert.deepEqual([...needsOf(mark)].sort(), ['installers-dyno', 'installers-gate', 'installers-pitcrew', 'installers-pitwall', 'installers-spotter'], 'the marker is saved only after every build');
  assert.deepEqual(conjuncts(mark.if).sort(), ["github.event_name == 'schedule'", "needs.installers-gate.outputs.run == 'true'", 'success()'], 'never for a skipped night');
});

test('a night ended early for a train in CI warns that it measured nothing, and the nightly passes that reason', () => {
  const v = verdict({ app: 'dyno', event: 'schedule', planRun: 'false', skipReason: 'train', releaseResult: 'skipped' });
  assert.equal(v.ok, true);
  assert.equal(v.level, 'warning');
  assert.match(v.line, /a train was in CI.*not measured, not green/);
  assert.match(verdict({ app: 'dyno', event: 'schedule', planRun: 'false', releaseResult: 'skipped' }).line, /unchanged since the last nightly/);
  const reasons = withs(loadWorkflow('nightly.yml')).filter((w) => w.with['skip-reason'] !== undefined);
  assert.equal(reasons.length, 4);
  for (const r of reasons) assert.equal(expr(r.with['skip-reason']), "needs.gate.outputs.train_skip == 'true' && 'train' || ''", r.where);
});

test('an inactive app: a manual run is NOT MEASURED and fails as such (ER-33), a push only says so, a tag is never excused', () => {
  for (const event of ['workflow_dispatch', 'schedule']) {
    const v = verdict({ app: 'dyno', event, ref: 'refs/heads/main', planRun: 'false', skipReason: 'inactive', releaseResult: 'skipped' });
    assert.deepEqual([v.ok, v.level, v.notMeasured], [false, 'error', true], event);
    assert.match(v.line, /^installers: NOT MEASURED \(dyno\): dyno is inactive \(docs\/ci-active-apps\.toml/, event);
    assert.match(v.line, /no app job ran/, event);
  }
  const push = verdict({ app: 'dyno', event: 'push', ref: 'refs/heads/main', planRun: 'false', skipReason: 'inactive', releaseResult: 'skipped' });
  assert.deepEqual([push.ok, push.level], [true, 'notice']);
  assert.match(push.line, /dyno is inactive \(docs\/ci-active-apps\.toml/);
  // Positive controls: a measured run is not flagged, a manual run without the reason fails as "not built", a tag with it is not excused.
  assert.equal(verdict({ app: 'dyno', event: 'workflow_dispatch', planRun: 'true', releaseResult: 'success' }).notMeasured, undefined);
  const plain = verdict({ app: 'dyno', event: 'workflow_dispatch', planRun: 'false', releaseResult: 'skipped' });
  assert.equal(plain.ok, false);
  assert.equal(plain.notMeasured, undefined);
  assert.equal(verdict({ app: 'dyno', event: 'push', ref: 'refs/tags/dyno-v1.2.0', planRun: 'false', skipReason: 'inactive', releaseResult: 'skipped' }).ok, false);
});

test('release-verdict.mjs exits 6, the not-measured code, for an inactive app on a manual run and prints NOT MEASURED', () => {
  const r = spawnSync(process.execPath, [join(PADDOCK, 'conformance', 'ci', 'release-verdict.mjs')], {
    env: { PATH: process.env.PATH, APP: 'dyno', EVENT: 'workflow_dispatch', REF: 'refs/heads/x', PLAN_RUN: 'false', SKIP_REASON: 'inactive', RELEASE_RESULT: 'skipped' },
    encoding: 'utf8',
  });
  assert.equal(r.status, 6, r.stderr);
  assert.match(r.stdout, /::error::installers: NOT MEASURED \(dyno\)/);
});

test('the release-verdict action is documented and runs the script', () => {
  assert.ok(stepsOf(loadAction('release-verdict').runs).some((st) => /conformance\/ci\/release-verdict\.mjs/.test(st.run ?? '')));
  assert.match(read('.gitea', 'actions', 'README.md'), /\| `release-verdict` \|/);
});
