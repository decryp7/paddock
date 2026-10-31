// The local HTTPS update feed of the scripted update tests (macOS: run.mjs --update-test; Windows: update-win.mjs). Bound to one VM
// bridge address only, with the test CA's server certificate; `/ok/latest.json` is always the CURRENT case's manifest (the old build has
// that one URL built in), the other files are the current case's folder. The case `down` resets every connection.
//
// The server runs in its own Node process. The runners wait for the guest in synchronous calls (update-win.mjs runs every
// `vm-win.sh job` through spawnSync for up to 15 minutes); a server in the runner's own process cannot answer while its event loop
// is blocked there: the kernel still accepts the TCP connection, but the TLS handshake never comes and the app reports "timeout:
// connect". Every method talks to that process and returns a promise.
import { fork } from 'node:child_process';
import { createReadStream, existsSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:https';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { feedRoute } from './update-judge.mjs';

const SELF = fileURLToPath(import.meta.url);

/** How long `close()` waits for the feed process to end before killing it. */
const CLOSE_DEADLINE_MS = 3000;

/**
 * Starts the feed process and resolves once it listens; rejects with the listen error (a port in use, say).
 * @param {{ assets: string, host: string, port: number }} o  assets: the folder with feed/ and ca/ (spotter-update-assets*.sh); port 0
 *   lets the system pick one, which `port` then names
 * @returns {Promise<{ port: number, setCase(id: string): Promise<void>, feedLog(): Promise<string[]>, clearFeedLog(): Promise<void>, close(): Promise<void> }>}
 */
export function startFeed({ assets, host, port }) {
  const child = fork(SELF, ['--serve'], { stdio: ['ignore', 'inherit', 'inherit', 'ipc'] });
  let next = 0;
  const waiting = new Map();
  const exited = new Promise((r) => child.once('exit', r));
  const ended = () => child.exitCode !== null || child.signalCode !== null;
  const fail = (id, e) => { const w = waiting.get(id); if (w) { waiting.delete(id); w.reject(e); } };
  // A send to a process that has gone away is reported here and to the send's callback, never thrown at the runner (which must still
  // stop and revert the VM).
  child.on('error', (e) => { for (const id of [...waiting.keys()]) fail(id, e); });
  child.on('message', (m) => {
    const w = waiting.get(m.id);
    if (!w) return;
    waiting.delete(m.id);
    if (m.error) w.reject(new Error(m.error));
    else w.resolve(m.value);
  });
  child.once('exit', (code, signal) => {
    for (const w of waiting.values()) w.reject(new Error(`the feed process ended (${signal ?? code})`));
    waiting.clear();
  });
  const ask = (op, arg) =>
    new Promise((resolve, reject) => {
      if (ended() || !child.connected) { reject(new Error('the feed process has ended')); return; }
      const id = ++next;
      waiting.set(id, { resolve, reject });
      child.send({ id, op, arg }, (e) => { if (e) fail(id, e); });
    });
  const feed = {
    setCase: (id) => ask('setCase', id),
    feedLog: () => ask('log'),
    clearFeedLog: () => ask('clear'),
    // Ends the feed process; one that does not end within the deadline is killed through this handle (rule 53: never by a pid).
    close: async () => {
      if (ended()) return;
      let timer;
      const deadline = new Promise((r) => { timer = setTimeout(r, CLOSE_DEADLINE_MS, 'deadline'); });
      const stop = (async () => {
        await ask('close').catch(() => {});
        if (child.connected) child.disconnect();
        await exited;
      })();
      if ((await Promise.race([stop, deadline])) === 'deadline') child.kill('SIGKILL');
      clearTimeout(timer);
      await exited;
    },
  };
  return ask('listen', { assets, host, port }).then(
    (bound) => Object.assign(feed, { port: bound }),
    async (e) => { child.kill(); await exited; throw e; },
  );
}

/** The feed process: one HTTPS server driven by the runner's messages; it ends when the runner goes away. */
function serve() {
  let server;
  let current = 'ok';
  let log = [];
  const ops = {
    listen: ({ assets, host, port }) => new Promise((resolve, reject) => {
      const root = join(assets, 'feed');
      server = createServer({ key: readFileSync(join(assets, 'ca', 'server.key')), cert: readFileSync(join(assets, 'ca', 'server.pem')) }, (req, res) => {
        if (current === 'down') { req.socket.destroy(); return; }
        const route = feedRoute(req.url ?? '/', current);
        log.push(`${req.method} ${req.url} ${route ? 200 : 404}`);
        if (!route || !existsSync(join(root, route.file))) { res.writeHead(404).end(); return; }
        res.writeHead(200, { 'content-type': route.type, 'content-length': statSync(join(root, route.file)).size });
        createReadStream(join(root, route.file)).on('error', () => res.destroy()).pipe(res);
      });
      server.once('error', reject);
      server.listen(port, host, () => resolve(server.address().port));
    }),
    setCase: (id) => { current = id; },
    log: () => log,
    clear: () => { log = []; },
    close: () => new Promise((resolve) => {
      if (!server?.listening) { resolve(); return; }
      server.closeAllConnections();
      server.close(() => resolve());
    }),
  };
  process.on('message', async ({ id, op, arg }) => {
    try {
      process.send({ id, value: await ops[op](arg) });
    } catch (e) {
      process.send({ id, error: [e.code, e.message].filter(Boolean).join(' ') });
    }
  });
  process.on('disconnect', () => process.exit(0));
}

if (process.argv[1] === SELF && process.argv[2] === '--serve') serve();
