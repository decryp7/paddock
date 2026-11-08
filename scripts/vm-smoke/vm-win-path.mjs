// The Windows test VM's driver is `scripts/vm-win.sh` in this repo (ADR 0040 §1; ADR 0042, amendment of 2026-11-08, §7).
// `PADDOCK_VM_WIN` overrides it, for a fake driver in a test; by default no copy outside the repo is used.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `scripts/vm-win.sh`, next to this folder. */
export const VM_WIN_IN_REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'vm-win.sh');

/** The driver to run: the override when set and not empty, else the repo's own. */
export function vmWinPath(env = process.env) {
  return env.PADDOCK_VM_WIN || VM_WIN_IN_REPO;
}

/** The driver of this process. */
export const VMWIN = vmWinPath();
