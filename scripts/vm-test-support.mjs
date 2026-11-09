// Shared by vm-lock.test.mjs and vm-win.test.mjs (the Windows test VM tooling, which is a macOS tool: Fusion, the login keychain,
// BSD stat, sysctl). Not a test file: `npm run test:scripts` runs only *.test.mjs.
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

/** Options for a test that needs macOS tools. Off macOS it is skipped with this reason, which the test report shows (ER-33: "not measured", never a silent pass). */
export const macOnly = process.platform === 'darwin' ? {} : { skip: 'macOS only (Fusion, sysctl, BSD stat, the login keychain): not measured on this OS' };

/**
 * Builds or fetches `./dev` once, before the first test starts. The lock tests run the real `./dev ci slot`, whose first call
 * compiles paddock-dev on a clean machine (CI): that must not count against a test's own bound, or the first tests fail on the build time.
 * The bound is the build's, not a test's wait.
 */
export function warmDev() {
  const r = spawnSync('bash', [join(here, '..', 'dev'), '--version'], { encoding: 'utf8', timeout: 900_000 }); // conformance: hang-guard — a build of the tool, 15 min bound only stops a hung compile
  if (r.status !== 0) throw new Error(`./dev could not be built for the lock tests (exit ${r.status}): ${String(r.stderr).trim().split('\n').slice(-3).join(' | ')}`);
}
