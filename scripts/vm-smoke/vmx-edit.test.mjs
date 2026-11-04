import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixDiskName, stripVnc, withVnc } from './vmx-edit.mjs';

const vmx = 'displayName = "x"\nnvme0:0.fileName = "paddock-win11-test-000011.vmdk"\nmemsize = "8192"\n';
test('adding and stripping the VNC lines leaves every other line alone', () => {
  const on = withVnc(vmx, 5995, 'pw');
  assert.match(on, /RemoteDisplay\.vnc\.enabled = "TRUE"/);
  assert.equal(stripVnc(on).trimEnd(), vmx.trimEnd());
  assert.equal((withVnc(on, 5996, 'q').match(/RemoteDisplay\.vnc\.enabled/g) ?? []).length, 1, 'no duplicate lines');
});
test('a vmx naming a missing redo log is pointed at the newest existing one; an existing name is kept', () => {
  const files = ['paddock-win11-test.vmdk', 'paddock-win11-test-000009.vmdk', 'paddock-win11-test-000010.vmdk'];
  assert.match(fixDiskName(vmx, files), /fileName = "paddock-win11-test-000010\.vmdk"/);
  assert.equal(fixDiskName(vmx, [...files, 'paddock-win11-test-000011.vmdk']), vmx);
  assert.equal(fixDiskName(vmx, ['other.vmdk']), vmx, 'nothing to point at: unchanged');
});
