// Review P1-1 (2026-11-08): the harness can only ever drive the one test VM, <dev disk>/vm-data/paddock-win11-test, derived from the
// repository's git config alone; there is no override. The folders here are invented (C125: no machine's disk is named).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
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
  // caller's environment pass. Dropped: every PADDOCK_* variable that picks or places the binary (PADDOCK_DEV_HOME, PADDOCK_DEV ...), and
  // every GIT_* one (GIT_DIR, GIT_WORK_TREE pick which checkout's tool is built). Kept: the three that only restrict the build
  // (PADDOCK_DEV_DOWNLOAD=0: a release job never downloads, ADR 0022; PADDOCK_TOOLCHAIN_FAILED=1; PADDOCK_DEV_FRESH=1).
  const set = { PATH: '/opt/ci/cargo/bin:/usr/bin', CARGO_HOME: '/srv/x/cargo', RUSTUP_HOME: '/srv/x/rustup', FOO_TOOLCHAIN: 'kept',
    PADDOCK_DEV_HOME: '/planted', PADDOCK_DEV: '/planted/dev', PADDOCK_CI_MOUNT: '/planted', PADDOCK_DEV_TARGET_DIR: '/planted',
    GIT_WORK_TREE: '/planted', GIT_DIR: '/planted/.git', PADDOCK_DEV_DOWNLOAD: '0', PADDOCK_TOOLCHAIN_FAILED: '1', PADDOCK_DEV_FRESH: '1' };
  const old = Object.fromEntries(Object.keys(set).map((k) => [k, process.env[k]]));
  Object.assign(process.env, set);
  try {
    devPlace('vm-data', { pinned: true, run });
    process.env.PADDOCK_DEV_DOWNLOAD = '1'; // only 0 is kept: a caller cannot switch a download on
    process.env.PADDOCK_TOOLCHAIN_FAILED = '0';
    process.env.PADDOCK_DEV_FRESH = '0';
    devPlace('vm-data', { pinned: true, run });
  } finally {
    for (const [k, v] of Object.entries(old)) if (v === undefined) delete process.env[k]; else process.env[k] = v;
  }
  const env = calls[1][2].env;
  assert.equal(env.PATH, '/opt/ci/cargo/bin:/usr/bin');
  assert.deepEqual([env.CARGO_HOME, env.RUSTUP_HOME, env.FOO_TOOLCHAIN], ['/srv/x/cargo', '/srv/x/rustup', 'kept']);
  assert.deepEqual(Object.keys(env).filter((k) => /^(PADDOCK_|GIT_)/.test(k)).sort(), ['PADDOCK_DEV_DOWNLOAD', 'PADDOCK_DEV_FRESH', 'PADDOCK_TOOLCHAIN_FAILED']);
  assert.deepEqual([env.PADDOCK_DEV_DOWNLOAD, env.PADDOCK_TOOLCHAIN_FAILED, env.PADDOCK_DEV_FRESH], ['0', '1', '1']);
  assert.deepEqual(Object.keys(calls[2][2].env).filter((k) => /^(PADDOCK_|GIT_)/.test(k)), [], 'a caller cannot switch a download on');
  // It runs from the repository, so the caller's working folder cannot pick the checkout either.
  assert.equal(calls[1][2].cwd, join(here, '..', '..'));
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

test('target.sh runs ./dev from the repository with the caller\'s PATH and toolchain, no PADDOCK_* or GIT_* variable, and no download in a release job', () => {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'win-target-')));
  try {
    mkdirSync(join(dir, 'scripts', 'vm-win'), { recursive: true });
    mkdirSync(join(dir, 'fakebin'));
    mkdirSync(join(dir, 'elsewhere'));
    copyFileSync(join(here, '..', 'vm-win', 'target.sh'), join(dir, 'scripts', 'vm-win', 'target.sh'));
    // A fake curl that records its use; a stand-in ./dev that asks the real wrapper's own predicate (download_enabled, read from ./dev)
    // whether it would download, calls curl if so, and reports what it inherited.
    writeFileSync(join(dir, 'fakebin', 'curl'), `#!/bin/bash\necho curl >> "${dir}/curl.log"\n`, { mode: 0o755 });
    writeFileSync(join(dir, 'dev'), `#!/bin/bash
eval "$(awk '/^download_enabled\\(\\) \\{/,/^\\}/' "${join(here, '..', '..', 'dev')}")"
if download_enabled; then curl -sS fake; fi
echo "/srv/x/\${PADDOCK_DEV_HOME:-none}-\${PADDOCK_DEV:-none}-\${PADDOCK_CI_MOUNT:-none}-\${GIT_WORK_TREE:-none}-\${GIT_DIR:-none}-\${CARGO_HOME:-none}-$PWD-$(printenv PATH)"
`, { mode: 0o755 });
    const path = `${join(dir, 'fakebin')}:/opt/ci/cargo/bin:/usr/bin:/bin`;
    const source = (env) => {
      rmSync(join(dir, 'curl.log'), { force: true });
      const out = execFileSync('bash', ['-c', 'source "$1"; printf %s "$WIN_VM_DIR"', 'x', join(dir, 'scripts', 'vm-win', 'target.sh')], {
        encoding: 'utf8', cwd: join(dir, 'elsewhere'), stdio: ['ignore', 'pipe', 'pipe'], // not a socket: bash reads ~/.bashrc on one and would extend PATH
        env: { PATH: path, HOME: process.env.HOME, TMPDIR: process.env.TMPDIR ?? '/tmp', GIT_WORK_TREE: '/planted', GIT_DIR: '/planted/.git', CARGO_HOME: 'c',
          PADDOCK_DEV_HOME: '/planted', PADDOCK_DEV: '/planted/dev', PADDOCK_CI_MOUNT: '/planted', ...env },
      });
      return [out, existsSync(join(dir, 'curl.log'))];
    };
    // A release job (GITHUB_ACTIONS=true, PADDOCK_DEV_DOWNLOAD=0) never downloads (ADR 0022 amendment); nothing planted gets through;
    // the PATH is exactly the caller's; ./dev runs from the repository, not from the caller's folder.
    const [out, downloaded] = source({ GITHUB_ACTIONS: 'true', PADDOCK_DEV_DOWNLOAD: '0' });
    assert.equal(out, `/srv/x/none-none-none-none-none-c-${dir}-${path}/paddock-win11-test`);
    assert.equal(downloaded, false, 'a release job must not download ./dev');
    // The fake curl is live: a plain CI job (no PADDOCK_DEV_DOWNLOAD) is allowed to download, and a caller cannot switch it on elsewhere.
    assert.equal(source({ GITHUB_ACTIONS: 'true' })[1], true);
    assert.equal(source({ PADDOCK_DEV_DOWNLOAD: '1' })[1], false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
