// The Windows test VM's driver is `scripts/vm-win.sh` in this repo (ADR 0040 §1; ADR 0042, amendment of 2026-11-08, §7).
// The driver itself refuses every VM but paddock-win11-test (scripts/vm-win/target.sh). `PADDOCK_VM_WIN` is for the harness's
// own tests only: it is honoured only for a fake driver under the per-user temp root (`getconf DARWIN_USER_TEMP_DIR`, /tmp
// where there is none; never the caller's $TMPDIR), and refused otherwise (review P1-1).
import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `scripts/vm-win.sh`, next to this folder. */
export const VM_WIN_IN_REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'vm-win.sh');

/** The per-user temp root fakes must live under; not taken from $TMPDIR. */
export const TEST_TEMP_ROOT = (() => {
  try {
    return execFileSync('getconf', ['DARWIN_USER_TEMP_DIR'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() || '/tmp';
  } catch {
    return '/tmp';
  }
})();

/** The driver to run: the repo's own, or a test's fake driver under the temp root given in PADDOCK_VM_WIN (an empty value is unset). */
export function vmWinPath(env = process.env) {
  const want = env.PADDOCK_VM_WIN;
  if (!want) return VM_WIN_IN_REPO;
  let real = null;
  let tmp = null;
  try {
    real = realpathSync(want);
    tmp = realpathSync(TEST_TEMP_ROOT);
  } catch {
    real = null;
  }
  if (!real || !real.startsWith(tmp + sep)) {
    throw new Error(`PADDOCK_VM_WIN is for tests only (a fake driver under ${TEST_TEMP_ROOT}); got ${want}. Unset it: the harness runs ${VM_WIN_IN_REPO}`);
  }
  return real;
}

/** The driver of this process. */
export const VMWIN = vmWinPath();
