import assert from 'node:assert/strict';
import { test } from 'node:test';
import { paddockConfig } from './config.ts';
import { paddockTrace } from './trace.ts';

test('WebKit on macOS keeps failure traces without screenshots', () => {
  assert.deepEqual(paddockTrace('webkit', 'darwin'), { mode: 'retain-on-failure', screenshots: false, snapshots: true });
});

test('every other engine and OS keeps the plain failure trace', () => {
  for (const [browser, platform] of [['webkit', 'linux'], ['webkit', 'win32'], ['chromium', 'darwin'], ['chromium', 'linux'], ['firefox', 'darwin']] as const) {
    assert.equal(paddockTrace(browser, platform), 'retain-on-failure', `${browser} on ${platform}`);
  }
});

test('the preset gives the webkit project the helper and chromium the plain trace', () => {
  const c = paddockConfig({ app: 'demo', defaultPort: 4390, env: { PW_PORT: '4391' } });
  const trace = (name: string) => c.projects?.find((p) => p.name === name)?.use?.trace;
  assert.deepEqual(trace('webkit'), paddockTrace('webkit'));
  assert.equal(trace('chromium'), undefined, 'chromium inherits use.trace');
  assert.equal(c.use?.trace, 'retain-on-failure');
});
