// conformance/ci/tauri-tools.mjs: the pinned bundler tools for AppImage and NSIS bundles (ADR 0085 WP-9; Dyno 1.1.1 WP-2).
// Downloads go to a local stub server, never the network.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { crc32, deflateRawSync } from 'node:zlib';
import {
  cacheKey, check, defaultCache, EMBEDDED, install, kindsFor, LINUXDEPLOY, main, manifestPath, patchAppImageMagic, PINNED_FOR_CLI, PINS, retryable, TAURI_SHA1, toolsDir, unzip,
} from '../ci/tauri-tools.mjs';
import { makeApp } from './helpers.mjs';

const CI = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci');
const PADDOCK = join(CI, '..', '..');
const sha = (b) => createHash('sha256').update(b).digest('hex');
const sha1 = (b) => createHash('sha1').update(b).digest('hex');
const scratch = () => mkdtempSync(join(tmpdir(), 'paddock-tauri-tools-'));
const LINUX = { host: { platform: 'linux', arch: 'x64' } };
const WINDOWS = { host: { platform: 'win32', arch: 'x64' } };

/** A stub server: `routes` maps a path to [status, body]; it counts the requests per path. */
async function serve(routes) {
  const hits = {};
  const server = createServer((req, res) => {
    hits[req.url] = (hits[req.url] ?? 0) + 1;
    const [status, body] = routes[req.url] ?? [404, 'not found'];
    res.writeHead(status);
    res.end(body);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  return { base, hits, close: () => new Promise((r) => server.close(r)) };
}

/** A zip archive of `files` ({ name: Buffer | string }), deflated unless `stored`; `crcOf` may falsify a CRC. */
function zip(files, { stored = false, crcOf = (d) => crc32(d) } = {}) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, content] of Object.entries(files)) {
    const data = Buffer.from(content);
    const body = stored ? data : deflateRawSync(data);
    const n = Buffer.from(name);
    const h = Buffer.alloc(30);
    h.writeUInt32LE(0x04034b50, 0);
    h.writeUInt16LE(stored ? 0 : 8, 8);
    h.writeUInt32LE(crcOf(data), 14);
    h.writeUInt32LE(body.length, 18);
    h.writeUInt32LE(data.length, 22);
    h.writeUInt16LE(n.length, 26);
    const c = Buffer.alloc(46);
    c.writeUInt32LE(0x02014b50, 0);
    c.writeUInt16LE(stored ? 0 : 8, 10);
    c.writeUInt32LE(crcOf(data), 16);
    c.writeUInt32LE(body.length, 20);
    c.writeUInt32LE(data.length, 24);
    c.writeUInt16LE(n.length, 28);
    c.writeUInt32LE(offset, 42);
    locals.push(h, n, body);
    central.push(c, n);
    offset += h.length + n.length + body.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(cd.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

const APPRUN = Buffer.from('#!/bin/sh\necho apprun\n');
const DEPLOY = Buffer.from('\x7fELF\x02\x01\x01\x00AI\x02 linuxdeploy stand-in');
const appimagePins = (base, deploy = DEPLOY) => ({
  appimage: [
    { name: 'AppRun', version: 'apprun-old', url: `${base}/AppRun-x86_64`, sha256: sha(APPRUN), file: 'AppRun-x86_64', dest: 'AppRun-x86_64' },
    { name: 'linuxdeploy', version: `linuxdeploy-${LINUXDEPLOY}`, url: `${base}/linuxdeploy-x86_64.AppImage`, sha256: sha(deploy), file: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`, dest: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`, patch: 'appimage-magic' },
  ],
});
const appimageRoutes = (body = DEPLOY) => ({ '/AppRun-x86_64': [200, APPRUN], '/linuxdeploy-x86_64.AppImage': [200, body] });
const NSIS_EXE = Buffer.from('MZ nsis stand-in');
const UTILS = Buffer.from('MZ nsis_tauri_utils stand-in');
const NSIS_ZIP = zip({ 'nsis-3.13/': '', 'nsis-3.13/NSIS.exe': NSIS_EXE, 'nsis-3.13/makensis.exe': 'MZ makensis', 'nsis-3.13/Stubs/lzma-x86-unicode': 'stub' });
const nsisPins = (base) => ({
  nsis: [
    { name: 'NSIS', version: 'nsis-3.13', url: `${base}/nsis-3.13.zip`, sha256: sha(NSIS_ZIP), file: 'nsis-3.13.zip', unzip: 'nsis-3.13/', dest: 'NSIS' },
    { name: 'nsis_tauri_utils', version: 'nsis_tauri_utils-v0.5.3', url: `${base}/nsis_tauri_utils.dll`, sha256: sha(UTILS), file: 'nsis_tauri_utils.dll', dest: 'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll' },
  ],
});
const nsisSha1s = { nsis: { 'NSIS/NSIS.exe': sha1(NSIS_EXE), 'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll': sha1(UTILS) } };
const nsisRoutes = { '/nsis-3.13.zip': [200, NSIS_ZIP], '/nsis_tauri_utils.dll': [200, UTILS] };
const quiet = () => {
  const lines = [];
  return Object.assign((l) => lines.push(l), { lines });
};
const manifestOf = (tools) => Object.fromEntries(readFileSync(manifestPath(tools), 'utf8').trim().split('\n').map((l) => l.split('  ').reverse()));

/** The PADDOCK_TAURI_PINS file of the tests (refused in CI). */
function pinsFile(dir, o) {
  const f = join(dir, 'pins.json');
  writeFileSync(f, JSON.stringify({ retryDelayMs: 0, ...o }));
  return f;
}

test('tauri-tools: the real pins are exact release URLs with a SHA-256, under the names tauri-bundler 2.10.3 looks for', () => {
  for (const p of [...PINS.appimage, ...PINS.nsis]) {
    assert.match(p.url, /^https:\/\/github\.com\/(tauri-apps|linuxdeploy)\/[\w-]+\/releases\/download\/[^/]+\/[^/]+$/);
    assert.match(p.sha256, /^[0-9a-f]{64}$/);
    assert.ok(p.url.includes(`/${p.version}/`), `${p.name}: the version is the release tag in the URL`);
  }
  assert.deepEqual(PINS.appimage.map((p) => p.dest), ['AppRun-x86_64', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`, 'linuxdeploy-plugin-appimage.AppImage']);
  // The plugin: the dated build the user chose (2026-11-12), under the name linuxdeploy.rs:249-255 looks for, so Tauri skips its
  // download of the moving `continuous` build.
  assert.deepEqual(PINS.appimage[2], {
    name: 'linuxdeploy-plugin-appimage',
    version: '1-alpha-20250213-1',
    url: 'https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/1-alpha-20250213-1/linuxdeploy-plugin-appimage-x86_64.AppImage',
    sha256: '992d502a248e14ab185448ddf6f6e7d25558cb84d4623c354c3af350c25fccb3',
    file: 'linuxdeploy-plugin-appimage-1-alpha-20250213-1-x86_64.AppImage',
    dest: 'linuxdeploy-plugin-appimage.AppImage',
  });
  // nsis/mod.rs:38-43: the URLs Tauri itself downloads, so a seeded folder is the folder Tauri would have made.
  assert.deepEqual(PINS.nsis.map((p) => p.url), [
    'https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip',
    'https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll',
  ]);
  assert.deepEqual(PINS.nsis.map((p) => p.dest), ['NSIS', 'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll']);
  assert.deepEqual(Object.keys(TAURI_SHA1.nsis), ['NSIS/NSIS.exe', 'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll']);
  assert.deepEqual(EMBEDDED.appimage.map((e) => e.dest), ['linuxdeploy-plugin-gtk.sh', 'linuxdeploy-plugin-gstreamer.sh']);
  assert.equal(cacheKey(['appimage']), 'tauri-tools-appimage-apprun-old-f30140a43a0a-linuxdeploy-07333c6-36a2d7e274d1-1-alpha-20250213-1-992d502a248e');
  assert.equal(cacheKey(['nsis']), 'tauri-tools-nsis-nsis-3.13-ba63dffc4410-nsis_tauri_utils-v0.5.3-5ba143b5db4a');
  const moved = { ...PINS, appimage: PINS.appimage.map((p, i) => (i === 1 ? { ...p, sha256: 'f'.repeat(64) } : p)) };
  assert.notEqual(cacheKey(['appimage'], moved), cacheKey(['appimage']), 'a changed hash is a new cache key');
});

test('tauri-tools: the kinds follow the bundle list and the host; the kept cache folder', () => {
  assert.deepEqual(kindsFor('deb,appimage', 'linux', 'x64'), ['appimage']);
  assert.deepEqual(kindsFor('nsis', 'win32', 'x64'), ['nsis']);
  assert.deepEqual(kindsFor('nsis', 'darwin', 'arm64'), [], 'NSIS tools are pinned for the Windows runner only');
  assert.deepEqual(kindsFor('app,dmg', 'darwin', 'arm64'), []);
  assert.deepEqual(kindsFor('appimage', 'linux', 'arm64'), [], 'nothing pinned for arm64');
  assert.equal(defaultCache({ HOME: '/h' }), '/h/.cache/paddock-tauri-tools');
  assert.equal(defaultCache({ HOME: '/h', XDG_CACHE_HOME: '/x' }), '/x/paddock-tauri-tools');
  assert.equal(defaultCache({ HOME: '/h', XDG_CACHE_HOME: 'rel' }), '/h/.cache/paddock-tauri-tools', 'a relative XDG_CACHE_HOME is ignored');
  assert.equal(defaultCache({ HOME: '/h', PADDOCK_TAURI_TOOLS_CACHE: '/c' }), '/c', 'the action names the cached folder');
  assert.equal(defaultCache({ HOME: '/c/Users/ci', LOCALAPPDATA: 'C:\\Users\\ci\\AppData\\Local' }, 'win32'), join('C:\\Users\\ci\\AppData\\Local', 'paddock-tauri-tools'), 'Windows: not Git bash\'s HOME');
});

test('tauri-tools: the tools folder is cargo metadata\'s target_directory plus .tauri, asked in src-tauri', () => {
  const calls = [];
  const run = (cmd, args, o) => {
    calls.push([cmd, args, o.cwd]);
    return { status: 0, stdout: JSON.stringify({ target_directory: '/b/target-dyno' }) };
  };
  assert.equal(toolsDir('/w/apps/dyno/app/src-tauri', run), join('/b/target-dyno', '.tauri'));
  assert.deepEqual(calls, [['cargo', ['metadata', '--format-version', '1', '--no-deps'], '/w/apps/dyno/app/src-tauri']]);
  assert.throws(() => toolsDir('/x', () => ({ status: 101, stdout: '', stderr: 'error: could not find Cargo.toml' })), (e) => /^::error::`cargo metadata --no-deps` in \/x failed \(exit 101\): error: could not find Cargo\.toml\. Next step:/.test(e.annotation));
});

test('tauri-tools: AppImage: a cold cache downloads and verifies; a warm one installs with the server gone; the folder starts empty', async () => {
  const s = await serve(appimageRoutes());
  const dir = scratch();
  const cache = join(dir, 'cache');
  const tools = join(dir, 'target', '.tauri');
  const pins = appimagePins(s.base);
  const cold = await install({ kinds: ['appimage'], cache, tools, pins, log: quiet() });
  assert.deepEqual(cold.map((d) => d.source), ['download', 'download']);
  await s.close();
  // A kept build folder holds what an earlier run (or a branch) left: install empties it first.
  writeFileSync(join(tools, 'linuxdeploy-plugin-appimage.AppImage'), 'left by an earlier run');
  writeFileSync(join(tools, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), 'patched by tauri');
  const warm = await install({ kinds: ['appimage'], cache, tools, pins, fetch: () => assert.fail('a warm cache downloads nothing'), log: quiet() });
  assert.deepEqual(warm.map((d) => d.source), ['cache', 'cache']);
  assert.deepEqual(readdirSync(tools).sort(), ['AppRun-x86_64', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`]);
  assert.deepEqual(readFileSync(join(tools, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)), DEPLOY);
  assert.equal(statSync(join(tools, 'AppRun-x86_64')).mode & 0o111, 0o111, 'executable');
  // The manifest: what the folder holds after Tauri's build, linuxdeploy with its magic zeroed, the embedded scripts.
  assert.deepEqual(manifestOf(tools), {
    'AppRun-x86_64': sha(APPRUN),
    [`linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`]: sha(patchAppImageMagic(DEPLOY)),
    'linuxdeploy-plugin-gstreamer.sh': EMBEDDED.appimage[1].sha256,
    'linuxdeploy-plugin-gtk.sh': EMBEDDED.appimage[0].sha256,
  });
  assert.deepEqual([...patchAppImageMagic(DEPLOY).subarray(8, 11)], [0, 0, 0], 'the bytes Tauri\'s dd zeroes');
  assert.deepEqual(patchAppImageMagic(DEPLOY).subarray(11), DEPLOY.subarray(11));
});

test('tauri-tools: install refuses a folder that is not <build folder>/.tauri (it empties it)', async () => {
  const dir = scratch();
  writeFileSync(join(dir, 'keep.txt'), 'x');
  await assert.rejects(install({ kinds: ['appimage'], cache: join(dir, 'c'), tools: dir, pins: appimagePins('http://127.0.0.1:9'), log: quiet() }), (e) => /must be <build folder>\/\.tauri/.test(e.annotation));
  assert.ok(existsSync(join(dir, 'keep.txt')));
});

test('tauri-tools: NSIS: the zip becomes NSIS/, the plugin goes where Tauri puts it, and Tauri\'s own SHA-1 checks pass', async () => {
  const s = await serve(nsisRoutes);
  const dir = scratch();
  const tools = join(dir, 'target', '.tauri');
  mkdirSync(join(tools, 'NSIS', 'Bin'), { recursive: true });
  writeFileSync(join(tools, 'NSIS', 'Bin', 'makensis.exe'), 'a planted makensis in a kept build folder');
  const log = quiet();
  const r = await install({ kinds: ['nsis'], cache: join(dir, 'c'), tools, pins: nsisPins(s.base), sha1s: nsisSha1s, log });
  await s.close();
  assert.deepEqual(r.map((d) => d.source), ['download', 'download']);
  assert.ok(!existsSync(join(tools, 'NSIS', 'Bin')), 'nothing kept survives: Tauri checks only NSIS.exe and the plugin, so a kept makensis would be used');
  assert.deepEqual(readFileSync(join(tools, 'NSIS', 'NSIS.exe')), NSIS_EXE);
  assert.deepEqual(readFileSync(join(tools, 'NSIS', 'Plugins', 'x86-unicode', 'additional', 'nsis_tauri_utils.dll')), UTILS);
  assert.deepEqual(manifestOf(tools), {
    'NSIS/NSIS.exe': sha(NSIS_EXE),
    'NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll': sha(UTILS),
    'NSIS/Stubs/lzma-x86-unicode': sha(Buffer.from('stub')),
    'NSIS/makensis.exe': sha(Buffer.from('MZ makensis')),
  });
  assert.match(log.lines.join('\n'), /NSIS nsis-3\.13 .* from the download: 3 files ->/);
  assert.deepEqual(check({ tools }), { matched: 4, findings: [] });
});

test('tauri-tools: NSIS: a pin Tauri would throw away (its SHA-1 differs) fails install instead of a silent download', async () => {
  const s = await serve(nsisRoutes);
  const dir = scratch();
  const wrong = { nsis: { ...nsisSha1s.nsis, 'NSIS/NSIS.exe': '6d45d4e8ccf18fe1fdac9164058f356102a2ecf2' } };
  await assert.rejects(
    install({ kinds: ['nsis'], cache: join(dir, 'c'), tools: join(dir, '.tauri'), pins: nsisPins(s.base), sha1s: wrong, log: quiet() }),
    (e) => /^::error::NSIS\/NSIS\.exe has SHA-1 [0-9a-f]{40}, but tauri-bundler expects 6d45d4e8ccf18fe1fdac9164058f356102a2ecf2: Tauri would delete the pinned folder/.test(e.annotation),
  );
  await s.close();
});

test('tauri-tools: unzip reads stored and deflated entries and refuses unsafe names, bad CRCs and non-zips', () => {
  assert.deepEqual(unzip(zip({ 'a/': '', 'a/x.txt': 'hello', 'a/y.bin': Buffer.from([0, 1, 2]) })).map((e) => [e.name, e.data.toString('hex')]), [['a/x.txt', '68656c6c6f'], ['a/y.bin', '000102']]);
  assert.deepEqual(unzip(zip({ 's.txt': 'stored' }, { stored: true }))[0].data.toString(), 'stored');
  for (const name of ['../evil', 'a/../../evil', '/abs', 'C:/x', 'a\\b']) assert.throws(() => unzip(zip({ [name]: 'x' })), /leaves its folder/, name);
  assert.throws(() => unzip(zip({ 'x.txt': 'data' }, { crcOf: () => 1 })), /does not match its size or CRC-32/);
  assert.throws(() => unzip(Buffer.from('not a zip at all, just some bytes')), /not a zip archive/);
});

test('tauri-tools: a cached copy that does not match its hash is downloaded again, with a warning', async () => {
  const s = await serve(appimageRoutes());
  const dir = scratch();
  const cache = join(dir, 'cache');
  mkdirSync(cache, { recursive: true });
  writeFileSync(join(cache, 'AppRun-x86_64'), 'corrupt');
  const log = quiet();
  const r = await install({ kinds: ['appimage'], cache, tools: join(dir, '.tauri'), pins: appimagePins(s.base), log });
  await s.close();
  assert.deepEqual(r.map((d) => d.source), ['download', 'download']);
  assert.ok(log.lines.some((l) => /^::warning::The cached AppRun .* does not match its pinned SHA-256/.test(l)));
  assert.deepEqual(readFileSync(join(cache, 'AppRun-x86_64')), APPRUN);
});

test('tauri-tools: the pins were read from the @tauri-apps/cli the lockfile installs', () => {
  const lock = JSON.parse(readFileSync(join(PADDOCK, 'package-lock.json'), 'utf8'));
  const cli = lock.packages['node_modules/@tauri-apps/cli']?.version;
  assert.equal(cli, PINNED_FOR_CLI,
    `package-lock.json has @tauri-apps/cli ${cli}, the pins were read from ${PINNED_FOR_CLI}: read its tauri-bundler's linuxdeploy.rs ` +
    'prepare_tools and nsis/mod.rs, move the URLs, hashes and file names in conformance/ci/tauri-tools.mjs, then PINNED_FOR_CLI');
});

test('tauri-tools: a 504 is retried after 2 s and 4 s, then fails with one ::error:: naming the URL, the status and the next step', async () => {
  const s = await serve({ '/AppRun-x86_64': [504, 'Gateway Time-out'] });
  const dir = scratch();
  const log = quiet();
  const sleeps = [];
  const env = { HOME: dir, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(s.base), retryDelayMs: undefined }) };
  const code = await main(['install', '--bundles', 'appimage', '--cache', join(dir, 'c'), '--tools', join(dir, 't', '.tauri')], env, { ...LINUX, log, sleep: async (ms) => sleeps.push(ms) });
  await s.close();
  assert.equal(code, 1);
  assert.equal(s.hits['/AppRun-x86_64'], 3, 'three tries');
  assert.deepEqual(sleeps, [2000, 4000]);
  const errors = log.lines.filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1, log.lines.join('\n'));
  assert.match(errors[0], new RegExp(`Downloading AppRun \\(apprun-old\\) for the appimage bundle failed: ${s.base}/AppRun-x86_64 answered HTTP 504\\. Next step: re-run`));
  assert.match(errors[0], /actions\/cache key tauri-tools-appimage-/);
  assert.ok(!existsSync(join(dir, 't', '.tauri', 'AppRun-x86_64')));
});

test('tauri-tools: no answer at all (connection refused) is named as such; a 4xx other than 408 and 429 is not retried', async () => {
  const gone = await serve({});
  await gone.close();
  const dir = scratch();
  const log = quiet();
  const args = (d) => ['install', '--bundles', 'appimage', '--cache', join(d, 'c'), '--tools', join(d, 't', '.tauri')];
  assert.equal(await main(args(dir), { HOME: dir, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(gone.base) }) }, { ...LINUX, log }), 1);
  assert.match(log.lines.at(-1), new RegExp(`^::error::Downloading AppRun .*${gone.base}/AppRun-x86_64 got no answer \\(ECONNREFUSED\\)\\. Next step:`));
  assert.deepEqual([404, 403, 408, 429, 500, 504, null].map((status) => retryable({ status })), [false, false, true, true, true, true, true]);
  for (const [status, tries] of [[404, 1], [429, 3]]) {
    const s = await serve({ '/AppRun-x86_64': [status, 'x'] });
    const d = scratch();
    const l = quiet();
    assert.equal(await main(args(d), { HOME: d, PADDOCK_TAURI_PINS: pinsFile(d, { pins: appimagePins(s.base) }) }, { ...LINUX, log: l }), 1);
    await s.close();
    assert.equal(s.hits['/AppRun-x86_64'], tries, `HTTP ${status}`);
    assert.match(l.lines.at(-1), new RegExp(`answered HTTP ${status}\\. Next step:`));
  }
});

test('tauri-tools: a failed write to the cache leaves no .part file and prints one ::error::', async () => {
  const s = await serve(appimageRoutes());
  const dir = scratch();
  const log = quiet();
  const write = (path, bytes) => {
    writeFileSync(path, bytes.subarray(0, 3));
    throw Object.assign(new Error('ENOSPC: no space left on device'), { code: 'ENOSPC' });
  };
  const code = await main(['install', '--bundles', 'appimage', '--cache', join(dir, 'c'), '--tools', join(dir, 't', '.tauri')], { HOME: dir, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(s.base) }) }, { ...LINUX, log, write });
  await s.close();
  assert.equal(code, 1);
  assert.match(log.lines.at(-1), /^::error::Writing AppRun to the cache .* failed: ENOSPC: no space left on device\. Next step:/);
  assert.deepEqual(readdirSync(join(dir, 'c')), [], 'no .part file and no cached copy');
});

test('tauri-tools: the pins override is refused in a CI run', async () => {
  const dir = scratch();
  const log = quiet();
  assert.equal(await main(['install', '--bundles', 'appimage'], { HOME: dir, GITHUB_ACTIONS: 'true', PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins('http://127.0.0.1:9') }) }, { log }), 2);
  assert.equal(log.lines.filter((l) => l.startsWith('::error::')).length, 1);
  assert.match(log.lines[0], /^::error::PADDOCK_TAURI_PINS is set in a CI run/);
});

test('tauri-tools: a download whose hash differs fails loudly and is never used or cached', async () => {
  const other = Buffer.from('a different linuxdeploy');
  const s = await serve(appimageRoutes(other));
  const dir = scratch();
  const log = quiet();
  const code = await main(['install', '--bundles', 'appimage', '--cache', join(dir, 'c'), '--tools', join(dir, 't', '.tauri')], { HOME: dir, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(s.base) }) }, { ...LINUX, log });
  await s.close();
  assert.equal(code, 1);
  assert.equal(s.hits['/linuxdeploy-x86_64.AppImage'], 1, 'a mismatch is not retried');
  assert.match(log.lines.at(-1), new RegExp(`^::error::linuxdeploy from ${s.base}/linuxdeploy-x86_64\\.AppImage has SHA-256 ${sha(other)}, but ${sha(DEPLOY)} is pinned: the file was not used\\. Next step:`));
  assert.ok(!existsSync(join(dir, 'c', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)));
  assert.ok(!existsSync(join(dir, 't', '.tauri', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)));
});

test('tauri-tools: a tag run reads no kept cache (ADR 0085 D9): every tool is downloaded and verified in the run', async () => {
  const s = await serve(appimageRoutes());
  const dir = scratch();
  const kept = join(dir, 'kept');
  mkdirSync(kept);
  writeFileSync(join(kept, 'AppRun-x86_64'), APPRUN);
  writeFileSync(join(kept, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), DEPLOY);
  const log = quiet();
  const env = { HOME: dir, RUNNER_TEMP: dir, GITHUB_REF: 'refs/tags/dyno-v1.1.1', PADDOCK_TAURI_TOOLS_CACHE: kept, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(s.base) }) };
  assert.equal(await main(['install', '--bundles', 'deb,appimage', '--tools', join(dir, 'b', '.tauri')], env, { ...LINUX, log }), 0, log.lines.join('\n'));
  await s.close();
  assert.deepEqual([s.hits['/AppRun-x86_64'], s.hits['/linuxdeploy-x86_64.AppImage']], [1, 1], 'downloaded although the kept cache had both');
  assert.match(log.lines.join('\n'), /a tag run reads no kept cache/);
  assert.deepEqual(readdirSync(dir).filter((f) => f.startsWith('paddock-tauri-tools-')), [], 'the run\'s own cache folder is removed');
});

test('tauri-tools: check passes the pinned folder and flags anything else, with no exception (an error on a tag)', async () => {
  const s = await serve(appimageRoutes());
  const dir = scratch();
  const tools = join(dir, '.tauri');
  const env = { HOME: dir, PADDOCK_TAURI_PINS: pinsFile(dir, { pins: appimagePins(s.base) }) };
  assert.equal(await main(['install', '--bundles', 'appimage', '--cache', join(dir, 'c'), '--tools', tools], env, { ...LINUX, log: quiet() }), 0);
  await s.close();
  // What a Tauri build leaves: linuxdeploy patched in place by its dd.
  const deploy = join(tools, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`);
  writeFileSync(deploy, patchAppImageMagic(readFileSync(deploy)));
  const run = async (e = env) => {
    const log = quiet();
    const code = await main(['check', '--bundles', 'appimage', '--tools', tools], e, { ...LINUX, log });
    return { code, out: log.lines.join('\n') };
  };
  let r = await run();
  assert.equal(r.code, 0);
  assert.match(r.out, /2 files in .* match their pins; 0 finding\(s\)\./);
  // A plugin Tauri fetched itself (no pin for it here) is a finding like any other unpinned file: no named exception is left.
  writeFileSync(join(tools, 'linuxdeploy-plugin-appimage.AppImage'), 'from continuous');
  r = await run();
  assert.equal(r.code, 0);
  assert.match(r.out, /^::warning::Tauri tools: linuxdeploy-plugin-appimage\.AppImage is not pinned \(Tauri downloaded or wrote it itself\)/m);
  r = await run({ ...env, GITHUB_REF: 'refs/tags/dyno-v1.1.1' });
  assert.equal(r.code, 1, 'a tag run fails on any unpinned tool');
  assert.match(r.out, /^::error::Tauri tools: linuxdeploy-plugin-appimage\.AppImage is not pinned/m);
  // A changed pinned file and a link are findings too.
  writeFileSync(join(tools, 'AppRun-x86_64'), 'changed');
  symlinkSync(join(dir, 'c'), join(tools, 'linked'));
  const c = check({ tools });
  assert.deepEqual(c.findings.sort(), ['AppRun-x86_64 does not have its pinned SHA-256', 'linked is a link (the tools must be files in the build folder)', 'linuxdeploy-plugin-appimage.AppImage is not pinned (Tauri downloaded or wrote it itself)']);
  const none = join(dir, 'none', '.tauri');
  assert.deepEqual(check({ tools: none }).findings, [`check broken: no manifest ${none}.sha256 (install did not run for this folder)`]);
});

test('tauri-tools: a host without pins only says so; bad options are refused', async () => {
  const dir = scratch();
  const log = quiet();
  assert.equal(await main(['install', '--bundles', 'app,dmg'], { HOME: dir }, { host: { platform: 'darwin', arch: 'arm64' }, log }), 0);
  assert.match(log.lines[0], /^tauri-tools: no pinned bundler tools for app,dmg on darwin\/arm64/);
  assert.equal(await main(['check', '--bundles', 'nsis'], { HOME: dir }, { host: { platform: 'darwin', arch: 'arm64' }, log: quiet() }), 0);
  const key = quiet();
  assert.equal(await main(['key', '--bundles', 'nsis'], { HOME: dir }, { ...WINDOWS, log: key }), 0);
  assert.deepEqual(key.lines, [cacheKey(['nsis'])]);
  await assert.rejects(main(['install', '--bogus', 'x'], { HOME: dir }, { log: quiet() }), /unknown option --bogus/);
  await assert.rejects(main(['install'], { HOME: dir }, { log: quiet() }), /--bundles is required/);
});

test('the tauri-bundle action keeps the verified tools in actions/cache on branch runs only (ADR 0085 D9: no cache on a tag)', () => {
  const action = readFileSync(join(PADDOCK, '.gitea', 'actions', 'tauri-bundle', 'action.yml'), 'utf8');
  const at = action.indexOf('id: tools');
  const cacheAt = action.indexOf('uses: actions/cache@');
  assert.ok(at > 0 && at < cacheAt && cacheAt < action.indexOf('tauri-bundle.sh'), 'key, cache, then the build');
  assert.match(action, /id: tools\n\s+if: "runner\.os == 'Linux' && !startsWith\(github\.ref, 'refs\/tags\/'\)"\n/);
  assert.match(action, /if: steps\.tools\.outputs\.key != ''\n/);
  assert.match(action, /path: \$\{\{ steps\.tools\.outputs\.dir \}\}\n\s+key: \$\{\{ steps\.tools\.outputs\.key \}\}/);
  const block = action.slice(action.indexOf('run: |\n', at) + 'run: |\n'.length, action.indexOf('    - uses: actions/cache@'));
  const script = block.split('\n').map((l) => l.slice(8)).join('\n').replaceAll('${{ github.action_path }}', join(PADDOCK, '.gitea', 'actions', 'tauri-bundle')).replaceAll('${{ inputs.bundles }}', '$BUNDLES');
  const here = process.platform === 'linux' && process.arch === 'x64';
  for (const [bundles, key] of [['deb,appimage', here ? cacheKey(['appimage']) : ''], ['app,dmg', '']]) {
    const dir = scratch();
    const out = join(dir, 'out');
    const env = join(dir, 'env');
    const r = spawnSync('bash', ['-c', script], { encoding: 'utf8', env: { PATH: `${dirname(process.execPath)}:/usr/bin:/bin`, HOME: dir, GITHUB_OUTPUT: out, GITHUB_ENV: env, BUNDLES: bundles } });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    assert.equal(readFileSync(out, 'utf8'), `dir=${dir}/.cache/paddock-tauri-tools\nkey=${key}\n`, bundles);
    assert.equal(readFileSync(env, 'utf8'), `PADDOCK_TAURI_TOOLS_CACHE=${dir}/.cache/paddock-tauri-tools\n`);
  }
});

// ---- tauri-bundle.sh wiring ------------------------------------------------------------------------------------------------
/** A repo whose stub `cargo metadata` names $STUB_TARGET and whose stub `npx` runs `npx` (the stub tauri build). */
function toolsRepo(npx) {
  const root = makeApp({
    'app/src-tauri/Cargo.toml': '',
    'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.2.3' }),
    'bin/npm': '#!/bin/bash\n',
    'bin/npx': `#!/bin/bash\necho "npx $*" >> "$BIN_LOG"\n${npx}\n`,
    'bin/cargo': '#!/bin/bash\necho "cargo $* in $PWD" >> "$BIN_LOG"\nprintf \'{"target_directory":"%s"}\' "$STUB_TARGET"\n',
  });
  for (const b of ['npm', 'npx', 'cargo']) chmodSync(join(root, 'bin', b), 0o755);
  return root;
}
const bundleEnv = (root, env) => ({ PATH: `${join(root, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, HOME: root, CARGO_TARGET_DIR: join(root, 'target'), STUB_TARGET: join(root, 'target'), BIN_LOG: join(root, 'calls.log'), ...env });
const bundle = (root, bundles, env = {}) => spawnSync('bash', [join(CI, 'tauri-bundle.sh'), bundles], { cwd: root, encoding: 'utf8', env: bundleEnv(root, env) });
/** The same, without blocking the event loop (a stub server in this process must answer it). */
const bundleAsync = (root, bundles, env = {}) =>
  new Promise((resolve) => {
    const p = spawn('bash', [join(CI, 'tauri-bundle.sh'), bundles], { cwd: root, env: bundleEnv(root, env) });
    let out = '';
    p.stdout.on('data', (d) => (out += d));
    p.stderr.on('data', (d) => (out += d));
    p.on('close', (status) => resolve({ status, out }));
  });
// The stub build: the tools are in place before it runs (so Tauri downloads nothing); it zeroes linuxdeploy's magic as
// Tauri's dd does (linuxdeploy.rs:267-276), then leaves an AppImage.
const BUILD = (extra = '') =>
  `t="$STUB_TARGET/.tauri"\ntest -x "$t/AppRun-x86_64" && test -x "$t/linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage" || { echo "tools missing in $t"; exit 9; }\n` +
  `dd if=/dev/zero bs=1 count=3 seek=8 conv=notrunc of="$t/linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage" 2>/dev/null\n` +
  `${extra}\nmkdir -p "$CARGO_TARGET_DIR/dist/bundle/appimage" && touch "$CARGO_TARGET_DIR/dist/bundle/appimage/Demo_1.2.3_amd64.AppImage"`;

function warmCache(root) {
  const cache = join(root, 'kept');
  mkdirSync(cache, { recursive: true });
  writeFileSync(join(cache, 'AppRun-x86_64'), APPRUN);
  writeFileSync(join(cache, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), DEPLOY);
  return cache;
}
const linuxPins = (root, base = 'http://127.0.0.1:9') => pinsFile(root, { pins: appimagePins(base), host: { platform: 'linux', arch: 'x64' } });

test('tauri-bundle: an AppImage build puts the pinned tools in <build folder>/.tauri, passes useLocalToolsDir, and downloads nothing', () => {
  const root = toolsRepo(BUILD());
  const r = bundle(root, 'deb,appimage', { PADDOCK_TAURI_TOOLS_CACHE: warmCache(root), PADDOCK_TAURI_PINS: linuxPins(root) });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /tauri-tools: linuxdeploy linuxdeploy-07333c6 .* from the cache ->/);
  const log = readFileSync(join(root, 'calls.log'), 'utf8');
  assert.match(log, /^cargo metadata --format-version 1 --no-deps in \S*\/app\/src-tauri$/m, 'asked where tauri-cli asks (cwd = src-tauri)');
  assert.match(log, /npx tauri build --bundles deb,appimage --config \.\.\/target\/tools\.conf\.json -- --profile dist/);
  assert.deepEqual(JSON.parse(readFileSync(join(root, 'target', 'tools.conf.json'), 'utf8')), { bundle: { useLocalToolsDir: true } });
  assert.ok(existsSync(join(root, 'dist/Demo_1.2.3_amd64.AppImage')));
  assert.match(r.stdout, /tauri-tools: 2 files in .* match their pins; 0 finding\(s\)/, 'check ran after the build');
});

test('tauri-bundle: after the build, anything Tauri fetched itself warns on a branch and fails a tag', async () => {
  const root = toolsRepo(BUILD('touch "$STUB_TARGET/.tauri/linuxdeploy-abc1234-x86_64.AppImage"'));
  const branch = bundle(root, 'appimage', { PADDOCK_TAURI_TOOLS_CACHE: warmCache(root), PADDOCK_TAURI_PINS: linuxPins(root) });
  assert.equal(branch.status, 0, branch.stdout + branch.stderr);
  assert.match(branch.stdout, /::warning::Tauri tools: linuxdeploy-abc1234-x86_64\.AppImage is not pinned/);
  // A tag reads no kept cache: it downloads every tool in the run (from the stub server).
  const s = await serve(appimageRoutes());
  const tag = await bundleAsync(root, 'appimage', { RUNNER_TEMP: root, GITHUB_REF: 'refs/tags/dyno-v1.1.1', PADDOCK_TAURI_PINS: linuxPins(root, s.base) });
  await s.close();
  assert.equal(tag.status, 1, tag.out);
  assert.match(tag.out, /::error::Tauri tools: linuxdeploy-abc1234-x86_64\.AppImage is not pinned/);
});

test('tauri-bundle: a failed tool download stops before the build; a failed build prints one ::error:: with the next step', () => {
  const root = toolsRepo('exit 0');
  const r = bundle(root, 'appimage', { PADDOCK_TAURI_TOOLS_CACHE: join(root, 'empty'), PADDOCK_TAURI_PINS: linuxPins(root) });
  assert.equal(r.status, 1);
  assert.match(r.stdout, /::error::Downloading AppRun \(apprun-old\) for the appimage bundle failed: http:\/\/127\.0\.0\.1:9\/AppRun-x86_64 got no answer/);
  assert.doesNotMatch(readFileSync(join(root, 'calls.log'), 'utf8'), /npx tauri build/, 'tauri build never ran');
  const failing = toolsRepo('exit 3');
  const b = bundle(failing, 'deb');
  assert.equal(b.status, 3);
  assert.match(b.stdout, /::error::tauri build --bundles deb failed \(exit 3\)\. Next step: /);
  assert.doesNotMatch(readFileSync(join(failing, 'calls.log'), 'utf8'), /cargo metadata|tools\.conf/, 'a deb build needs no tools folder');
});
