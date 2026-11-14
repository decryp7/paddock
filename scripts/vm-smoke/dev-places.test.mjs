// scripts/dev-places.mjs and scripts/dev-places.sh: the dev folders are derived from the repository's git config (C125), the two
// copies agree with each other, and both fail closed. The repositories here are temporary and the folders invented (/srv/...).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { devDisk, devTargets, devTargetsOf, parentOf, vmData, vmDataPinned } from '../dev-places.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SH = join(here, '..', 'dev-places.sh');

test('the dev-targets folder is the named one, else the parent of the worktree root', () => {
  assert.equal(devTargetsOf({ named: '/srv/dt/', worktreeRoot: '/srv/disk-a/dev-targets/wt' }), '/srv/dt');
  assert.equal(devTargetsOf({ named: '', worktreeRoot: '/srv/disk-b/dev-targets/wt' }), '/srv/disk-b/dev-targets');
  const root = () => '/srv/disk-b/dev-targets/wt';
  assert.equal(devDisk({}, root), '/srv/disk-b');
  assert.equal(vmData({}, root), '/srv/disk-b/vm-data');
  assert.equal(vmData({ PADDOCK_VM_DATA: '/srv/vms/' }, root), '/srv/vms');
  assert.equal(vmDataPinned('/srv/disk-b/dev-targets/wt'), '/srv/disk-b/vm-data');
});

test('it fails closed with the next step', () => {
  assert.throws(() => devTargetsOf({ named: '', worktreeRoot: '' }), /cannot be derived.*paddock\.worktreeRoot/);
  assert.throws(() => devTargetsOf({ named: 'rel/dt', worktreeRoot: '' }), /not an absolute path/);
  assert.throws(() => parentOf('/wt', 'the worktree root'), /has no parent folder/);
  assert.throws(() => vmDataPinned(''), /cannot be derived/);
  assert.throws(() => devTargets({}, () => ''), /cannot be derived/);
});

/** A temporary repository holding a copy of dev-places.sh, so the shell copy reads that repository's git config. */
const repoWith = (root) => {
  const d = mkdtempSync(join(process.env.PADDOCK_TEST_SCRATCH ?? tmpdir(), 'dev-places-'));
  mkdirSync(join(d, 'scripts'));
  copyFileSync(SH, join(d, 'scripts', 'dev-places.sh'));
  spawnSync('git', ['init', '-q', d]);
  if (root) spawnSync('git', ['-C', d, 'config', 'paddock.worktreeRoot', root]);
  return d;
};
const sh = (repo, fn, env = {}) => {
  const clean = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/^(PADDOCK_(DEV_TARGETS|WORKTREE_ROOT|VM_DATA)|GIT_)/.test(k)));
  const r = spawnSync('bash', ['-c', `. '${join(repo, 'scripts', 'dev-places.sh')}'; ${fn}`], { encoding: 'utf8', env: { ...clean, ...env } });
  return { status: r.status, out: r.stdout.trim(), err: r.stderr };
};

test('the shell copy derives the same folders from git config, and the pinned one ignores the environment', () => {
  const repo = repoWith('/srv/disk-b/dev-targets/wt');
  try {
    assert.deepEqual(sh(repo, 'paddock_dev_targets'), { status: 0, out: '/srv/disk-b/dev-targets', err: '' });
    assert.equal(sh(repo, 'paddock_dev_disk').out, '/srv/disk-b');
    assert.equal(sh(repo, 'paddock_vm_data').out, '/srv/disk-b/vm-data');
    assert.equal(sh(repo, 'paddock_vm_data', { PADDOCK_VM_DATA: '/srv/vms' }).out, '/srv/vms');
    assert.equal(sh(repo, 'paddock_dev_targets', { PADDOCK_DEV_TARGETS: '/srv/dt' }).out, '/srv/dt');
    const moved = { PADDOCK_VM_DATA: '/srv/elsewhere', PADDOCK_DEV_TARGETS: '/srv/elsewhere/dt', GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'paddock.worktreeRoot', GIT_CONFIG_VALUE_0: '/srv/elsewhere/dt/wt' };
    assert.equal(sh(repo, 'paddock_vm_data_pinned', moved).out, '/srv/disk-b/vm-data');
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});

test('the shell copy fails closed with an ::error:: line and status 3', () => {
  const repo = repoWith('');
  try {
    for (const fn of ['paddock_dev_targets', 'paddock_dev_disk', 'paddock_vm_data', 'paddock_vm_data_pinned']) {
      const r = sh(repo, fn);
      assert.equal(r.status, 3, `${fn}: ${r.err}`);
      assert.equal(r.out, '', fn);
      assert.match(r.err, /::error::dev-places: .*paddock\.worktreeRoot/, fn);
    }
    assert.equal(sh(repo, 'paddock_dev_targets', { PADDOCK_DEV_TARGETS: 'rel' }).status, 3);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
});
