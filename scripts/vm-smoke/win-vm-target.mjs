// The one Windows VM the harness may ever drive (ADR 0040 §1; review P1-1, 2026-11-08). There is no override: the user's own
// Fusion VMs must be impossible to target. vm-win.sh enforces the same folder on its side (scripts/vm-win/target.sh).
//
//   node scripts/vm-smoke/win-vm-target.mjs --vmx    prints the checked .vmx path, or exits 2 with an ::error:: line
import { realpathSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const WIN_VM_NAME = 'paddock-win11-test';
/** The test VM's folder; the .vmx, the SSH keys and the snapshots all live below it. */
export const WIN_VM_DIR = '/Volumes/d7sst7/vms/paddock-win11-test';
/** The test VM's .vmx, derived from the folder. */
export const WIN_VMX = join(WIN_VM_DIR, 'vm', `${WIN_VM_NAME}.vmwarevm`, `${WIN_VM_NAME}.vmx`);

/**
 * Throws unless the folder and the .vmx exist and resolve (links followed) to exactly the test VM's paths. Call it right
 * before acting on the VM (rule 53: verify immediately before acting).
 * @param {{realpath?: (p: string) => string}} [o] injected for tests
 */
export function checkWinVmTarget({ realpath = realpathSync } = {}) {
  const want = `the harness only drives ${WIN_VM_DIR}`;
  let dir;
  let vmx;
  try {
    dir = realpath(WIN_VM_DIR);
    vmx = realpath(WIN_VMX);
  } catch (e) {
    throw new Error(`refusing: the Windows test VM is not there (${e.code ?? e.message}); ${want} (only drives it, nothing else)`);
  }
  if (dir !== WIN_VM_DIR) throw new Error(`refusing: ${WIN_VM_DIR} resolves to ${dir}; ${want} (only drives the real folder)`);
  if (vmx !== WIN_VMX) throw new Error(`refusing: ${WIN_VMX} resolves to ${vmx}; ${want} (only drives the real .vmx)`);
  return { dir, vmx };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) {
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
