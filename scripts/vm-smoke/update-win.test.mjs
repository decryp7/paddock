import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertTestVm } from './update-win.mjs';

const script = (name) => `#!/usr/bin/env bash\nset -euo pipefail\ndir=\${PADDOCK_WIN_VM_DIR:-/x/${name}}\nname=${name}\nvmx="$dir/vm/$name.vmwarevm/$name.vmx"\n`;

test('assertTestVm accepts the script of the dedicated test VM', () => {
  assertTestVm(script('paddock-win11-test'), {});
  assertTestVm(script('paddock-win11-test'), { PADDOCK_WIN_VM_DIR: '/Volumes/d7sst7/vms/paddock-win11-test/' });
});

test('assertTestVm refuses a script that drives any other VM (ER-14: never the personal Windows VM)', () => {
  assert.throws(() => assertTestVm(script('personal-win11'), {}), /does not target the test VM/);
  assert.throws(() => assertTestVm('echo hi\n', {}), /does not target the test VM/);
});

test('assertTestVm refuses a PADDOCK_WIN_VM_DIR that points elsewhere', () => {
  assert.throws(() => assertTestVm(script('paddock-win11-test'), { PADDOCK_WIN_VM_DIR: '/Users/me/Virtual Machines' }), /not the paddock-win11-test folder/);
});
