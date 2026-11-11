import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildEnv } from '../ci/build-env.mjs';
import { checkPin, defaultGrant, executableIn, grantCommand, install, parseArgs, parseRustPin, platformId, urlFor } from '../ci/install-test-browser.mjs';
import { decide, tagFor } from '../ci/plan.mjs';
import { firstLaunch, releaseNotes } from '../ci/release-notes.mjs';
import { makeApp, PADDOCK, pe } from './helpers.mjs';

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

test('install-test-browser: a failed grant warns and the install carries on; no command runs nothing', () => {
  const ran = [];
  const warned = [];
  const warn = (m) => warned.push(m);
  defaultGrant('C:\\ci\\browser', { command: ['icacls.exe', ['x']], run: (c, a) => ran.push([c, a]), warn });
  assert.deepEqual(ran, [['icacls.exe', ['x']]]);
  assert.deepEqual(warned, []);
  assert.doesNotThrow(() =>
    defaultGrant('C:\\ci\\browser', { command: ['icacls.exe', ['x']], run: () => { throw new Error('Access is denied.'); }, warn }),
  );
  assert.equal(warned.length, 1);
  assert.match(warned[0], /warning: could not let app containers read C:\\ci\\browser \(Access is denied\.\)/);
  defaultGrant('/d', { command: null, run: () => assert.fail('nothing to run'), warn });
  assert.equal(warned.length, 1);
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

// A signed build's stub: the bundle holds a real installer and its real Tauri signature (fixtures/updater-sig, made with
// `tauri signer generate` and `tauri signer sign` of @tauri-apps/cli 2.12.3), so updater-key-check.mjs can verify them.
const SIG = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'updater-sig');
const PUB = readFileSync(join(SIG, 'key.pub'), 'utf8').trim();
/** The stub npx of a signed build: `tauri signer generate` writes a pair whose public half is the fixture key; `tauri build`
 * logs its arguments and, on a separate line, the key variables of its environment, then leaves the signed fixture installer. */
function signedRepo() {
  const root = bundleRepo({ installers: [] });
  writeFileSync(join(root, 'bin/npx'), [
    '#!/bin/bash',
    'if [ "$1 $2 $3" = "tauri signer generate" ]; then',
    '  echo "npx-args $*" >> "$BIN_LOG"',
    '  while [ $# -gt 0 ]; do case "$1" in --write-keys) f="$2"; shift ;; esac; shift; done',
    `  printf "THROWAWAY-SK\\n" > "$f"; printf "%s\\n" "${PUB}" > "$f.pub"; exit 0`,
    'fi',
    'echo "npx-args $*" >> "$BIN_LOG"',
    'echo "npx-env key=${TAURI_SIGNING_PRIVATE_KEY:-unset}|pw=${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-unset}|pub=${TAURI_UPDATER_PUBKEY:-unset}" >> "$BIN_LOG"',
    'd="${PADDOCK_BUILD_FOLDER:-$CARGO_TARGET_DIR}"',
    'mkdir -p "$d/dist/bundle/nsis"',
    `cp "${join(SIG, 'Demo_1.2.3_x64-setup.exe')}" "${join(SIG, 'Demo_1.2.3_x64-setup.exe.sig')}" "$d/dist/bundle/nsis/"`,
  ].join('\n'));
  return root;
}
const logOf = (root) => readFileSync(join(root, 'calls.log'), 'utf8');
const TAG = { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/tags/demo-v1.2.3' };

test('tauri-bundle: with both secrets the keys are stripped, the updater config is written and used, and the .sig verifies', () => {
  const root = signedRepo();
  const spaced = `${PUB.slice(0, 10)} \n${PUB.slice(10)}\r\n`;
  const r = bundle(root, ['deb'], { TAURI_SIGNING_PRIVATE_KEY: 'AB CD\nEF\r\n', TAURI_UPDATER_PUBKEY: spaced });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(logOf(root), new RegExp(`^npx-env key=ABCDEF\\|pw=unset\\|pub=${PUB.replace(/[+/=]/g, '\\$&')}$`, 'm'));
  assert.match(logOf(root), /--config \.\.\/target\/updater\.conf\.json -- --profile dist$/m);
  assert.deepEqual(JSON.parse(readFileSync(join(root, 'target/updater.conf.json'), 'utf8')), { bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: PUB } } });
  assert.match(r.stdout, /updater-key-check: 1 signature\(s\) verify against key 29DBCCA3854ABCBC/);
});

// ADR 0085 D7 (WP-4): npm ci and --prepare run branch code (install and build scripts); the updater key never reaches them, and
// it is never an argument of any program (a process list shows arguments to every user of the machine).
test('tauri-bundle: on a tag the key reaches tauri build only, in its environment, never npm ci, --prepare or any argv', () => {
  const root = signedRepo();
  const seen = '[key=${TAURI_SIGNING_PRIVATE_KEY:-unset}|pw=${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-unset}|pub=${TAURI_UPDATER_PUBKEY:-unset}]';
  writeFileSync(join(root, 'bin/npm'), `#!/bin/bash\necho "npm $* ${seen}" >> "$BIN_LOG"\n`);
  // `env K=V cmd` would put the key in env's own argv: a stub env records its arguments, then runs the real one.
  writeFileSync(join(root, 'bin/env'), '#!/bin/bash\necho "env-args $*" >> "$BIN_LOG"\nexec /usr/bin/env "$@"\n');
  chmodSync(join(root, 'bin/env'), 0o755);
  const r = bundle(root, ['deb', '--prepare', `echo "prepare ${seen}" >> "$BIN_LOG"`], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: 'SECRETKEY', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'SECRETPW', TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const log = logOf(root).split('\n');
  assert.equal(log[0], 'npm ci [key=unset|pw=unset|pub=unset]');
  assert.equal(log[1], 'prepare [key=unset|pw=unset|pub=unset]');
  assert.match(log[2], /^npx-args tauri build /);
  assert.equal(log[3], `npx-env key=SECRETKEY|pw=SECRETPW|pub=${PUB}`);
  assert.equal(log.filter((x) => x.startsWith('env-args')).length, 0, 'no env wrapper');
  assert.deepEqual(log.filter((x) => /SECRETKEY|SECRETPW/.test(x)), [log[3]], 'only in the build\'s environment, never in any recorded argv');
  assert.doesNotMatch(r.stdout + r.stderr, /SECRETKEY|SECRETPW/, 'never printed');
});

test('tauri-bundle: a CI run that is not a tag refuses a real key, and signs with a throw-away key with no password in argv', () => {
  const leaked = signedRepo();
  const r = bundle(leaked, ['deb'], { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/heads/fix/x', TAURI_SIGNING_PRIVATE_KEY: 'SECRETKEY', TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout, /^::error::The updater signing key reached a run that is not a tag \(GITHUB_REF=refs\/heads\/fix\/x\)/m);
  assert.ok(!existsSync(join(leaked, 'calls.log')), 'neither npm ci nor the build ran');
  assert.doesNotMatch(r.stdout + r.stderr, /SECRETKEY/, 'the key is never printed');

  const root = signedRepo();
  const d = bundle(root, ['deb'], { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/heads/main', TAURI_UPDATER_PUBKEY: 'REALPUB' });
  assert.equal(d.status, 0, d.stdout + d.stderr);
  assert.match(d.stdout, /::notice::This build is signed with a throw-away updater key made in this job/);
  const log = logOf(root);
  assert.match(log, /^npx-args tauri signer generate --ci --force --write-keys \S+$/m, 'no --password: nothing secret on the command line');
  assert.match(log, new RegExp(`^npx-env key=THROWAWAY-SK\\|pw=unset\\|pub=${PUB.replace(/[+/=]/g, '\\$&')}$`, 'm'));
  assert.equal(JSON.parse(readFileSync(join(root, 'target/updater.conf.json'), 'utf8')).plugins.updater.pubkey, PUB, 'the build carries the throw-away public key');
  assert.match(d.stdout, /updater-key-check: 1 signature\(s\) verify/);
  // No public key: the app has no updater, so nothing is signed (and no key pair is made).
  const plain = bundleRepo();
  assert.equal(bundle(plain, ['deb'], { GITHUB_ACTIONS: 'true', GITHUB_REF: 'refs/heads/main' }).status, 0);
  assert.match(logOf(plain), /\[key=unset\|pub=unset\]/);
});

test('tauri-bundle: a tag never makes a throw-away key: the public key without the private key fails before npm ci', () => {
  const root = signedRepo();
  const r = bundle(root, ['deb'], { ...TAG, TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout, /^::error::This tag run has the updater public key but no private key \(GITHUB_REF=refs\/tags\/demo-v1\.2\.3\)/m);
  assert.ok(!existsSync(join(root, 'calls.log')), 'no npm ci, no signer generate, no build');
});

test('tauri-bundle: a signature from another key than the one the build carries fails the build', () => {
  const root = signedRepo();
  // Another Ed25519 key in minisign's format: the fixture .sig names a different key id.
  const other = Buffer.from(`untrusted comment: minisign public key: 0\n${Buffer.concat([Buffer.from('Ed'), Buffer.alloc(8, 7), Buffer.alloc(32, 9)]).toString('base64')}\n`).toString('base64');
  const r = bundle(root, ['deb'], { TAURI_SIGNING_PRIVATE_KEY: 'K', TAURI_UPDATER_PUBKEY: other });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout, /::error::Updater signature: .*Demo_1\.2\.3_x64-setup\.exe\.sig: signed by key 29DBCCA3854ABCBC, not 0707070707070707/);
});

test('tauri-bundle: a strip-keys failure stops the build before anything reads the keys', () => {
  const root = signedRepo();
  // A quote is not base64: strip-keys.mjs refuses it (exit 1), and the step must stop, not eval an empty output.
  const r = bundle(root, ['deb'], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: "AB'CD", TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout, /^::error::strip-keys\.mjs failed/m);
  assert.ok(!existsSync(join(root, 'calls.log')), 'nothing ran');
});

test('tauri-bundle: the app\'s beforeBuildCommand runs before tauri build without the key, and tauri build gets it emptied', () => {
  const root = signedRepo();
  writeFileSync(join(root, 'app/src-tauri/tauri.conf.json'), JSON.stringify({ version: '1.2.3', build: { beforeBuildCommand: 'echo "hook in $(basename "$PWD") [key=${TAURI_SIGNING_PRIVATE_KEY:-unset}|debug=$TAURI_ENV_DEBUG]" >> "$BIN_LOG"' } }));
  const r = bundle(root, ['deb'], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: 'SECRETKEY', TAURI_UPDATER_PUBKEY: PUB });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  const log = logOf(root).split('\n');
  const hook = log.findIndex((l) => l.startsWith('hook in'));
  const build = log.findIndex((l) => l.startsWith('npx-args tauri build'));
  assert.ok(hook >= 0 && hook < build, 'the hook ran first');
  assert.equal(log[hook], 'hook in app [key=unset|debug=false]');
  assert.match(log[build], /--config \.\.\/target\/no-hook\.conf\.json -- --profile dist$/, 'the emptying configuration comes last');
  assert.deepEqual(JSON.parse(readFileSync(join(root, 'target/no-hook.conf.json'), 'utf8')), { build: { beforeBuildCommand: null } }, 'null: the merge patch deletes the key, so tauri build has no hook');
  // A failing hook stops before tauri build.
  const bad = signedRepo();
  writeFileSync(join(bad, 'app/src-tauri/tauri.conf.json'), JSON.stringify({ version: '1.2.3', build: { beforeBuildCommand: 'exit 4' } }));
  const f = bundle(bad, ['deb']);
  assert.equal(f.status, 4);
  assert.match(f.stdout, /::error::beforeBuildCommand `exit 4` failed \(exit 4\)/);
  assert.doesNotMatch(logOf(bad), /tauri build/);
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

test('tauri-bundle: an NSIS build fails when the app, a sidecar or a bundled library imports the VC++ runtime (ADR 0032)', () => {
  const conf = JSON.stringify({ version: '1.2.3', mainBinaryName: 'demo', bundle: { externalBin: ['binaries/demo-agent'], resources: { 'binaries/lib/': 'lib/' } } });
  const nsis = (files) => {
    const root = bundleRepo({ installers: [] });
    writeFileSync(join(root, 'app/src-tauri/tauri.conf.json'), conf);
    for (const [p, b] of Object.entries(files)) {
      mkdirSync(dirname(join(root, p)), { recursive: true });
      writeFileSync(join(root, p), b);
    }
    return bundle(root, ['nsis']);
  };
  const clean = pe({ imports: ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'] });
  const dynamic = pe({ imports: ['KERNEL32.dll', 'VCRUNTIME140.dll'] });
  const ok = nsis({ 'target/dist/demo.exe': clean, 'target/dist/demo-agent.exe': clean, 'app/src-tauri/binaries/lib/x.dll': clean, 'target/dist/stale-old.exe': dynamic });
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  assert.match(ok.stdout, /3 Windows binaries checked/, 'the shipped files only: a stale exe in the kept build folder is not read');
  const agent = nsis({ 'target/dist/demo.exe': clean, 'target/dist/demo-agent.exe': dynamic });
  assert.equal(agent.status, 1);
  assert.match(agent.stdout, /::error::.*demo-agent\.exe: imports VCRUNTIME140\.dll/);
  const lib = nsis({ 'target/dist/demo.exe': clean, 'target/dist/demo-agent.exe': clean, 'app/src-tauri/binaries/lib/x.dll': dynamic });
  assert.equal(lib.status, 1);
  assert.match(lib.stdout, /x\.dll: imports VCRUNTIME140\.dll/);
  const missing = nsis({ 'target/dist/demo-agent.exe': clean, 'target/dist/other.exe': clean });
  assert.equal(missing.status, 1, 'a missing app binary is an error, not a pass');
  assert.match(missing.stdout, /the app's binary is missing: none of demo\.exe/);
  assert.doesNotMatch(bundle(bundleRepo(), ['deb']).stdout, /Windows binaries checked/, 'other bundles are not checked');
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
  const line = action.slice(action.lastIndexOf('run: |\n') + 'run: |\n'.length).split('\n').map((l) => l.slice(8)).join('\n').replace('${{ github.action_path }}', join(PADDOCK, '.gitea', 'actions', 'tauri-bundle'));
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
