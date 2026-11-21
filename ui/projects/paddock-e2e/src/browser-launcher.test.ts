import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { browserProblems, LAUNCHER_PREFIX } from './browser.ts';
import { browserLauncher, internalBrowserUse, LauncherError, launcherScript, type LauncherSpec, shellQuote, writeLauncher } from './browser-launcher.ts';

const posix = process.platform !== 'win32';
const scratch = () => mkdtempSync(join(tmpdir(), 'launcher-test-'));

/** A stand-in browser: records its name, working folder, arguments (one per line) and an environment variable, then exits. */
function standIn(dir: string, name: string, record: string): string {
  mkdirSync(dir, { recursive: true });
  const path = join(dir, name);
  writeFileSync(path, `#!/bin/sh\necho '${name}' > '${record}'\npwd -P >> '${record}'\nfor a in "$@"; do printf '%s\\n' "$a" >> '${record}'; done\nprintf 'ENV=%s\\n' "$PROBE_VAR" >> '${record}'\n`, { mode: 0o755 });
  chmodSync(path, 0o755);
  return path;
}

/** The files a launcher without a pinned browser reads: a fake `playwright install --dry-run` (it counts its calls) and a fake `playwright-core`. */
function fakePlaywright(dir: string, headlessShellDir: string, chromiumPath: string) {
  const cli = join(dir, 'cli.js');
  writeFileSync(cli, `require('node:fs').appendFileSync(${JSON.stringify(join(dir, 'dry-runs.txt'))}, 'x');\nconsole.log('Chrome Headless Shell 1 (playwright chromium-headless-shell v1)\\n  Install location:    ${headlessShellDir}\\n  Download url:        https://x\\n\\nFFmpeg (playwright ffmpeg v1)\\n  Install location:    /pw/ffmpeg\\n');\n`);
  const core = join(dir, 'core.js');
  writeFileSync(core, `module.exports = { chromium: { executablePath: () => ${JSON.stringify(chromiumPath)} } };\n`);
  return { node: process.execPath, cli, core, cache: join(dir, 'cache'), cwd: join(dir, 'internal cwd') } satisfies LauncherSpec;
}

const run = (wrapper: string, args: string[], cwd: string, env: Record<string, string> = {}) =>
  spawnSync(wrapper, args, { cwd, env: { ...process.env, ...env }, encoding: 'utf8' });

test('shellQuote keeps spaces and single quotes inside one word', { skip: !posix }, () => {
  const text = `it's a "path" with $x and spaces`;
  assert.equal(spawnSync('/bin/sh', ['-c', `printf '%s' ${shellQuote(text)}`], { encoding: 'utf8' }).stdout, text);
});

test('the wrapper starts the browser in the internal folder; arguments and environment pass through unchanged', { skip: !posix }, () => {
  const dir = scratch();
  try {
    const record = join(dir, 'record.txt');
    const real = standIn(dir, 'stand-in browser', record);
    const cwd = join(dir, 'internal cwd', "it's");
    const wrapper = writeLauncher(join(dir, 'launcher'), { cwd, pinned: real, cache: join(dir, 'c'), node: '', cli: '', core: '' });
    assert.ok(wrapper.includes(LAUNCHER_PREFIX));
    assert.ok((statSync(wrapper).mode & 0o111) !== 0, 'executable');
    const callers = join(dir, 'checkout');
    mkdirSync(callers);
    const args = ['--user-data-dir=/a b/c', '--flag', 'it\'s "x"', '', '$HOME', 'about:blank'];
    const r = run(wrapper, args, callers, { PROBE_VAR: 'kept' });
    assert.equal(r.status, 0, r.stderr);
    const [, seenCwd, ...rest] = readFileSync(record, 'utf8').split('\n');
    assert.equal(seenCwd, realpathSync(cwd), 'the browser started in the internal folder');
    assert.deepEqual(rest.slice(0, args.length), args, 'arguments unchanged');
    assert.equal(rest[args.length], 'ENV=kept', 'environment unchanged');
    // Positive control: the same stand-in without the wrapper starts in the caller's folder (the checkout).
    assert.equal(run(real, [], callers).status, 0);
    assert.equal(readFileSync(record, 'utf8').split('\n')[1], realpathSync(callers));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('Playwright’s own browser is found when it starts, by the arguments: the headless shell for --headless, the full Chromium otherwise, and the answer is kept', { skip: !posix }, () => {
  const dir = scratch();
  try {
    const record = join(dir, 'record.txt');
    const shellDir = join(dir, 'pw', 'chromium_headless_shell-1');
    standIn(join(shellDir, 'chrome-headless-shell-mac'), 'chrome-headless-shell', record);
    const full = standIn(join(dir, 'pw', 'chromium-1'), 'Chromium', record);
    const spec = fakePlaywright(dir, shellDir, full);
    const wrapper = writeLauncher(join(dir, 'launcher'), spec);
    const callers = join(dir, 'checkout');
    mkdirSync(callers);
    const started = (args: string[]) => {
      rmSync(record, { force: true });
      const r = run(wrapper, args, callers);
      assert.equal(r.status, 0, r.stderr);
      const [name, cwd] = readFileSync(record, 'utf8').split('\n');
      assert.equal(cwd, realpathSync(spec.cwd), 'in the internal folder');
      return name;
    };
    assert.equal(started(['--headless', '--x']), 'chrome-headless-shell');
    assert.equal(started(['--headless=old']), 'chrome-headless-shell');
    // Headed (no --headless), and the new headless mode (a full Chromium option): the full Chromium, never the headless shell.
    assert.equal(started(['--x']), 'Chromium');
    assert.equal(started(['--headless=new']), 'Chromium');
    // The look ran once for the headless shell; later starts read the answer from the cache.
    assert.equal(readFileSync(join(dir, 'dry-runs.txt'), 'utf8'), 'x');
    assert.equal(started(['--headless']), 'chrome-headless-shell');
    assert.equal(readFileSync(join(dir, 'dry-runs.txt'), 'utf8'), 'x', 'no second dry run');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a browser that is not installed prints an ::error:: line at start, runs nothing, and never falls back to the checkout', { skip: !posix }, () => {
  const dir = scratch();
  try {
    const record = join(dir, 'record.txt');
    const spec = fakePlaywright(dir, join(dir, 'no', 'such', 'folder'), join(dir, 'no', 'chromium'));
    const wrapper = writeLauncher(join(dir, 'launcher'), spec);
    for (const args of [['--headless'], ['--x']]) {
      const r = run(wrapper, args, dir);
      assert.equal(r.status, 127);
      assert.match(r.stderr, /::error::browser launcher: the Playwright (headless-shell|chromium) is not installed .* Next: run npx playwright install/);
    }
    assert.equal(existsSync(record), false);
    // A wrapper whose folder cannot be entered stops too.
    const blocked = writeLauncher(join(dir, 'launcher'), { ...spec, cwd: join(dir, 'blocked') });
    rmSync(join(dir, 'blocked'), { recursive: true });
    writeFileSync(join(dir, 'blocked'), 'a file, not a folder');
    const b = run(blocked, ['--headless'], dir);
    assert.equal(b.status, 126);
    assert.match(b.stderr, /::error::browser launcher: cannot enter the internal folder\. Next: /);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the browser keeps its pid: the wrapper replaces itself with the browser (exec)', () => {
  assert.match(launcherScript({ cwd: '/c', pinned: '/r/b', cache: '/k', node: '', cli: '', core: '' }), /\nexec '\/r\/b' "\$@"\n$/);
  assert.match(launcherScript({ cwd: '/c', cache: '/k', node: '/n', cli: '/c.js', core: '/p.js' }), /\nexec "\$real" "\$@"\n$/);
});

test('workers writing the wrapper at once each get a complete, executable script (write then rename)', { skip: !posix }, async () => {
  const dir = scratch();
  try {
    const real = standIn(dir, 'b', join(dir, 'r.txt'));
    const spec: LauncherSpec = { cwd: join(dir, 'cwd'), pinned: real, cache: join(dir, 'c'), node: '', cli: '', core: '' };
    const here = new URL('./browser-launcher.ts', import.meta.url).href;
    const script = `import { writeLauncher } from '${here}'; console.log(writeLauncher(process.argv[1], JSON.parse(process.argv[2])));`;
    const runs = Array.from({ length: 12 }, () =>
      new Promise<string>((resolve, reject) => {
        const p = spawn(process.execPath, ['--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--input-type=module', '-e', script, join(dir, 'launcher'), JSON.stringify(spec)], { stdio: ['ignore', 'pipe', 'inherit'] });
        let out = '';
        p.stdout.on('data', (b) => (out += b));
        p.on('exit', (code) => (code === 0 ? resolve(out.trim()) : reject(new Error(`worker exited ${code}`))));
      }),
    );
    const paths = await Promise.all(runs);
    assert.equal(new Set(paths).size, 1, 'one name for one content');
    assert.equal(readFileSync(paths[0]!, 'utf8'), launcherScript(spec));
    assert.deepEqual(readdirSync(join(dir, 'launcher')).filter((f) => f.endsWith('.tmp')), [], 'no private file left behind');
    assert.equal(spawnSync(paths[0]!, []).status, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a wrapper that cannot be written stops with an ::error:: line', { skip: !posix }, () => {
  const dir = scratch();
  try {
    writeFileSync(join(dir, 'file'), 'x');
    assert.throws(
      () => writeLauncher(join(dir, 'file', 'under-a-file'), { cwd: join(dir, 'cwd'), pinned: '/r/b', cache: '/k', node: '', cli: '', core: '' }),
      (e: Error) => e instanceof LauncherError && /::error::browser launcher: cannot write .* Next: /.test(e.message),
    );
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
    assert.throws(() => browserLauncher(join(dir, 'root'), { CHROMIUM_TEST_PATH: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' }, 'darwin'), /installed browser/);
    assert.deepEqual(readdirSync(dir), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('loading a config looks for no browser: with none installed (WebKit only, or a wrong version) the wrapper is written and nothing throws', () => {
  const dir = scratch();
  try {
    const env = { PADDOCK_JOB_TEMP: dir, PLAYWRIGHT_BROWSERS_PATH: join(dir, 'empty-browsers') };
    const use = internalBrowserUse(env);
    if (process.platform === 'darwin') {
      assert.ok(use.launchOptions.executablePath?.includes(LAUNCHER_PREFIX));
      assert.equal(existsSync(join(dir, 'paddock-e2e-browser', 'browsers')), false, 'no look for a browser, no cache');
    } else {
      assert.equal(use.launchOptions.executablePath, undefined);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the guard accepts exactly the wrapper the launcher returned, not a file that only has its name', () => {
  const dir = scratch();
  try {
    const wrapper = writeLauncher(join(dir, 'launcher'), { cwd: join(dir, 'cwd'), pinned: '/r/b', cache: '/k', node: '', cli: '', core: '' });
    assert.deepEqual(browserProblems({ launchOptions: { executablePath: wrapper } }, {}), []);
    const lookalike = join(dir, 'elsewhere', `${LAUNCHER_PREFIX}0123456789abcdef.sh`);
    assert.match(browserProblems({ launchOptions: { executablePath: lookalike } }, {}).join('\n'), /is not the pinned browser/, 'positive control');
    assert.match(browserProblems({ launchOptions: { executablePath: wrapper.replace('.sh', 'x.sh') } }, {}).join('\n'), /is not the pinned browser/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
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
