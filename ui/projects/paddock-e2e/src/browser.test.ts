import assert from 'node:assert/strict';
import { test } from 'node:test';
import { tmpdir } from 'node:os';
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
  const root = '/var/folders/zz/T/paddock-e2e-browser';
  const launch = internalBrowserLaunch(root);
  assert.deepEqual(browserProblems({ launchOptions: launch }, {}), []);
  for (const folder of [launch.downloadsPath, launch.tracesDir, launch.env?.['MAC_CHROMIUM_TMPDIR'], ...(launch.args ?? []).map((a) => a.split('=')[1])]) {
    assert.ok(folder?.startsWith(`${root}/`), String(folder));
  }
  // Positive control: pointing any one of them at the external volume goes red, and names which.
  const vol = '/Volumes/vault-dev/x';
  const bad: [string, NonNullable<BrowserUse['launchOptions']>][] = [
    ['downloadsPath', { ...launch, downloadsPath: vol }],
    ['tracesDir', { ...launch, tracesDir: vol }],
    ['env MAC_CHROMIUM_TMPDIR', { ...launch, env: { MAC_CHROMIUM_TMPDIR: vol } }],
    ['env TMPDIR', { env: { TMPDIR: vol } }],
    ['--crash-dumps-dir', { args: [`--crash-dumps-dir=${vol}`] }],
    ['--user-data-dir', { args: [`--user-data-dir=${vol}`] }],
  ];
  for (const [what, options] of bad) {
    assert.match(browserProblems({ launchOptions: options }, {}).join('\n'), new RegExp(`${what} ${vol} is on a removable volume`), what);
  }
  assert.equal(isRemovableVolume('/Volumes/vault-dev/caches'), true);
  assert.equal(isRemovableVolume('/var/folders/zz/T/x'), false);
});

test('the scratch root is the job’s temp folder, else the OS temp folder, and the config uses it', () => {
  assert.equal(browserScratchRoot({ PADDOCK_JOB_TEMP: '/home/r/tmp/job' }), '/home/r/tmp/job/paddock-e2e-browser');
  assert.ok(!isRemovableVolume(browserScratchRoot({})) || tmpdir().startsWith('/Volumes/'));
  const config = paddockConfig({ app: 'x', defaultPort: 4999, env: { PADDOCK_JOB_TEMP: '/home/r/tmp/job' } });
  const chromium = config.projects?.find((p) => p.name === 'chromium')?.use?.launchOptions;
  assert.equal(chromium?.downloadsPath, '/home/r/tmp/job/paddock-e2e-browser/downloads');
  assert.throws(() => paddockConfig({ app: 'x', defaultPort: 4999, env: { PADDOCK_JOB_TEMP: '/Volumes/vault-dev/tmp' } }), /removable volume/);
});
