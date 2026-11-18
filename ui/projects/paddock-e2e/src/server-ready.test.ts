import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { startServers, ServerNotReady } from './server-ready.ts';

const here = import.meta.dirname;
const node = (code: string) => `${JSON.stringify(process.execPath)} -e ${JSON.stringify(code)}`;
const freePort = async (): Promise<number> =>
  new Promise((resolve) => {
    const s = createServer().listen(0, () => {
      const { port } = s.address() as { port: number };
      s.close(() => resolve(port));
    });
  });
const quiet = { log: () => undefined };
/** Polls `condition` every 25 ms until it holds; fails naming `what` after `ms`. */
async function until(condition: () => boolean, what: string, ms = 10_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (!condition()) {
    assert.ok(Date.now() < deadline, `timed out waiting for ${what}`);
    // conformance: hang-guard — a 10 s bound (the deadline above) on a condition that other processes make true
    await new Promise((r) => setTimeout(r, 25));
  }
}

/** A process's identity: pid plus start time (rule 53), so a reused pid is not mistaken for the original. */
interface Proc {
  readonly pid: number;
  readonly started: string;
}
const startOf = (pid: number): string => spawnSync('ps', ['-o', 'lstart=', '-p', String(pid)], { encoding: 'utf8' }).stdout.trim();
const identify = (pid: number): Proc => {
  const started = startOf(pid);
  assert.ok(started, `process ${pid} is running`);
  return { pid, started };
};
const alive = (p: Proc) => startOf(p.pid) === p.started;
async function allGone(procs: readonly Proc[], what: string): Promise<void> {
  await until(() => !procs.some(alive), `${what}: the processes to end`, 5000).catch(() => undefined);
  assert.deepEqual(procs.filter(alive), [], `${what}: processes still running`);
}

/** A server that never answers: a node process with a grandchild; both pids go to `file`. */
const treeServer = (file: string) =>
  node(
    `const g=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});` +
      `require('fs').writeFileSync(${JSON.stringify(file)},process.pid+' '+g.pid);setInterval(()=>{},1000)`,
  );
async function treeOf(file: string): Promise<Proc[]> {
  await until(() => existsSync(file), `${file}`);
  const [a, b] = readFileSync(file, 'utf8').split(' ').map(Number) as [number, number];
  return [identify(a), identify(b)];
}
const scratch = () => mkdtempSync(join(tmpdir(), 'paddock-server-ready-'));

test('positive control: a command that exits at once fails fast with one ::error:: line (URL, probe, exit status, stderr tail, next step)', async () => {
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
      assert.match(m, /stderr tail: EADDRINUSE: address already in use/);
      assert.match(m, /last probe GET http:\/\/localhost:\d+: connection refused/);
      assert.match(m, /Next step: .*port already in use/);
      return true;
    },
  );
  assert.ok(Date.now() - started < 10_000, 'failed fast, not at the 30 s limit');
});

test('positive control: a server that never answers fails at its timeout, naming URL and probe, and its whole tree is killed', async () => {
  const dir = scratch();
  const file = join(dir, 'pids');
  const port = await freePort();
  const started = Date.now();

  const pending = startServers([{ command: treeServer(file), url: `http://localhost:${port}`, timeoutMs: 1500, reuseExisting: false }], quiet);
  const failure = assert.rejects(pending, /^Error: ::error::e2e server http:\/\/localhost:\d+ did not answer within 1\.5 s \(probed GET http:\/\/localhost:\d+ \d+ times; last: connection refused/);
  const tree = await treeOf(file);
  await failure;
  assert.ok(Date.now() - started < 10_000);
  await allGone(tree, 'after the timeout');
  rmSync(dir, { recursive: true, force: true });
});

test('a server that answers is ready, its start-up output is logged, and stop ends the process and its grandchild', async () => {
  const dir = scratch();
  const file = join(dir, 'pids');
  const port = await freePort();
  const lines: string[] = [];
  const code =
    `const g=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});` +
    `require('fs').writeFileSync(${JSON.stringify(file)},process.pid+' '+g.pid);` +
    `require('http').createServer((q,r)=>r.end('ok')).listen(${port}, () => console.log('listening'))`;
  const running = await startServers([{ command: node(code), url: `http://localhost:${port}`, timeoutMs: 20_000, reuseExisting: false }], { log: (l) => lines.push(l) });
  const tree = await treeOf(file);
  assert.ok((await fetch(`http://localhost:${port}`)).ok);
  assert.ok(lines.some((l) => l.includes('listening')), lines.join('|'));
  await running.stop();
  await allGone(tree, 'after stop');
  await assert.rejects(fetch(`http://localhost:${port}`));
  rmSync(dir, { recursive: true, force: true });
});

test('a server already on the port is refused unless reuse is on', async () => {
  const s = createServer((q, r) => r.end('stale')).listen(0);
  await new Promise((r) => s.once('listening', r));
  const { port } = s.address() as { port: number };
  try {
    await assert.rejects(startServers([{ command: node(`setInterval(() => {}, 1000)`), url: `http://localhost:${port}`, timeoutMs: 5000, reuseExisting: false }], quiet), /::error::.*something already answers there/);
    const reused = await startServers([{ command: node(`process.exit(9)`), url: `http://localhost:${port}`, timeoutMs: 5000, reuseExisting: true }], quiet);
    await reused.stop();
  } finally {
    s.close();
  }
});

test('stderr stays in the log after the server is ready, and a server that ends during the run prints one ::error:: line', async () => {
  const port = await freePort();
  const lines: string[] = [];
  const code = `require('http').createServer((q,r)=>r.end('ok')).listen(${port}, () => setTimeout(() => { console.error('late warning'); setTimeout(() => process.exit(7), 100) }, 300))`;
  const running = await startServers([{ command: node(code), url: `http://localhost:${port}`, timeoutMs: 20_000, reuseExisting: false }], { log: (l) => lines.push(l) });
  await until(() => lines.some((l) => l.startsWith('::error::')), 'the ::error:: line', 5000);
  await running.stop();
  assert.ok(lines.some((l) => l.includes('[e2e server err] late warning')), lines.join('\n'));
  const errors = lines.filter((l) => l.startsWith('::error::'));
  assert.equal(errors.length, 1, lines.join('\n'));
  assert.match(errors[0]!, /ended during the run \(exit status 7\).*stderr tail: late warning/);
});

test('stop signals the group even when the leader has already exited: its orphaned child is killed too', async () => {
  const dir = scratch();
  const file = join(dir, 'pids');
  const port = await freePort();
  // The leader starts a child, becomes ready, then exits; the child (same group) keeps running.
  const code =
    `const g=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});` +
    `require('fs').writeFileSync(${JSON.stringify(file)},process.pid+' '+g.pid);` +
    `const s=require('http').createServer((q,r)=>r.end('ok')).listen(${port}, () => setTimeout(() => process.exit(0), 300))`;
  const running = await startServers([{ command: node(code), url: `http://localhost:${port}`, timeoutMs: 20_000, reuseExisting: false }], quiet);
  const [leader, orphan] = await treeOf(file);
  await allGone([leader!], 'the leader exits');
  assert.ok(alive(orphan!), 'the orphan outlives its leader');
  await running.stop();
  await allGone([orphan!], 'after stop');
  rmSync(dir, { recursive: true, force: true });
});

test('the command runs in baseDir, and a relative cwd resolves against it', async () => {
  const dir = scratch();
  const port = await freePort();
  const code = `require('fs').writeFileSync('cwd.txt', process.cwd()); require('http').createServer((q,r)=>r.end('ok')).listen(${port})`;
  const sub = join(dir, 'sub');
  spawnSync('mkdir', [sub]);
  const running = await startServers([{ command: node(code), url: `http://localhost:${port}`, timeoutMs: 20_000, reuseExisting: false, cwd: 'sub' }], { ...quiet, baseDir: dir });
  await running.stop();
  assert.equal(readFileSync(join(sub, 'cwd.txt'), 'utf8'), spawnSync('realpath', [sub], { encoding: 'utf8' }).stdout.trim());
  rmSync(dir, { recursive: true, force: true });
});

test('a process that is interrupted while the servers start leaves none running (SIGTERM, SIGINT and SIGHUP), the whole tree', async () => {
  for (const signal of ['SIGTERM', 'SIGINT', 'SIGHUP'] as const) {
    const dir = scratch();
    const file = join(dir, 'pids');
    const port = await freePort();
    const parent = join(dir, 'parent.ts');
    writeFileSync(
      parent,
      `import { startServers } from ${JSON.stringify(join(here, 'server-ready.ts'))};\n` +
        `await startServers([{ command: ${JSON.stringify(treeServer(file))}, url: 'http://localhost:${port}', timeoutMs: 60000, reuseExisting: false }], { log: () => undefined });\n`,
    );
    const child = spawn(process.execPath, ['--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', parent], { stdio: 'ignore' });
    const ended = new Promise<string | null>((r) => child.once('exit', (_c, s) => r(s)));
    const tree = await treeOf(file);
    child.kill(signal);
    assert.equal(await ended, signal, 'the signal still ends the parent');
    await allGone(tree, `after ${signal} to the parent`);
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a process that exits normally while its server still runs leaves none running (the exit handler)', async () => {
  const dir = scratch();
  const file = join(dir, 'pids');
  const port = await freePort();
  const parent = join(dir, 'parent.ts');
  writeFileSync(
    parent,
    `import { startServers } from ${JSON.stringify(join(here, 'server-ready.ts'))};\n` +
      `import { createServer } from 'node:http';\n` +
      `const ok = createServer((q, r) => r.end('ok'));\n` +
      `const cmd = ${JSON.stringify(
        node(
          `const g=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});` +
            `require('fs').writeFileSync(${JSON.stringify(file)},process.pid+' '+g.pid);require('http').createServer((q,r)=>r.end('ok')).listen(${port})`,
        ),
      )};\n` +
      `await startServers([{ command: cmd, url: 'http://localhost:${port}', timeoutMs: 20000, reuseExisting: false }], { log: () => undefined });\n` +
      `ok.close(); process.exit(0);\n`,
  );
  const child = spawn(process.execPath, ['--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', parent], { stdio: 'ignore' });
  const tree = await treeOf(file);
  await new Promise((r) => child.once('exit', r));
  await allGone(tree, 'after the parent exited');
  rmSync(dir, { recursive: true, force: true });
});
