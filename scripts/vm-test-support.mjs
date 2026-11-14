// Shared by vm-lock.test.mjs and vm-win.test.mjs (the Windows test VM tooling, which is a macOS tool: Fusion, the login keychain,
// BSD stat, sysctl). Not a test file: `npm run test:scripts` runs only *.test.mjs.
import { spawnSync } from 'node:child_process';
import { existsSync, realpathSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Reaper, waitProgressing } from './test-kit.mjs';

const here = dirname(fileURLToPath(import.meta.url));

/** Options for a test that needs macOS tools. Off macOS it is skipped with this reason, which the test report shows (ER-33: "not measured", never a silent pass). */
export const macOnly = process.platform === 'darwin' ? {} : { skip: 'macOS only (Fusion, sysctl, BSD stat, the login keychain): not measured on this OS' };

/**
 * Builds or fetches `./dev` once, before the first test starts, and returns the program the lock tests run as PADDOCK_DEV: the
 * cached binary (`./dev --print-binary`), or the wrapper when the tool's inputs have uncommitted changes (no cached binary then).
 * The lock tests run the real `./dev ci slot`, whose first call compiles paddock-dev on a clean machine (CI): that must not count
 * against a test's own bound, or the first tests fail on the build time. The bound is the build's, not a test's wait.
 * The binary, not the wrapper: every wrapper call starts about fifteen programs (git status over the tool's inputs, rustc -vV through
 * rustup, the cache prune) before the tool itself, 0.17 s idle and 0.4 s at load 36 against 0.01 s for the binary
 * (docs/benchmarks.md, "2026-11-14, VM lock and kept-clone tests under load"), and a lock test makes dozens of calls.
 */
export function warmDev() {
  const dev = join(here, '..', 'dev');
  const r = spawnSync('bash', [dev, '--version'], { encoding: 'utf8', timeout: 900_000 }); // conformance: hang-guard — a build of the tool, 15 min bound only stops a hung compile
  if (r.status !== 0) throw new Error(`./dev could not be built for the lock tests (exit ${r.status}): ${String(r.stderr).trim().split('\n').slice(-3).join(' | ')}`);
  const bin = spawnSync('bash', [dev, '--print-binary'], { encoding: 'utf8', timeout: 900_000 }); // conformance: hang-guard — names the binary built above, 15 min bound only stops a hung compile
  const path = bin.status === 0 ? bin.stdout.trim() : '';
  return path && existsSync(path) ? path : dev;
}

const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; } };
const commandOf = (pid) => spawnSync('ps', ['-o', 'command=', '-p', String(pid)], { encoding: 'utf8' }).stdout.trim();

/**
 * The `slot hold` processes still running whose lock root is inside `scratch` (their command line names `--root <dir>`).
 * A holder is a detached process (its own process group, so killing a test's holder script does not reach it); it ends only
 * when its owner folder is gone or a release asks it to (crates/paddock-slots, hold.rs).
 */
export function holdersUnder(scratch) {
  const ps = spawnSync('ps', ['-axo', 'pid=,command='], { encoding: 'utf8' }).stdout ?? '';
  return ps.split('\n').map((l) => l.trim().match(/^(\d+) (.* ci slot hold .*--root (\S+).*)$/)).filter((m) => m && inside(m[3], scratch))
    .map((m) => ({ pid: Number(m[1]), command: m[2] }));
}
/** Whether `path` is in `scratch`, also when one of them is the canonical form (macOS: /var is /private/var). */
const inside = (path, scratch) => {
  const roots = [scratch];
  try { roots.push(realpathSync(scratch)); } catch { /* gone */ }
  return roots.some((r) => path === r || path.startsWith(`${r}/`));
};

/**
 * The guard for a lock test's holders (ER-14: no process outlives its test). Ends every `slot hold` under `scratch` the way the
 * lock means it to end, by removing its owner folder, then waits for the processes to exit on progress (ADR 0103: the kit's poll,
 * which backs off from 1 ms, and a 15 s bound on no holder ending, which only stops a holder that ignores its folder). A holder
 * still running then is killed (after checking that its pid is still that holder: rule 53) and reported, so a leak fails the test
 * that made it instead of a later job's cleanup. Returns the holders it had to kill (empty when all ended by themselves).
 */
export async function endHolders(scratch) {
  const holders = holdersUnder(scratch);
  for (const h of holders) {
    const owner = h.command.match(/--owner (\S+)/)?.[1];
    if (owner && inside(owner, scratch)) rmSync(owner, { recursive: true, force: true });
  }
  const running = () => holders.filter((h) => alive(h.pid)).length;
  try {
    await waitProgressing(`${holders.length} slot holder(s) to see their owner folder gone`, () => running() === 0, { noProgressMs: 15_000, progress: running });
  } catch { /* the survivors are killed and reported below */ }
  const killed = holders.filter((h) => alive(h.pid) && commandOf(h.pid) === h.command);
  for (const h of killed) { try { process.kill(h.pid, 'SIGKILL'); } catch { /* gone */ } }
  return killed;
}

/**
 * The last guard of a test file (its `after`): every process still running whose command line names the file's scratch folder (a
 * fake ssh, a vm-win.sh a test did not end) is ended by identity and reported (ADR 0103, the kit's Reaper with the folder as marker).
 * Returns what it had to end. Run it only after the holders' own end ([endHolders]), so a holder is never killed before it could
 * end by itself.
 */
export async function reapUnder(scratch, what) {
  const ended = [];
  for (const marker of new Set([scratch, (() => { try { return realpathSync(scratch); } catch { return scratch; } })()])) {
    ended.push(...await new Reaper(what, { marker }).reap());
  }
  return ended;
}
