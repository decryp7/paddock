import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { calls, fakeDev, run, scratch, shim, systemFolder, TOOLS, write } from './helpers.mjs';

/**
 * A fake tart that keeps its VMs in <home>/vms (one file per VM) and records every call. Knobs: FAKE_SMOKE_RC (exit of the
 * guest script), FAKE_BOOT_FAIL (exec never answers), FAKE_SMOKE_HANG (the smoke prints READY and then waits to be killed).
 * `run` blocks until `stop`, like the real one.
 */
function rig({ freeKb = 100 * 1048576, load = '0.5' } = {}) {
  const bin = scratch('paddock-bin-');
  const home = scratch('paddock-tarthome-');
  mkdirSync(join(home, 'vms'));
  shim(bin, 'tart', `vms="${home}/vms"
case "$1" in
  list) ls "$vms" ;;
  pull) ;;
  clone) : > "$vms/$3" ;;
  run) while [ ! -e "$vms/stopped" ]; do sleep 0.05; done ;;
  stop) : > "$vms/stopped" ;;
  delete) rm -f "$vms/$2" ;;
  exec)
    if [ "$3" = true ]; then [ -z "$FAKE_BOOT_FAIL" ]; exit; fi
    echo "READY"
    if [ -n "$FAKE_SMOKE_HANG" ]; then while true; do sleep 0.05; done; fi
    exit "\${FAKE_SMOKE_RC:-0}" ;;
esac`);
  shim(bin, 'df', `echo "Filesystem 1024-blocks Used Available Capacity"; echo "/dev/x 1 1 ${freeKb} 1% /"`);
  fakeDev(bin);
  shim(bin, 'uptime', `echo " 1:11  up 1 day, 1 user, load averages: ${load} 1.0 1.0"`);
  const stage = scratch('paddock-src-');
  write(stage, { 'N/Demo.app/Contents/Info.plist': 'n', 'N1/Demo.app/Contents/Info.plist': 'n1', 'smoke.sh': '#!/bin/bash\n' });
  return { bin, home, stage };
}
const base = (r, extra = {}) => ({
  bin: r.bin,
  cwd: r.home,
  env: { TART: join(r.bin, 'tart'), TART_HOME: r.home, PADDOCK_DEV: join(r.bin, 'dev'), PADDOCK_VM_POLL: '0', PADDOCK_VM_BOOT_TIMEOUT: '5', PADDOCK_VM_RUN_TIMEOUT: '20', ...extra },
});
const go = (r, args, extra) => run('macos-vm-smoke.sh', args, base(r, extra));
const args = (r) => ['--old', join(r.stage, 'N/Demo.app'), '--new', join(r.stage, 'N1/Demo.app'), '--smoke', join(r.stage, 'smoke.sh')];
const left = (r) => readdirSync(join(r.home, "vms")).filter((n) => n.startsWith("paddock-smoke-"));

test('a passing run: pinned image, headless clone with a read-only share, the smoke runs, the VM is stopped and deleted', () => {
  const r = rig();
  const res = go(r, args(r));
  assert.equal(res.status, 0, res.out);
  const log = calls(r.bin);
  assert.match(log.find((l) => l.startsWith('tart pull')), /^tart pull ghcr\.io\/cirruslabs\/macos-tahoe-base@sha256:[0-9a-f]{64}$/);
  assert.match(log.find((l) => l.startsWith('tart clone')), /^tart clone ghcr\.io\/cirruslabs\/macos-tahoe-base@sha256:[0-9a-f]{64} paddock-smoke-macos-/);
  assert.ok(log.some((l) => /^tart run --no-graphics --dir=stage:.*\/stage:ro paddock-smoke-macos-/.test(l)), log.join('\n'));
  assert.ok(/tart exec[^]*PADDOCK_DISPOSABLE_MAC=1/.test(calls(r.bin).join('\n')), 'the smoke runs with the disposable flag');
  assert.ok(log.some((l) => l.startsWith('tart stop')));
  assert.ok(log.some((l) => l.startsWith('tart delete')));
  assert.deepEqual(left(r), []);
  assert.ok(log.some((l) => /^dev ci slot acquire --max 1 --owner \S+ --root .*\/\.paddock-vm$/.test(l)), 'a host-wide VM slot under TART_HOME');
  assert.ok(log.some((l) => /^dev ci slot release --slot s1 --token t1 --root /.test(l)), 'given back');
  assert.ok(!existsSync(join(r.home, '.paddock-vm', 'slots', 's1.owner')), 'the VM slot is released');
});

test('a failing smoke fails the run with its exit code and the VM is still deleted', () => {
  const r = rig();
  const res = go(r, args(r), { FAKE_SMOKE_RC: '7' });
  assert.equal(res.status, 7, res.out);
  assert.deepEqual(left(r), []);
});

test('a VM that never answers fails the run after the boot timeout and is deleted', () => {
  const r = rig();
  const res = go(r, args(r), { FAKE_BOOT_FAIL: '1', PADDOCK_VM_BOOT_TIMEOUT: '1' });
  assert.equal(res.status, 7, res.out);
  assert.deepEqual(left(r), []);
  assert.ok(!calls(r.bin).some((l) => l.startsWith('tart exec') && l.includes('bash')), 'the smoke never ran');
});

test('not enough free disk refuses before anything is pulled or cloned', () => {
  const r = rig({ freeKb: 10 * 1048576 });
  const res = go(r, args(r));
  assert.equal(res.status, 4, res.out);
  assert.match(res.out, /at least 60 GB/);
  assert.ok(!calls(r.bin).some((l) => /^tart (pull|clone|run)/.test(l)));
});

test('a high load is waited for, then refused after the wait', () => {
  const r = rig({ load: '30.2' });
  const res = go(r, args(r), { PADDOCK_VM_LOAD_WAIT: '0' });
  assert.equal(res.status, 5, res.out);
  assert.ok(!calls(r.bin).some((l) => /^tart (pull|clone|run)/.test(l)));
});

test('VMs are serialised host-wide by a VM slot: a held one is waited for and refused after the wait; a dead run\'s is free; at most two', () => {
  const r = rig();
  const slots = join(r.home, '.paddock-vm', 'slots');
  const other = scratch('paddock-vm-other-'); // a live run elsewhere on this Mac holds the slot
  mkdirSync(join(slots, 's1.lock'), { recursive: true });
  writeFileSync(join(slots, 's1.owner'), `token=x\nowner=${other}\n`);
  const busy = go(r, args(r), { PADDOCK_VM_SLOT_WAIT: '1' });
  assert.equal(busy.status, 3, busy.out);
  assert.match(busy.out, /holds this Mac's VM slots; waiting/);
  assert.match(busy.out, /stayed held for 1s/);
  assert.ok(existsSync(join(slots, 's1.lock')), 'another run\'s slot is not touched');
  assert.ok(!calls(r.bin).some((l) => l.startsWith('tart clone')));
  const two = go(r, args(r), { PADDOCK_VM_MAX: '2' });
  assert.equal(two.status, 0, two.out);
  assert.match(two.out, /VM slot s2/, 'a second macOS VM may run beside it');
  assert.equal(go(r, args(r), { PADDOCK_VM_MAX: '3' }).status, 2, 'never three macOS VMs');
  rmSync(other, { recursive: true }); // that run ended without releasing (killed): its holder is gone with its folder
  const ok = go(r, args(r));
  assert.equal(ok.status, 0, ok.out);
});

test('a CI job names its VMs with its own prefix, so its leftover check never sees another job\'s VM', () => {
  const r = rig();
  const res = go(r, args(r), { PADDOCK_VM_PREFIX: 'paddock-smoke-77-1-' });
  assert.equal(res.status, 0, res.out);
  assert.ok(calls(r.bin).some((l) => /^tart clone \S+ paddock-smoke-77-1-macos-/.test(l)));
  assert.equal(go(r, args(r), { PADDOCK_VM_PREFIX: '../x' }).status, 2);
  const wf = readFileSync(join(TOOLS, '..', '.gitea', 'workflows', 'service-smoke.yml'), 'utf8');
  assert.match(wf, /PADDOCK_VM_PREFIX: paddock-smoke-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}-/);
  assert.match(wf, /grep "\^\$PADDOCK_VM_PREFIX"/, 'the leftover check looks only at this job\'s VMs');
  assert.doesNotMatch(wf, /grep '\^paddock-smoke-'/);
});

test('usage errors: missing builds, unknown option, unknown guest, different app names, missing tart', () => {
  const r = rig();
  assert.equal(go(r, []).status, 2);
  assert.equal(go(r, ['--bogus']).status, 2);
  assert.equal(go(r, [...args(r), '--guest', 'windows']).status, 2);
  assert.equal(go(r, ['--old', join(r.stage, 'nope'), '--new', join(r.stage, 'N1/Demo.app'), '--smoke', join(r.stage, 'smoke.sh')]).status, 2);
  write(r.stage, { 'N1/Other.app/x': 'x' });
  assert.equal(go(r, ['--old', join(r.stage, 'N/Demo.app'), '--new', join(r.stage, 'N1/Other.app'), '--smoke', join(r.stage, 'smoke.sh')]).status, 2);
  assert.equal(go(r, args(r), { TART: join(r.bin, 'absent') }).status, 1);
  assert.ok(!calls(r.bin).some((l) => l.startsWith('tart clone')));
});

test('the linux guest uses the pinned ubuntu image, its own VM name and the linux share', () => {
  const r = rig();
  const res = go(r, [...args(r), '--guest', 'linux']);
  assert.equal(res.status, 0, res.out);
  const log = calls(r.bin);
  const clone = log.filter((l) => l.startsWith('tart clone')).at(-1);
  assert.match(clone, /^tart clone ghcr\.io\/cirruslabs\/ubuntu@sha256:[0-9a-f]{64} paddock-smoke-linux-/);
  assert.ok(log.join('\n').includes('/mnt/shared/stage'));
  assert.deepEqual(left(r), []);
});

test('SMOKE variables are forwarded to the guest', () => {
  const r = rig();
  const res = go(r, args(r), { PADDOCK_SMOKE_NEW_VERSION: '9.9.9' });
  assert.equal(res.status, 0, res.out);
  assert.ok(/tart exec[^]*PADDOCK_SMOKE_NEW_VERSION=9\.9\.9/.test(calls(r.bin).join('\n')));
});

test('Ctrl-C / TERM in the middle of the smoke still stops and deletes the VM and releases the lock', async () => {
  const r = rig();
  const o = base(r, { FAKE_SMOKE_HANG: '1' });
  const child = spawn('bash', [join(TOOLS, 'macos-vm-smoke.sh'), ...args(r)], {
    cwd: o.cwd,
    env: { PATH: `${o.bin}:${systemFolder()}`, HOME: o.cwd, TMPDIR: process.env.TMPDIR ?? '/tmp', ...o.env },
  });
  let seen = '';
  const ready = new Promise((resolve) => child.stdout.on('data', (d) => { seen += d; if (seen.includes('READY')) resolve(); }));
  const closed = new Promise((resolve) => child.on('close', (code) => resolve(code)));
  await ready;
  child.kill('SIGTERM');
  const code = await closed;
  assert.equal(code, 130);
  assert.deepEqual(left(r), []);
  assert.ok(!existsSync(join(r.home, '.paddock-vm', 'slots', 's1.owner')), 'the VM slot is given back');
});
