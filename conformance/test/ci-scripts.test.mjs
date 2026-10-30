import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildEnv } from '../ci/build-env.mjs';
import { checkPin, executableIn, grantCommand, install, parseArgs, parseRustPin, platformId, urlFor } from '../ci/install-test-browser.mjs';
import { decide, tagFor } from '../ci/plan.mjs';
import { firstLaunch, releaseNotes } from '../ci/release-notes.mjs';
import { makeApp, PADDOCK } from './helpers.mjs';

const CI = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci');
const node = (script, args, env = {}, cwd) => spawnSync(process.execPath, [join(CI, script), ...args], { encoding: 'utf8', cwd, env: { PATH: process.env.PATH, ...env } });

// ---- plan -------------------------------------------------------------------------------------------------------------
test('plan: a nightly is skipped only when main is unchanged since a green nightly', () => {
  assert.deepEqual(decide({ event: 'schedule', sha: 'abc', green: 'true' }).run, false);
  assert.match(decide({ event: 'schedule', sha: 'abc', green: 'true' }).reason, /has not changed since the last green run \(abc\)/);
  assert.equal(decide({ event: 'schedule', sha: 'abc', green: 'false' }).run, true);
  assert.equal(decide({ event: 'schedule', sha: 'abc', green: '' }).run, true);
  assert.equal(decide({ event: 'pull_request', sha: 'abc', green: 'true' }).run, true, 'only nightlies consult the marker');
  assert.equal(decide({ event: 'workflow_dispatch', sha: 'abc', green: 'true' }).run, true);
});

test('plan: a push to main with a v* tag on the commit is covered by the tag run; other pushes run', () => {
  const tags = 'aaa111\trefs/tags/v1.0.0\nbbb222\trefs/tags/v1.1.0\nccc333\trefs/tags/v1.2.0^{}\n';
  assert.equal(tagFor('bbb222', tags), 'v1.1.0');
  assert.equal(tagFor('ccc333', tags), 'v1.2.0', 'annotated tags list the peeled commit');
  assert.equal(tagFor('zzz', tags), null);
  assert.equal(tagFor('bbb222', ''), null);
  const push = (sha, ref = 'refs/heads/main') => decide({ event: 'push', ref, sha, tags });
  assert.equal(push('bbb222').run, false);
  assert.match(push('bbb222').reason, /tagged v1\.1\.0/);
  assert.equal(push('ddd444').run, true);
  assert.equal(push('bbb222', 'refs/heads/feature').run, true, 'a branch push is never skipped');
  assert.equal(decide({ event: 'push', ref: 'refs/tags/v1.1.0', sha: 'bbb222', tags }).run, true, 'the tag run itself runs');
});

test('plan CLI: writes run=false to GITHUB_OUTPUT and a notice', () => {
  const dir = makeApp({ 'out.txt': '' });
  const r = node('plan.mjs', [], { EVENT: 'schedule', SHA: 'abc', GREEN: 'true', GITHUB_OUTPUT: join(dir, 'out.txt') });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /::notice::main has not changed/);
  assert.equal(readFileSync(join(dir, 'out.txt'), 'utf8'), 'run=false\n');
  assert.match(node('plan.mjs', [], { EVENT: 'pull_request', SHA: 'abc' }).stdout, /run=true/);
});

// ---- build env --------------------------------------------------------------------------------------------------------
test('build-env: the short commit from GITHUB_SHA, else git, else unknown; the UTC date', () => {
  const now = () => new Date('2026-10-24T23:59:59Z');
  assert.deepEqual(buildEnv({ env: { GITHUB_SHA: 'ABCDEF0123456789abcdef0123456789abcdef01' }, now }), { PADDOCK_BUILD_COMMIT: 'abcdef012345', PADDOCK_BUILD_DATE: '2026-10-24' });
  assert.equal(buildEnv({ env: {}, head: () => '1234567890abcdef', now }).PADDOCK_BUILD_COMMIT, '1234567890ab');
  assert.equal(buildEnv({ env: { GITHUB_SHA: 'not a sha' }, head: () => null, now }).PADDOCK_BUILD_COMMIT, 'unknown');
  assert.equal(buildEnv({ env: {}, head: () => 'garbage!', now }).PADDOCK_BUILD_COMMIT, 'unknown');
});

test('build-env CLI appends to GITHUB_ENV', () => {
  const dir = makeApp({ 'env.txt': 'A=1\n' });
  const r = node('build-env.mjs', [], { GITHUB_SHA: 'abcdef0123456789', GITHUB_ENV: join(dir, 'env.txt') });
  assert.equal(r.status, 0, r.stderr);
  assert.match(readFileSync(join(dir, 'env.txt'), 'utf8'), /^A=1\nPADDOCK_BUILD_COMMIT=abcdef012345\nPADDOCK_BUILD_DATE=\d{4}-\d\d-\d\d\n$/);
});

// ---- release notes ----------------------------------------------------------------------------------------------------
test('release notes: the changelog, then the First launch block with the product name', () => {
  const notes = releaseNotes({ appName: 'Pitwall', changelog: '### Fixes\n\n- one\n\n\n' });
  assert.match(notes, /^### Fixes\n\n- one\n\n### First launch\n\nPitwall is not code-signed yet/);
  assert.match(notes, /Open Anyway\*\* ?\n?/);
  assert.match(notes, /Windows protected your PC/);
  assert.ok(notes.endsWith('\n') && !notes.endsWith('\n\n'));
  assert.equal(releaseNotes({ appName: 'X', changelog: 'a\n', withFirstLaunch: false }), 'a\n');
  const extra = releaseNotes({ appName: 'Pitcrew', changelog: '- one\n', extra: '  **Existing installs:** reinstall once.\n' });
  assert.match(extra, /Open Anyway[^\n]*\n[^\n]*SmartScreen[^\n]*\n\n\*\*Existing installs:\*\* reinstall once\.\n$/, 'an app\'s own paragraph follows the block');
  assert.equal(releaseNotes({ appName: 'X', changelog: 'a\n', withFirstLaunch: false, extra: '   ' }), 'a\n', 'a blank extra adds nothing');
  assert.throws(() => firstLaunch('bad\nname'), /not a product name/);
  assert.throws(() => firstLaunch('X', '/nonexistent/file.md'));
});

test('release notes CLI', () => {
  const dir = makeApp({ 'c.md': '- fix\n' });
  const out = join(dir, 'RELEASE_NOTES.md');
  assert.equal(node('release-notes.mjs', ['--app', 'Dyno', '--changelog', join(dir, 'c.md'), '--out', out]).status, 0);
  assert.match(readFileSync(out, 'utf8'), /Dyno is not code-signed yet/);
  const withExtra = join(dir, 'EXTRA.md');
  assert.equal(node('release-notes.mjs', ['--app', 'Dyno', '--changelog', join(dir, 'c.md'), '--out', withExtra, '--extra', 'Reinstall once.']).status, 0);
  assert.match(readFileSync(withExtra, 'utf8'), /\n\nReinstall once\.\n$/);
  assert.equal(node('release-notes.mjs', ['--app', 'Dyno']).status, 2);
  assert.equal(node('release-notes.mjs', ['--wat']).status, 2);
});

// ---- test browser -----------------------------------------------------------------------------------------------------
const sha = (b) => createHash('sha256').update(b).digest('hex');
const RUST = `const PINNED_VERSION: &str = "140.0.1";\nconst BASE: &str = "https://storage.example.test/cft";\nconst SUMS: [(&str, &str); 2] = [("linux64", "${'a'.repeat(64)}"), ("mac-arm64", "${'b'.repeat(64)}")];\n`;

test('install-test-browser: pins from JSON or Rust source are validated; ids, urls and executables per platform', () => {
  const pin = parseRustPin(RUST);
  assert.deepEqual(pin, { version: '140.0.1', base: 'https://storage.example.test/cft', sums: { linux64: 'a'.repeat(64), 'mac-arm64': 'b'.repeat(64) } });
  assert.throws(() => parseRustPin('fn main() {}'), /could not read the Chromium pin/);
  assert.equal(checkPin(pin), pin);
  assert.throws(() => checkPin({ version: '1', base: 'http://insecure', sums: {} }), /https base/);
  assert.throws(() => checkPin({ version: '1', base: 'https://x', sums: { linux64: 'zz' } }), /not a SHA-256/);
  assert.equal(platformId('darwin', 'arm64'), 'mac-arm64');
  assert.equal(platformId('win32', 'x64'), 'win64');
  assert.throws(() => platformId('linux', 'arm64'), /no pinned Chromium for linux\/arm64/);
  assert.equal(urlFor(pin, 'linux64'), 'https://storage.example.test/cft/140.0.1/linux64/chrome-linux64.zip');
  assert.match(executableIn('/d', 'mac-arm64'), /Google Chrome for Testing\.app\/Contents\/MacOS\/Google Chrome for Testing$/);
  assert.equal(executableIn('/d', 'win64'), join('/d', 'chrome-win64', 'chrome.exe'));
  assert.deepEqual(parseArgs(['--pin', 'p.json', '--env-prefix', 'DEBRIEF']), { pin: 'p.json', 'env-prefix': 'DEBRIEF' });
  assert.throws(() => parseArgs([]), /exactly one of/);
  assert.throws(() => parseArgs(['--pin', 'a', '--rust-source', 'b']), /exactly one of/);
  assert.throws(() => parseArgs(['--pin', 'a', '--env-prefix', 'lower']), /upper-case/);
  assert.throws(() => parseArgs(['--pin']), /needs a value/);
});

test('install-test-browser: downloads once, verifies the checksum before extracting, and reuses the pinned version', async () => {
  const bytes = Buffer.from('fake zip');
  const pin = { version: '9.9.9', base: 'https://storage.example.test/cft', sums: { linux64: sha(bytes) } };
  const dir = join(makeApp({ 'a.txt': '' }), 'browser');
  const fetched = [];
  const extract = (zip, to) => {
    assert.equal(readFileSync(zip).toString(), 'fake zip');
    mkdirSync(join(to, 'chrome-linux64'), { recursive: true });
    writeFileSync(join(to, 'chrome-linux64', 'chrome'), '#!/bin/sh\n');
    chmodSync(join(to, 'chrome-linux64', 'chrome'), 0o755);
  };
  const fetchBytes = async (url) => { fetched.push(url); return bytes; };
  const granted = [];
  const grant = (d) => granted.push(d);
  const exe = await install({ pin, dir, id: 'linux64', fetchBytes, extract, grant });
  assert.equal(exe, join(dir, 'chrome-linux64', 'chrome'));
  assert.equal(await install({ pin, dir, id: 'linux64', fetchBytes, extract, grant }), exe);
  assert.equal(fetched.length, 1, 'the second call found the pinned version');
  assert.deepEqual(granted, [dir, dir], 'app containers get access on a fresh install and on a reused one');
  await install({ pin: { ...pin, version: '10.0.0' }, dir, id: 'linux64', fetchBytes, extract, grant });
  assert.equal(fetched.length, 2, 'a new pin downloads again');
});

test('install-test-browser: on Windows, app containers may read and run the browser (Chrome checks it before a sandboxed helper starts)', () => {
  assert.equal(grantCommand('/d', 'linux'), null);
  assert.equal(grantCommand('/d', 'darwin'), null);
  const [cmd, args] = grantCommand('C:\\ci\\browser', 'win32', { SystemRoot: 'C:\\Windows' });
  assert.equal(cmd, 'C:\\Windows\\System32\\icacls.exe');
  assert.deepEqual(args, ['C:\\ci\\browser', '/grant', '*S-1-15-2-1:(OI)(CI)(RX)', '/grant', '*S-1-15-2-2:(OI)(CI)(RX)', '/Q']);
});

test('install-test-browser: a wrong checksum extracts nothing; a missing platform and a short archive are errors', async () => {
  const pin = { version: '1', base: 'https://x.test', sums: { linux64: 'c'.repeat(64) } };
  const dir = join(makeApp({ 'a.txt': '' }), 'browser');
  let extracted = false;
  await assert.rejects(install({ pin, dir, id: 'linux64', fetchBytes: async () => Buffer.from('tampered'), extract: () => { extracted = true; }, grant: () => assert.fail('nothing to grant') }), /checksum mismatch/);
  assert.equal(extracted, false);
  assert.equal(existsSync(dir), false);
  await assert.rejects(install({ pin, dir, id: 'win64', fetchBytes: async () => Buffer.alloc(0) }), /no checksum for win64/);
  const bytes = Buffer.from('z');
  await assert.rejects(install({ pin: { ...pin, sums: { linux64: sha(bytes) } }, dir, id: 'linux64', fetchBytes: async () => bytes, extract: () => {}, grant: () => assert.fail('nothing to grant') }), /did not contain/);
});

// ---- tauri-bundle.sh ---------------------------------------------------------------------------------------------------
function bundleRepo({ withTauri = true, version = '1.2.3', installers = ['Demo_1.2.3_amd64.deb', 'Demo_1.2.3_amd64.deb.sig'] } = {}) {
  const root = makeApp({
    ...(withTauri ? { 'app/src-tauri/Cargo.toml': '', 'app/src-tauri/tauri.conf.json': JSON.stringify({ version }) } : {}),
    'bin/npm': '#!/bin/bash\necho "npm $*" >> "$BIN_LOG"\n',
    'bin/npx': `#!/bin/bash\necho "npx $* [key=\${TAURI_SIGNING_PRIVATE_KEY:-unset}|pub=\${TAURI_UPDATER_PUBKEY:-unset}]" >> "$BIN_LOG"\nd="\${PADDOCK_BUILD_FOLDER:-$CARGO_TARGET_DIR}"\nmkdir -p "$d/dist/bundle/deb"\n${installers.map((f) => `touch "$d/dist/bundle/deb/${f}"`).join('\n')}\n`,
  });
  chmodSync(join(root, 'bin/npm'), 0o755);
  chmodSync(join(root, 'bin/npx'), 0o755);
  return root;
}
const bundle = (root, args, env = {}) =>
  spawnSync('bash', [join(CI, 'tauri-bundle.sh'), ...args], {
    cwd: root,
    encoding: 'utf8',
    env: { PATH: `${join(root, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, CARGO_TARGET_DIR: join(root, 'target'), BIN_LOG: join(root, 'calls.log'), ...env },
  });

test('tauri-bundle: unsigned build without secrets, installers copied to dist, no signing env leaks into the build', () => {
  const root = bundleRepo();
  const r = bundle(root, ['deb']);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /No updater signing secrets/);
  assert.ok(existsSync(join(root, 'dist/Demo_1.2.3_amd64.deb')) && existsSync(join(root, 'dist/Demo_1.2.3_amd64.deb.sig')));
  const log = readFileSync(join(root, 'calls.log'), 'utf8');
  assert.match(log, /npm ci/);
  assert.match(log, /npx tauri build --bundles deb -- --profile dist \[key=unset\|pub=unset\]/);
});

test('tauri-bundle: with both secrets the keys are stripped, the updater config is written and used', () => {
  const root = bundleRepo();
  const r = bundle(root, ['deb'], { TAURI_SIGNING_PRIVATE_KEY: 'AB CD\nEF\r\n', TAURI_UPDATER_PUBKEY: ' GH\tIJ ' });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(readFileSync(join(root, 'calls.log'), 'utf8'), /--config \.\.\/target\/updater\.conf\.json -- --profile dist \[key=ABCDEF\|pub=GHIJ\]/);
  assert.deepEqual(JSON.parse(readFileSync(join(root, 'target/updater.conf.json'), 'utf8')), { bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: 'GHIJ' } } });
});

test('tauri-bundle: an installer of another version fails the build; a missing src-tauri skips; stale bundles are removed first', () => {
  const stale = bundle(bundleRepo({ installers: ['Demo_1.0.0_amd64.deb'] }), ['deb']);
  assert.equal(stale.status, 1);
  assert.match(stale.stdout, /::error::Installers from another version than 1\.2\.3: Demo_1\.0\.0_amd64\.deb/);
  const none = bundle(bundleRepo({ withTauri: false }), ['deb']);
  assert.equal(none.status, 0);
  assert.match(none.stdout, /::warning::app\/src-tauri does not exist yet/);
  const root = bundleRepo();
  mkdirSync(join(root, 'target/dist/bundle/deb'), { recursive: true });
  writeFileSync(join(root, 'target/dist/bundle/deb/Demo_0.9.0_amd64.deb'), '');
  assert.equal(bundle(root, ['deb']).status, 0, 'an old installer in the target folder is dropped before the build');
  assert.ok(!existsSync(join(root, 'dist/Demo_0.9.0_amd64.deb')));
  assert.equal(bundle(root, []).status !== 0, true, 'bundles are required');
});

test('tauri-bundle: --root is where npm ci runs, and the build-folder action\'s folder wins over CARGO_TARGET_DIR', () => {
  const root = bundleRepo();
  mkdirSync(join(root, 'ws'), { recursive: true });
  const folder = join(root, 'kept');
  writeFileSync(join(root, 'bin/npm'), '#!/bin/bash\necho "npm $* in $(basename "$PWD")" >> "$BIN_LOG"\n');
  const r = bundle(root, ['deb', '--app-dir', 'app', '--root', 'ws'], { PADDOCK_BUILD_FOLDER: folder, CARGO_TARGET_DIR: join(root, 'ignored') });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(existsSync(join(root, 'dist/Demo_1.2.3_amd64.deb')), 'collected from the build-folder action\'s folder');
  assert.ok(!existsSync(join(root, 'ignored')));
  assert.match(readFileSync(join(root, 'calls.log'), 'utf8'), /npm ci in ws/);
  assert.match(r.stdout + r.stderr, /No updater signing secrets/);
});

test('tauri-bundle: --prepare runs in --root after npm ci and before the build; a failing one stops the build', () => {
  const root = bundleRepo();
  mkdirSync(join(root, 'ws'), { recursive: true });
  writeFileSync(join(root, 'bin/npm'), '#!/bin/bash\necho "npm $* in $(basename "$PWD")" >> "$BIN_LOG"\n');
  const r = bundle(root, ['deb', '--app-dir', 'app', '--root', 'ws', '--prepare', 'npm run build -w ui']);
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const log = readFileSync(join(root, 'calls.log'), 'utf8').split('\n');
  assert.deepEqual(log.slice(0, 2), ['npm ci in ws', 'npm run build -w ui in ws']);
  assert.match(log[2], /^npx tauri build/);
  const bad = bundleRepo();
  const failed = bundle(bad, ['deb', '--prepare', 'exit 3']);
  assert.notEqual(failed.status, 0);
  assert.doesNotMatch(readFileSync(join(bad, 'calls.log'), 'utf8'), /npx tauri build/);
});

// The composite action's own `run:` line, as the runner would expand it: every caller reaches the script through it.
test("the tauri-bundle action's run line works with and without `prepare` (an empty one adds no flag)", () => {
  const action = readFileSync(join(PADDOCK, '.gitea', 'actions', 'tauri-bundle', 'action.yml'), 'utf8');
  const line = action.slice(action.indexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n').replace('${{ github.action_path }}', join(PADDOCK, '.gitea', 'actions', 'tauri-bundle'));
  for (const prepare of ['', 'npm run build -w ui']) {
    const root = bundleRepo();
    const r = spawnSync('bash', ['-c', line], {
      cwd: root,
      encoding: 'utf8',
      env: { PATH: `${join(root, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, CARGO_TARGET_DIR: join(root, 'target'), BIN_LOG: join(root, 'calls.log'), BUNDLES: 'deb', APP_DIR: 'app', ROOT: '.', PREPARE: prepare },
    });
    assert.equal(r.status, 0, `prepare "${prepare}": ${r.stdout}${r.stderr}`);
    const log = readFileSync(join(root, 'calls.log'), 'utf8');
    assert.equal(log.includes('npm run build -w ui'), prepare !== '', `prepare "${prepare}" runs exactly when given`);
  }
});

test('tauri-bundle: a real macOS signing identity turns the hardened runtime back on; ad hoc or unset does not', { skip: process.platform !== 'darwin' && 'the script adds it on macOS only' }, () => {
  const real = bundleRepo();
  assert.equal(bundle(real, ['deb'], { APPLE_SIGNING_IDENTITY: 'Developer ID Application: Example (ABCDE12345)' }).status, 0);
  assert.match(readFileSync(join(real, 'calls.log'), 'utf8'), /--config \.\.\/target\/hardened\.conf\.json/);
  assert.deepEqual(JSON.parse(readFileSync(join(real, 'target/hardened.conf.json'), 'utf8')), { bundle: { macOS: { hardenedRuntime: true } } });
  for (const identity of ['-', '']) {
    const adhoc = bundleRepo();
    assert.equal(bundle(adhoc, ['deb'], { APPLE_SIGNING_IDENTITY: identity }).status, 0);
    assert.doesNotMatch(readFileSync(join(adhoc, 'calls.log'), 'utf8'), /hardened/, `identity "${identity}"`);
  }
});

test('the shared sources the scripts read exist in this checkout', () => {
  assert.ok(existsSync(join(PADDOCK, 'docs/blocks/first-launch.md')));
});
