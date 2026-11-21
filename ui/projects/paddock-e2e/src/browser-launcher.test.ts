import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { browserProblems, isInstalledBrowser, LAUNCHER_PREFIX } from './browser.ts';
import { browserLauncher, headlessShellIn, installLocation, internalBrowserUse, LauncherError, launcherScript, realBrowser, shellQuote, writeLauncher } from './browser-launcher.ts';

const posix = process.platform !== 'win32';
const scratch = () => mkdtempSync(join(tmpdir(), 'launcher-test-'));

/** A stand-in browser: records its working folder, its arguments (one per line) and an environment variable, then exits. */
function standInBrowser(dir: string, record: string): string {
  const path = join(dir, 'stand-in browser');
  writeFileSync(path, `#!/bin/sh\npwd -P > '${record}'\nfor a in "$@"; do printf '%s\\n' "$a" >> '${record}'; done\nprintf 'ENV=%s\\n' "$PROBE_VAR" >> '${record}'\n`, { mode: 0o755 });
  chmodSync(path, 0o755);
  return path;
}

test('shellQuote keeps spaces and single quotes inside one word', { skip: !posix }, () => {
  const text = `it's a "path" with $x and spaces`;
  const out = spawnSync('/bin/sh', ['-c', `printf '%s' ${shellQuote(text)}`], { encoding: 'utf8' });
  assert.equal(out.stdout, text);
});

test('the wrapper starts the browser in the internal folder; arguments and environment pass through unchanged', { skip: !posix }, () => {
  const dir = scratch();
  try {
    const record = join(dir, 'record.txt');
    const real = standInBrowser(dir, record);
    const cwd = join(dir, 'internal cwd', "it's");
    const wrapper = writeLauncher(join(dir, 'launcher'), real, cwd);
    assert.ok(wrapper.includes(LAUNCHER_PREFIX));
    assert.ok((statSync(wrapper).mode & 0o111) !== 0, 'executable');
    const callers = join(dir, 'checkout');
    mkdirSync(callers);
    const args = ['--user-data-dir=/a b/c', '--flag', 'it\'s "x"', '', '$HOME', 'about:blank'];
    const r = spawnSync(wrapper, args, { cwd: callers, env: { ...process.env, PROBE_VAR: 'kept' } });
    assert.equal(r.status, 0, String(r.stderr));
    const [seenCwd, ...rest] = readFileSync(record, 'utf8').split('\n');
    assert.equal(seenCwd, realpathSync(cwd), 'the browser started in the internal folder');
    assert.deepEqual(rest.slice(0, args.length), args, 'arguments unchanged');
    assert.equal(rest[args.length], 'ENV=kept', 'environment unchanged');
    // Positive control: the same stand-in without the wrapper starts in the caller's folder (the checkout).
    const direct = spawnSync(real, [], { cwd: callers, env: process.env });
    assert.equal(direct.status, 0);
    assert.equal(readFileSync(record, 'utf8').split('\n')[0], realpathSync(callers));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the browser keeps its pid: the wrapper replaces itself with the browser (exec)', { skip: !posix }, () => {
  assert.match(launcherScript('/r/b', '/c'), /\nexec '\/r\/b' "\$@"\n$/);
});

test('workers writing the wrapper at once each get a complete, executable script (write then rename)', { skip: !posix }, async () => {
  const dir = scratch();
  try {
    const real = standInBrowser(dir, join(dir, 'r.txt'));
    const here = new URL('./browser-launcher.ts', import.meta.url).href;
    const script = `import { writeLauncher } from '${here}'; console.log(writeLauncher(process.argv[1], process.argv[2], process.argv[3]));`;
    const runs = Array.from({ length: 12 }, () =>
      new Promise<string>((resolve, reject) => {
        const p = spawn(process.execPath, ['--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--input-type=module', '-e', script, join(dir, 'launcher'), real, join(dir, 'cwd')], { stdio: ['ignore', 'pipe', 'inherit'] });
        let out = '';
        p.stdout.on('data', (b) => (out += b));
        p.on('exit', (code) => (code === 0 ? resolve(out.trim()) : reject(new Error(`worker exited ${code}`))));
      }),
    );
    const paths = await Promise.all(runs);
    assert.equal(new Set(paths).size, 1, 'one name for one content');
    assert.equal(readFileSync(paths[0]!, 'utf8'), launcherScript(real, join(dir, 'cwd')));
    assert.deepEqual(readdirSync(join(dir, 'launcher')).filter((f) => f.endsWith('.tmp')), [], 'no private file left behind');
    assert.equal(spawnSync(paths[0]!, []).status, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a wrapper that cannot be written stops with an ::error:: line, and never falls back to the checkout', { skip: !posix }, () => {
  const dir = scratch();
  try {
    writeFileSync(join(dir, 'file'), 'x');
    assert.throws(
      () => writeLauncher(join(dir, 'file', 'under-a-file'), '/r/b', join(dir, 'cwd')),
      (e: Error) => e instanceof LauncherError && /::error::browser launcher: cannot write .* Next: /.test(e.message),
    );
    // On macOS a missing browser is an error too, not a silent run in the checkout.
    assert.throws(() => browserLauncher(join(dir, 'root'), {}, 'darwin', () => false, () => { throw new Error('no cli'); }), /::error::browser launcher: Playwright could not say where its headless shell is/);
    assert.throws(() => browserLauncher(join(dir, 'root'), {}, 'darwin', () => false, () => 'Chrome Headless Shell 1\n  Install location:    /no/such/folder\n'), /headless shell is not installed in \/no\/such\/folder/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('where the wrapper is not needed or not allowed nothing is written', () => {
  const dir = scratch();
  try {
    assert.equal(browserLauncher(join(dir, 'root'), { CHROMIUM_TEST_PATH: '/r/b' }, 'linux'), undefined);
    assert.equal(browserLauncher(join(dir, 'root'), { CHROMIUM_TEST_PATH: '/r/b' }, 'win32'), undefined);
    assert.equal(browserLauncher('/Volumes/vault-dev/x', { CHROMIUM_TEST_PATH: '/r/b' }, 'darwin'), undefined);
    assert.deepEqual(readdirSync(dir), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the real browser: the pinned one, else Playwright’s headless shell found through its own install listing', () => {
  assert.equal(realBrowser({ CHROMIUM_TEST_PATH: '/pinned/chrome' }), '/pinned/chrome');
  assert.throws(() => realBrowser({ CHROMIUM_TEST_PATH: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' }), /installed browser/);
  const dry = 'Chrome Headless Shell 156.0 (playwright chromium-headless-shell v1248)\n  Install location:    /pw/chromium_headless_shell-1248\n  Download url:        https://x\n\nFFmpeg (playwright ffmpeg v1013)\n  Install location:    /pw/ffmpeg-1013\n';
  assert.equal(installLocation(dry), '/pw/chromium_headless_shell-1248');
  assert.equal(installLocation('FFmpeg only\n  Install location: /pw/ffmpeg\n'), undefined);
  const exe = process.platform === 'win32' ? 'chrome-headless-shell.exe' : 'chrome-headless-shell';
  const has = new Set(['/pw/chromium_headless_shell-1248', join('/pw/chromium_headless_shell-1248', 'chrome-headless-shell-x', exe)]);
  assert.equal(headlessShellIn('/pw/chromium_headless_shell-1248', (p) => has.has(p), () => ['chrome-headless-shell-x']), join('/pw/chromium_headless_shell-1248', 'chrome-headless-shell-x', exe));
  assert.throws(() => realBrowser({}, () => dry), /headless shell is not installed in \/pw\/chromium_headless_shell-1248/);
});

test('the browser guard accepts the wrapper, and a wrapper around an installed browser is never made', () => {
  assert.deepEqual(browserProblems({ launchOptions: { executablePath: `/t/${LAUNCHER_PREFIX}0123.sh` } }, {}), []);
  assert.ok(browserProblems({ launchOptions: { executablePath: '/t/other.sh' } }, {}).length > 0, 'positive control: another path is still refused');
  assert.equal(isInstalledBrowser('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), true);
});

test('internalBrowserUse puts the wrapper (macOS) or the pinned browser (elsewhere) in executablePath, and `off` leaves the pinned one', () => {
  const dir = scratch();
  try {
    const env = { PADDOCK_JOB_TEMP: dir, CHROMIUM_TEST_PATH: process.execPath };
    const on = internalBrowserUse(env).launchOptions.executablePath;
    if (process.platform === 'darwin') assert.ok(on?.includes(LAUNCHER_PREFIX), String(on));
    else assert.equal(on, process.execPath);
    assert.equal(internalBrowserUse(env, 'off').launchOptions.executablePath, process.execPath);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
