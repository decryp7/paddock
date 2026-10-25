import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PLATFORMS, pickDownloads, renderManifest } from './pin-chromium.mjs';

const json = {
  channels: {
    Stable: {
      version: '1.2.3.4',
      downloads: {
        chrome: [
          { platform: 'win64', url: 'https://x/win64.zip' },
          { platform: 'win32', url: 'https://x/win32.zip' },
          { platform: 'mac-arm64', url: 'https://x/mac.zip' },
        ],
      },
    },
  },
};

test('only platforms Spotter supports are picked', () => {
  const { version, urls } = pickDownloads(json, 'Stable');
  assert.equal(version, '1.2.3.4');
  assert.deepEqual(Object.keys(urls).sort(), ['macos-aarch64', 'windows-x86_64']);
  assert.throws(() => pickDownloads(json, 'Nope'), /not found/);
});

test('the manifest lists the version and one entry per platform', () => {
  const text = renderManifest('1.2.3.4', { 'linux-x86_64': { url: 'https://x/l.zip', sha256: 'ab'.repeat(32) } });
  assert.match(text, /^version = "1\.2\.3\.4"$/m);
  assert.match(text, /key = "linux-x86_64"\nurl = "https:\/\/x\/l\.zip"\nsha256 = "(ab){32}"/);
  assert.equal(Object.keys(PLATFORMS).length, 5);
});
