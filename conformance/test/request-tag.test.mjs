// The tag request (.gitea/workflows/tag.yml, conformance/ci/request-tag.mjs): the checks, and the workflow's structure.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkGreenRun, checkNotSkipCi, checkVersion, gateWorkflow, parseTag, runChecks, summary } from '../ci/request-tag.mjs';
import { PADDOCK } from './helpers.mjs';

const SHA = 'a'.repeat(40);
const files = (o) => (p) => o[p] ?? null;
const conf = (v) => JSON.stringify({ version: v });

test('tag names: <app>-vX.Y.Z and vX.Y.Z, nothing else', () => {
  assert.deepEqual(parseTag('pitcrew-v1.2.3'), { app: 'pitcrew', version: '1.2.3' });
  assert.deepEqual(parseTag('shell-demo-v0.1.0-rc.1'), { app: 'shell-demo', version: '0.1.0-rc.1' });
  assert.deepEqual(parseTag('v0.3.0'), { app: null, version: '0.3.0' });
  for (const bad of ['pitcrew-1.2.3', 'pitcrew-v1.2', 'Pitcrew-v1.2.3', 'pitcrew-v1.2.3;rm', 'v1.2.3 ', '-v1.2.3', 'latest']) assert.throws(() => parseTag(bad), /is not/, bad);
});

test('a skip-ci commit is refused, every spelling', () => {
  for (const m of ['x [skip ci]', 'x\n\n[CI SKIP]', 'x [no ci]', 'x [skip actions]']) assert.throws(() => checkNotSkipCi(m, SHA), /skip-ci/, m);
  checkNotSkipCi('fix: skip ci handling in the docs', SHA);
});

test('the version source must say the tag version (tauri.conf.json, a demo Cargo.toml, the ui package)', () => {
  const c = 'apps/dyno/app/src-tauri/tauri.conf.json';
  assert.equal(checkVersion('dyno', 'dyno-v1.1.0', files({ [c]: conf('1.1.0') })), '1.1.0');
  assert.throws(() => checkVersion('dyno', 'dyno-v1.1.0', files({ [c]: conf('1.0.0') })), /says 1\.1\.0, tauri\.conf\.json says 1\.0\.0/);
  const cargo = 'apps/shell-demo/src-tauri/Cargo.toml';
  assert.equal(checkVersion('shell-demo', 'shell-demo-v0.1.0', files({ [cargo]: '[package]\nname = "x"\nversion = "0.1.0"\n' })), '0.1.0');
  assert.throws(() => checkVersion('shell-demo', 'shell-demo-v0.2.0', files({ [cargo]: '[package]\nversion = "0.1.0"\n' })), /Cargo\.toml says 0\.1\.0/);
  const ui = 'ui/projects/paddock-ui/package.json';
  assert.equal(checkVersion(null, 'v0.3.0', files({ [ui]: conf('0.3.0') })), '0.3.0');
  assert.throws(() => checkVersion(null, 'v0.4.0', files({ [ui]: conf('0.3.0') })), /says 0\.4\.0/);
  assert.throws(() => checkVersion('nope', 'nope-v1.0.0', files({})), /no version source/);
  assert.throws(() => checkVersion(null, 'v1.0.0', files({})), /cannot be read/);
});

test('the gate workflow is the app\'s own, else ci.yml', () => {
  assert.equal(gateWorkflow('debrief', files({ 'apps/debrief/paddock.ci.json': '{"workflow":"debrief.yml"}' })), 'debrief.yml');
  assert.equal(gateWorkflow('spotter', files({ 'apps/spotter/paddock.ci.json': '{"cargo":"."}' })), 'ci.yml');
  assert.equal(gateWorkflow(null, files({})), 'ci.yml');
});

const run = (o) => ({ id: 1, head_sha: SHA, path: 'ci.yml@refs/heads/main', status: 'completed', conclusion: 'success', ...o });

test('a green run needs the exact sha, the right workflow, completed and success; none listed is not measured', () => {
  assert.equal(checkGreenRun([run({})], SHA, 'ci.yml').id, 1);
  assert.throws(() => checkGreenRun([], SHA, 'ci.yml'), /not measured/);
  assert.throws(() => checkGreenRun([run({ head_sha: 'b'.repeat(40) })], SHA, 'ci.yml'), /not measured/);
  assert.throws(() => checkGreenRun([run({ path: 'nightly.yml@refs/heads/main' })], SHA, 'ci.yml'), /not measured/);
  assert.throws(() => checkGreenRun([run({ conclusion: 'failure' })], SHA, 'ci.yml'), /is failure, not success/);
  assert.throws(() => checkGreenRun([run({ status: 'in_progress', conclusion: '' })], SHA, 'ci.yml'), /in_progress/);
  assert.throws(() => checkGreenRun([run({ conclusion: 'cancelled' })], SHA, 'ci.yml'), /cancelled/);
});

test('the newest run decides: an old green run does not cover a newer red one', () => {
  assert.throws(() => checkGreenRun([run({ id: 1 }), run({ id: 2, conclusion: 'failure' })], SHA, 'ci.yml'), /run \(2\)/);
  assert.equal(checkGreenRun([run({ id: 1, conclusion: 'failure' }), run({ id: 2 })], SHA, 'ci.yml').id, 2);
});

function world(over = {}) {
  const c = 'apps/dyno/app/src-tauri/tauri.conf.json';
  return {
    tag: 'dyno-v1.1.0',
    sha: SHA,
    read: files({ [c]: conf('1.1.0'), 'apps/dyno/paddock.ci.json': '{"workflow":"dyno.yml"}' }),
    git: (a) => (a[0] === 'branch' ? '  origin/main\n' : a[0] === 'log' ? 'feat: x\n' : ''),
    fetchRuns: async () => [run({ path: 'dyno.yml@refs/heads/main' })],
    ...over,
  };
}

test('all checks pass on a good request', async () => {
  const r = await runChecks(world());
  assert.equal(r.length, 7);
  assert.ok(r.every((x) => x.ok), JSON.stringify(r.filter((x) => !x.ok)));
  assert.match(summary('dyno-v1.1.0', SHA, r), /All checks passed/);
});

test('each failing check is reported by name, all of them, and the summary says no tag was made', async () => {
  const r = await runChecks(
    world({
      git: (a) => (a[0] === 'branch' ? '' : a[0] === 'log' ? 'x [skip ci]' : 'abc\trefs/tags/dyno-v1.1.0'),
      read: files({}),
      fetchRuns: async () => {
        throw new Error('not measured: HTTP 401');
      },
    }),
  );
  assert.deepEqual(r.filter((x) => !x.ok).map((x) => x.name), ['commit is on origin', 'not a skip-ci commit', 'tag does not exist', 'version matches the tag', 'CI is green on exactly this commit']);
  assert.match(summary('dyno-v1.1.0', SHA, r), /Refused: 5 check\(s\) failed\.\*\* No tag was made/);
});

test('a bad tag or sha stops early with the input failure', async () => {
  assert.equal((await runChecks(world({ tag: 'dyno-1.1.0' }))).filter((x) => !x.ok)[0].name, 'tag name');
  const r = await runChecks(world({ sha: 'abc' }));
  assert.equal(r.filter((x) => !x.ok)[0].name, 'sha format');
  assert.equal(r.length, 2);
});

// ---- the workflow's structure -------------------------------------------------------------------------------------------------
const wf = readFileSync(join(PADDOCK, '.gitea', 'workflows', 'tag.yml'), 'utf8');
const code = wf.split('\n').filter((l) => !l.trim().startsWith('#')).join('\n');

test('tag.yml: manual only, with the two inputs, validated before anything runs', () => {
  assert.match(code, /^on:\n  workflow_dispatch:\n    inputs:\n      tag:[\s\S]*?\n      sha:/m);
  assert.doesNotMatch(code, /^\s{2}(push|pull_request|schedule):/m);
  assert.match(code, /\^\[a-z0-9\]\+\(-\[a-z0-9\]\+\)\*-v\[0-9\]\+/);
  assert.match(code, /\^\[0-9a-f\]\{40\}\$/);
  assert.ok(code.indexOf('name: Inputs') < code.indexOf('actions/checkout'), 'inputs are checked before the checkout');
});

test('tag.yml: inputs reach scripts only through env, never spliced into run text', () => {
  const lines = code.split('\n');
  const runs = [];
  lines.forEach((l, i) => {
    if (!/^\s+run: \|\s*$/.test(l)) return;
    const base = l.length - l.trimStart().length;
    for (let j = i + 1; j < lines.length && (lines[j].trim() === '' || lines[j].length - lines[j].trimStart().length > base); j++) runs.push(lines[j]);
  });
  assert.ok(runs.length > 10, 'run blocks were found');
  assert.doesNotMatch(runs.join('\n'), /\$\{\{\s*inputs\./);
});

test('tag.yml: the bot token is in the push step only, masked, and nothing echoes it', () => {
  const steps = code.split(/^ {6}- /m);
  const holders = steps.filter((s) => /secrets\.RELEASE_BOT_TOKEN/.test(s));
  assert.equal(holders.length, 1);
  assert.match(holders[0], /name: Make the tag as release-bot and push it/);
  assert.match(holders[0], /::add-mask::\$RELEASE_BOT_TOKEN/);
  assert.doesNotMatch(code, /echo[^\n]*\$RELEASE_BOT_TOKEN(?!")|set -x|-x\b.*bash/);
  assert.doesNotMatch(code, /secrets\.RELEASE_BOT_TOKEN[\s\S]*secrets\.RELEASE_BOT_TOKEN/);
  assert.match(code, /persist-credentials: false/);
});

test('tag.yml: minimal permissions, a Linux runner, no skip-ci text, tag-keyed concurrency without cancelling', () => {
  assert.match(code, /^permissions:\n  contents: read\n/m);
  assert.doesNotMatch(code, /write-all|contents: write/);
  assert.match(code, /runs-on: ubuntu-latest/);
  assert.doesNotMatch(wf, /\[(skip ci|ci skip|no ci|skip actions)\]/i);
  assert.match(code, /group: tag-\$\{\{ inputs\.tag \}\}\n\s+cancel-in-progress: false/);
});

test('tag.yml: the checks run before the push, and the push step makes an annotated tag of the sha', () => {
  const at = (s) => code.indexOf(s);
  assert.ok(at('request-tag.mjs') < at('Tag gates (apps)') && at('Tag gates (apps)') < at('Make the tag as release-bot'));
  assert.match(code, /git [^\n]*tag -a "\$TAG" "\$SHA"/);
  assert.match(code, /push "[^"]+" "refs\/tags\/\$TAG"/);
  assert.match(code, /::error::tag:[^\n]*Check that release-bot/);
  assert.match(code, /GITHUB_STEP_SUMMARY/);
});
