// Dyno's macOS helper is signed by the release's own step, not by Tauri's beforeBuildCommand (ADR 0085 amendment 0085-2026-11-12 item 11 strips the
// updater key before npm, prepare and that hook; dyno-release.yml run 8395 failed on the tag because the hook, which signed only with the key, found none).
// A dispatched run is release=0, so CI cannot exercise the tag path before the real tag: these tests run it, on the scripts the macOS tag job runs, with a
// throw-away key made by the Tauri CLI in a temp folder (no real key anywhere). Needs the Tauri CLI of the npm workspace (`npm ci` at the root; a checkout
// without it names PADDOCK_TEST_NODE_MODULES, a node_modules folder that has it).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { delimiter, dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { parsePublicKey, verifySignature } from '../release/updater-key-check.mjs';
import { job, loadAction, loadWorkflow, stepsOf } from './lib/workflow.mjs';

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const SCRIPTS = join(REPO, 'apps', 'dyno', 'app', 'src-tauri', 'scripts');
const NODE_MODULES = process.env.PADDOCK_TEST_NODE_MODULES ?? join(REPO, 'node_modules');
const TAURI = join(NODE_MODULES, '.bin', 'tauri');
const TRIPLE = 'aarch64-apple-darwin';
const sha = (f) => createHash('sha256').update(readFileSync(f)).digest('hex');
const exe = (p, body) => { writeFileSync(p, body); chmodSync(p, 0o755); };

/** A throw-away key pair from the Tauri CLI: { key, pub } as the secrets hold them (single-line base64). */
function throwawayKey(dir) {
  assert.ok(existsSync(TAURI), `the Tauri CLI is missing (${TAURI}): run npm ci at the repository root or set PADDOCK_TEST_NODE_MODULES`);
  const r = spawnSync(TAURI, ['signer', 'generate', '--ci', '--force', '--write-keys', join(dir, 'k.key')], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  const one = (f) => readFileSync(f, 'utf8').replace(/\s+/g, '');
  return { key: one(join(dir, 'k.key')), pub: one(join(dir, 'k.key.pub')) };
}

/** apps/dyno as the scripts see it (copied scripts, a fake built target folder) and stand-ins for cargo, rustc and codesign. */
function tree() {
  const dir = mkdtempSync(join(tmpdir(), 'dyno-helper-sign-test-'));
  const app = join(dir, 'apps', 'dyno');
  const tauri = join(app, 'app', 'src-tauri');
  mkdirSync(join(tauri, 'scripts'), { recursive: true });
  for (const f of ['sidecar.mjs', 'sidecar-lib.mjs', 'sign-helper.mjs']) cpSync(join(SCRIPTS, f), join(tauri, 'scripts', f));
  mkdirSync(join(dir, 'conformance', 'src', 'sidecar'), { recursive: true });
  cpSync(join(REPO, 'conformance', 'src', 'sidecar', 'sidecar.mjs'), join(dir, 'conformance', 'src', 'sidecar', 'sidecar.mjs'));
  writeFileSync(join(tauri, 'tauri.conf.json'), JSON.stringify({ version: '9.9.9' }));
  symlinkSync(NODE_MODULES, join(app, 'app', 'node_modules'));
  mkdirSync(join(app, 'target', 'dist'), { recursive: true });
  for (const n of ['dyno-agent', 'dyno', 'dyno-bench', 'dyno-bench-gpu', 'dyno-helper']) writeFileSync(join(app, 'target', 'dist', n), `built ${n}\n`);
  const bin = join(dir, 'bin');
  mkdirSync(bin);
  exe(join(bin, 'rustc'), `#!/bin/sh\necho "host: ${TRIPLE}"\n`);
  exe(join(bin, 'cargo'), '#!/bin/sh\n[ "$1" = metadata ] && exit 1\nexit 0\n'); // no build; no metadata, so the target folder is <apps/dyno>/target
  exe(join(bin, 'codesign'), '#!/bin/sh\nfor last; do :; done\nprintf "SEALED" >> "$last"\n'); // changes the bytes, as a seal does
  return { dir, app, tauri, binaries: join(tauri, 'binaries'), bin, scripts: join(tauri, 'scripts') };
}

function env(t, extra = {}) {
  const e = { ...process.env, PATH: `${t.bin}${delimiter}${process.env.PATH}`, TAURI_ENV_TARGET_TRIPLE: TRIPLE, ...extra };
  for (const k of ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'GITHUB_REF', 'GITEA_REF', 'GITHUB_REF_TYPE', 'GITEA_REF_TYPE']) if (!(k in extra)) delete e[k];
  return e;
}
const TAG = { GITHUB_REF: 'refs/tags/dyno-v9.9.9' };
const run = (t, script, args, extra) => spawnSync(process.execPath, [join(t.scripts, script), ...args], { cwd: t.app, env: env(t, extra), encoding: 'utf8' });
const out = (r) => `${r.stdout}${r.stderr}`;

test('the signing step on a tag with a key: a non-empty signature that verifies against the key, and the hash of the signed bytes', () => {
  const t = tree();
  const { key, pub } = throwawayKey(t.dir);
  const built = run(t, 'sign-helper.mjs', ['build'], TAG); // no key in this part's environment
  assert.equal(built.status, 0, out(built));
  const helper = join(t.binaries, `dyno-helper-${TRIPLE}`);
  assert.equal(readFileSync(helper, 'utf8'), 'built dyno-helper\nSEALED', 'built from the target folder and sealed ad hoc');
  const signed = run(t, 'sign-helper.mjs', ['sign'], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' });
  assert.equal(signed.status, 0, out(signed));
  const sig = readFileSync(join(t.binaries, 'dyno-helper.minisig'), 'utf8');
  assert.ok(sig.length > 0, 'the signature file is not empty');
  assert.deepEqual(Object.keys(verifySignature(parsePublicKey(pub), sig, readFileSync(helper))), ['ok', 'id'], 'verifies against the throw-away public key');
  assert.match(Buffer.from(sig, 'base64').toString(), /dyno-helper-9\.9\.9-aarch64-apple-darwin/, 'signed under dyno-helper-<version>-<triple>');
  assert.equal(readFileSync(join(t.binaries, 'dyno-helper.sha256'), 'utf8'), `${sha(helper)}\n`);
});

test('the signing step on a tag without the key fails with an ::error::; off a tag it leaves empty files and a warning', () => {
  const t = tree();
  assert.equal(run(t, 'sign-helper.mjs', ['build'], TAG).status, 0);
  const tag = run(t, 'sign-helper.mjs', ['sign'], TAG);
  assert.equal(tag.status, 1);
  assert.match(out(tag), /::error::sign-helper: TAURI_SIGNING_PRIVATE_KEY did not reach the helper signing step.*Next step:/);
  const dispatch = run(t, 'sign-helper.mjs', ['sign'], { GITHUB_REF: 'refs/heads/main' });
  assert.equal(dispatch.status, 0, out(dispatch));
  assert.equal(readFileSync(join(t.binaries, 'dyno-helper.minisig'), 'utf8'), '');
  assert.match(out(dispatch), /::warning::no TAURI_SIGNING_PRIVATE_KEY/);
});

test('the hook (sidecar.mjs) on a tag, with no key: passes on the helper the step signed and keeps its bytes', () => {
  const t = tree();
  const { key } = throwawayKey(t.dir);
  assert.equal(run(t, 'sign-helper.mjs', ['build'], TAG).status, 0);
  assert.equal(run(t, 'sign-helper.mjs', ['sign'], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: key }).status, 0);
  const helper = join(t.binaries, `dyno-helper-${TRIPLE}`);
  const before = readFileSync(helper);
  const hook = run(t, 'sidecar.mjs', [], TAG); // the hook's environment holds no key
  assert.equal(hook.status, 0, out(hook));
  assert.deepEqual(readFileSync(helper), before, 'the hook did not replace the signed helper with the built one');
  assert.notEqual(readFileSync(helper, 'utf8'), 'built dyno-helper\n');
  assert.ok(readFileSync(join(t.binaries, 'dyno-helper.minisig')).length > 0);
  assert.match(readFileSync(join(t.binaries, 'dyno-agent-' + TRIPLE), 'utf8'), /built dyno-agent/, 'the other sidecars are still copied');
});

test('the hook on a tag without the signing step fails with an ::error:: naming the step (positive control: the check sees a missing step)', () => {
  const t = tree();
  const hook = run(t, 'sidecar.mjs', [], TAG);
  assert.equal(hook.status, 1);
  assert.match(out(hook), /::error::the macOS dyno-helper has no signature on a tag run: the signing step \(sign-helper\.mjs, run by tauri-bundle\.sh as --pre-sign\)/);
  // A stale signature of another file does not count: the hash must be that of the helper's bytes.
  const { key } = throwawayKey(t.dir);
  assert.equal(run(t, 'sign-helper.mjs', ['build'], TAG).status, 0);
  assert.equal(run(t, 'sign-helper.mjs', ['sign'], { ...TAG, TAURI_SIGNING_PRIVATE_KEY: key }).status, 0);
  writeFileSync(join(t.binaries, `dyno-helper-${TRIPLE}`), 'another build');
  assert.equal(run(t, 'sidecar.mjs', [], TAG).status, 1, 'a signature that does not cover the helper on disk is no signature');
});

test('the hook off a tag without the step keeps today\'s behaviour: empty signature files and a warning, exit 0', () => {
  const t = tree();
  const hook = run(t, 'sidecar.mjs', [], { GITHUB_REF: 'refs/heads/x' });
  assert.equal(hook.status, 0, out(hook));
  assert.equal(readFileSync(join(t.binaries, 'dyno-helper.minisig'), 'utf8'), '');
  assert.equal(readFileSync(join(t.binaries, 'dyno-helper.sha256'), 'utf8'), '');
  assert.match(out(hook), /::warning::/);
});

test('the hook never signs: with the key in its environment it still does not call the Tauri signer', () => {
  const t = tree();
  const { key } = throwawayKey(t.dir);
  const hook = run(t, 'sidecar.mjs', [], { GITHUB_REF: 'refs/heads/x', TAURI_SIGNING_PRIVATE_KEY: key });
  assert.equal(hook.status, 0, out(hook));
  assert.equal(readFileSync(join(t.binaries, 'dyno-helper.minisig'), 'utf8'), '', 'signing belongs to sign-helper.mjs');
});

test('wiring: dyno-release.yml hands the script to app-release.yml, which hands it to tauri-bundle; tauri-bundle gives the key to the sign part only', () => {
  const wf = loadWorkflow('dyno-release.yml');
  for (const id of ['installers', 'release']) assert.equal(job(wf, id).with['pre-sign'], 'apps/dyno/app/src-tauri/scripts/sign-helper.mjs', id);
  assert.ok(existsSync(join(REPO, 'apps/dyno/app/src-tauri/scripts/sign-helper.mjs')));
  const rel = loadWorkflow('app-release.yml');
  const step = stepsOf(job(rel, 'bundle')).find((s) => s.uses === './.gitea/actions/tauri-bundle');
  assert.equal(String(step.with['pre-sign']).replace(/\s+/g, ''), '${{inputs.pre-sign}}');
  assert.ok('pre-sign' in loadAction('tauri-bundle').inputs);
  const sh = readFileSync(join(REPO, 'conformance', 'ci', 'tauri-bundle.sh'), 'utf8').split('\n').filter((l) => !l.trim().startsWith('#'));
  const at = (re) => sh.findIndex((l) => re.test(l));
  assert.ok(at(/^\s*node "\$pre_sign" build$/) > at(/updater_key_take$/) && at(/^\s*node "\$pre_sign" build$/) > at(/npm ci/), 'the build part runs after npm ci and takes no key');
  assert.equal(at(/^\s*updater_key_exec node "\$pre_sign" sign$/), at(/^\s*node "\$pre_sign" build$/) + 1, 'the sign part runs through updater_key_exec');
  assert.ok(at(/updater_key_exec node "\$pre_sign" sign/) > at(/updater_key_throwaway "\$app_dir"/) && at(/updater_key_exec node "\$pre_sign" sign/) < at(/before-build\.mjs/), 'with the key the bundle gets, before the hook');
});

test('macOS only: the bundle\'s ad-hoc re-sign leaves the signed helper\'s bytes unchanged, so check-bundle.sh\'s hash holds', { skip: process.platform !== 'darwin' && 'needs macOS codesign and cc (not measured elsewhere)' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'dyno-helper-macho-'));
  writeFileSync(join(dir, 'Info.plist'), '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>net.decryptology.dyno.helper</string><key>CFBundleName</key><string>dyno-helper</string><key>CFBundleVersion</key><string>1</string></dict></plist>');
  writeFileSync(join(dir, 'm.c'), 'int main(void){return 0;}');
  const helper = join(dir, `dyno-helper-${TRIPLE}`);
  const cc = spawnSync('cc', ['-o', helper, join(dir, 'm.c'), '-Wl,-sectcreate,__TEXT,__info_plist,' + join(dir, 'Info.plist')], { encoding: 'utf8' });
  assert.equal(cc.status, 0, cc.stderr);
  assert.equal(spawnSync('codesign', ['--force', '-s', '-', helper]).status, 0);
  const signedHash = sha(helper); // what sign-helper.mjs records
  // What Tauri's bundler does: copy the sidecar to Contents/MacOS/dyno-helper and `codesign --force -s <identity>` it (ad hoc: "-").
  mkdirSync(join(dir, 'Dyno.app', 'Contents', 'MacOS'), { recursive: true });
  const bundled = join(dir, 'Dyno.app', 'Contents', 'MacOS', 'dyno-helper');
  cpSync(helper, bundled);
  const re = spawnSync('codesign', ['--force', '-s', '-', bundled], { encoding: 'utf8' });
  assert.equal(re.status, 0, re.stderr);
  assert.match(re.stderr, /replacing existing signature/);
  assert.equal(sha(bundled), signedHash, 'the re-sign changed the helper\'s bytes: check-bundle.sh would fail the release');
  assert.match(spawnSync('codesign', ['-dv', bundled], { encoding: 'utf8' }).stderr, /Identifier=net\.decryptology\.dyno\.helper/);
});
