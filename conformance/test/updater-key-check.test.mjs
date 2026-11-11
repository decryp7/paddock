// conformance/release/updater-key-check.mjs (every .sig verifies against the build's one public key) and
// conformance/ci/before-build.mjs (the app's beforeBuildCommand, run before tauri build without the key): ADR 0085 D7.
// The signature fixture is real Tauri output: `tauri signer generate` and `tauri signer sign` of @tauri-apps/cli 2.12.3 (the
// private key was thrown away after signing).
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { effectiveBuild, hookOf, main as beforeBuild, mergePatch } from '../ci/before-build.mjs';
import { checkSelfTest } from '../release/check-per-machine-installer.mjs';
import { checkDir, main, parsePublicKey, verifySignature } from '../release/updater-key-check.mjs';

const SIG = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'updater-sig');
const PUB = readFileSync(join(SIG, 'key.pub'), 'utf8').trim();
const FILE = 'Demo_1.2.3_x64-setup.exe';
const scratch = () => mkdtempSync(join(tmpdir(), 'paddock-updater-key-'));
const quiet = () => {
  const lines = [];
  return Object.assign((l) => lines.push(l), { lines });
};
const otherKey = (id = 7) => Buffer.from(`untrusted comment: x\n${Buffer.concat([Buffer.from('Ed'), Buffer.alloc(8, id), Buffer.alloc(32, 9)]).toString('base64')}\n`).toString('base64');

test('updater-key-check: a real Tauri signature verifies against its key, with minisign\'s key id', () => {
  const pub = parsePublicKey(PUB);
  assert.equal(pub.id, '29DBCCA3854ABCBC', 'the id minisign prints in the key\'s comment');
  const sig = readFileSync(join(SIG, `${FILE}.sig`), 'utf8');
  assert.deepEqual(verifySignature(pub, sig, readFileSync(join(SIG, FILE))), { ok: true, id: '29DBCCA3854ABCBC' });
  assert.equal(verifySignature(pub, sig, Buffer.from('another installer')).reason, 'the signature does not verify');
  assert.equal(verifySignature(parsePublicKey(otherKey()), sig, readFileSync(join(SIG, FILE))).reason, 'signed by key 29DBCCA3854ABCBC, not 0707070707070707');
  assert.match(verifySignature(pub, Buffer.from('one line').toString('base64'), Buffer.alloc(0)).reason, /not a minisign file/);
  assert.throws(() => parsePublicKey(Buffer.from('a\nb\n').toString('base64')), /not an Ed25519 minisign key/);
});

test('updater-key-check: the folder check finds every .sig, fails a changed installer, and never passes on nothing', () => {
  const dir = scratch();
  mkdirSync(join(dir, 'bundle', 'nsis'), { recursive: true });
  cpSync(SIG, join(dir, 'bundle', 'nsis'), { recursive: true });
  const log = quiet();
  assert.equal(main(['--pubkey', PUB, '--dir', dir], log), 0, log.lines.join('\n'));
  assert.match(log.lines.at(-1), /^updater-key-check: 1 signature\(s\) verify against key 29DBCCA3854ABCBC\.$/);
  writeFileSync(join(dir, 'bundle', 'nsis', FILE), 'MZ changed after signing');
  const bad = quiet();
  assert.equal(main(['--pubkey', PUB, '--dir', dir], bad), 1);
  assert.match(bad.lines[0], /^::error::Updater signature: .*\.sig: the signature does not verify\. Next step:/);
  const empty = scratch();
  assert.deepEqual(checkDir(PUB, empty).findings, [`check broken: no .sig file under ${empty}, so nothing was compared with the key`]);
  writeFileSync(join(empty, 'x.sig'), 'abc');
  assert.match(checkDir(PUB, empty).findings[0], /the signed file .* is missing/);
  assert.match(checkDir('not a key', empty).findings[0], /the updater public key cannot be read/);
  assert.equal(main(['--dir', empty], quiet()), 2);
});

test('per-machine installer gate: on a tag paddock-setup.exe must verify with the app\'s updater key', () => {
  const out = 'identifier=net.decryptology.dyno\nstop_all=present\nkey_fingerprint=29DBCCA3854ABCBC\n';
  const args = { stdout: out, status: 0, identifier: 'net.decryptology.dyno', name: 'a-setup.exe' };
  assert.deepEqual(checkSelfTest({ ...args, keyId: parsePublicKey(PUB).id }), []);
  assert.match(checkSelfTest({ ...args, keyId: '0707070707070707' }).join(), /verifies updates with key 29DBCCA3854ABCBC, but the app's updater key is 0707070707070707/);
  assert.match(checkSelfTest({ ...args, stdout: out.replace(/key_fingerprint=.*\n/, ''), keyId: '0707070707070707' }).join(), /with key missing/);
  assert.deepEqual(checkSelfTest(args), [], 'no key named (a dispatch): not compared');
});

test('before-build: the hook is merged as Tauri merges it, platform file and --config files in order', () => {
  assert.deepEqual(mergePatch({ a: 1, b: { c: 2, d: 3 } }, { b: { c: null, e: 4 }, f: 5 }), { a: 1, b: { d: 3, e: 4 }, f: 5 });
  const app = scratch();
  mkdirSync(join(app, 'src-tauri'));
  writeFileSync(join(app, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ build: { beforeBuildCommand: 'npm run build', frontendDist: 'x' } }));
  writeFileSync(join(app, 'src-tauri', 'tauri.windows.conf.json'), JSON.stringify({ build: { beforeBuildCommand: 'npm run build:win' } }));
  writeFileSync(join(app, 'sidecars.json'), JSON.stringify({ build: { beforeBuildCommand: { script: 'npm run all', cwd: 'sub' } } }));
  assert.equal(effectiveBuild({ appDir: app, platform: 'linux' }).build.beforeBuildCommand, 'npm run build');
  assert.equal(effectiveBuild({ appDir: app, platform: 'win32' }).build.beforeBuildCommand, 'npm run build:win');
  const merged = effectiveBuild({ appDir: app, platform: 'darwin', configs: ['sidecars.json'] }).build.beforeBuildCommand;
  assert.deepEqual(hookOf(merged, app), { script: 'npm run all', cwd: join(app, 'sub') });
  assert.equal(hookOf('', app), null);
  assert.equal(hookOf({ script: '' }, app), null);
  assert.throws(() => hookOf(3, app), /a hook must be/);
});

test('before-build: it runs the hook in the app folder with TAURI_ENV_DEBUG=false and stops on failure; beforeBundleCommand is refused', () => {
  const app = scratch();
  mkdirSync(join(app, 'src-tauri'));
  const conf = (build) => writeFileSync(join(app, 'src-tauri', 'tauri.conf.json'), JSON.stringify({ build }));
  conf({ beforeBuildCommand: 'echo built' });
  const calls = [];
  const run = (cmd, args, o) => {
    calls.push([cmd, args, o.cwd, o.env.TAURI_ENV_DEBUG, o.env.TAURI_SIGNING_PRIVATE_KEY]);
    return { status: 0 };
  };
  assert.equal(beforeBuild(['--app-dir', app], { env: { PATH: '/bin' }, run, log: quiet(), platform: 'linux' }), 0);
  assert.deepEqual(calls, [['bash', ['-c', 'echo built'], app, 'false', undefined]]);
  const log = quiet();
  assert.equal(beforeBuild(['--app-dir', app], { env: {}, run: () => ({ status: 3 }), log, platform: 'linux' }), 3);
  assert.match(log.lines.at(-1), /^::error::beforeBuildCommand `echo built` failed \(exit 3\)/);
  const printed = quiet();
  assert.equal(beforeBuild(['--app-dir', app, '--print'], { run: () => assert.fail('print runs nothing'), log: printed, platform: 'linux' }), 0);
  assert.deepEqual(printed.lines, ['echo built']);
  conf({ beforeBuildCommand: 'echo built', beforeBundleCommand: 'echo bundle' });
  const refused = quiet();
  assert.equal(beforeBuild(['--app-dir', app], { run: () => assert.fail('nothing runs'), log: refused, platform: 'linux' }), 1);
  assert.match(refused.lines[0], /beforeBundleCommand is set: tauri build would run it with the updater key/);
  conf({});
  const none = quiet();
  assert.equal(beforeBuild(['--app-dir', app], { run: () => assert.fail('nothing to run'), log: none, platform: 'linux' }), 0);
  assert.deepEqual(none.lines, ['before-build: no beforeBuildCommand to run.']);
});
