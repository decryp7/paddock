// The tag rehearsal (.gitea/workflows/tag-rehearsal.yml; user, 2026-11-20): a tag's release path run daily without a tag, a release or a publish.
// Two release-path failures showed only on a real tag (run 8384: a bare `dev` spawn; run 8395: Dyno's macOS helper unsigned in the key-stripped hook).
// Every control here is paired with its positive control: the check is shown to go red when the thing it watches is removed.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { isRehearsal, isTagRun } from '../src/util/is-tag-run.mjs';
import { rehearse, requireRehearsal } from '../ci/rehearse-release.mjs';
import { runChecks, summary } from '../ci/request-tag.mjs';
import { makeApp } from './helpers.mjs';
import { expr, job, loadAction, loadWorkflow, needsOf, stepsOf } from './lib/workflow.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const CI = join(HERE, '..', 'ci');
const SIG = join(HERE, 'fixtures', 'updater-sig');
const PUB = readFileSync(join(SIG, 'key.pub'), 'utf8').trim();
const REHEARSE = { PADDOCK_TAG_REHEARSAL: '1', GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/heads/main' };
const IS_TAG_RUN = join(HERE, '..', 'src', 'util', 'is-tag-run.mjs');

// ---- is-tag-run: the one definition ----
test('rehearsal: PADDOCK_TAG_REHEARSAL=1 on a branch answers "tag"; 0 and empty change nothing', () => {
  assert.equal(isTagRun(REHEARSE), true);
  assert.equal(isRehearsal(REHEARSE), true);
  assert.equal(isTagRun({ ...REHEARSE, PADDOCK_TAG_REHEARSAL: '0' }), false);
  assert.equal(isTagRun({ ...REHEARSE, PADDOCK_TAG_REHEARSAL: '' }), false);
  assert.equal(isTagRun({ GITHUB_REF: 'refs/heads/main' }), false, 'positive control: without the switch a branch is not a tag');
});

test('rehearsal with a real tag fails closed, in every way a tag shows (ref, either ref type); so does a bad value', () => {
  for (const env of [{ GITHUB_REF: 'refs/tags/dyno-v1.0.0' }, { GITEA_REF: 'refs/tags/dyno-v1.0.0' }, { GITHUB_REF_TYPE: 'tag' }, { GITEA_REF_TYPE: 'tag' }]) {
    assert.throws(() => isTagRun({ PADDOCK_TAG_REHEARSAL: '1', ...env }), /never runs on a tag/);
    const r = spawnSync(process.execPath, [IS_TAG_RUN], { env: { PADDOCK_TAG_REHEARSAL: '1', ...env }, encoding: 'utf8' });
    assert.equal(r.status, 4, JSON.stringify(env));
    assert.match(r.stdout + r.stderr, /::error::is-tag-run:/);
  }
  for (const v of ['true', 'yes', '2', ' 1']) assert.throws(() => isTagRun({ PADDOCK_TAG_REHEARSAL: v }), /is not 1, 0 or empty/);
  assert.equal(spawnSync(process.execPath, [IS_TAG_RUN], { env: { PADDOCK_TAG_REHEARSAL: 'true' }, encoding: 'utf8' }).status, 4);
  // positive controls: the same tag without the switch is a tag (exit 0), the switch on a branch is a tag (exit 0)
  assert.equal(spawnSync(process.execPath, [IS_TAG_RUN], { env: { GITHUB_REF: 'refs/tags/dyno-v1.0.0' }, encoding: 'utf8' }).status, 0);
  assert.equal(spawnSync(process.execPath, [IS_TAG_RUN], { env: REHEARSE, encoding: 'utf8' }).status, 0);
});

// ---- tauri-bundle.sh / updater-key.sh under the rehearsal ----
function bundleRepo() {
  const root = makeApp({
    'app/src-tauri/Cargo.toml': '',
    'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.2.3' }),
    'bin/npm': '#!/bin/bash\necho "npm $*" >> "$BIN_LOG"\n',
    'bin/npx': [
      '#!/bin/bash',
      'if [ "$1 $2 $3" = "tauri signer generate" ]; then',
      '  echo "npx-args $*" >> "$BIN_LOG"',
      '  while [ $# -gt 0 ]; do case "$1" in --write-keys) f="$2"; shift ;; esac; shift; done',
      `  printf "THROWAWAY-SK\\n" > "$f"; printf "%s\\n" "${PUB}" > "$f.pub"; exit 0`,
      'fi',
      'echo "npx-args $*" >> "$BIN_LOG"',
      'echo "npx-env key=${TAURI_SIGNING_PRIVATE_KEY:-unset}|pub=${TAURI_UPDATER_PUBKEY:-unset}" >> "$BIN_LOG"',
      'd="${PADDOCK_BUILD_FOLDER:-$CARGO_TARGET_DIR}"',
      'mkdir -p "$d/dist/bundle/nsis"',
      `cp "${join(SIG, 'Demo_1.2.3_x64-setup.exe')}" "${join(SIG, 'Demo_1.2.3_x64-setup.exe.sig')}" "$d/dist/bundle/nsis/"`,
    ].join('\n'),
  });
  chmodSync(join(root, 'bin/npm'), 0o755);
  chmodSync(join(root, 'bin/npx'), 0o755);
  return root;
}
const bundle = (root, env, ci = CI) =>
  spawnSync('bash', [join(ci, 'tauri-bundle.sh'), 'deb'], {
    cwd: root,
    encoding: 'utf8',
    env: { PATH: `${join(root, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, CARGO_TARGET_DIR: join(root, 'target'), BIN_LOG: join(root, 'calls.log'), ...env },
  });
const logOf = (root) => readFileSync(join(root, 'calls.log'), 'utf8');

test('rehearsal: the release path signs with a throw-away key made in the job, even when a (real) public key is given', () => {
  const root = bundleRepo();
  const r = bundle(root, { ...REHEARSE, TAURI_UPDATER_PUBKEY: 'REALPUB' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(logOf(root), /^npx-args tauri signer generate --ci --force --write-keys /m, 'a key pair was made');
  assert.match(logOf(root), new RegExp(`^npx-env key=THROWAWAY-SK\\|pub=${PUB.replace(/[+/=]/g, '\\$&')}$`, 'm'), 'the build got the throw-away pair, not REALPUB');
  assert.doesNotMatch(logOf(root), /REALPUB/);
  assert.match(r.stdout, /throw-away updater key[^\n]*tag rehearsal/);
  // and the signatures were verified against that key (the real release path's last step)
  assert.match(r.stdout, /updater-key-check: 1 signature\(s\) verify/);
});

test('rehearsal: the real private key is not reachable: a key in the environment stops the step before anything runs, and is never printed', () => {
  const root = bundleRepo();
  const r = bundle(root, { ...REHEARSE, TAURI_SIGNING_PRIVATE_KEY: 'SECRETKEY', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'SECRETPW', TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout, /::error::A private updater key reached the tag rehearsal/);
  assert.ok(!existsSync(join(root, 'calls.log')), 'neither npm ci nor the build ran');
  assert.doesNotMatch(r.stdout + r.stderr, /SECRETKEY|SECRETPW/);
  // positive control: the same key on a real tag is accepted (the rehearsal's refusal is not a general refusal)
  const real = bundleRepo();
  const t = bundle(real, { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/tags/demo-v1.2.3', TAURI_SIGNING_PRIVATE_KEY: 'SECRETKEY', TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(t.status, 0, t.stdout + t.stderr);
  assert.match(logOf(real), /npx-env key=SECRETKEY/);
});

test('rehearsal on a real tag fails closed in the build script, before anything runs', () => {
  const root = bundleRepo();
  const r = bundle(root, { PADDOCK_TAG_REHEARSAL: '1', GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/tags/demo-v1.2.3', TAURI_UPDATER_PUBKEY: PUB });
  assert.notEqual(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /::error::is-tag-run\.mjs refused \(exit 4\)/);
  assert.ok(!existsSync(join(root, 'calls.log')), 'nothing ran: no npm ci, no key, no build');
});

for (const [label, stub] of [['exits 1', 'process.exit(1);'], ['throws', "throw new Error('boom');"], ['is a syntax error', 'this is not javascript'], ['says "not a tag" (3)', 'process.exit(3);']]) {
  test(`rehearsal: an is-tag-run.mjs that ${label} fails the rehearsal, never a pass`, () => {
    const root = bundleRepo();
    const ci = join(mkdtempSync(join(tmpdir(), 'paddock-rehearsal-ci-')), 'conformance');
    for (const d of ['ci', 'release', 'src/util']) cpSync(join(CI, '..', d), join(ci, d), { recursive: true });
    writeFileSync(join(ci, 'src/util/is-tag-run.mjs'), `${stub}\n`);
    const r = bundle(root, { ...REHEARSE, TAURI_UPDATER_PUBKEY: PUB }, join(ci, 'ci'));
    assert.notEqual(r.status, 0, r.stdout + r.stderr);
    assert.match(r.stdout, /::error::is-tag-run\.mjs (exited [1-9]\d*|said "not a tag")/);
    assert.ok(!existsSync(join(root, 'calls.log')), 'nothing ran: no npm ci, no throw-away key, no build');
  });
}

// ---- rehearse-release.mjs: the release job's work, read back, never published ----
function releaseDir() {
  const dir = makeApp({ 'CHANGELOG.md': '## [1.2.3]\n\n- A change\n' });
  const rel = join(dir, 'release');
  mkdirSync(rel);
  cpSync(join(SIG, 'Demo_1.2.3_x64-setup.exe'), join(rel, 'Demo_1.2.3_x64-setup.exe'));
  cpSync(join(SIG, 'Demo_1.2.3_x64-setup.exe.sig'), join(rel, 'Demo_1.2.3_x64-setup.exe.sig'));
  return { dir, rel };
}

test('rehearse-release: makes the notes and latest.json of a release from signed installers and reads them back; its base URL is not a download address', () => {
  const { dir, rel } = releaseDir();
  const r = rehearse({ appName: 'Demo', changelog: join(dir, 'CHANGELOG.md'), dir: rel, version: '1.2.3', env: REHEARSE });
  assert.ok(r.platforms.some((p) => p.startsWith('windows-')), r.platforms.join());
  const latest = JSON.parse(readFileSync(join(rel, 'latest.json'), 'utf8'));
  assert.equal(latest.version, '1.2.3');
  for (const e of Object.values(latest.platforms)) assert.match(e.url, /^https:\/\/rehearsal\.invalid\//);
  assert.ok(existsSync(join(rel, 'RELEASE_NOTES.md')));
});

test('rehearse-release: unsigned installers fail it (positive control: a release would publish no latest.json)', () => {
  const { dir, rel } = releaseDir();
  rmSyncOne(join(rel, 'Demo_1.2.3_x64-setup.exe.sig'));
  assert.throws(() => rehearse({ appName: 'Demo', changelog: join(dir, 'CHANGELOG.md'), dir: rel, version: '1.2.3', env: REHEARSE }), /no \.sig next to any installer/);
});

test('rehearse-release: refuses outside a rehearsal and on a real tag; imports no publisher and reads no token', () => {
  const { dir, rel } = releaseDir();
  const args = { appName: 'Demo', changelog: join(dir, 'CHANGELOG.md'), dir: rel, version: '1.2.3' };
  assert.throws(() => rehearse({ ...args, env: { GITHUB_REF: 'refs/heads/main' } }), /PADDOCK_TAG_REHEARSAL is not 1/);
  assert.throws(() => rehearse({ ...args, env: { PADDOCK_TAG_REHEARSAL: '1', GITHUB_REF: 'refs/tags/demo-v1.2.3' } }), /never runs on a tag|real tag/);
  assert.doesNotThrow(() => requireRehearsal(REHEARSE));
  assert.ok(!existsSync(join(rel, 'latest.json')), 'nothing was made by a refused run');
  const code = readFileSync(join(CI, 'rehearse-release.mjs'), 'utf8').replace(/^\s*\/\/.*$/gm, '');
  assert.doesNotMatch(code, /publish\.mjs|publish-release|fetch\(|https?\.request|GITEA_TOKEN|RELEASE_TOKEN|process\.env\.[A-Z_]*TOKEN/);
});
function rmSyncOne(p) {
  writeFileSync(p, '');
  spawnSync('rm', ['-f', p]);
}

// ---- request-tag.mjs --dry-run ----
const SHA = 'a'.repeat(40);
const files = (o) => (p) => o[p] ?? null;
const green = async () => ({ status: 0, stdout: JSON.stringify({ verdict: 'green', run: 7 }) });
const gitOn = (tagExists) => (args) => (args[0] === 'branch' ? '  origin/main\n' : args[0] === 'ls-remote' ? (tagExists ? `${SHA}\trefs/tags/dyno-v1.2.0\n` : '') : 'a commit message');
const base = { tag: 'dyno-v1.2.0', sha: SHA, read: files({ 'apps/dyno/app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.2.0' }) }), verdict: green };

test('tag checks in dry-run mode: the same checks; only "tag does not exist" is a named exception, and a red verdict is still red', async () => {
  const dry = await runChecks({ ...base, git: gitOn(true), dryRun: true });
  assert.deepEqual(dry.filter((r) => !r.ok), [], JSON.stringify(dry));
  assert.match(dry.find((r) => r.name === 'tag does not exist').detail, /skipped \(dry run/);
  // positive controls: a real request for an existing tag is refused, and the dry run still goes red on every other check
  assert.equal((await runChecks({ ...base, git: gitOn(true) })).filter((r) => !r.ok)[0].name, 'tag does not exist');
  const red = await runChecks({ ...base, git: gitOn(true), dryRun: true, verdict: async () => ({ status: 5, stdout: JSON.stringify({ verdict: 'red', detail: 'run 9 is red' }) }) });
  assert.deepEqual(red.filter((r) => !r.ok).map((r) => r.name), ['CI is green on exactly this commit']);
  const wrongVersion = await runChecks({ ...base, tag: 'dyno-v9.9.9', git: gitOn(false), dryRun: true });
  assert.ok(wrongVersion.some((r) => r.name === 'version matches the tag' && !r.ok));
  assert.match(summary('dyno-v1.2.0', SHA, dry, true), /Tag rehearsal \(dry run, no tag is made\)/);
});

// ---- the workflows ----
const APPS = ['dyno', 'pitcrew', 'pitwall', 'spotter'];
const rehearsal = loadWorkflow('tag-rehearsal.yml');
const appRelease = loadWorkflow('app-release.yml');
const text = (f) => readFileSync(join(HERE, '..', '..', '.gitea', 'workflows', f), 'utf8');

test('tag-rehearsal.yml: a schedule and a dispatch, main only, and it publishes nothing: no secret, no token, no push, no release step', () => {
  const on = rehearsal.on ?? rehearsal.true;
  assert.ok(on.schedule?.length === 1 && 'workflow_dispatch' in on);
  assert.match(expr(job(rehearsal, 'guard').steps[0].run), /refs\/heads\/main/);
  // the whole file, comments left out: nothing of a publisher or a credential
  const code = text('tag-rehearsal.yml').replace(/^\s*#.*$/gm, '');
  assert.doesNotMatch(code, /RELEASE_BOT_TOKEN|RELEASE_TOKEN|TAURI_SIGNING|APPLE_SIGNING|PADDOCK_TAURI|publish-release|publish\.mjs|git push|git tag|\bgit -c [^\n]* tag /);
  assert.doesNotMatch(code, /contents: write/);
  // the only secret read is the dev tool's read token (BUILD_INPUTS_TOKEN)
  assert.deepEqual([...new Set([...code.matchAll(/secrets\.([A-Z_]+)/g)].map((m) => m[1]))], ['BUILD_INPUTS_TOKEN']);
  for (const id of Object.keys(rehearsal.jobs)) assert.equal(rehearsal.jobs[id].secrets, undefined, `${id} passes no secrets block`);
});

test('tag-rehearsal.yml: each app calls app-release.yml with rehearsal: true and the same inputs as its tag release job (no drift)', () => {
  for (const app of APPS) {
    const j = job(rehearsal, `rehearse-${app}`);
    assert.equal(j.uses, './.gitea/workflows/app-release.yml');
    assert.equal(j.with.rehearsal, true);
    const real = job(loadWorkflow(`${app}-release.yml`), 'release');
    const { rehearsal: _r, ...mine } = j.with;
    assert.deepEqual(mine, real.with, `${app}: the rehearsal's inputs are the release job's`);
    // positive control: the comparison sees a changed input
    assert.notDeepEqual({ ...mine, 'app-id': 'other' }, real.with);
  }
});

test('tag-rehearsal.yml: one app at a time (chained, ER-24), after the guard; a train in CI skips it; the verdict is red with the step named', () => {
  assert.deepEqual(needsOf(job(rehearsal, 'rehearse-dyno')), ['guard', 'tag-checks']);
  for (let i = 1; i < APPS.length; i++) assert.deepEqual(needsOf(job(rehearsal, `rehearse-${APPS[i]}`)), ['guard', `rehearse-${APPS[i - 1]}`]);
  for (const app of APPS) assert.match(expr(job(rehearsal, `rehearse-${app}`).if), /needs\.guard\.outputs\.skip != 'true'/);
  const guard = job(rehearsal, 'guard');
  assert.ok(stepsOf(guard).some((s) => s.uses === './.gitea/actions/schedule-skip' && s.with.workflow === 'tag-rehearsal.yml'));
  const verdict = job(rehearsal, 'verdict');
  assert.equal(expr(verdict.if), 'always()');
  assert.match(stepsOf(verdict)[0].run, /::error::tag rehearsal: failed step\(s\)/);
  assert.match(stepsOf(verdict)[0].run, /not measured, so not a pass/);
});

test('tag-rehearsal.yml: the tag checks run the same commands as tag.yml, in dry-run mode, from the repository root, and fail with a named step', () => {
  const run = stepsOf(job(rehearsal, 'tag-checks')).map((s) => s.run ?? '').join('\n');
  const tagYml = stepsOf(job(loadWorkflow('tag.yml'), 'check')).map((s) => s.run ?? '').join('\n');
  assert.match(run, /node conformance\/ci\/request-tag\.mjs [^\n]*--dry-run true/);
  for (const cmd of ['./dev gates ratchet --root', './dev check ipc-endpoints --root']) assert.ok(run.includes(cmd) && tagYml.includes(cmd), cmd);
  assert.match(run, /::error::tag rehearsal: step 'tag checks' failed/);
  assert.match(run, /::error::tag rehearsal: step 'tag gates' failed/);
});

test('app-release.yml: the rehearsal never reaches the publish job, which a test of its own condition shows', () => {
  const release = job(appRelease, 'release');
  assert.ok(stepsOf(release).some((s) => s.uses === './.paddock/.gitea/actions/publish-release'), 'positive control: the real release job does publish');
  assert.match(expr(release.if), /&& inputs\.rehearsal != true$/);
  const rehearseJob = job(appRelease, 'rehearse-release');
  assert.equal(expr(rehearseJob.if), 'inputs.rehearsal == true');
  assert.deepEqual(rehearseJob.permissions, { contents: 'read' });
  const body = JSON.stringify(rehearseJob);
  assert.doesNotMatch(body, /publish-release|publish\.mjs|RELEASE_TOKEN|contents.*write/);
  // the signing secrets are only ever given to the build on the app's release tag, never because of the rehearsal
  const bundleSteps = JSON.stringify(stepsOf(job(appRelease, 'bundle')));
  assert.doesNotMatch(bundleSteps, /secrets\.TAURI_SIGNING_PRIVATE_KEY[^}]*rehearsal|rehearsal[^}]*secrets\.TAURI_SIGNING_PRIVATE_KEY/);
  assert.equal(appRelease.env.PADDOCK_TAG_REHEARSAL, "${{ inputs.rehearsal && '1' || '' }}");
  assert.match(stepsOf(job(appRelease, 'bundle')).find((s) => s.name === 'Rehearsal guard').run, /refs\/tags\/\*\) echo "::error::tag rehearsal: this run is a real tag/);
});

test('publish-release: refuses PADDOCK_TAG_REHEARSAL=1 before reading anything or sending anything', () => {
  const run = stepsOf(loadAction('publish-release'))[0].run;
  const refuse = run.indexOf('PADDOCK_TAG_REHEARSAL');
  assert.ok(refuse >= 0, 'the guard exists');
  assert.ok(refuse < run.indexOf('release-notes.mjs') && refuse < run.indexOf('publish.mjs'), 'and runs first');
  // behaviour: run the guard line itself under bash with the variable set, and unset
  const line = run.split('\n').find((l) => l.includes('PADDOCK_TAG_REHEARSAL'));
  const on = spawnSync('bash', ['-c', line], { env: { PADDOCK_TAG_REHEARSAL: '1', PATH: process.env.PATH }, encoding: 'utf8' });
  assert.equal(on.status, 1);
  assert.match(on.stdout, /::error::publish-release was reached in a tag rehearsal/);
  assert.equal(spawnSync('bash', ['-c', line], { env: { PATH: process.env.PATH }, encoding: 'utf8' }).status, 0);
});
