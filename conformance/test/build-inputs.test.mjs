import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { ensureInput, installTool, readInventory, seedTauri, storeUrl, tauriToolsDir } from '../ci/build-inputs-reader.mjs';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const root = fileURLToPath(new URL('../..', import.meta.url));

const inventory = (entries) =>
  `format = 1\n${entries
    .map(
      (e) =>
        `\n[[input]]\nid = "${e.id}"\nversion = "1"\npackage = "${e.id}"\nfile = "${e.file}"\ntied_to = "@tauri-apps/cli@2.12.3"\n\n  [[input.platform]]\n  key = "linux-x86_64"\n  url = "https://example.invalid/${e.file}"\n  size = ${e.size}\n  sha256 = "${e.sha}"\n`,
    )
    .join('')}`;

const files = {
  'AppRun-x86_64': Buffer.from('apprun-bytes'),
  'linuxdeploy-x86_64.AppImage': Buffer.from('linuxdeploy-bytes'),
  'linuxdeploy-plugin-appimage-x86_64.AppImage': Buffer.from('plugin-bytes'),
  'runtime-x86_64': Buffer.from('runtime-bytes'),
};
const ids = {
  'AppRun-x86_64': 'apprun',
  'linuxdeploy-x86_64.AppImage': 'linuxdeploy',
  'linuxdeploy-plugin-appimage-x86_64.AppImage': 'linuxdeploy-plugin-appimage',
  'runtime-x86_64': 'appimage-type2-runtime',
};
const linuxInventory = inventory(Object.entries(files).map(([file, b]) => ({ id: ids[file], file, size: b.length, sha: sha(b) })));

/** A fake store: serves /<pkg>/<version>/<file> for `served`, and records each request's authorization header. */
async function fakeStore(served, { status } = {}) {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push({ url: req.url, auth: req.headers.authorization });
    if (status) return res.writeHead(status).end();
    const hit = Object.entries(served).find(([file]) => req.url.endsWith(`/${file}`));
    if (!hit) return res.writeHead(404).end();
    res.writeHead(200).end(hit[1]);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  after(() => server.close());
  return { base: `http://127.0.0.1:${server.address().port}/api/packages/paddock-build-inputs/generic`, seen };
}
const tmp = () => mkdtempSync(join(tmpdir(), 'bi-test-'));
const apprun = () => readInventory(linuxInventory).find((x) => x.id === 'apprun');

test('the real inventory parses, and every platform has a 64-hex SHA-256 and a positive size', () => {
  const inv = readInventory(readFileSync(join(root, 'build-inputs.toml'), 'utf8'));
  assert.deepEqual(inv.map((e) => e.id).sort(), ['apprun', 'appimage-type2-runtime', 'cargo-about', 'cargo-cyclonedx', 'git-cliff', 'linuxdeploy', 'linuxdeploy-plugin-appimage', 'nsis', 'nsis-tauri-utils'].sort());
  for (const e of inv) {
    assert.ok(e.platforms.length > 0, e.id);
    for (const p of e.platforms) {
      assert.match(p.sha256, /^[0-9a-f]{64}$/, e.id);
      assert.ok(p.size > 0, e.id);
      assert.match(p.url, /^https:\/\//);
    }
  }
});

test('the Tauri helpers are tied to the locked @tauri-apps/cli version', () => {
  const inv = readInventory(readFileSync(join(root, 'build-inputs.toml'), 'utf8'));
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  const locked = lock.packages['node_modules/@tauri-apps/cli'].version;
  for (const e of inv.filter((x) => x.tied_to)) {
    assert.equal(e.tied_to, `@tauri-apps/cli@${locked}`, `${e.id}: re-read the entry for the new Tauri CLI (ADR 0062 section 2)`);
  }
});

test('ensureInput fetches from the store with the token and verifies size and SHA-256', async () => {
  const store = await fakeStore(files);
  const dest = join(tmp(), 'AppRun-x86_64');
  const r = await ensureInput(apprun(), 'linux-x86_64', dest, { store: store.base, token: 'T0KEN' });
  assert.equal(readFileSync(dest, 'utf8'), 'apprun-bytes');
  assert.equal(r.tier, 'private-store');
  assert.equal(store.seen[0].url, '/api/packages/paddock-build-inputs/generic/apprun/1/AppRun-x86_64');
  assert.equal(store.seen[0].auth, 'token T0KEN');
});

test('a hash mismatch names expected and got, never the token, and leaves no file behind', async () => {
  const store = await fakeStore(files);
  const bad = readInventory(inventory([{ id: 'apprun', file: 'AppRun-x86_64', size: 12, sha: sha(Buffer.from('other')) }]))[0];
  const dest = join(tmp(), 'x');
  await assert.rejects(ensureInput(bad, 'linux-x86_64', dest, { store: store.base, token: 'T0KEN' }), (err) => {
    assert.match(err.message, /^::error title=build input apprun::/);
    assert.match(err.message, /SHA-256 mismatch/);
    assert.ok(err.message.includes(sha(Buffer.from('other'))));
    assert.ok(err.message.includes(sha(files['AppRun-x86_64'])));
    assert.ok(!err.message.includes('T0KEN'));
    return true;
  });
  assert.equal(existsSync(dest), false);
});

test('a size mismatch is named before the hash', async () => {
  const store = await fakeStore(files);
  const e = readInventory(inventory([{ id: 'apprun', file: 'AppRun-x86_64', size: 999, sha: sha(files['AppRun-x86_64']) }]))[0];
  await assert.rejects(ensureInput(e, 'linux-x86_64', join(tmp(), 'x'), { store: store.base, token: 't' }), /size mismatch.*expected 999.*got 12/s);
});

test('a 404, a 401 and a missing token each name their cause and the next step', async () => {
  const s404 = await fakeStore({});
  await assert.rejects(ensureInput(apprun(), 'linux-x86_64', join(tmp(), 'a'), { store: s404.base, token: 't' }), /HTTP 404.*apprun\/1.*Next: run `.*mirror/s);
  const s401 = await fakeStore({}, { status: 401 });
  await assert.rejects(ensureInput(apprun(), 'linux-x86_64', join(tmp(), 'b'), { store: s401.base, token: 'bad' }), /HTTP 401.*BUILD_INPUTS_TOKEN/s);
  await assert.rejects(ensureInput(apprun(), 'linux-x86_64', join(tmp(), 'c'), { store: s401.base, token: '' }), /BUILD_INPUTS_TOKEN is not set/);
});

test('a 5xx is retried twice, then reported with its status', async () => {
  const s = await fakeStore({}, { status: 504 });
  await assert.rejects(ensureInput(apprun(), 'linux-x86_64', join(tmp(), 'a'), { store: s.base, token: 't', backoffMs: 1 }), /HTTP 504/);
  assert.equal(s.seen.length, 3);
});

test('a refused connection is a named cause', async () => {
  await assert.rejects(ensureInput(apprun(), 'linux-x86_64', join(tmp(), 'a'), { store: 'http://127.0.0.1:9/p', token: 't', backoffMs: 1 }), /could not connect.*127\.0\.0\.1/s);
});

test('seedTauri (Linux) puts fresh copies where Tauri reads them and points LDAI_RUNTIME_FILE at the runtime', async () => {
  const store = await fakeStore(files);
  const home = tmp();
  const env = { HOME: home, XDG_CACHE_HOME: join(home, 'cache') };
  const dir = tauriToolsDir({ platform: 'linux', env });
  assert.equal(dir, join(home, 'cache', 'tauri'));
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'linuxdeploy-07333c6-x86_64.AppImage'), 'patched-by-an-earlier-run');
  const out = await seedTauri({ platform: 'linux', env, inventory: readInventory(linuxInventory), store: store.base, token: 't' });
  assert.equal(readFileSync(join(dir, 'AppRun-x86_64'), 'utf8'), 'apprun-bytes');
  assert.equal(readFileSync(join(dir, 'linuxdeploy-07333c6-x86_64.AppImage'), 'utf8'), 'linuxdeploy-bytes');
  assert.equal(readFileSync(join(dir, 'linuxdeploy-plugin-appimage.AppImage'), 'utf8'), 'plugin-bytes');
  assert.ok(statSync(join(dir, 'linuxdeploy-07333c6-x86_64.AppImage')).mode & 0o100, 'executable');
  assert.equal(readFileSync(out.env.LDAI_RUNTIME_FILE, 'utf8'), 'runtime-bytes');
  assert.equal(out.summary.length, 4);
  assert.match(out.summary[0], /^\S+ \S+ private-store \d+ bytes \d+ ms$/);
});

test('seedTauri on macOS seeds nothing', async () => {
  const out = await seedTauri({ platform: 'darwin', env: {}, inventory: [], store: 'http://127.0.0.1:9', token: '' });
  assert.deepEqual(out.env, {});
  assert.deepEqual(out.summary, []);
});

test('storeUrl follows Gitea generic package paths', () => {
  assert.equal(storeUrl('https://h/api/packages/o/generic', { package: 'p', version: 'v', file: 'f' }), 'https://h/api/packages/o/generic/p/v/f');
});

const lib = join(root, 'conformance', 'ci');
const sh = (script, env = {}) => spawnSync('bash', ['-c', `set -euo pipefail; here=${lib}; . ${lib}/tauri-build-lib.sh; ${script}`], { encoding: 'utf8', env: { PATH: process.env.PATH, ...env } });

test('tauri_build names the last download of a failed build (jobs 31430 and 31446)', () => {
  const r = sh(`tauri_build bash -c 'echo Downloading https://github.com/a/AppRun-x86_64; echo Downloading https://github.com/b/linuxdeploy.AppImage; echo "http status: 504"; exit 1'`);
  assert.equal(r.status, 1);
  assert.match(r.stdout, /::error title=tauri build failed::.*https:\/\/github\.com\/b\/linuxdeploy\.AppImage/);
});

test('tauri_build passes a successful build through quietly', () => {
  const r = sh(`tauri_build echo built`);
  assert.equal(r.status, 0);
  assert.doesNotMatch(r.stdout, /::error/);
});

test('seed_build_inputs without a token fails with the ::error:: on Linux and is skipped only on request', () => {
  if (process.platform !== 'linux') return;
  const r = sh('seed_build_inputs', {});
  assert.equal(r.status, 1);
  assert.match(r.stderr, /::error title=build input apprun::.*BUILD_INPUTS_TOKEN is not set/);
  assert.equal(sh('seed_build_inputs', { PADDOCK_BUILD_INPUTS_SKIP: '1' }).status, 0);
});

test('tauri_verbose turns --verbose on only for the AppImage bundle', () => {
  const r = sh(`tauri_verbose deb,appimage; echo "[\${verbose[*]:-}]"; tauri_verbose nsis; echo "[\${verbose[*]:-}]"`);
  const [a, b] = r.stdout.trim().split('\n');
  assert.equal(b, '[]');
  assert.equal(a, process.platform === 'linux' ? '[--verbose]' : '[]');
});

test('installTool unpacks a hashed archive and puts the executable in the bin folder', async () => {
  const src = tmp();
  mkdirSync(join(src, 'pkg-1.0'));
  writeFileSync(join(src, 'pkg-1.0', 'mytool'), '#!/bin/sh\necho hi\n');
  const archive = join(src, 'mytool.tar.gz');
  assert.equal(spawnSync('tar', ['-czf', archive, '-C', src, 'pkg-1.0']).status, 0);
  const bytes = readFileSync(archive);
  const inv = readInventory(`format = 1\n[[input]]\nid = "mytool"\nversion = "1.0"\npackage = "mytool"\nfile = "mytool.tar.gz"\nbin = "mytool"\n\n  [[input.platform]]\n  key = "linux-x86_64"\n  url = "https://example.invalid/x"\n  size = ${bytes.length}\n  sha256 = "${sha(bytes)}"\n`);
  const store = await fakeStore({ 'mytool.tar.gz': bytes });
  const binDir = join(tmp(), 'bin');
  const r = await installTool({ id: 'mytool', inventory: inv, binDir, store: store.base, token: 't' });
  assert.equal(r.dest, join(binDir, 'mytool'));
  assert.ok(statSync(r.dest).mode & 0o100);
  assert.match(readFileSync(r.dest, 'utf8'), /echo hi/);
});

test('installTool rejects an archive whose hash is not the pin', async () => {
  const inv = readInventory(`format = 1\n[[input]]\nid = "mytool"\nversion = "1.0"\npackage = "mytool"\nfile = "mytool.tar.gz"\nbin = "mytool"\n\n  [[input.platform]]\n  key = "linux-x86_64"\n  url = "https://example.invalid/x"\n  size = 3\n  sha256 = "${'0'.repeat(64)}"\n`);
  const store = await fakeStore({ 'mytool.tar.gz': Buffer.from('abc') });
  await assert.rejects(installTool({ id: 'mytool', inventory: inv, binDir: join(tmp(), 'bin'), store: store.base, token: 't' }), /SHA-256 mismatch/);
});

test('dead_proxy_on is off unless asked, and then refuses a non-exempt fetch', () => {
  assert.equal(sh('dead_proxy_on; echo "[${HTTPS_PROXY:-}]"').stdout.trim(), '[]');
  const r = sh('dead_proxy_on; echo "$HTTPS_PROXY $NODE_USE_ENV_PROXY"', { PADDOCK_DEAD_PROXY: '1', PADDOCK_DEAD_PROXY_PROBE: 'https://nothing-should-resolve.example.invalid/' });
  assert.equal(r.status, 0, r.stderr + r.stdout);
  assert.match(r.stdout, /dead-proxy run: on/);
  assert.match(r.stdout, /http:\/\/127\.0\.0\.1:9 1/);
});

test('the dead-proxy probe reports "check broken" when a non-exempt fetch gets through (ER-33)', async () => {
  const server = createServer((req, res) => res.writeHead(200).end('ok'));
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  try {
    // 127.0.0.1 is exempt, so a probe on it is not refused: the check cannot see anything.
    const r = await new Promise((resolve) => {
      const p = spawn('bash', ['-c', `set -euo pipefail; here=${lib}; . ${lib}/tauri-build-lib.sh; dead_proxy_on`], { env: { PATH: process.env.PATH, PADDOCK_DEAD_PROXY: '1', PADDOCK_DEAD_PROXY_PROBE: `http://127.0.0.1:${server.address().port}/` } });
      let out = '';
      p.stdout.on('data', (d) => (out += d));
      p.on('close', (code) => resolve({ code, out }));
    });
    assert.equal(r.code, 1);
    assert.match(r.out, /::error title=dead-proxy run::check broken/);
  } finally {
    server.close();
  }
});
