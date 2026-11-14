// Review P1-1 (2026-11-08): the harness can only ever drive the one test VM, <dev disk>/vm-data/paddock-win11-test, derived from the
// repository's git config alone; there is no override. The folders here are invented (C125: no machine's disk is named).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WIN_VM_DIR, WIN_VMX, WIN_VM_UNKNOWN, checkWinVmTarget, devPlace, winVmDirIn, winVmxIn } from './win-vm-target.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const fakeRealpath = (map) => (p) => { if (p in map) return map[p]; const e = new Error(`ENOENT ${p}`); e.code = 'ENOENT'; throw e; };
const DIR = winVmDirIn('/srv/disk-b/vm-data');
const VMX = winVmxIn(DIR);

test('the target is the one test VM under the derived vm-data folder, and its .vmx is derived from the folder', () => {
  assert.equal(DIR, '/srv/disk-b/vm-data/paddock-win11-test');
  assert.equal(VMX, '/srv/disk-b/vm-data/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx');
  // This machine's folder is derived the same way, or not at all; never a named disk.
  if (WIN_VM_DIR) {
    assert.match(WIN_VM_DIR, /\/vm-data\/paddock-win11-test$/);
    assert.equal(WIN_VMX, winVmxIn(WIN_VM_DIR));
  } else {
    assert.match(WIN_VM_UNKNOWN, /paddock\.worktreeRoot/);
  }
});

test('the folder comes from the repository\'s own ./dev places --pinned, and its refusal is passed on', () => {
  const calls = [];
  const run = (prog, args) => { calls.push([prog, args]); return '/srv/disk-b/vm-data\n'; };
  assert.equal(devPlace('vm-data', { pinned: true, run }), '/srv/disk-b/vm-data');
  assert.match(calls[0][0], /\/dev$/);
  assert.deepEqual(calls[0][1].filter((a) => !a.startsWith('/')), ['places', '--get', 'vm-data', '--repo', '--pinned']);
  const refuse = () => { const e = new Error('exit 2'); e.stderr = 'error: The dev-targets folder cannot be derived: no worktree root is set.\n'; throw e; };
  assert.throws(() => devPlace('vm-data', { pinned: true, run: refuse }), /--pinned` failed: error: The dev-targets folder cannot be derived/);
  assert.throws(() => devPlace('vm-data', { run: () => '' }), /printed nothing/);
});

test('the real folder and .vmx pass', () => {
  assert.deepEqual(checkWinVmTarget({ dir: DIR, realpath: fakeRealpath({ [DIR]: DIR, [VMX]: VMX }) }), { dir: DIR, vmx: VMX });
});

test('a folder or .vmx that resolves anywhere else is refused, and so is a missing one', () => {
  assert.throws(() => checkWinVmTarget({ dir: DIR, realpath: fakeRealpath({ [DIR]: '/srv/disk-b/vm-data/users-vm', [VMX]: VMX }) }), /only drives \/srv\/disk-b\/vm-data\/paddock-win11-test/);
  assert.throws(() => checkWinVmTarget({ dir: DIR, realpath: fakeRealpath({ [DIR]: DIR, [VMX]: '/Users/x/Virtual Machines/Win.vmwarevm/Win.vmx' }) }), /only drives/);
  assert.throws(() => checkWinVmTarget({ dir: DIR, realpath: fakeRealpath({}) }), /only drives/);
});

test('a folder that cannot be derived refuses everything (fail closed)', () => {
  assert.throws(() => checkWinVmTarget({ dir: '', unknown: 'no worktree root', realpath: () => DIR }), /cannot be derived \(no worktree root\)/);
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
