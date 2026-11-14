// The one Windows VM the harness may ever drive (ADR 0040 §1; review P1-1, 2026-11-08). There is no override: the user's own
// Fusion VMs must be impossible to target. vm-win.sh enforces the same folder on its side (scripts/vm-win/target.sh). The folder is
// derived from the repository's git config alone (C125: no disk is named here).
//
//   node scripts/vm-smoke/win-vm-target.mjs --vmx    prints the checked .vmx path, or exits 2 with an ::error:: line
import { realpathSync } from 'node:fs';
import { join } from 'node:path';
import { isMain } from '../../conformance/src/util/is-main.mjs';
import { vmDataPinned } from '../dev-places.mjs';

export const WIN_VM_NAME = 'paddock-win11-test';
/** The test VM's folder under a vm-data folder: <vm-data>/paddock-win11-test. */
export const winVmDirIn = (vmData) => join(vmData, WIN_VM_NAME);
/** The test VM's .vmx under its folder. */
export const winVmxIn = (dir) => join(dir, 'vm', `${WIN_VM_NAME}.vmwarevm`, `${WIN_VM_NAME}.vmx`);

// From the repository's own git config with an empty environment (scripts/dev-places.mjs): no variable can move it.
const [derived, why] = (() => {
  try {
    return [winVmDirIn(vmDataPinned()), ''];
  } catch (e) {
    return ['', e.message];
  }
})();
/** Why the folder cannot be derived, or '' (it can). */
export const WIN_VM_UNKNOWN = why;
/** The test VM's folder (<dev disk>/vm-data/paddock-win11-test, docs/dev-setup.md); '' when it cannot be derived. */
export const WIN_VM_DIR = derived;
/** The test VM's .vmx, derived from the folder; '' when the folder cannot be derived. */
export const WIN_VMX = derived ? winVmxIn(derived) : '';

/**
 * Throws unless the folder and the .vmx exist and resolve (links followed) to exactly the test VM's paths. Call it right
 * before acting on the VM (rule 53: verify immediately before acting). Refuses (fail closed) when the folder cannot be derived.
 * @param {{realpath?: (p: string) => string, dir?: string, unknown?: string}} [o] injected for tests
 */
export function checkWinVmTarget({ realpath = realpathSync, dir: wantDir = WIN_VM_DIR, unknown = WIN_VM_UNKNOWN } = {}) {
  if (!wantDir) throw new Error(`refusing: the Windows test VM's folder cannot be derived (${unknown || 'no folder'}); the harness only drives the test VM`);
  const wantVmx = winVmxIn(wantDir);
  const want = `the harness only drives ${wantDir}`;
  let dir;
  let vmx;
  try {
    dir = realpath(wantDir);
    vmx = realpath(wantVmx);
  } catch (e) {
    throw new Error(`refusing: the Windows test VM is not there (${e.code ?? e.message}); ${want} (only drives it, nothing else)`);
  }
  if (dir !== wantDir) throw new Error(`refusing: ${wantDir} resolves to ${dir}; ${want} (only drives the real folder)`);
  if (vmx !== wantVmx) throw new Error(`refusing: ${wantVmx} resolves to ${vmx}; ${want} (only drives the real .vmx)`);
  return { dir, vmx };
}

if (isMain(import.meta.url)) {
  if (process.argv[2] !== '--vmx') {
    console.error('usage: win-vm-target.mjs --vmx');
    process.exit(2);
  }
  try {
    process.stdout.write(`${checkWinVmTarget().vmx}\n`);
  } catch (e) {
    console.log(`::error::${e.message}. Nothing was started or changed.`);
    process.exit(2);
  }
}
