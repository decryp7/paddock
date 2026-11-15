// Review P1-1 (2026-11-08): the harness can only ever drive the one test VM, <dev disk>/vm-data/paddock-win11-test, derived from the
// repository's git config alone; there is no override. The folders here are invented (C125: no machine's disk is named).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
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
  const run = (prog, args, opts) => { calls.push([prog, args, opts]); return '/srv/disk-b/vm-data\n'; };
  assert.equal(devPlace('vm-data', { pinned: true, run }), '/srv/disk-b/vm-data');
  assert.match(calls[0][0], /\/dev$/);
  assert.deepEqual(calls[0][1].filter((a) => !a.startsWith('/')), ['places', '--get', 'vm-data', '--repo', '--pinned']);
  // ./dev finds its toolchain the way it does for the caller (CI's cargo is not under $HOME/.cargo/bin), so PATH and the rest of the
  // caller's environment pass; every PADDOCK_* variable is dropped, so PADDOCK_DEV_HOME, PADDOCK_DEV and the like cannot pick a binary.
  const set = { PATH: '/opt/ci/cargo/bin:/usr/bin', CARGO_HOME: '/srv/x/cargo', RUSTUP_HOME: '/srv/x/rustup', FOO_TOOLCHAIN: 'kept',
    PADDOCK_DEV_HOME: '/planted', PADDOCK_DEV: '/planted/dev', PADDOCK_CI_MOUNT: '/planted', PADDOCK_DEV_TARGET_DIR: '/planted' };
  const old = Object.fromEntries(Object.keys(set).map((k) => [k, process.env[k]]));
  Object.assign(process.env, set);
  try {
    devPlace('vm-data', { pinned: true, run });
  } finally {
    for (const [k, v] of Object.entries(old)) if (v === undefined) delete process.env[k]; else process.env[k] = v;
  }
  const env = calls[1][2].env;
  assert.equal(env.PATH, '/opt/ci/cargo/bin:/usr/bin');
  assert.deepEqual([env.CARGO_HOME, env.RUSTUP_HOME, env.FOO_TOOLCHAIN], ['/srv/x/cargo', '/srv/x/rustup', 'kept']);
  assert.deepEqual(Object.keys(env).filter((k) => k.startsWith('PADDOCK_')), []);
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

test('target.sh runs ./dev with the caller\'s PATH and toolchain variables and no PADDOCK_* variable', () => {
  const dir = mkdtempSync(join(tmpdir(), 'win-target-'));
  try {
    mkdirSync(join(dir, 'scripts', 'vm-win'), { recursive: true });
    copyFileSync(join(here, '..', 'vm-win', 'target.sh'), join(dir, 'scripts', 'vm-win', 'target.sh'));
    // A stand-in ./dev that reports what it inherited.
    writeFileSync(join(dir, 'dev'), '#!/bin/sh\necho "/srv/x/${PADDOCK_DEV_HOME:-none}-${PADDOCK_DEV:-none}-${PADDOCK_DEV_TARGETS:-none}-${PADDOCK_CI_MOUNT:-none}-${CARGO_BUILD_JOBS:-none}-${RUSTUP_HOME:-none}-${CARGO_HOME:-none}-${RUSTUP_TOOLCHAIN:-none}-$PATH"\n', { mode: 0o755 });
    const out = execFileSync('bash', ['-c', 'source "$1"; printf %s "$WIN_VM_DIR"', 'x', join(dir, 'scripts', 'vm-win', 'target.sh')], {
      encoding: 'utf8', env: { ...process.env, PADDOCK_DEV_HOME: '/planted', PADDOCK_DEV: '/planted/dev', PADDOCK_DEV_TARGETS: '/planted', PADDOCK_CI_MOUNT: '/planted',
        CARGO_BUILD_JOBS: '2', RUSTUP_HOME: 'r', CARGO_HOME: 'c', RUSTUP_TOOLCHAIN: 't', PATH: '/opt/ci/cargo/bin:/usr/bin:/bin' },
    });
    // The toolchain's location (PATH, CARGO_HOME ...) passes; every PADDOCK_* variable is gone. (The stub runs under /bin/sh with that PATH.)
    // (a shell's own start-up files may append to PATH, so the caller's entries must lead it)
    assert.match(out, /^\/srv\/x\/none-none-none-none-2-r-c-t-\/opt\/ci\/cargo\/bin:\/usr\/bin:\/bin(:[^]*)?\/paddock-win11-test$/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
