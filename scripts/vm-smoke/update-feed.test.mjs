import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { startFeed } from './update-feed.mjs';

// A throwaway CA and a server certificate for 127.0.0.1, made for this test only (never the test assets' keys).
let assets;
before(() => {
  assets = mkdtempSync(join(tmpdir(), 'update-feed-test-'));
  const ca = join(assets, 'ca');
  mkdirSync(ca);
  mkdirSync(join(assets, 'feed', 'ok'), { recursive: true });
  writeFileSync(join(assets, 'feed', 'ok', 'latest.json'), '{"version":"0.0.0-test.0"}');
  writeFileSync(join(ca, 'san.ext'), 'subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\n');
  const ssl = (...args) => {
    const r = spawnSync('openssl', args, { cwd: ca, encoding: 'utf8' });
    assert.equal(r.status, 0, `openssl ${args[0]} failed: ${r.stderr}`);
  };
  ssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=update feed test CA', '-keyout', 'ca.key', '-out', 'ca.pem');
  ssl('req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=127.0.0.1', '-keyout', 'server.key', '-out', 'server.csr');
  ssl('x509', '-req', '-in', 'server.csr', '-CA', 'ca.pem', '-CAkey', 'ca.key', '-CAcreateserial', '-days', '1', '-extfile', 'san.ext', '-out', 'server.pem');
});
after(() => rmSync(assets, { recursive: true, force: true }));

const freePort = () => new Promise((res, rej) => {
  const s = createServer();
  s.once('error', rej);
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});

// What the guest does while the runner waits in a synchronous call (`vm-win.sh job` through spawnSync): fetch the manifest over
// HTTPS. Prints the status; the hang deadline only ends a fetch that never gets an answer.
const FETCH = `
const https = require('node:https');
const [url, ca] = process.argv.slice(1);
https.get(url, { ca: require('node:fs').readFileSync(ca) }, (r) => { console.log(r.statusCode); r.resume(); r.on('end', () => process.exit(0)); })
  .on('error', (e) => { console.log('error ' + e.message); process.exit(1); });
`;

test('the feed answers while the runner is blocked in a synchronous call (Windows: vm-win.sh job runs through spawnSync)', async () => {
  const port = await freePort();
  const feed = await startFeed({ assets, host: '127.0.0.1', port });
  try {
    await feed.setCase('ok');
    await feed.clearFeedLog();
    const r = spawnSync(process.execPath, ['-e', FETCH, `https://127.0.0.1:${port}/ok/latest.json`, join(assets, 'ca', 'ca.pem')], {
      encoding: 'utf8',
      timeout: 20_000,
    });
    assert.equal(r.signal, null, 'the fetch got no answer: the feed cannot serve while the runner blocks (the TLS handshake waits)');
    assert.equal(r.stdout.trim(), '200');
    assert.deepEqual(await feed.feedLog(), ['GET /ok/latest.json 200']);
  } finally {
    await feed.close();
  }
});

test('the case down resets the connection, and the log is cleared on request', async () => {
  const port = await freePort();
  const feed = await startFeed({ assets, host: '127.0.0.1', port });
  try {
    await feed.setCase('down');
    const r = spawnSync(process.execPath, ['-e', FETCH, `https://127.0.0.1:${port}/ok/latest.json`, join(assets, 'ca', 'ca.pem')], {
      encoding: 'utf8',
      timeout: 20_000,
    });
    assert.equal(r.signal, null, 'the fetch got no answer');
    assert.match(r.stdout, /^error /);
    assert.deepEqual(await feed.feedLog(), []);
  } finally {
    await feed.close();
  }
});

test('a port in use is an error, not a feed that never answers', async () => {
  const port = await freePort();
  const first = await startFeed({ assets, host: '127.0.0.1', port });
  try {
    await assert.rejects(startFeed({ assets, host: '127.0.0.1', port }), /EADDRINUSE/);
  } finally {
    await first.close();
  }
});
