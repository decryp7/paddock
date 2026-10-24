// An app that uses Paddock's shared actions (plan X10) satisfies the checks that used to look for the copied scripts.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const macConf = JSON.stringify({ bundle: { macOS: { signingIdentity: '-', hardenedRuntime: false } } });
const wf = (step) => `jobs:\n  b:\n    steps:\n      - ${step}\n`;
const W = '.gitea/workflows/build.yml';

test('C04: the macos-verify and tauri-bundle actions and the app-release workflow count as verifying the .app and the DMG', async () => {
  for (const step of ['uses: decryp7/paddock/.gitea/actions/macos-verify@v0.3.0', 'uses: decryp7/paddock/.gitea/actions/tauri-bundle@v0.3.0']) {
    assert.deepEqual(await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf, [W]: wf(step) })), [], step);
  }
  const release = 'jobs:\n  release:\n    uses: decryp7/paddock/.gitea/workflows/app-release.yml@v0.3.0\n';
  assert.deepEqual(await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf, [W]: release })), []);
  assert.match(messages(await findingsOf('C04', makeApp({ 'app/src-tauri/tauri.macos.conf.json': macConf, [W]: wf('uses: actions/checkout@v4') }))), /no CI step runs/);
});

test('C03: the conformance action counts as the icon check', async () => {
  const icons = await findingsOf('C03', makeApp({ [W]: wf('uses: decryp7/paddock/.gitea/actions/conformance@v0.3.0') }));
  assert.doesNotMatch(messages(icons), /no CI step checks the icons/);
});

test('C15: the install-test-browser action sets the require variable itself', async () => {
  const files = { 'app/playwright.config.ts': 'export default {};\n' };
  assert.match(messages(await findingsOf('C15', makeApp({ ...files, [W]: wf('uses: actions/checkout@v4') }))), /REQUIRE_TEST_BROWSER/);
  assert.doesNotMatch(messages(await findingsOf('C15', makeApp({ ...files, [W]: wf('uses: decryp7/paddock/.gitea/actions/install-test-browser@v0.3.0') }))), /REQUIRE_TEST_BROWSER/);
});
