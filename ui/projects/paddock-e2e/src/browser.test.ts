import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertChromeForTesting, browserProblems, CHROME_ENV, isInstalledBrowser } from './browser.ts';

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
  assert.match(browserProblems({ launchOptions: { executablePath: PINNED } }, {})[0] ?? '', /not the pinned browser named in PADDOCK_E2E_CHROME/);
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
