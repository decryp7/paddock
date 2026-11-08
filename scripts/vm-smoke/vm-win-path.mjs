// The Windows test VM's driver is `scripts/vm-win.sh` in this repo (ADR 0040 §1; ADR 0042, amendment of 2026-11-08, §7).
// The driver itself refuses every VM but paddock-win11-test (scripts/vm-win/target.sh). `PADDOCK_VM_WIN` is for the harness's
// own tests only: it is honoured only for a fake driver that exists under $TMPDIR, and refused otherwise (review P1-1).
import { realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `scripts/vm-win.sh`, next to this folder. */
export const VM_WIN_IN_REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'vm-win.sh');

/** The driver to run: the repo's own, or a test's fake driver under $TMPDIR given in PADDOCK_VM_WIN (an empty value is unset). */
export function vmWinPath(env = process.env) {
  const want = env.PADDOCK_VM_WIN;
  if (!want) return VM_WIN_IN_REPO;
  let real = null;
  let tmp = null;
  try {
    real = realpathSync(want);
    tmp = realpathSync(tmpdir());
  } catch {
    real = null;
  }
  if (!real || !real.startsWith(tmp + sep)) {
    throw new Error(`PADDOCK_VM_WIN is for tests only (a fake driver under ${tmpdir()}); got ${want}. Unset it: the harness runs ${VM_WIN_IN_REPO}`);
  }
  return real;
}

/** The driver of this process. */
export const VMWIN = vmWinPath();
