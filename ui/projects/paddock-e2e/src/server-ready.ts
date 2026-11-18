// Starts an app's dev server for a Playwright run and fails once, at start-up, with one `::error::` line when it never
// becomes ready (ER-27). Playwright's own `webServer` fails with "Timed out waiting ... from config.webServer" and no cause;
// in CI run 8248 a page that never loaded cost every test its 2 min limit until the 10800 s job limit. Shared by every
// Paddock Playwright config (rule 42, ER-22) through {@link paddockServers}.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';

/** One server a run needs. */
export interface ServerSpec {
  /** The shell command that serves the app. */
  readonly command: string;
  /** The URL that answers once the app is up. */
  readonly url: string;
  /** How long the first answer may take. Carried over from Playwright's former `webServer.timeout` (180 s default); not newly measured. */
  readonly timeoutMs: number;
  /** Accept a server that already answers (PW_REUSE_SERVER=1); otherwise it is an error, as a stale one would be tested instead of the build. */
  readonly reuseExisting: boolean;
}

/** The start-up failure; its message is the single `::error::` line. */
export class ServerNotReady extends Error {}

export interface RunningServers {
  /** Ends every server this call started (the whole process tree). */
  stop(): Promise<void>;
}

export interface StartOptions {
  /** Where the server's output goes (default: standard output, so the job log shows the start-up). */
  readonly log?: (line: string) => void;
}

const TAIL_LINES = 12;
const POLL_MS = 500;
const PROBE_MS = 3000;
const GRACE_MS = 5000;

/** Playwright's own readiness rule for a URL: 2xx, 3xx or 400-403 means something serves it. */
const answers = (status: number) => (status >= 200 && status < 404);

async function probe(url: string): Promise<{ ok: boolean; what: string }> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(PROBE_MS), redirect: 'manual' });
    await res.body?.cancel();
    return { ok: answers(res.status), what: `HTTP ${res.status}` };
  } catch (e) {
    const cause = (e as { cause?: { code?: string } }).cause?.code;
    const name = (e as Error).name;
    return { ok: false, what: cause === 'ECONNREFUSED' ? 'connection refused' : name === 'TimeoutError' ? `no answer within ${PROBE_MS / 1000} s` : cause ?? (e as Error).message };
  }
}

function killTree(child: ChildProcess, signal: 'SIGTERM' | 'SIGKILL'): void {
  if (child.pid === undefined || child.exitCode !== null) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch {
    /* already gone */
  }
}

async function startOne(spec: ServerSpec, log: (line: string) => void): Promise<() => Promise<void>> {
  const first = await probe(spec.url);
  if (first.ok) {
    if (spec.reuseExisting) {
      log(`[e2e server] ${spec.url} already answers (${first.what}); reused (PW_REUSE_SERVER=1)`);
      return async () => undefined;
    }
    throw new ServerNotReady(
      `::error::e2e server ${spec.url}: something already answers there (${first.what}) and PW_REUSE_SERVER is not on, so a stale server would be tested instead of the build. Next step: stop whatever uses that port, or choose another with the PW_PORT variables.`,
    );
  }
  const tail: string[] = [];
  let up = false; // after the first answer only the start-up is logged: a dev server's per-page console warnings would flood the job log
  const keep = (chunk: Buffer, stream: 'out' | 'err') => {
    for (const line of chunk.toString().split(/\r?\n/)) {
      if (!line) continue;
      if (!up) log(`[e2e server ${stream}] ${line}`);
      if (stream === 'err') {
        tail.push(line);
        if (tail.length > TAIL_LINES) tail.shift();
      }
    }
  };
  log(`[e2e server] starting: ${spec.command}  (waiting for ${spec.url}, at most ${spec.timeoutMs / 1000} s)`);
  const began = Date.now();
  const child = spawn(spec.command, { shell: true, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'], env: process.env });
  child.stdout?.on('data', (c: Buffer) => keep(c, 'out'));
  child.stderr?.on('data', (c: Buffer) => keep(c, 'err'));
  let exit: string | undefined;
  child.on('error', (e) => (exit = `could not start: ${e.message}`));
  child.on('exit', (code, signal) => (exit = signal ? `ended by ${signal}` : `exit status ${code}`));
  const stop = async () => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    killTree(child, 'SIGTERM');
    const gone = await Promise.race([new Promise<boolean>((r) => child.once('exit', () => r(true))), new Promise<boolean>((r) => setTimeout(() => r(false), GRACE_MS))]);
    if (!gone) killTree(child, 'SIGKILL');
  };
  let last = first;
  for (;;) {
    if (exit !== undefined) {
      await stop();
      throw new ServerNotReady(
        `::error::e2e server ${spec.url}: the server command \`${spec.command}\` ended before the page answered (${exit}, after ${Math.round((Date.now() - began) / 1000)} s); last probe GET ${spec.url}: ${last.what}; stderr tail: ${tail.join(' | ') || '(none)'}. Next step: read the start-up output above; a port already in use is the usual cause (another run on the same PW_PORT).`,
      );
    }
    if (Date.now() - began >= spec.timeoutMs) {
      await stop();
      throw new ServerNotReady(
        `::error::e2e server ${spec.url} did not answer within ${spec.timeoutMs / 1000} s (probed GET ${spec.url} every ${POLL_MS} ms; last: ${last.what}; the server command is still running; stderr tail: ${tail.join(' | ') || '(none)'}). Next step: read the start-up output above (a build error or a wrong port), or raise serveTimeoutMs only with a measured start-up time.`,
      );
    }
    last = await probe(spec.url);
    if (last.ok) {
      up = true;
      log(`[e2e server] ${spec.url} ready after ${((Date.now() - began) / 1000).toFixed(1)} s (${last.what})`);
      return stop;
    }
    await new Promise((r) => setTimeout(r, POLL_MS));
  }
}

/** Starts the servers one after another; on a failure ends those already started and throws {@link ServerNotReady}. */
export async function startServers(specs: readonly ServerSpec[], options: StartOptions = {}): Promise<RunningServers> {
  const log = options.log ?? ((line: string) => console.log(line));
  const stops: Array<() => Promise<void>> = [];
  const stop = async () => {
    await Promise.all(stops.splice(0).map((s) => s()));
  };
  try {
    for (const spec of specs) stops.push(await startOne(spec, log));
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}
