// The tag request (.gitea/workflows/tag.yml, conformance/ci/request-tag.mjs): the checks, and the workflow's structure.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkNotSkipCi, checkVerdict, checkVersion, gateWorkflow, parseTag, runChecks, summary } from '../ci/request-tag.mjs';
import { PADDOCK } from './helpers.mjs';
import { job as jobOf, loadWorkflow, needsOf } from './lib/workflow.mjs';

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

const res = (status, o = {}) => ({ status, stdout: JSON.stringify(o), stderr: '' });

test('the verdict of ./dev: only exit 0 with a green verdict passes; 5, 6 and silence never do', () => {
  assert.equal(checkVerdict(res(0, { verdict: 'green', run: 7, detail: 'x' })), 7);
  assert.throws(() => checkVerdict(res(5, { verdict: 'red', detail: 'ci.yml on aaaaaaaaaa: red or partial. run 9 failed' })), /run 9 failed.*request the tag/);
  assert.throws(() => checkVerdict(res(6, { verdict: 'not_measured', detail: 'not measured. no run' })), /not measured. no run/);
  // Exit 0 without a green verdict, a crash, a missing tool and unreadable output are all "not measured".
  assert.throws(() => checkVerdict(res(0, {})), /not measured: .*did not answer/);
  assert.throws(() => checkVerdict(res(1, { ok: false, error: 'boom' })), /did not answer \(exit 1: boom\)/);
  assert.throws(() => checkVerdict({ status: null, error: new Error('ENOENT') }), /did not answer \(exit none: ENOENT\)/);
  assert.throws(() => checkVerdict({ status: 0, stdout: 'not json', stderr: '' }), /did not answer/);
});

test('the verdict is read from the last line, so tool chatter before it does not matter', () => {
  const out = `building...\n${JSON.stringify({ verdict: 'green', run: 3 })}\n`;
  assert.equal(checkVerdict({ status: 0, stdout: out, stderr: '' }), 3);
});

function world(over = {}) {
  const c = 'apps/dyno/app/src-tauri/tauri.conf.json';
  return {
    tag: 'dyno-v1.1.0',
    sha: SHA,
    read: files({ [c]: conf('1.1.0'), 'apps/dyno/paddock.ci.json': '{"workflow":"dyno.yml"}' }),
    git: (a) => (a[0] === 'branch' ? '  origin/main\n' : a[0] === 'log' ? 'feat: x\n' : ''),
    verdict: async () => res(0, { verdict: 'green', run: 1 }),
    ...over,
  };
}

test('all checks pass on a good request', async () => {
  const r = await runChecks(world());
  assert.equal(r.length, 7);
  assert.ok(r.every((x) => x.ok), JSON.stringify(r.filter((x) => !x.ok)));
  assert.match(summary('dyno-v1.1.0', SHA, r), /All checks passed/);
});

test('P1-3: a commit only on a feature branch is refused; main and release/* are accepted', async () => {
  const on = (b) => world({ git: (a) => (a[0] === 'branch' ? b : a[0] === 'log' ? 'feat: x' : '') });
  for (const b of ['  origin/feature/x\n', '  origin/fix/y\n  origin/mainline\n', '  origin/release\n']) {
    assert.equal((await runChecks(on(b))).find((x) => !x.ok)?.name, 'commit is on origin main or release/*', b);
  }
  for (const b of ['  origin/main\n', '  origin/release/1.2\n  origin/feature/x\n']) assert.ok((await runChecks(on(b))).every((x) => x.ok), b);
});

test('each failing check is reported by name, all of them, and the summary says no tag was made', async () => {
  const r = await runChecks(
    world({
      git: (a) => (a[0] === 'branch' ? '' : a[0] === 'log' ? 'x [skip ci]' : 'abc\trefs/tags/dyno-v1.1.0'),
      read: files({}),
      verdict: async () => res(6, { verdict: 'not_measured', detail: 'not measured. HTTP 403' }),
    }),
  );
  assert.deepEqual(r.filter((x) => !x.ok).map((x) => x.name), ['commit is on origin main or release/*', 'not a skip-ci commit', 'tag does not exist', 'version matches the tag', 'CI is green on exactly this commit']);
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
/** The text of one job (`  check:` up to the next job). */
const job = (id) => {
  const m = new RegExp(`^  ${id}:\\n([\\s\\S]*?)(?=^  [a-z]+:\\n|(?![\\s\\S]))`, 'm').exec(code);
  assert.ok(m, `job ${id}`);
  return m[1];
};
const check = job('check');
const push = job('push');
const runBlocks = (text) => {
  const lines = text.split('\n');
  const out = [];
  lines.forEach((l, i) => {
    if (!/^\s+run: \|\s*$/.test(l)) return;
    const base = l.length - l.trimStart().length;
    for (let j = i + 1; j < lines.length && (lines[j].trim() === '' || lines[j].length - lines[j].trimStart().length > base); j++) out.push(lines[j]);
  });
  return out.join('\n');
};

test('tag.yml: manual only, with the two inputs, validated before anything runs', () => {
  assert.match(code, /^on:\n  workflow_dispatch:\n    inputs:\n      tag:[\s\S]*?\n      sha:/m);
  assert.doesNotMatch(/^on:\n([\s\S]*?)^permissions:/m.exec(code)[1], /^\s{2}(push|pull_request|schedule):/m);
  assert.match(check, /\^\[a-z0-9\]\+\(-\[a-z0-9\]\+\)\*-v\[0-9\]\+/);
  assert.match(check, /\^\[0-9a-f\]\{40\}\$/);
  assert.ok(check.indexOf('name: Inputs') < check.indexOf('actions/checkout'), 'inputs are checked before the checkout');
});

test('tag.yml: inputs reach scripts only through env, never spliced into run text', () => {
  const runs = runBlocks(code);
  assert.ok(runs.split('\n').length > 20, 'run blocks were found');
  assert.doesNotMatch(runs, /\$\{\{\s*inputs\./);
  assert.doesNotMatch(runs, /\$\{\{\s*secrets\./);
});

test('P1-1: the check job names actions: read (unnamed scopes are None on Gitea 28)', () => {
  assert.match(check, /permissions:\n      contents: read\n      actions: read\n/);
  assert.doesNotMatch(push, /actions: read/);
  assert.match(push, /permissions:\n      contents: read\n/);
  assert.doesNotMatch(code, /write-all|: write/);
  assert.match(code, /^permissions:\n  contents: read\n/m);
});

test('P1-2: both jobs run only from main, the checker comes from main in its own folder, the tagged tree is never checked out', () => {
  for (const j of [check, push]) assert.match(j, /^    if: github\.ref == 'refs\/heads\/main'$/m);
  assert.match(check, /uses: actions\/checkout@v4\n        with:\n          ref: main\n          path: trusted\n/);
  assert.equal((code.match(/actions\/checkout/g) ?? []).length, 1, 'one checkout, of main');
  assert.doesNotMatch(code, /ref: \$\{\{\s*inputs\.sha/);
  assert.doesNotMatch(push, /actions\/checkout/);
  assert.match(check, /node conformance\/ci\/request-tag\.mjs/);
  assert.equal((check.match(/working-directory: trusted/g) ?? []).length, 2, 'checks and gates run in the trusted folder');
  // the tagged commit is data: an archive extracted to a separate folder, and --root points there; nothing is run from it
  assert.match(check, /git [^\n]*archive "\$SHA" \| tar -x -C "\$target"/);
  assert.match(check, /\.\/dev gates ratchet --root "\$target"/);
  assert.match(check, /\.\/dev check ipc-endpoints --root "\$target"/);
  assert.doesNotMatch(runBlocks(check), /\bcd "?\$target|working-directory: \$|npm |cargo /);
});

test('P1-2: the push job has no checkout, needs check, fetches the sha, hooks off on every git command, token off argv', () => {
  assert.deepEqual(needsOf(jobOf(loadWorkflow('tag.yml'), 'push')), ['check']);
  const r = runBlocks(push);
  assert.match(r, /git init -q "\$dir"/);
  assert.match(r, /fetch -q --no-tags "\$url" "\$SHA"/);
  const gits = r.split('\n').filter((l) => /(^|[\s=])git\s(?!init)/.test(l) && !/^\s*#/.test(l) && !/echo/.test(l));
  assert.ok(gits.length >= 3, 'fetch, tag, push');
  for (const l of gits) assert.match(l, /core\.hooksPath=\/dev\/null/, l);
  assert.match(r, /git -c core\.hooksPath=\/dev\/null -c user\.name=release-bot [^\n]*tag -a "\$TAG" "\$SHA"/);
  assert.match(r, /push "\$url" "refs\/tags\/\$TAG"/);
  assert.match(r, /export GIT_ASKPASS=/);
  // the token is never an argument: it appears only in the mask line, the empty test and the env assignment for askpass
  for (const l of r.split('\n').filter((x) => /RELEASE_BOT_TOKEN|JOB_TOKEN/.test(x))) {
    assert.match(l, /add-mask|::error::tag: the secret|-z "\$RELEASE_BOT_TOKEN"|GIT_AUTH_PASS="\$(RELEASE_BOT|JOB)_TOKEN" git/, l);
  }
  assert.doesNotMatch(r, /extraheader|https?:\/\/[^"\s]*@|base64/);
  assert.doesNotMatch(r, /set -x/);
});

test('tag.yml: the bot token is in the push job only, masked before use', () => {
  assert.equal((code.match(/secrets\.RELEASE_BOT_TOKEN/g) ?? []).length, 1);
  assert.doesNotMatch(check, /RELEASE_BOT_TOKEN/);
  const r = runBlocks(push);
  assert.ok(r.indexOf('::add-mask::$RELEASE_BOT_TOKEN') >= 0);
  assert.ok(r.indexOf('::add-mask::$RELEASE_BOT_TOKEN') < r.indexOf('GIT_AUTH_PASS="$RELEASE_BOT_TOKEN"'));
});

test('tag.yml: a Linux runner, no skip-ci text, tag-keyed concurrency without cancelling', () => {
  assert.equal((code.match(/runs-on: ubuntu-latest/g) ?? []).length, 2);
  assert.doesNotMatch(wf, /\[(skip ci|ci skip|no ci|skip actions)\]/i);
  assert.match(code, /group: tag-\$\{\{ inputs\.tag \}\}\n\s+cancel-in-progress: false/);
});

test('P2: the app/shared split is a regex on the tag (^v[0-9]), not startsWith', () => {
  assert.doesNotMatch(code, /startsWith\(inputs\.tag/);
  assert.match(runBlocks(check), /if \[\[ "\$TAG" =~ \^v\[0-9\] \]\]; then/);
});

test('docs: tag.yml, releasing.md and AGENTS.md say plainly it is a guard rail, not a security boundary', () => {
  for (const [name, text] of [['tag.yml', wf], ['releasing.md', readFileSync(join(PADDOCK, 'docs', 'blocks', 'releasing.md'), 'utf8')], ['AGENTS.md', readFileSync(join(PADDOCK, 'AGENTS.md'), 'utf8')]]) {
    assert.match(text.replace(/\s+/g, ' '), /guard rail against mistakes, not a security boundary/i, name);
  }
  assert.match(wf, /TOCTOU/);
  assert.doesNotMatch(wf + readFileSync(join(PADDOCK, 'docs', 'blocks', 'releasing.md'), 'utf8'), /\.\/dev release tag/);
});
