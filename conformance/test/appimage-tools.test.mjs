// conformance/ci/appimage-tools.mjs: the pinned linuxdeploy and AppRun for AppImage bundles (Dyno 1.1.1 WP-2). Downloads go to a
// local stub server, never the network.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { cacheKey, defaultDirs, install, LINUXDEPLOY, main, PINNED_FOR_CLI, PINS, retryable, unpinned } from '../ci/appimage-tools.mjs';
import { makeApp } from './helpers.mjs';

const CI = join(dirname(fileURLToPath(import.meta.url)), '..', 'ci');
const sha = (b) => createHash('sha256').update(b).digest('hex');
const scratch = () => mkdtempSync(join(tmpdir(), 'paddock-appimage-'));

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

const APPRUN = Buffer.from('#!/bin/sh\necho apprun\n');
const DEPLOY = Buffer.from('\x7fELF linuxdeploy stand-in');
const pinsAt = (base, deploy = DEPLOY) => [
  { name: 'AppRun', version: 'apprun-old', url: `${base}/AppRun-x86_64`, sha256: sha(APPRUN), file: 'AppRun-x86_64' },
  { name: 'linuxdeploy', version: `linuxdeploy-${LINUXDEPLOY}`, url: `${base}/linuxdeploy-x86_64.AppImage`, sha256: sha(deploy), file: `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage` },
];
const routesFor = (body = DEPLOY) => ({ '/AppRun-x86_64': [200, APPRUN], '/linuxdeploy-x86_64.AppImage': [200, body] });
const quiet = () => {
  const lines = [];
  return Object.assign((l) => lines.push(l), { lines });
};

test('appimage-tools: the real pins are exact release URLs with a SHA-256, under the names tauri-bundler looks for', () => {
  for (const p of PINS.x86_64) {
    assert.match(p.url, /^https:\/\/github\.com\/tauri-apps\/binary-releases\/releases\/download\/[^/]+\/[^/]+$/);
    assert.match(p.sha256, /^[0-9a-f]{64}$/);
    assert.ok(p.url.includes(`/${p.version}/`), `${p.name}: the version is the release tag in the URL`);
  }
  assert.deepEqual(PINS.x86_64.map((p) => p.file), ['AppRun-x86_64', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`]);
  assert.equal(cacheKey(), 'appimage-tools-x86_64-apprun-old-f30140a43a0a-linuxdeploy-07333c6-36a2d7e274d1');
  const moved = PINS.x86_64.map((p, i) => (i === 1 ? { ...p, sha256: 'f'.repeat(64) } : p));
  assert.notEqual(cacheKey('x86_64', moved), cacheKey(), 'a changed hash is a new cache key');
});

test('appimage-tools: Tauri\'s tools folder is the dirs crate\'s cache folder (XDG_CACHE_HOME when absolute, else ~/.cache)', () => {
  assert.deepEqual(defaultDirs({ HOME: '/h' }), { cache: '/h/.cache/paddock-appimage-tools', tools: '/h/.cache/tauri' });
  assert.deepEqual(defaultDirs({ HOME: '/h', XDG_CACHE_HOME: '/x' }), { cache: '/x/paddock-appimage-tools', tools: '/x/tauri' });
  assert.equal(defaultDirs({ HOME: '/h', XDG_CACHE_HOME: 'rel' }).tools, '/h/.cache/tauri', 'a relative XDG_CACHE_HOME is ignored, as dirs does');
  assert.equal(defaultDirs({ HOME: '/h', PADDOCK_APPIMAGE_TOOLS: '/c' }).cache, '/c', 'the action names the cached folder');
});

test('appimage-tools: a cold cache downloads and verifies; a warm cache installs with the server gone (GitHub blocked)', async () => {
  const s = await serve(routesFor());
  const dir = scratch();
  const cache = join(dir, 'cache');
  const tools = join(dir, 'tauri');
  const cold = await install({ cache, tools, pins: pinsAt(s.base), log: quiet() });
  assert.deepEqual(cold.map((d) => d.source), ['download', 'download']);
  await s.close();
  // Tauri zeroes linuxdeploy's AppImage magic in place: the next run copies the verified bytes over it again.
  writeFileSync(join(tools, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), 'patched by tauri');
  const warm = await install({ cache, tools, pins: pinsAt(s.base), fetch: () => assert.fail('a warm cache downloads nothing'), log: quiet() });
  assert.deepEqual(warm.map((d) => d.source), ['cache', 'cache']);
  assert.deepEqual(readFileSync(join(tools, 'AppRun-x86_64')), APPRUN);
  assert.deepEqual(readFileSync(join(tools, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)), DEPLOY);
  assert.equal(statSync(join(tools, 'AppRun-x86_64')).mode & 0o111, 0o111, 'executable');
});

test('appimage-tools: a cached copy that does not match its hash is downloaded again, with a warning', async () => {
  const s = await serve(routesFor());
  const dir = scratch();
  const cache = join(dir, 'cache');
  mkdirSync(cache, { recursive: true });
  writeFileSync(join(cache, 'AppRun-x86_64'), 'corrupt');
  const log = quiet();
  const r = await install({ cache, tools: join(dir, 'tauri'), pins: pinsAt(s.base), log });
  await s.close();
  assert.deepEqual(r.map((d) => d.source), ['download', 'download']);
  assert.ok(log.lines.some((l) => /^::warning::The cached AppRun .* does not match its pinned SHA-256/.test(l)));
  assert.deepEqual(readFileSync(join(cache, 'AppRun-x86_64')), APPRUN);
});

test('appimage-tools: the pins were read from the @tauri-apps/cli the lockfile installs', () => {
  const lock = JSON.parse(readFileSync(join(CI, '..', '..', 'package-lock.json'), 'utf8'));
  const cli = lock.packages['node_modules/@tauri-apps/cli']?.version;
  assert.equal(cli, PINNED_FOR_CLI,
    `package-lock.json has @tauri-apps/cli ${cli}, the AppImage pins were read from ${PINNED_FOR_CLI}: read its tauri-bundler's prepare_tools ` +
    '(src/bundle/linux/appimage/linuxdeploy.rs), move the URLs, hashes and file names in conformance/ci/appimage-tools.mjs, then PINNED_FOR_CLI');
});

test('appimage-tools: a 504 is retried after 2 s and 4 s, then fails with one ::error:: naming the URL, the status and the next step', async () => {
  const s = await serve({ '/AppRun-x86_64': [504, 'Gateway Time-out'] });
  const dir = scratch();
  const log = quiet();
  const sleeps = [];
  const env = { HOME: dir, PADDOCK_APPIMAGE_PINS: pinsFile(dir, s.base, DEPLOY, {}) };
  const code = await main(['install', '--cache', join(dir, 'c'), '--tools', join(dir, 't')], env, { log, sleep: async (ms) => sleeps.push(ms) });
  await s.close();
  assert.equal(code, 1);
  assert.equal(s.hits['/AppRun-x86_64'], 3, 'three tries');
  assert.deepEqual(sleeps, [2000, 4000]);
  const errors = log.lines.filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1, log.lines.join('\n'));
  assert.match(errors[0], new RegExp(`Downloading AppRun \\(apprun-old\\) for the AppImage failed: ${s.base}/AppRun-x86_64 answered HTTP 504\\. Next step: re-run`));
  assert.match(errors[0], /actions\/cache key appimage-tools-x86_64-/);
  assert.ok(!existsSync(join(dir, 't', 'AppRun-x86_64')));
});

test('appimage-tools: no answer at all (connection refused) is named as such', async () => {
  const s = await serve({});
  await s.close();
  const dir = scratch();
  const log = quiet();
  const code = await main(['install', '--cache', join(dir, 'c'), '--tools', join(dir, 't')], { HOME: dir, PADDOCK_APPIMAGE_PINS: pinsFile(dir, s.base) }, { log, delayMs: 0 });
  assert.equal(code, 1);
  assert.match(log.lines.at(-1), new RegExp(`^::error::Downloading AppRun .*${s.base}/AppRun-x86_64 got no answer \\(ECONNREFUSED\\)\\. Next step:`));
});

test('appimage-tools: a 4xx other than 408 and 429 is not retried', async () => {
  assert.deepEqual([404, 403, 408, 429, 500, 504, null].map((status) => retryable({ status })), [false, false, true, true, true, true, true]);
  for (const [status, tries] of [[404, 1], [429, 3]]) {
    const s = await serve({ '/AppRun-x86_64': [status, 'x'] });
    const dir = scratch();
    const log = quiet();
    assert.equal(await main(['install', '--cache', join(dir, 'c'), '--tools', join(dir, 't')], { HOME: dir, PADDOCK_APPIMAGE_PINS: pinsFile(dir, s.base) }, { log }), 1);
    await s.close();
    assert.equal(s.hits['/AppRun-x86_64'], tries, `HTTP ${status}`);
    assert.match(log.lines.at(-1), new RegExp(`answered HTTP ${status}\\. Next step:`));
  }
});

test('appimage-tools: a failed write to the cache leaves no .part file and prints one ::error::', async () => {
  const s = await serve(routesFor());
  const dir = scratch();
  const log = quiet();
  const write = (path, bytes) => {
    writeFileSync(path, bytes.subarray(0, 3));
    throw Object.assign(new Error('ENOSPC: no space left on device'), { code: 'ENOSPC' });
  };
  const code = await main(['install', '--cache', join(dir, 'c'), '--tools', join(dir, 't')], { HOME: dir, PADDOCK_APPIMAGE_PINS: pinsFile(dir, s.base) }, { log, write });
  await s.close();
  assert.equal(code, 1);
  assert.match(log.lines.at(-1), /^::error::Writing AppRun to the cache .* failed: ENOSPC: no space left on device\. Next step:/);
  assert.deepEqual(readdirSync(join(dir, 'c')), [], 'no .part file and no cached copy');
});

test('appimage-tools: the pins override is refused in a CI run', async () => {
  const dir = scratch();
  const log = quiet();
  assert.equal(await main(['install'], { HOME: dir, GITHUB_ACTIONS: 'true', PADDOCK_APPIMAGE_PINS: pinsFile(dir, 'http://127.0.0.1:9') }, { log }), 2);
  assert.deepEqual(log.lines.filter((l) => l.startsWith('::error::')).length, 1);
  assert.match(log.lines[0], /^::error::PADDOCK_APPIMAGE_PINS is set in a CI run/);
  assert.ok(!existsSync(join(dir, '.cache', 'tauri')), 'nothing installed');
});

test('appimage-tools: a download whose hash differs fails loudly and is never used or cached', async () => {
  const s = await serve(routesFor(Buffer.from('a different linuxdeploy')));
  const dir = scratch();
  const log = quiet();
  const code = await main(['install', '--cache', join(dir, 'c'), '--tools', join(dir, 't')], { HOME: dir, PADDOCK_APPIMAGE_PINS: pinsFile(dir, s.base) }, { log, delayMs: 0 });
  await s.close();
  assert.equal(code, 1);
  assert.equal(s.hits['/linuxdeploy-x86_64.AppImage'], 1, 'a mismatch is not retried');
  assert.match(log.lines.at(-1), new RegExp(`^::error::linuxdeploy from ${s.base}/linuxdeploy-x86_64\\.AppImage has SHA-256 ${sha(Buffer.from('a different linuxdeploy'))}, but ${sha(DEPLOY)} is pinned: the file was not used\\. Next step:`));
  assert.ok(!existsSync(join(dir, 'c', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)));
  assert.ok(!existsSync(join(dir, 't', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`)));
  assert.deepEqual(readdirSync(join(dir, 'c')).filter((f) => f.endsWith('.part')), [], 'no .part file');
});

test('appimage-tools: check warns about a linuxdeploy Tauri fetched itself; a host without pins only warns', async () => {
  const dir = scratch();
  mkdirSync(join(dir, 't'));
  writeFileSync(join(dir, 't', `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), '');
  writeFileSync(join(dir, 't', 'linuxdeploy-abc1234-x86_64.AppImage'), '');
  writeFileSync(join(dir, 't', 'linuxdeploy-plugin-appimage.AppImage'), '');
  assert.deepEqual(unpinned(join(dir, 't')), ['linuxdeploy-abc1234-x86_64.AppImage']);
  const log = quiet();
  assert.equal(await main(['check', '--tools', join(dir, 't')], { HOME: dir }, { log, arch: 'x86_64' }), 0);
  assert.match(log.lines.join('\n'), /::warning::Tauri downloaded linuxdeploy-abc1234-x86_64\.AppImage itself \(unpinned/);
  const tag = quiet();
  assert.equal(await main(['check', '--tools', join(dir, 't')], { HOME: dir, GITHUB_REF: 'refs/tags/dyno-v1.1.1' }, { log: tag, arch: 'x86_64' }), 1, 'a tag run fails');
  assert.match(tag.lines.join('\n'), /^::error::Tauri downloaded linuxdeploy-abc1234-x86_64\.AppImage itself/m);
  const other = quiet();
  assert.equal(await main(['install'], { HOME: dir }, { log: other, arch: null }), 0);
  assert.match(other.lines[0], /^::warning::No pinned AppImage tools for /);
  await assert.rejects(main(['install', '--bogus', 'x'], { HOME: dir }, { log: quiet() }), /unknown option --bogus/);
});

function pinsFile(dir, base, deploy = DEPLOY, extra = { retryDelayMs: 0 }) {
  const f = join(dir, 'pins.json');
  writeFileSync(f, JSON.stringify({ arch: 'x86_64', pins: pinsAt(base, deploy), ...extra }));
  return f;
}

test('the tauri-bundle action keys actions/cache on the pins and names the cached folder for the script', () => {
  const PADDOCK = join(CI, '..', '..');
  const action = readFileSync(join(PADDOCK, '.gitea', 'actions', 'tauri-bundle', 'action.yml'), 'utf8');
  const at = action.indexOf('id: appimage');
  assert.ok(at > 0 && at < action.indexOf('uses: actions/cache@') && action.indexOf('uses: actions/cache@') < action.indexOf('tauri-bundle.sh'), 'key, cache, then the build');
  assert.match(action, /path: \$\{\{ steps\.appimage\.outputs\.dir \}\}\n\s+key: \$\{\{ steps\.appimage\.outputs\.key \}\}/);
  assert.match(action, /if: runner\.os == 'Linux' && contains\(format\(',\{0\},', inputs\.bundles\), ',appimage,'\)/);
  const block = action.slice(action.indexOf('run: |\n', at) + 'run: |\n'.length, action.indexOf('    - uses: actions/cache@'));
  const script = block.split('\n').map((l) => l.slice(8)).join('\n').replace('${{ github.action_path }}', join(PADDOCK, '.gitea', 'actions', 'tauri-bundle'));
  const dir = scratch();
  const out = join(dir, 'out');
  const env = join(dir, 'env');
  const r = spawnSync('bash', ['-c', script], { encoding: 'utf8', env: { PATH: `${dirname(process.execPath)}:/usr/bin:/bin`, HOME: dir, GITHUB_OUTPUT: out, GITHUB_ENV: env } });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(readFileSync(out, 'utf8'), `dir=${dir}/.cache/paddock-appimage-tools\nkey=${cacheKey()}\n`);
  assert.equal(readFileSync(env, 'utf8'), `PADDOCK_APPIMAGE_TOOLS=${dir}/.cache/paddock-appimage-tools\n`);
});

// ---- tauri-bundle.sh wiring ------------------------------------------------------------------------------------------------
function appimageRepo(npx) {
  const root = makeApp({
    'app/src-tauri/Cargo.toml': '',
    'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.2.3' }),
    'bin/npm': '#!/bin/bash\n',
    'bin/npx': `#!/bin/bash\necho "npx $*" >> "$BIN_LOG"\n${npx}\n`,
  });
  chmodSync(join(root, 'bin/npm'), 0o755);
  chmodSync(join(root, 'bin/npx'), 0o755);
  return root;
}
const bundle = (root, bundles, env = {}) =>
  spawnSync('bash', [join(CI, 'tauri-bundle.sh'), bundles], {
    cwd: root,
    encoding: 'utf8',
    env: { PATH: `${join(root, 'bin')}:${dirname(process.execPath)}:/usr/bin:/bin`, HOME: root, CARGO_TARGET_DIR: join(root, 'target'), BIN_LOG: join(root, 'calls.log'), ...env },
  });

test('tauri-bundle: an AppImage build puts the pinned tools where Tauri looks before it builds, and checks after', () => {
  const root = appimageRepo(
    // The stub build sees the tools already in place (so Tauri would download nothing) and leaves an AppImage.
    `test -x "$HOME/.cache/tauri/AppRun-x86_64" && test -x "$HOME/.cache/tauri/linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage" || { echo "tools missing"; exit 9; }\n` +
      `mkdir -p "$CARGO_TARGET_DIR/dist/bundle/appimage" && touch "$CARGO_TARGET_DIR/dist/bundle/appimage/Demo_1.2.3_amd64.AppImage"`,
  );
  // A warm cache: no server at all.
  const cache = join(root, 'kept');
  mkdirSync(cache);
  writeFileSync(join(cache, 'AppRun-x86_64'), APPRUN);
  writeFileSync(join(cache, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), DEPLOY);
  const r = bundle(root, 'deb,appimage', { PADDOCK_APPIMAGE_TOOLS: cache, PADDOCK_APPIMAGE_PINS: pinsFile(root, 'http://127.0.0.1:9') });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /appimage-tools: linuxdeploy linuxdeploy-07333c6 .* from the cache/);
  assert.ok(existsSync(join(root, 'dist/Demo_1.2.3_amd64.AppImage')));
});

test('tauri-bundle: after an AppImage build it runs check: a linuxdeploy Tauri fetched itself warns, and fails a tag run', () => {
  const root = appimageRepo(
    // The stub build behaves like a Tauri whose bundler moved to another linuxdeploy: it downloads that one itself.
    'touch "$HOME/.cache/tauri/linuxdeploy-abc1234-x86_64.AppImage"\n' +
      'mkdir -p "$CARGO_TARGET_DIR/dist/bundle/appimage" && touch "$CARGO_TARGET_DIR/dist/bundle/appimage/Demo_1.2.3_amd64.AppImage"',
  );
  const cache = join(root, 'kept');
  mkdirSync(cache);
  writeFileSync(join(cache, 'AppRun-x86_64'), APPRUN);
  writeFileSync(join(cache, `linuxdeploy-${LINUXDEPLOY}-x86_64.AppImage`), DEPLOY);
  const env = { PADDOCK_APPIMAGE_TOOLS: cache, PADDOCK_APPIMAGE_PINS: pinsFile(root, 'http://127.0.0.1:9') };
  const branch = bundle(root, 'appimage', env);
  assert.equal(branch.status, 0, branch.stdout + branch.stderr);
  assert.match(branch.stdout, /::warning::Tauri downloaded linuxdeploy-abc1234-x86_64\.AppImage itself/);
  const tag = bundle(root, 'appimage', { ...env, GITHUB_REF: 'refs/tags/dyno-v1.1.1' });
  assert.equal(tag.status, 1);
  assert.match(tag.stdout, /::error::Tauri downloaded linuxdeploy-abc1234-x86_64\.AppImage itself/);
});

test('tauri-bundle: a failed tool download stops before the build; a failed build prints one ::error:: with the next step', () => {
  const root = appimageRepo('exit 0');
  const r = bundle(root, 'appimage', { PADDOCK_APPIMAGE_TOOLS: join(root, 'kept'), PADDOCK_APPIMAGE_PINS: pinsFile(root, 'http://127.0.0.1:9') });
  assert.equal(r.status, 1);
  assert.match(r.stdout, /::error::Downloading AppRun \(apprun-old\) for the AppImage failed: http:\/\/127\.0\.0\.1:9\/AppRun-x86_64 got no answer/);
  assert.ok(!existsSync(join(root, 'calls.log')), 'tauri build never ran');
  const failing = appimageRepo('exit 3');
  const b = bundle(failing, 'deb');
  assert.equal(b.status, 3);
  assert.match(b.stdout, /::error::tauri build --bundles deb failed \(exit 3\)\. Next step: /);
});
