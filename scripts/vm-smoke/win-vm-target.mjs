// The one Windows VM the harness may ever drive (ADR 0040 §1; review P1-1, 2026-11-08). There is no override: the user's own
// Fusion VMs must be impossible to target. vm-win.sh enforces the same folder on its side (scripts/vm-win/target.sh). The folder is
// derived from the repository's git config alone, through `./dev places --pinned` (C125: no disk is named here).
//
//   node scripts/vm-smoke/win-vm-target.mjs --vmx    prints the checked .vmx path, or exits 2 with an ::error:: line
import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../../conformance/src/util/is-main.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * The environment ./dev runs under: the caller's own, without any `PADDOCK_*` variable (PADDOCK_DEV, PADDOCK_DEV_HOME,
 * PADDOCK_CI_MOUNT, PADDOCK_DEV_TARGET_DIR and the like pick or place the binary, so none can). ./dev finds its toolchain the way it
 * does for the caller: CI's cargo is not under $HOME/.cargo/bin, so a fixed PATH made ./dev stop with "no Rust compiler runs here".
 */
const devEnv = () => Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('PADDOCK_')));

/**
 * One folder from `./dev places` (dev-targets, dev-disk, vm-data, lock-root, main-checkout; docs/dev-setup.md), run through the
 * repository's own ./dev, never a program the environment names, without `PADDOCK_*` variables (the first run
 * after its inputs change builds paddock-dev in the default cache, about 35 s measured on 2026-11-15, 63 s on the macOS runner). `pinned` ignores every environment variable (paddock-dev reads
 * git config with an empty environment). Throws with paddock-dev's reason when the folder cannot be derived (fail closed).
 * @param {string} name
 * @param {{pinned?: boolean, run?: typeof execFileSync}} [o] `run` injected for tests
 */
export function devPlace(name, { pinned = false, run = execFileSync } = {}) {
  const args = ['places', '--get', name, '--repo', REPO, ...(pinned ? ['--pinned'] : [])];
  try {
    const out = String(run(join(REPO, 'dev'), args, { encoding: 'utf8', env: devEnv(), stdio: ['ignore', 'pipe', 'pipe'], timeout: 600_000 })).trim();
    if (!out) throw new Error('it printed nothing');
    return out;
  } catch (e) {
    const why = String(e.stderr || e.message || e).trim().split('\n').pop();
    throw new Error(`\`./dev places --get ${name}${pinned ? ' --pinned' : ''}\` failed: ${why}`);
  }
}

export const WIN_VM_NAME = 'paddock-win11-test';
/** The test VM's folder under a vm-data folder: <vm-data>/paddock-win11-test. */
export const winVmDirIn = (vmData) => join(vmData, WIN_VM_NAME);
/** The test VM's .vmx under its folder. */
export const winVmxIn = (dir) => join(dir, 'vm', `${WIN_VM_NAME}.vmwarevm`, `${WIN_VM_NAME}.vmx`);

// From the repository's own git config with an empty environment (`./dev places --pinned`): no variable can move it.
const [derived, why] = (() => {
  try {
    return [winVmDirIn(devPlace('vm-data', { pinned: true })), ''];
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
