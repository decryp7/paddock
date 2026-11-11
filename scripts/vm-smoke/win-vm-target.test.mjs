// Review P1-1 (2026-11-08): the harness can only ever drive /Volumes/d8sst7/vm-data/paddock-win11-test; there is no override.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WIN_VM_DIR, WIN_VMX, checkWinVmTarget } from './win-vm-target.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const fakeRealpath = (map) => (p) => { if (p in map) return map[p]; const e = new Error(`ENOENT ${p}`); e.code = 'ENOENT'; throw e; };

test('the target is the one test VM and its .vmx is derived from the folder', () => {
  assert.equal(WIN_VM_DIR, '/Volumes/d8sst7/vm-data/paddock-win11-test');
  assert.equal(WIN_VMX, '/Volumes/d8sst7/vm-data/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx');
});

test('the real folder and .vmx pass', () => {
  assert.deepEqual(checkWinVmTarget({ realpath: fakeRealpath({ [WIN_VM_DIR]: WIN_VM_DIR, [WIN_VMX]: WIN_VMX }) }), { dir: WIN_VM_DIR, vmx: WIN_VMX });
});

test('a folder or .vmx that resolves anywhere else is refused, and so is a missing one', () => {
  assert.throws(() => checkWinVmTarget({ realpath: fakeRealpath({ [WIN_VM_DIR]: '/Volumes/d8sst7/vm-data/users-vm', [WIN_VMX]: WIN_VMX }) }), /only drives \/Volumes\/d8sst7\/vm-data\/paddock-win11-test/);
  assert.throws(() => checkWinVmTarget({ realpath: fakeRealpath({ [WIN_VM_DIR]: WIN_VM_DIR, [WIN_VMX]: '/Users/x/Virtual Machines/Win.vmwarevm/Win.vmx' }) }), /only drives/);
  assert.throws(() => checkWinVmTarget({ realpath: fakeRealpath({}) }), /only drives/);
});

test('no harness file takes the VM folder or .vmx from the environment, and the shell scripts get the .vmx from this module', () => {
  const files = readdirSync(here).filter((f) => /\.(mjs|sh|ps1)$/.test(f) && !f.endsWith('.test.mjs'));
  const overrides = files.filter((f) => /PADDOCK_WIN_VMX|PADDOCK_WIN_VM_DIR/.test(readFileSync(join(here, f), 'utf8')));
  assert.deepEqual(overrides, []);
  for (const f of ['chain-release.sh', 'make-no-dyno-snapshot.sh']) {
    const s = readFileSync(join(here, f), 'utf8');
    assert.match(s, /VMX="\$\(node "\$HERE\/win-vm-target\.mjs" --vmx\)" \|\| exit 2/, f);
    assert.doesNotMatch(s, /paddock-win11-test\.vmx/, f);
  }
});

test('run-win.mjs checks the target before it writes the .vmx or starts anything', () => {
  const s = readFileSync(join(here, 'run-win.mjs'), 'utf8');
  assert.match(s, /const writeVmx = \(text\) => \{ checkWinVmTarget\(\);/);
  const main = s.indexOf('async function main(');
  assert.ok(main > 0);
  const body = s.slice(main);
  assert.ok(body.indexOf('checkWinVmTarget()') >= 0 && body.indexOf('checkWinVmTarget()') < body.indexOf('vm(['), 'checkWinVmTarget() must come before the first vm([...]) call in main');
});
