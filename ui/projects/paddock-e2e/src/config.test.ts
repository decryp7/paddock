import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CHROME_ENV, CHROMIUM_TEST_ENV } from './browser.ts';
import { paddockConfig, SCREENSHOT_SPECS } from './config.ts';

const base = { app: 'demo', defaultPort: 4390 };
const names = (c: { projects?: readonly { name?: string }[] }) => (c.projects ?? []).map((p) => p.name);

test('the preset: chromium and webkit, the port from the variable, no retries, traces on failure, one worker', () => {
  const c = paddockConfig({ ...base, serve: (port) => `serve --port ${port}`, env: { PW_PORT: '4391' } });
  assert.deepEqual(names(c), ['chromium', 'webkit']);
  assert.equal(c.use?.baseURL, 'http://localhost:4391');
  assert.equal(c.retries, 0);
  assert.equal(c.forbidOnly, false);
  assert.equal(c.workers, 1);
  assert.equal(c.use?.trace, 'retain-on-failure');
  assert.deepEqual(c.webServer, { command: 'serve --port 4391', url: 'http://localhost:4391', reuseExistingServer: true, timeout: 180_000 });
  assert.deepEqual(c.reporter, [['list']]);
});

test('the port: a local default, a named variable, an error for a bad value and on CI without one', () => {
  assert.equal(paddockConfig({ ...base, env: {} }).use?.baseURL, 'http://localhost:4390');
  assert.equal(paddockConfig({ ...base, portEnv: 'DEMO_E2E_PORT', env: { DEMO_E2E_PORT: '4500', PW_PORT: '1' } }).use?.baseURL, 'http://localhost:4500');
  assert.throws(() => paddockConfig({ ...base, env: { PW_PORT: 'nope' } }), /PW_PORT is not a port/);
  assert.throws(() => paddockConfig({ ...base, env: { CI: 'true' } }), /CI: set PW_PORT/);
  assert.equal(paddockConfig({ ...base, env: { CI: 'true', PW_PORT: '4400' } }).use?.baseURL, 'http://localhost:4400');
});

test('on CI: test.only is refused and a server already on the port is not reused', () => {
  const c = paddockConfig({ ...base, serve: () => 'serve', env: { CI: 'true', PW_PORT: '4400' } });
  assert.equal(c.forbidOnly, true);
  assert.equal((c.webServer as { reuseExistingServer: boolean }).reuseExistingServer, false);
});

test('no web server unless the app names a command, and webkit can be left out', () => {
  const c = paddockConfig({ ...base, webkit: false, env: {} });
  assert.equal(c.webServer, undefined);
  assert.deepEqual(names(c), ['chromium']);
});

test('the screenshots project exists only when SCREENSHOTS is on, at 1440 x 900 @2x; the others ignore those specs', () => {
  const off = paddockConfig({ ...base, env: { SCREENSHOTS: '0' } });
  assert.deepEqual(names(off), ['chromium', 'webkit']);
  assert.ok(off.projects?.every((p) => p.testIgnore === SCREENSHOT_SPECS));
  const on = paddockConfig({ ...base, env: { SCREENSHOTS: '1' } });
  assert.deepEqual(names(on), ['chromium', 'webkit', 'screenshots']);
  const shots = on.projects?.find((p) => p.name === 'screenshots');
  assert.deepEqual(shots?.use?.viewport, { width: 1440, height: 900 });
  assert.equal(shots?.use?.deviceScaleFactor, 2);
  assert.equal(shots?.testMatch, SCREENSHOT_SPECS);
  for (const f of ['e2e/screenshots.spec.ts', 'e2e/screenshots/capture.shot.ts']) assert.ok(SCREENSHOT_SPECS.test(f), f);
  assert.ok(!SCREENSHOT_SPECS.test('e2e/flows.spec.ts'));
});

test('Chrome for Testing only through CHROMIUM_TEST_PATH (or the older variable); an installed Chrome is refused', () => {
  const pinned = '/opt/cft/chrome-linux64/chrome';
  const c = paddockConfig({ ...base, env: { [CHROMIUM_TEST_ENV]: pinned } });
  assert.equal(c.projects?.find((p) => p.name === 'chromium')?.use?.launchOptions?.executablePath, pinned);
  assert.equal(c.projects?.find((p) => p.name === 'webkit')?.use?.launchOptions?.executablePath, undefined);
  assert.equal(paddockConfig({ ...base, env: { [CHROME_ENV]: pinned } }).projects?.[0]?.use?.launchOptions?.executablePath, pinned);
  assert.equal(paddockConfig({ ...base, env: {} }).projects?.[0]?.use?.launchOptions?.executablePath, undefined);
  assert.throws(() => paddockConfig({ ...base, env: { [CHROMIUM_TEST_ENV]: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' } }), /installed browser/);
  assert.throws(() => paddockConfig({ ...base, extend: { use: { channel: 'chrome' } }, env: {} }), /channel "chrome"/);
});

test('sharding: PADDOCK_E2E_BLOB writes a named blob report per shard', () => {
  const c = paddockConfig({ ...base, env: { PADDOCK_E2E_BLOB: '1', PADDOCK_E2E_SHARD: '2-of-4' } });
  assert.deepEqual(c.reporter, [['blob', { outputDir: 'blob-report', fileName: 'demo-2-of-4.zip' }], ['list']]);
  assert.deepEqual(paddockConfig({ ...base, env: { PADDOCK_E2E_BLOB: '1' } }).reporter?.[0], ['blob', { outputDir: 'blob-report', fileName: 'demo-all.zip' }]);
});

test('extra reporters, window size, timeout and extensions are applied', () => {
  const c = paddockConfig({
    ...base,
    reporters: [['@decryp7/paddock-e2e/retried-reporter']],
    viewport: { width: 1000, height: 700 },
    timeoutMs: 5000,
    extend: { expect: { timeout: 7000 } },
    env: {},
  });
  assert.deepEqual(c.reporter, [['list'], ['@decryp7/paddock-e2e/retried-reporter']]);
  assert.deepEqual(c.use?.viewport, { width: 1000, height: 700 });
  assert.equal(c.timeout, 5000);
  assert.equal(c.expect?.timeout, 7000);
});
