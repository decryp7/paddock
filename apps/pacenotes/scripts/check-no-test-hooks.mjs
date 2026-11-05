// The worker's `test-hooks` feature makes it abort, spin or exit on files with a magic suffix, for the crash tests. It must never be
// in a normal build: `cargo tree -e no-dev,features` over the whole workspace (dev-dependencies left out, as in a release build)
// must not show it. The self dev-dependency of pacenotes-worker turns it on for tests only.
import { execFileSync } from 'node:child_process';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const FEATURE = 'test-hooks';

/** The verdict for a `cargo tree` run: `{ status, stdout, stderr }` (status 0 when it printed a tree). */
export function verdict({ status, stdout = '', stderr = '' }) {
  if (status !== 0) {
    return { ok: false, message: `cargo tree failed, so the guard cannot say the feature is off:\n${stderr.trim()}` };
  }
  return stdout.includes(`feature "${FEATURE}"`)
    ? { ok: false, message: `the ${FEATURE} feature is on in a build without dev-dependencies:\n${stdout.trim()}` }
    : { ok: true, message: `the ${FEATURE} feature is off in a normal build` };
}

/** Runs the guard in `cwd` with `run` (injected for tests). */
export function check(cwd, run = defaultRun) {
  return verdict(run(cwd));
}

function defaultRun(cwd) {
  try {
    const stdout = execFileSync('cargo', ['tree', '--workspace', '-e', 'no-dev,features', '--target', 'all'], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120_000, maxBuffer: 64 * 1024 * 1024 });
    return { status: 0, stdout, stderr: '' };
  } catch (e) {
    return { status: e.status ?? 1, stdout: String(e.stdout ?? ''), stderr: String(e.stderr ?? e.message) };
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const r = check(dirname(dirname(fileURLToPath(import.meta.url))));
  console.log(r.message);
  process.exit(r.ok ? 0 : 1);
}
