import { test } from 'node:test';
import assert from 'node:assert/strict';
import { entryFor, platformKey } from './install-test-browser.mjs';

const TEXT = `version = "1"

[[platform]]
key = "linux-x86_64"
url = "https://example.test/l.zip"
sha256 = "aa"

[[platform]]
key = "macos-aarch64"
url = "https://example.test/m.zip"
sha256 = "bb"
`;

test('picks the entry of a platform', () => {
  assert.deepEqual(entryFor(TEXT, 'macos-aarch64'), { url: 'https://example.test/m.zip', sha256: 'bb' });
  assert.throws(() => entryFor(TEXT, 'windows-x86_64'));
});

test('maps node platforms to manifest keys', () => {
  assert.equal(platformKey('darwin', 'arm64'), 'macos-aarch64');
  assert.equal(platformKey('win32', 'x64'), 'windows-x86_64');
  assert.throws(() => platformKey('freebsd', 'x64'));
});
