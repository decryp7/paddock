import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { basename, dirname } from 'node:path';
import { VM_WIN_IN_REPO, vmWinPath } from './vm-win-path.mjs';

test('the default driver is the repo copy next to scripts/vm-smoke, and it exists', () => {
  assert.equal(basename(VM_WIN_IN_REPO), 'vm-win.sh');
  assert.equal(basename(dirname(VM_WIN_IN_REPO)), 'scripts');
  assert.ok(existsSync(VM_WIN_IN_REPO), `${VM_WIN_IN_REPO} is missing`);
  assert.equal(vmWinPath({}), VM_WIN_IN_REPO);
});

test('PADDOCK_VM_WIN overrides the driver; an empty value does not', () => {
  assert.equal(vmWinPath({ PADDOCK_VM_WIN: '/tmp/fake-vm-win.sh' }), '/tmp/fake-vm-win.sh');
  assert.equal(vmWinPath({ PADDOCK_VM_WIN: '' }), VM_WIN_IN_REPO);
});

test('no harness module keeps a driver path outside the repo', async () => {
  const { readdirSync, readFileSync } = await import('node:fs');
  const here = dirname(new URL(import.meta.url).pathname);
  const bad = readdirSync(here).filter((f) => /\.(mjs|sh|ps1)$/.test(f) && !f.endsWith('.test.mjs')).filter((f) => /dev-targets\/scratch\/[^'"`\s]*vm-win\.sh/.test(readFileSync(`${here}/${f}`, 'utf8')));
  assert.deepEqual(bad, []);
});
