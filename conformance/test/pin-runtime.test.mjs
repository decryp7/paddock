// The shared pin script and the catalogue reader, against a local HTTP server (no network).
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { cftDownloads, main, measure, moveUrl, parseArgs, pin } from '../ci/pin-runtime.mjs';
import { installFromCatalogue, parseArgs as installArgs } from '../ci/install-test-browser.mjs';
import { entryFor, platformKey, readCatalogue, writeCatalogue } from '../ci/runtime-catalogue.mjs';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const files = new Map();
let server;
let base;
const scratch = mkdtempSync(join(tmpdir(), 'pin-runtime-test-'));

before(async () => {
  server = createServer((req, res) => {
    const body = files.get(req.url);
    if (body === undefined) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': typeof body === 'string' ? 'application/json' : 'application/zip' });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}`;
});
after(() => {
  server.close();
  rmSync(scratch, { recursive: true, force: true });
});

const catalogue = (version, urls) => `# A comment that stays.
format = 1
name = "chromium"
version = "${version}"
source = "Chrome for Testing"

[[platform]]
key = "linux-x86_64"
url = "${urls.linux}"
size = 1
sha256 = "${'0'.repeat(64)}"

[[platform]]
key = "macos-aarch64"
url = "${urls.mac}"
`;

describe('the catalogue reader', () => {
  test('reads the pinned values and finds an entry; keys of this machine', () => {
    const c = readCatalogue(catalogue('1.0', { linux: 'https://h/1.0/l.zip', mac: 'https://h/1.0/m.zip' }));
    assert.equal(c.version, '1.0');
    assert.equal(c.name, 'chromium');
    assert.deepEqual(entryFor(c, 'linux-x86_64'), { key: 'linux-x86_64', url: 'https://h/1.0/l.zip', size: 1, sha256: '0'.repeat(64) });
    assert.throws(() => entryFor(c, 'windows-x86_64'), /no build for windows-x86_64/);
    assert.throws(() => readCatalogue('format = 1\n'), /no version/);
    assert.throws(() => readCatalogue('version = "1"\n[[platform]]\nkey = "x"\n'), /needs a key and a url/);
    assert.equal(platformKey('darwin', 'arm64'), 'macos-aarch64');
    assert.equal(platformKey('win32', 'x64'), 'windows-x86_64');
    assert.throws(() => platformKey('sunos', 'sparc'), /no catalogue key/);
  });

  test('rewrites version, url, size and sha256 in place and adds the missing lines', () => {
    const text = catalogue('1.0', { linux: 'https://h/1.0/l.zip', mac: 'https://h/1.0/m.zip' });
    const next = writeCatalogue(text, '2.0', {
      'linux-x86_64': { url: 'https://h/2.0/l.zip', size: 10, sha256: 'a'.repeat(64) },
      'macos-aarch64': { url: 'https://h/2.0/m.zip', size: 20, sha256: 'b'.repeat(64) },
    });
    assert.ok(next.startsWith('# A comment that stays.\n'));
    const c = readCatalogue(next);
    assert.equal(c.version, '2.0');
    assert.deepEqual(entryFor(c, 'macos-aarch64'), { key: 'macos-aarch64', url: 'https://h/2.0/m.zip', size: 20, sha256: 'b'.repeat(64) });
    assert.equal(entryFor(c, 'linux-x86_64').size, 10);
    assert.equal(next.match(/^version = /gm).length, 1, 'only the top-level version');
    assert.throws(() => writeCatalogue(text, '2', {}), /no new pin for linux-x86_64/);
    assert.throws(() => writeCatalogue('format = 1\n', '2', {}), /no version/);
  });

  test("Paddock's own chromium.toml reads", () => {
    const c = readCatalogue(readFileSync(new URL('../../crates/paddock-chromium/src/chromium.toml', import.meta.url), 'utf8'));
    assert.match(c.version, /^\d+\.\d+\.\d+\.\d+$/);
    for (const e of c.entries) assert.match(e.sha256, /^[0-9a-f]{64}$/);
  });
});

describe('pin-runtime', () => {
  test('re-pins the URLs as they are: sizes and checksums measured while reading', async () => {
    const l = Buffer.from('linux build');
    const m = Buffer.from('mac build, a bit longer');
    files.set('/1.0/l.zip', l);
    files.set('/1.0/m.zip', m);
    const next = await pin(catalogue('1.0', { linux: `${base}/1.0/l.zip`, mac: `${base}/1.0/m.zip` }));
    const c = readCatalogue(next);
    assert.deepEqual(entryFor(c, 'linux-x86_64'), { key: 'linux-x86_64', url: `${base}/1.0/l.zip`, size: l.length, sha256: sha(l) });
    assert.deepEqual(entryFor(c, 'macos-aarch64'), { key: 'macos-aarch64', url: `${base}/1.0/m.zip`, size: m.length, sha256: sha(m) });
  });

  test('--version moves every URL to the new version', async () => {
    files.set('/2.0/l.zip', Buffer.from('l2'));
    files.set('/2.0/m.zip', Buffer.from('m2'));
    const file = join(scratch, 'moved.toml');
    writeFileSync(file, catalogue('1.0', { linux: `${base}/1.0/l.zip`, mac: `${base}/1.0/m.zip` }));
    const logged = [];
    await main([file, '--version', '2.0'], { log: (m) => logged.push(m) });
    const c = readCatalogue(readFileSync(file, 'utf8'));
    assert.equal(c.version, '2.0');
    assert.equal(entryFor(c, 'macos-aarch64').sha256, sha(Buffer.from('m2')));
    assert.ok(logged.some((m) => m.includes('review the diff')));
    assert.equal(moveUrl('https://h/1.0/x/1.0.zip', '1.0', '2.0'), 'https://h/2.0/x/1.0.zip');
    assert.throws(() => moveUrl('https://h/x.zip', '1.0', '2.0'), /does not name version 1\.0 once/);
  });

  test('--chrome-for-testing takes the channel version and URLs', async () => {
    files.set('/cft/9.9.9.9/linux64/chrome-linux64.zip', Buffer.from('cft linux'));
    files.set('/cft/9.9.9.9/mac-arm64/chrome-mac-arm64.zip', Buffer.from('cft mac'));
    const json = {
      channels: {
        Stable: {
          version: '9.9.9.9',
          downloads: {
            chrome: [
              { platform: 'linux64', url: `${base}/cft/9.9.9.9/linux64/chrome-linux64.zip` },
              { platform: 'mac-arm64', url: `${base}/cft/9.9.9.9/mac-arm64/chrome-mac-arm64.zip` },
            ],
          },
        },
      },
    };
    files.set('/cft.json', JSON.stringify(json));
    const next = await pin(catalogue('1.0', { linux: 'https://h/1.0/l.zip', mac: 'https://h/1.0/m.zip' }), { channel: 'Stable', endpoint: `${base}/cft.json` });
    const c = readCatalogue(next);
    assert.equal(c.version, '9.9.9.9');
    assert.equal(entryFor(c, 'linux-x86_64').sha256, sha(Buffer.from('cft linux')));
    assert.throws(() => cftDownloads(json, 'Beta', []), /channel Beta not found/);
    assert.throws(() => cftDownloads(json, 'Stable', ['windows-x86_64']), /no build for windows-x86_64/);
    await assert.rejects(pin(catalogue('1.0', { linux: 'https://h/1.0/l.zip', mac: 'https://h/1.0/m.zip' }), { channel: 'Stable', endpoint: `${base}/missing.json` }), /HTTP 404/);
  });

  test('refuses what cannot be pinned', async () => {
    await assert.rejects(measure(`${base}/missing.zip`), /HTTP 404/);
    await assert.rejects(measure('http://example.test/a.zip'), /not an https address/);
    files.set('/empty.zip', Buffer.alloc(0));
    await assert.rejects(measure(`${base}/empty.zip`), /is empty/);
  });

  test('the command line', () => {
    assert.deepEqual(parseArgs(['c.toml']), { file: 'c.toml' });
    assert.deepEqual(parseArgs(['c.toml', '--chrome-for-testing']), { file: 'c.toml', channel: 'Stable' });
    assert.deepEqual(parseArgs(['c.toml', '--chrome-for-testing', 'Beta', '--endpoint', 'https://e']), { file: 'c.toml', channel: 'Beta', endpoint: 'https://e' });
    assert.throws(() => parseArgs([]), /usage/);
    assert.throws(() => parseArgs(['a', 'b']), /one catalogue/);
    assert.throws(() => parseArgs(['a', '--x']), /unknown option/);
    assert.throws(() => parseArgs(['a', '--version']), /needs a value/);
    assert.throws(() => parseArgs(['a', '--version', '1', '--chrome-for-testing']), /not both/);
  });
});

describe('install-test-browser from a catalogue', () => {
  test('installs the build the catalogue pins for a platform, checksum first', async () => {
    const zip = Buffer.from('fake zip');
    files.set('/1.0/l.zip', zip);
    const text = catalogue('1.0', { linux: `${base}/1.0/l.zip`, mac: `${base}/1.0/m.zip` }).replace('0'.repeat(64), sha(zip));
    const dir = join(scratch, 'browser');
    const extract = (_, to) => {
      mkdirSync(join(to, 'chrome-linux64'), { recursive: true });
      writeFileSync(join(to, 'chrome-linux64', 'chrome'), '#!/bin/sh\n');
      chmodSync(join(to, 'chrome-linux64', 'chrome'), 0o755);
    };
    const fetchBytes = async (url) => Buffer.from(await (await fetch(url)).arrayBuffer());
    const exe = await installFromCatalogue({ text, dir, key: 'linux-x86_64', fetchBytes, extract });
    assert.equal(exe, join(dir, 'chrome-linux64', 'chrome'));
    await assert.rejects(installFromCatalogue({ text, dir: join(scratch, 'b2'), key: 'macos-aarch64', fetchBytes, extract }), /not a SHA-256/);
    await assert.rejects(installFromCatalogue({ text: text.replace('linux-x86_64', 'windows-aarch64'), dir, key: 'windows-aarch64', fetchBytes, extract }), /no build for windows-aarch64/);
    assert.deepEqual(installArgs(['--catalogue', 'c.toml']), { catalogue: 'c.toml' });
    assert.throws(() => installArgs(['--catalogue', 'a', '--pin', 'b']), /exactly one of/);
  });
});
