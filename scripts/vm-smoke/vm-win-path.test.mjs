import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { VM_WIN_IN_REPO, vmWinPath } from './vm-win-path.mjs';

test('the default driver is the repo copy next to scripts/vm-smoke, and it exists', () => {
  assert.equal(basename(VM_WIN_IN_REPO), 'vm-win.sh');
  assert.equal(basename(dirname(VM_WIN_IN_REPO)), 'scripts');
  assert.ok(existsSync(VM_WIN_IN_REPO), `${VM_WIN_IN_REPO} is missing`);
  assert.equal(vmWinPath({}), VM_WIN_IN_REPO);
});

test('PADDOCK_VM_WIN is for tests only: a fake driver under $TMPDIR; anything else is refused; an empty value is unset', () => {
  const dir = mkdtempSync(join(tmpdir(), 'vm-win-path-'));
  const fake = join(dir, 'fake-vm-win.sh');
  writeFileSync(fake, '#!/bin/sh\n');
  try {
    assert.equal(vmWinPath({ PADDOCK_VM_WIN: fake }), realpathSync(fake));
    assert.equal(vmWinPath({ PADDOCK_VM_WIN: '' }), VM_WIN_IN_REPO);
    assert.throws(() => vmWinPath({ PADDOCK_VM_WIN: '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh' }), /PADDOCK_VM_WIN is for tests only/);
    assert.throws(() => vmWinPath({ PADDOCK_VM_WIN: '/usr/bin/true' }), /PADDOCK_VM_WIN is for tests only/);
    assert.throws(() => vmWinPath({ PADDOCK_VM_WIN: join(dir, 'missing.sh') }), /PADDOCK_VM_WIN is for tests only/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('no harness module keeps a driver path outside the repo', async () => {
  const { readdirSync, readFileSync } = await import('node:fs');
  const here = dirname(new URL(import.meta.url).pathname);
  const bad = readdirSync(here).filter((f) => /\.(mjs|sh|ps1)$/.test(f) && !f.endsWith('.test.mjs')).filter((f) => /dev-targets\/scratch\/[^'"`\s]*vm-win\.sh/.test(readFileSync(`${here}/${f}`, 'utf8')));
  assert.deepEqual(bad, []);
});
