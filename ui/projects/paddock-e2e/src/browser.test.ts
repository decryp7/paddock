import assert from 'node:assert/strict';
import { test } from 'node:test';
import { join, normalize, posix } from 'node:path';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { internalBrowserUse } from './browser-launcher.ts';
import { paddockConfig } from './config.ts';
import { assertChromeForTesting, browserProblems, browserScratchRoot, type BrowserUse, internalBrowserLaunch, isRemovableVolume, CHROME_ENV, CHROMIUM_TEST_ENV, isInstalledBrowser, pinnedChrome } from './browser.ts';

const PINNED = '/Users/me/.cache/chrome-for-testing/mac_arm-131/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing';

test('Playwright’s own Chromium passes', () => {
  assert.deepEqual(browserProblems(undefined, {}), []);
  assert.deepEqual(browserProblems({}, {}), []);
  assert.deepEqual(browserProblems({ channel: 'chromium' }, {}), []);
});

test('an installed Chrome or Edge is refused, by channel or by path, even when named in the variable', () => {
  assert.match(browserProblems({ channel: 'chrome' }, {})[0] ?? '', /channel "chrome" launches the installed browser/);
  assert.match(browserProblems({ launchOptions: { channel: 'msedge' } }, {})[0] ?? '', /channel "msedge"/);
  const installed = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    '/opt/google/chrome/chrome',
    '/usr/bin/google-chrome-stable',
  ];
  for (const path of installed) {
    assert.ok(isInstalledBrowser(path), path);
    assert.match(browserProblems({ launchOptions: { executablePath: path } }, { [CHROME_ENV]: path })[0] ?? '', /is the installed browser/);
  }
  assert.equal(isInstalledBrowser(PINNED), false);
});

test('another binary only when it is the pinned one named in the variable', () => {
  assert.deepEqual(browserProblems({ launchOptions: { executablePath: PINNED } }, { [CHROME_ENV]: PINNED }), []);
  assert.match(browserProblems({ launchOptions: { executablePath: PINNED } }, {})[0] ?? '', /not the pinned browser named in CHROMIUM_TEST_PATH \(or PADDOCK_E2E_CHROME\)/);
});

test('the config guard checks the config and every project (with the config’s use under it)', () => {
  const ok = { use: { viewport: null }, projects: [{ name: 'a' }, { name: 'b', use: { channel: 'chromium' } }] };
  assert.equal(assertChromeForTesting(ok, {}), ok);
  assert.throws(
    () => assertChromeForTesting({ use: { channel: 'chrome' }, projects: [{ name: 'a' }, { use: { launchOptions: { executablePath: '/opt/google/chrome/chrome' } } }] }, {}),
    (e: Error) =>
      /^The e2e would use an installed browser \(rule 7\)/.test(e.message) &&
      e.message.includes('config: channel "chrome"') &&
      e.message.includes('project a: channel "chrome"') &&
      e.message.includes('project (unnamed): executablePath /opt/google/chrome/chrome'),
  );
});

test('the pinned browser is named in CHROMIUM_TEST_PATH, or in PADDOCK_E2E_CHROME; the first wins', () => {
  assert.equal(pinnedChrome({}), undefined);
  assert.equal(pinnedChrome({ [CHROME_ENV]: 'b' }), 'b');
  assert.equal(pinnedChrome({ [CHROME_ENV]: 'b', [CHROMIUM_TEST_ENV]: 'a' }), 'a');
  assert.equal(pinnedChrome({ [CHROMIUM_TEST_ENV]: '' }), undefined);
  assert.deepEqual(browserProblems({ launchOptions: { executablePath: PINNED } }, { [CHROMIUM_TEST_ENV]: PINNED }), []);
});

test('every writable browser folder is under the internal temp root, and a removable volume is refused (ER-14)', () => {
  const root = posix.join('/var/folders/zz/T', 'paddock-e2e-browser');
  const launch = internalBrowserLaunch(root, {});
  assert.deepEqual(browserProblems({ launchOptions: launch }, {}), []);
  const under = (folder: string | undefined) => folder !== undefined && normalize(folder).replace(/\\/g, '/').startsWith(`${root}/`);
  for (const folder of [launch.downloadsPath, launch.env?.['MAC_CHROMIUM_TMPDIR'], ...(launch.args ?? []).map((a) => a.split('=')[1])]) {
    assert.ok(under(folder), String(folder));
  }
  // Positive control: pointing any one of them at the external volume goes red, and names which and the variable to set.
  const vol = '/Volumes/vault-dev/x';
  const bad: [string, NonNullable<BrowserUse['launchOptions']>][] = [
    ['downloadsPath', { ...launch, downloadsPath: vol }],
    ['env MAC_CHROMIUM_TMPDIR', { ...launch, env: { MAC_CHROMIUM_TMPDIR: vol } }],
    ['env TMPDIR', { env: { TMPDIR: vol } }],
    ['--crash-dumps-dir', { args: [`--crash-dumps-dir=${vol}`] }],
    ['--user-data-dir', { args: [`--user-data-dir=${vol}`] }],
  ];
  for (const [what, options] of bad) {
    const problems = browserProblems({ launchOptions: options }, {}).join('\n');
    assert.match(problems, new RegExp(`${what} ${vol} is on a removable volume`), what);
    assert.match(problems, /Set PADDOCK_JOB_TEMP/, what);
  }
  // The process's own TMPDIR counts too (Playwright's profile folder), also when launchOptions.env is set without one.
  assert.match(browserProblems({ launchOptions: launch }, { TMPDIR: vol }).join('\n'), /process env TMPDIR \/Volumes\/vault-dev\/x is on a removable volume/);
  assert.equal(isRemovableVolume('/Volumes/vault-dev/caches'), true);
  assert.equal(isRemovableVolume('/var/folders/zz/T/x'), false);
});

test('the browser keeps the caller’s environment: Playwright starts it with launchOptions.env instead of process.env (ER-47)', () => {
  const env = { PATH: '/usr/bin', HOME: '/home/r', LANG: 'C.UTF-8', SystemRoot: 'C:\\Windows', TEMP: 'C:\\Users\\r\\AppData\\Local\\Temp' };
  const launch = internalBrowserLaunch('/r', env);
  for (const [k, v] of Object.entries(env)) assert.equal(launch.env?.[k], v, k);
  assert.equal(launch.env?.['MAC_CHROMIUM_TMPDIR'], join('/r', 'tmp'));
  assert.deepEqual(internalBrowserUse(env, 'off').launchOptions.env?.['PATH'], '/usr/bin');
  // Positive control: the real process environment passes through the config too.
  const config = paddockConfig({ app: 'x', launcher: 'off', defaultPort: 4999 });
  const passed = config.projects?.find((p) => p.name === 'chromium')?.use?.launchOptions?.env;
  assert.equal(passed?.['PATH'], process.env['PATH']);
  if (process.platform === 'win32') assert.equal(passed?.['SystemRoot'], process.env['SystemRoot']);
});

test('the scratch root is the job’s temp folder, else the OS temp folder, and the config uses it', () => {
  assert.equal(browserScratchRoot({ PADDOCK_JOB_TEMP: '/home/r/tmp/job' }), join('/home/r/tmp/job', 'paddock-e2e-browser'));
  const config = paddockConfig({ app: 'x', launcher: 'off', defaultPort: 4999, env: { PADDOCK_JOB_TEMP: '/home/r/tmp/job' } });
  const chromium = config.projects?.find((p) => p.name === 'chromium')?.use?.launchOptions;
  assert.equal(chromium?.downloadsPath, join('/home/r/tmp/job', 'paddock-e2e-browser', 'downloads'));
  assert.throws(() => paddockConfig({ app: 'x', launcher: 'off', defaultPort: 4999, env: { PADDOCK_JOB_TEMP: '/Volumes/vault-dev/tmp' } }), /removable volume/);
});

test('every Playwright config spreads the internal folders and runs the guard: a removable PADDOCK_JOB_TEMP stops each (positive control: a good one loads)', () => {
  const root = fileURLToPath(new URL('../../../../', import.meta.url));
  const configs = [
    'apps/debrief/app/playwright.config.ts',
    'apps/debrief/app/scripts/screenshots/playwright.config.ts',
    'apps/debrief/app/scripts/visual-qa/playwright.config.ts',
    'apps/dyno/app/playwright.config.ts',
    'apps/pitcrew/app/playwright.config.ts',
    'apps/pitwall/app/playwright.config.ts',
    'apps/pitwall/app/e2e/screenshots/playwright.config.ts',
    'apps/spotter/app/playwright.config.ts',
    'ui/playwright.config.ts',
  ];
  const list = (rel: string, jobTemp: string) =>
    spawnSync('npx', ['playwright', 'test', '--list', '--config', join(root, rel)], {
      cwd: join(root, 'ui'),
      env: { ...process.env, PW_SKIP_BUILD_GUARD: '1', PW_PORT: '4999', PADDOCK_JOB_TEMP: jobTemp, CHROMIUM_TEST_PATH: process.execPath },
      encoding: 'utf8',
      shell: process.platform === 'win32',
      timeout: 120_000,
    });
  for (const rel of configs) {
    const good = list(rel, join(tmpdir(), 'pw-config-probe'));
    // The config loaded (spec files may still fail to compile in this checkout; that is not what is measured here).
    assert.match(good.stdout, /Listing tests/, `${rel} should load: ${good.stderr}`);
    assert.doesNotMatch(`${good.stderr}${good.stdout}`, /removable volume/, rel);
    const bad = list(rel, '/Volumes/vault-dev/tmp');
    assert.doesNotMatch(bad.stdout, /Listing tests/, `${rel} loaded with a browser folder on /Volumes/`);
    assert.match(`${bad.stderr}${bad.stdout}`, /removable volume/, rel);
  }
});
