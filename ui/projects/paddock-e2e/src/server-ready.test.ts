import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
import { startServers, ServerNotReady } from './server-ready.ts';

const node = (code: string) => `${JSON.stringify(process.execPath)} -e ${JSON.stringify(code)}`;
const freePort = async (): Promise<number> =>
  new Promise((resolve) => {
    const s = createServer().listen(0, () => {
      const { port } = s.address() as { port: number };
      s.close(() => resolve(port));
    });
  });
const quiet = { log: () => undefined };

test('positive control: a server command that exits at once fails fast with one ::error:: line (URL, probe, exit status, stderr tail, next step)', async () => {
  const port = await freePort();
  const started = Date.now();
  await assert.rejects(
    startServers([{ command: node(`console.error('EADDRINUSE: address already in use'); process.exit(3)`), url: `http://localhost:${port}`, timeoutMs: 30_000, reuseExisting: false }], quiet),
    (e: unknown) => {
      assert.ok(e instanceof ServerNotReady);
      const m = e.message;
      assert.match(m, /^::error::/);
      assert.equal(m.split('\n').length, 1, 'one line');
      assert.ok(m.includes(`http://localhost:${port}`), m);
      assert.match(m, /exit status 3/);
      assert.match(m, /EADDRINUSE/);
      assert.match(m, /GET .* connection refused|ECONNREFUSED|no answer/i);
      assert.match(m, /port is already in use|next step/i);
      return true;
    },
  );
  assert.ok(Date.now() - started < 10_000, 'failed fast, not at the 30 s limit');
});

test('positive control: a server that never answers fails at the configured timeout, naming it and the probe, and is stopped', async () => {
  const port = await freePort();
  const started = Date.now();
  await assert.rejects(
    startServers([{ command: node(`setInterval(() => {}, 1000)`), url: `http://localhost:${port}`, timeoutMs: 1500, reuseExisting: false }], quiet),
    /::error::.*did not answer within 1\.5 s.*http:\/\/localhost:\d+/,
  );
  assert.ok(Date.now() - started < 10_000);
});

test('a server that answers is ready, its output is logged, and stop ends it', async () => {
  const port = await freePort();
  const lines: string[] = [];
  const code = `require('http').createServer((q,r)=>r.end('ok')).listen(${port}, () => console.log('listening'))`;
  const running = await startServers([{ command: node(code), url: `http://localhost:${port}`, timeoutMs: 20_000, reuseExisting: false }], { log: (l) => lines.push(l) });
  assert.ok((await fetch(`http://localhost:${port}`)).ok);
  assert.ok(lines.some((l) => l.includes('listening')), lines.join('|'));
  await running.stop();
  await assert.rejects(fetch(`http://localhost:${port}`));
});

test('a server already on the port is refused unless reuse is on', async () => {
  const s = createServer((q, r) => r.end('stale')).listen(0);
  await new Promise((r) => s.once('listening', r));
  const { port } = s.address() as { port: number };
  try {
    await assert.rejects(startServers([{ command: node(`setInterval(() => {}, 1000)`), url: `http://localhost:${port}`, timeoutMs: 5000, reuseExisting: false }], quiet), /::error::.*already (answers|in use)/);
    const reused = await startServers([{ command: node(`process.exit(9)`), url: `http://localhost:${port}`, timeoutMs: 5000, reuseExisting: true }], quiet);
    await reused.stop();
  } finally {
    s.close();
  }
});
