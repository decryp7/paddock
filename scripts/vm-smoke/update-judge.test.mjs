import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CASES, failureDetail, feedRoute, plistVersion, staticTexts, updateState } from './update-judge.mjs';

test('the window state comes from the messages of the Updates dialog', () => {
  assert.equal(updateState(['Checking for updates…'], '1.0.0-test.2').state, 'checking');
  assert.equal(updateState(['Version 1.0.0-test.2 is available (31.2 MB).'], '1.0.0-test.2').state, 'available');
  assert.equal(updateState(['Downloading version 1.0.0-test.2…'], '1.0.0-test.2').state, 'downloading');
  assert.equal(updateState(['Version 1.0.0-test.2 is downloaded and its signature verified. Restart to update.'], '1.0.0-test.2').state, 'ready');
  assert.equal(updateState(['Installing version 1.0.0-test.2…'], '1.0.0-test.2').state, 'installing');
  assert.equal(updateState(['You have the latest version.'], '1.0.0-test.2').state, 'upToDate');
  assert.equal(updateState(['Updates are not available in this build.'], '1.0.0-test.2').state, 'unavailable');
  assert.equal(updateState(['something else'], '1.0.0-test.2').state, 'unknown');
});

test('a message about another version is not this version\'s state (the dots are not wildcards)', () => {
  assert.equal(updateState(['Version 1.0.0-test.3 is available.'], '1.0.0-test.2').state, 'unknown');
  assert.equal(updateState(['Version 1x0y0-test.2 is available.'], '1.0.0-test.2').state, 'unknown');
});

test('a failure wins over the older lines in the window and carries the reason', () => {
  const texts = ['Downloading version 1.0.0-test.2…', 'The last step did not finish.', 'The download was deleted. Do not install it by hand.', 'Try again'];
  const s = updateState(texts, '1.0.0-test.2');
  assert.equal(s.state, 'failed');
  assert.match(s.detail, /The download was deleted/);
  assert.equal(failureDetail(['nothing']), '');
});

test('static texts are read from an axdrive tree dump', () => {
  const dump = ['AXWindow  Updates', '  AXStaticText  Checking for updates… (frame 1)', '  AXButton  Stop checking', '  AXStaticText  Not checked yet.'].join('\n');
  assert.deepEqual(staticTexts(dump), ['Checking for updates…', 'Not checked yet.']);
});

test('the feed serves the current case under the one URL the old build knows, and only that case\'s files', () => {
  assert.deepEqual(feedRoute('/ok/latest.json', 'badsig'), { file: 'badsig/latest.json', type: 'application/json' });
  assert.deepEqual(feedRoute('/badsig/Spotter_aarch64.app.tar.gz', 'badsig'), { file: 'badsig/Spotter_aarch64.app.tar.gz', type: 'application/gzip' });
  assert.equal(feedRoute('/ok/Spotter_aarch64.app.tar.gz', 'badsig'), null, 'another case\'s file is not served');
  assert.equal(feedRoute('/ok/../keys/spotter-test.key', 'ok'), null);
  assert.equal(feedRoute('/ok/%2e%2e/keys/x', 'ok'), null);
  assert.equal(feedRoute('/etc/passwd', 'ok'), null);
});

test('only an app version is read from the plist', () => {
  assert.equal(plistVersion('1.0.0-test.1\n'), '1.0.0-test.1');
  assert.equal(plistVersion('The domain/default pair does not exist'), '');
});

test('every refused case ends on the old version and the cases are unique', () => {
  assert.equal(new Set(CASES.map((c) => c.id)).size, CASES.length);
  for (const c of CASES.filter((x) => x.failed)) assert.equal(c.finalVersion, 'old');
});
