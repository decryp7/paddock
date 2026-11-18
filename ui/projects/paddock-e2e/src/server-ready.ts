// Starts an app's dev server for a Playwright run and fails once, at start-up, with one `::error::` line when it never
// becomes ready (ER-27). Playwright's own `webServer` fails with "Timed out waiting ... from config.webServer" and no cause;
// in CI run 8248 a page that never loaded cost every test its 2 min limit until the 10800 s job limit. Shared by every
// Paddock Playwright config (rule 42, ER-22) through `paddockServers` (config.ts).
//
// Server processes never outlive the run (ER-14): each is started as its own process group, every group is killed when the
// run ends, is interrupted (SIGINT, SIGTERM, SIGHUP; Playwright runs no teardown after an interrupted setup) or exits.
import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';

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
  /** The command's folder; relative to the config file's folder (as Playwright's `webServer.cwd`), default that folder. */
  readonly cwd?: string;
}

/** The start-up failure; its message is the single `::error::` line. */
export class ServerNotReady extends Error {}

export interface RunningServers {
  /** Ends every server this call started (the whole process group). */
  stop(): Promise<void>;
}

export interface StartOptions {
  /** Where the server's output and the end-of-server error go (default: standard output, so the job log shows them). */
  readonly log?: (line: string) => void;
  /** The folder relative `cwd` values and the default `cwd` resolve against (the config file's folder; default the current folder). */
  readonly baseDir?: string;
}

const TAIL_LINES = 12;
/** The poll schedule is Playwright's own (`waitFor` in playwright/lib/runner: 100, 250, 500 ms, then 1 s), so readiness is noticed as fast as before. */
const POLL_SCHEDULE_MS = [100, 250, 500] as const;
const POLL_STEADY_MS = 1000;
/**
 * One probe's limit. It decides only how long one try may hang, never the run's limit (`timeoutMs`): a localhost server that has not
 * answered in 3 s is "not ready this round", and the next poll tries again.
 */
const PROBE_MS = 3000;

/** Playwright's own readiness rule for a URL: 2xx, 3xx or 400-403 means something serves it. */
const answers = (status: number) => status >= 200 && status < 404;

async function probe(url: string): Promise<{ ok: boolean; what: string }> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(PROBE_MS), redirect: 'manual' });
    await res.body?.cancel();
    return { ok: answers(res.status), what: `HTTP ${res.status}` };
  } catch (e) {
    const cause = (e as { cause?: { code?: string } }).cause?.code;
    const name = (e as Error).name;
    return { ok: false, what: cause === 'ECONNREFUSED' ? 'connection refused' : name === 'TimeoutError' ? `no answer within ${PROBE_MS / 1000} s` : (cause ?? (e as Error).message) };
  }
}

// Every server group this process started and has not ended. The children are held (not looked up by a name read earlier), so a
// group id is signalled only while its leader is ours (rule 53).
const live = new Set<ChildProcess>();
let installed = false;

/** Kills one server's whole group at once (SIGKILL, as Playwright's own default does; `taskkill /T /F` on Windows). */
function killGroup(child: ChildProcess): void {
  const pid = child.pid;
  if (pid === undefined) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore' });
    return;
  }
  try {
    process.kill(-pid, 'SIGKILL');
  } catch {
    /* the group is already gone */
  }
}

function killAll(): void {
  for (const child of live) killGroup(child);
  live.clear();
}

function installCleanup(): void {
  if (installed) return;
  installed = true;
  process.once('exit', killAll);
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
    const onSignal = () => {
      killAll();
      process.off(signal, onSignal);
      // With no other handler (Playwright's own, which then ends the run) the signal's default action must still end this process.
      if (process.listenerCount(signal) === 0) process.kill(process.pid, signal);
    };
    process.on(signal, onSignal);
  }
}

async function startOne(spec: ServerSpec, log: (line: string) => void, baseDir: string): Promise<() => Promise<void>> {
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
  const keep = (chunk: Buffer, stream: 'out' | 'err') => {
    for (const line of chunk.toString().split(/\r?\n/)) {
      if (!line) continue;
      log(`[e2e server ${stream}] ${line}`);
      if (stream === 'err') {
        tail.push(line);
        if (tail.length > TAIL_LINES) tail.shift();
      }
    }
  };
  const cwd = spec.cwd === undefined ? baseDir : isAbsolute(spec.cwd) ? spec.cwd : resolve(baseDir, spec.cwd);
  log(`[e2e server] starting in ${cwd}: ${spec.command}  (waiting for ${spec.url}, at most ${spec.timeoutMs / 1000} s)`);
  installCleanup();
  const began = Date.now();
  const child = spawn(spec.command, { shell: true, cwd, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'], env: process.env });
  live.add(child);
  child.stdout?.on('data', (c: Buffer) => keep(c, 'out'));
  child.stderr?.on('data', (c: Buffer) => keep(c, 'err'));
  let exit: string | undefined;
  let stopping = false;
  let ready = false;
  const ended = new Promise<void>((done) => child.once('close', () => done()));
  child.on('error', (e) => (exit = `could not start: ${e.message}`));
  child.on('exit', (code, signal) => {
    exit = signal ? `ended by ${signal}` : `exit status ${code}`;
    // The leader is gone: the group may still hold its children, which `stop` kills.
    if (ready && !stopping) {
      log(`::error::e2e server ${spec.url}: the server command \`${spec.command}\` ended during the run (${exit}); the tests that follow cannot load the page. stderr tail: ${tail.join(' | ') || '(none)'}. Next step: read the server output above.`);
    }
  });
  const stop = async () => {
    stopping = true;
    killGroup(child); // always the group, even when the leader has already exited: its children may not have
    live.delete(child);
    await Promise.race([ended, new Promise<void>((r) => setTimeout(r, 2000).unref())]);
  };
  let last = first;
  let polls = 0;
  try {
    for (;;) {
      if (exit !== undefined) {
        throw new ServerNotReady(
          `::error::e2e server ${spec.url}: the server command \`${spec.command}\` ended before the page answered (${exit}, after ${Math.round((Date.now() - began) / 1000)} s); last probe GET ${spec.url}: ${last.what}; stderr tail: ${tail.join(' | ') || '(none)'}. Next step: read the start-up output above; a port already in use is the usual cause (another run on the same PW_PORT).`,
        );
      }
      if (Date.now() - began >= spec.timeoutMs) {
        throw new ServerNotReady(
          `::error::e2e server ${spec.url} did not answer within ${spec.timeoutMs / 1000} s (probed GET ${spec.url} ${polls + 1} times; last: ${last.what}; the server command is still running; stderr tail: ${tail.join(' | ') || '(none)'}). Next step: read the start-up output above (a build error or a wrong port), or raise serveTimeoutMs only with a measured start-up time.`,
        );
      }
      last = await probe(spec.url);
      if (last.ok) break;
      await new Promise((r) => setTimeout(r, POLL_SCHEDULE_MS[polls] ?? POLL_STEADY_MS));
      polls++;
    }
  } catch (e) {
    await stop(); // an abandoned start-up leaves nothing running
    throw e;
  }
  ready = true;
  log(`[e2e server] ${spec.url} ready after ${((Date.now() - began) / 1000).toFixed(1)} s (${last.what})`);
  return stop;
}

/** Starts the servers one after another; on a failure ends those already started and throws {@link ServerNotReady}. */
export async function startServers(specs: readonly ServerSpec[], options: StartOptions = {}): Promise<RunningServers> {
  const log = options.log ?? ((line: string) => console.log(line));
  const baseDir = options.baseDir ?? process.cwd();
  const stops: Array<() => Promise<void>> = [];
  const stop = async () => {
    await Promise.all(stops.splice(0).map((s) => s()));
  };
  try {
    for (const spec of specs) stops.push(await startOne(spec, log, baseDir));
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}
