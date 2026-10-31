// Rule 54, "green that proves something": a release run that built no installers says so, and a nightly, manual or tag run that was
// meant to build them and did not fails. The nightly builds every released app's installers, with the same inputs as the app's own
// release workflow.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { failedJobNames, verdict } from '../ci/release-verdict.mjs';
import { PADDOCK } from './helpers.mjs';

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
    const wf = read('.gitea', 'workflows', `${app}-release.yml`);
    assert.match(wf.slice(wf.indexOf('\n  verdict:')), /needs: \[plan, gates, installers, release\]/, app);
    assert.match(wf, /gates-result: \$\{\{ needs\.gates\.result \}\}/, app);
    assert.match(wf, /\non:\n  push:\n    tags: \["/, `${app}: a tag push only; the nightly builds the installers`);
  }
});

test('a push the plan said to build, with no installers, is a warning that says the run proves nothing', () => {
  const v = verdict({ app: 'dyno', event: 'push', ref: 'refs/heads/main', planRun: 'true', releaseResult: 'skipped' });
  assert.deepEqual([v.ok, v.level], [true, 'warning']);
  assert.match(v.line, /proves nothing about the release path/);
});

test('every app release workflow ends in a verdict job that always runs and reads the plan and the release job', () => {
  for (const app of APPS) {
    const wf = read('.gitea', 'workflows', `${app}-release.yml`);
    const job = wf.slice(wf.indexOf('\n  verdict:'));
    assert.match(job, /needs: \[plan, gates, installers, release\]/, app);
    assert.match(job, /if: \$\{\{ always\(\) \}\}/, app);
    assert.match(job, new RegExp(`uses: \\./\\.gitea/actions/release-verdict\\n\\s+with:\\n\\s+app: ${app}\\n\\s+plan-run: \\$\\{\\{ needs\\.plan\\.outputs\\.run \\}\\}\\n\\s+release-result: \\$\\{\\{ needs\\.release\\.result != 'skipped' && needs\\.release\\.result \\|\\| needs\\.installers\\.result \\}\\}`), app);
    assert.doesNotMatch(wf, /^\s+schedule:/m, `${app}: the nightly installers are nightly.yml's (a scheduled run here skipped them)`);
  }
});

/** The `with:` block of a job, comments and blank lines left out. */
function withBlock(wf, job) {
  const at = wf.indexOf(`\n  ${job}:`);
  assert.ok(at >= 0, job);
  const rest = wf.slice(at + 1);
  const end = rest.slice(1).search(/\n  \S/);
  const body = end < 0 ? rest : rest.slice(0, end + 1);
  const w = body.slice(body.indexOf('    with:\n'), body.indexOf('    secrets:'));
  return w.split('\n').filter((l) => l.trim() && !l.trim().startsWith('#')).join('\n');
}

test('the nightly builds every app\'s installers with the app\'s own release inputs and secrets, and its verdict needs them all', () => {
  const nightly = read('.gitea', 'workflows', 'nightly.yml');
  for (const app of APPS) {
    const own = read('.gitea', 'workflows', `${app}-release.yml`);
    assert.equal(withBlock(nightly, `installers-${app}`), withBlock(own, 'installers'), `${app}: the nightly's inputs equal ${app}-release.yml's`);
    const secrets = (wf, job) => { const s = wf.slice(wf.indexOf(`\n  ${job}:`)); return s.slice(s.indexOf('    secrets:'), s.indexOf('    secrets:') + 600).split('\n').slice(0, 6).join('\n'); };
    assert.equal(secrets(nightly, `installers-${app}`), secrets(own, 'installers'), `${app}: the same secrets`);
    const job = nightly.slice(nightly.indexOf(`\n  installers-${app}:`));
    assert.match(job.slice(0, job.indexOf('    with:')), /needs: \[installers-gate\]\n    if: \$\{\{ needs\.installers-gate\.outputs\.run == 'true' \}\}/, `${app}: only the gate, a job that always runs`);
  }
  const verdictJob = nightly.slice(nightly.indexOf('\n  installers-verdict:'));
  for (const app of APPS) assert.match(verdictJob.slice(0, verdictJob.indexOf('steps:')), new RegExp(`installers-${app}`), `${app}: in the verdict's needs`);
  assert.match(verdictJob, /if: \$\{\{ always\(\) \}\}/);
  for (const app of APPS) assert.match(verdictJob, new RegExp(`app: ${app}\\n\\s+plan-run: \\$\\{\\{ needs\\.installers-gate\\.outputs\\.run \\}\\}`), `${app}: the verdict knows the gate's answer`);
  assert.match(nightly, /\n  installers-gate:\n(?:.*\n)*?.*nightly-installers-\$\{\{ github\.sha \}\}-\$\{\{ hashFiles\('ci-image\/\*\*', 'rust-toolchain\*', '\.nvmrc'\) \}\}/, 'the gate keys on the commit, the toolchain image and the pinned Rust and Node');
  assert.match(nightly, /\n  installers-mark:\n(?:.*\n)*?    needs: \[installers-dyno, installers-spotter, installers-pitcrew, installers-pitwall\]/, 'the marker is saved only after every build');
});

test('the release-verdict action is documented and runs the script', () => {
  assert.match(read('.gitea', 'actions', 'release-verdict', 'action.yml'), /conformance\/ci\/release-verdict\.mjs/);
  assert.match(read('.gitea', 'actions', 'README.md'), /\| `release-verdict` \|/);
});
