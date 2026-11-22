// The dev-tool-cache steps (conformance/ci/dev-tool-cache.sh, ADR 0111) run against a stand-in ./dev: the key is the launcher's hash
// plus OS, architecture and the salt (no restore-keys), a restored binary is judged by what ./dev did with it, a miss says "miss"
// and is saved only after the tool reported its own hash, and the summary never shows a failed save as a pass (ER-33).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

const SCRIPT = join(PADDOCK, 'conformance', 'ci', 'dev-tool-cache.sh');
const HASH = 'a'.repeat(40);

/** A repository root whose `dev` answers --wrapper-hash and `self hash` as the test says. */
function fixture({ state = 'clean', selfHash = `embedded ${HASH}`, stderr = '' } = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-dev-tool-cache-')));
  const dev = `#!/bin/sh
case "$1" in
  --wrapper-hash) printf 'dev: hash ${HASH} inputs: stand-in\\n${HASH}\\n${state}\\n' ;;
  self) printf '%s\\n' '${stderr}' >&2; printf '%s\\n' '${selfHash}' ;;
esac
`;
  writeFileSync(join(root, 'dev'), dev);
  chmodSync(join(root, 'dev'), 0o755);
  mkdirSync(join(root, 'home'));
  return root;
}

function run(root, args, env = {}) {
  const outputs = join(root, 'output');
  const summary = join(root, 'summary');
  writeFileSync(outputs, '');
  const r = spawnSync('bash', [SCRIPT, ...args], {
    env: { PATH: '/usr/bin:/bin', HOME: join(root, 'home'), PADDOCK_REPO: root, PADDOCK_DEV_TOKEN: 'read-token', RUNNER_OS: 'Linux', RUNNER_ARCH: 'X64', GITHUB_OUTPUT: outputs, GITHUB_STEP_SUMMARY: summary, ...env },
    encoding: 'utf8',
  });
  const out = Object.fromEntries(readFileSync(outputs, 'utf8').split('\n').filter(Boolean).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]));
  let sum = '';
  try { sum = readFileSync(summary, 'utf8'); } catch { /* no summary line */ }
  return { status: r.status, text: r.stdout + r.stderr, out, sum };
}

test('the key is the launcher hash, the OS, the architecture and the salt, with the folder of that hash', () => {
  const root = fixture();
  const r = run(root, ['key']);
  assert.equal(r.status, 0, r.text);
  assert.equal(r.out.usable, 'true');
  assert.equal(r.out.key, `dev-tool-v1-Linux-X64-${HASH}`);
  assert.equal(r.out.path, join(root, 'home', '.paddock-dev', 'bin', HASH));
  assert.equal(run(root, ['key'], { PADDOCK_DEV_TOOL_CACHE_VERSION: '2' }).out.key, `dev-tool-v2-Linux-X64-${HASH}`, 'the salt retires every saved binary');
});

test('a job that builds from source never touches the cache: a download-saved binary is never restored into bin-source (ADR 0022 section 3.3)', () => {
  const root = fixture();
  const noDownload = { PADDOCK_DEV_DOWNLOAD: '0' };
  for (const [why, env] of [['DOWNLOAD=0', noDownload], ['FRESH=1', { PADDOCK_DEV_FRESH: '1' }], ['no read token', { PADDOCK_DEV_TOKEN: '' }]]) {
    const r = run(root, ['key'], env);
    assert.equal(r.out.usable, 'false', why);
    assert.equal(r.out.key, undefined, `${why}: no key, so nothing is restored or saved`);
  }
  assert.match(run(root, ['key'], { PADDOCK_DEV_TOKEN: '' }).sum, /dev-tool cache: not used \(this job builds \.\/dev from source/, 'the job summary says why the cache was not used');
  // Positive control: the same checkout with a token and downloads on does get a key.
  assert.equal(run(root, ['key']).out.usable, 'true');
  // One definition, asked by setup-rust's compile-cache step too.
  assert.equal(run(root, ['no-download'], noDownload).status, 0);
  assert.equal(run(root, ['no-download'], { PADDOCK_DEV_TOKEN: '' }).status, 0, 'no token is the same as DOWNLOAD=0');
  assert.notEqual(run(root, ['no-download']).status, 0, 'a token and downloads on: the tool may be downloaded');
});

test('a checkout with uncommitted tool inputs is not cached: usable is false, never a made-up key', () => {
  const r = run(fixture({ state: 'dirty' }), ['key']);
  assert.equal(r.out.usable, 'false');
  assert.equal(r.out.key, undefined);
  assert.match(r.text, /::notice::dev-tool-cache: the tool's inputs have uncommitted changes/);
});

test('a miss builds, is called a miss, and is saved only after the tool reported its own hash', () => {
  const r = run(fixture({ stderr: 'dev: building paddock-dev x in y (compiler z, try 1).' }), ['verify', 'false', '']);
  assert.equal(r.status, 0, r.text);
  assert.deepEqual([r.out.state, r.out.save], ['miss', 'true']);
});

// Run 8576: ./dev was built by an earlier step of the job, so verify's own run printed no build line and it said `hit save=false`
// although the restore had found nothing: the tool was never saved (42 of 44 jobs rebuilt it, 5bdd263cc onward). The state comes from
// the restore step's output, not from the log of verify's own run.
test('a restore miss with the tool already built earlier in the job is a miss and is saved', () => {
  for (const hit of ['', 'false']) {
    const r = run(fixture({ stderr: '' }), ['verify', hit, '']);
    assert.equal(r.status, 0, r.text);
    assert.deepEqual([r.out.state, r.out.save], ['miss', 'true'], `cache-hit '${hit}'`);
  }
});

test('a hit that ./dev ran as it was is a hit and is not saved again', () => {
  const r = run(fixture(), ['verify', 'true', 'the-key']);
  assert.deepEqual([r.out.state, r.out.save], ['hit', 'false']);
});

test('a restored binary that ./dev had to rebuild is rejected, said aloud and not saved (the key exists)', () => {
  const r = run(fixture({ stderr: 'dev: the cached tool binary x does not run or is not for hash y; rebuilding it.\ndev: building paddock-dev x in y (compiler z, try 1).' }), ['verify', 'true', 'the-key']);
  assert.deepEqual([r.out.state, r.out.save], ['rejected', 'false']);
  assert.match(r.text, /::warning::dev-tool-cache: the binary restored under this key was rejected and rebuilt/);
});

test('a tool that reports another hash fails the step and is never saved', () => {
  const r = run(fixture({ selfHash: `embedded ${'b'.repeat(40)}` }), ['verify', 'false', '']);
  assert.notEqual(r.status, 0);
  assert.match(r.text, /::error::dev-tool-cache: the tool reports 'embedded b+', not 'embedded a+'/);
  assert.equal(r.out.save, undefined);
});

test('the summary line names the key and the outcome, and a save the lookup did not confirm is a warning, not a pass', () => {
  const root = fixture();
  const sum = (args) => run(root, ['summary', ...args], { KEY: 'the-key' });
  assert.match(sum(['hit', 'false', '']).sum, /`the-key`: restored: exact hit/);
  assert.match(sum(['miss', 'false', '']).sum, /restored: miss \(built\); saved: NO/);
  assert.match(sum(['miss', 'true', 'true']).sum, /restored: miss \(built\); saved: yes \(found again by key\)/);
  const bad = sum(['miss', 'true', 'false']);
  assert.match(bad.sum, /saved: NOT confirmed/);
  assert.match(bad.text, /::warning::dev-tool-cache:/);
  assert.match(sum(['not run', '', '']).sum, /not measured \(not run\)/, 'a step that did not run is not measured');
});

test('the summary prints the audit line from the restore outputs: outcome exact, partial or miss, and whether the save was found', () => {
  const root = fixture();
  const sum = (args) => run(root, ['summary', ...args], { KEY: 'the-key' }).text;
  assert.match(sum(['hit', 'false', '', 'true', 'the-key']), /cache-report: name=dev-tool outcome=exact saved=n\/a key=the-key/);
  assert.match(sum(['miss', 'true', 'true', '', '']), /cache-report: name=dev-tool outcome=miss saved=yes key=the-key/);
  assert.match(sum(['miss', 'true', 'false', '', '']), /cache-report: name=dev-tool outcome=miss saved=no key=the-key/, 'an unconfirmed save is "no"');
  assert.doesNotMatch(sum(['not run', '', '']), /cache-report:/, 'a step that did not run reports nothing');
});
