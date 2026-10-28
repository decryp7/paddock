import assert from 'node:assert/strict';
import { test } from 'node:test';
import { run, scratch, shim } from './helpers.mjs';

test('vm-smoke.sh: a bad guest is a usage error', () => {
  const res = run('vm-smoke.sh', ['windows'], { bin: scratch('paddock-bin-') });
  assert.equal(res.status, 2, res.out);
  assert.match(res.out, /usage/);
});

test('vm-smoke.sh: a missing tart or an unmounted disk fails fast with a message', { skip: process.platform !== 'darwin' }, () => {
  const bin = scratch('paddock-bin-');
  let res = run('vm-smoke.sh', ['macos'], { bin, env: { TART: 'no-such-tart' } });
  assert.equal(res.status, 1, res.out);
  assert.match(res.out, /tart is not installed/);
  shim(bin, 'tart');
  res = run('vm-smoke.sh', ['macos'], { bin, env: { TART: 'tart', TART_HOME: '/Volumes/paddock-no-such-disk/tart' } });
  assert.equal(res.status, 1, res.out);
  assert.match(res.out, /not mounted/);
});
