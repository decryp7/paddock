import assert from 'node:assert/strict';
import { test } from 'node:test';
import { BrokerError, Lease, status, vm } from './broker.mjs';

const fake = (replies) => {
  const calls = [];
  const spawn = (bin, args) => {
    calls.push(args.join(' '));
    const r = replies.shift() ?? { status: 0, stdout: '{"ok":true}' };
    return { status: r.status ?? 0, stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
  };
  return { calls, spawn };
};
const repo = '/repo';

test('lease takes the id from the broker and passes this process as the holder', () => {
  const f = fake([{ stdout: '{"ok":true,"lease":"win.1.2"}' }]);
  const l = Lease.take(repo, 'win', 'dyno run', { spawn: f.spawn, waitSec: 30 });
  assert.equal(l.id, 'win.1.2');
  assert.match(f.calls[0], new RegExp(`^--json vm lease win --for dyno run --holder-pid ${process.pid} --wait 30$`));
});

test('a refused lease is could-not-run with the holder named, other failures are not', () => {
  const f = fake([{ status: 4, stdout: '{"ok":false,"error":"the VM is leased to \\"x\\"","hint":"wait","exit":4,"kind":"refused"}' }, { status: 1, stdout: '', stderr: 'boom\n' }]);
  assert.throws(() => Lease.take(repo, 'win', 'p', { spawn: f.spawn }), (e) => e instanceof BrokerError && e.couldNotRun && /leased to "x"/.test(e.message) && /wait/.test(e.message));
  assert.throws(() => vm(repo, ['status', 'win'], { spawn: f.spawn }), (e) => !e.couldNotRun && /boom/.test(e.message));
});

test('commands carry the lease id and the right words; a password never appears', () => {
  const f = fake([]);
  const l = new Lease(repo, 'win.1.2', 'win', { spawn: f.spawn });
  l.start({ args: ['--vnc-experimental', '--rosetta=rosetta'], maxLoad: 20 });
  l.stop({ graceSec: 5 });
  l.revert('gui-session-tools');
  l.vncOn({ port: 5995, passwordEnv: 'PADDOCK_VNC_PW' });
  l.vncOff();
  l.set('sata0:0.present', 'FALSE');
  assert.deepEqual(f.calls, [
    '--json vm start win.1.2 --arg --vnc-experimental --arg --rosetta=rosetta --max-load 20',
    '--json vm stop win.1.2 --grace 5',
    '--json vm revert win.1.2 --to gui-session-tools',
    '--json vm vnc win.1.2 --port 5995 --password-env PADDOCK_VNC_PW',
    '--json vm vnc win.1.2 --off',
    '--json vm set win.1.2 --key sata0:0.present --value FALSE',
  ]);
});

test('release never throws, status needs no lease, and the stamp names lease, vm, snapshot and command', () => {
  const f = fake([{ status: 1, stderr: 'x' }, { stdout: '{"ok":true,"running":false}' }]);
  const l = new Lease(repo, 'win.1.2', 'win', { spawn: f.spawn });
  assert.equal(l.release(), null);
  assert.equal(status(repo, 'win', { spawn: f.spawn }).running, false);
  assert.deepEqual(l.stamp({ snapshot: 's', command: 'c' }), { lease: 'win.1.2', vm: 'win', snapshot: 's', command: 'c' });
});

test('a missing binary is a clear failure', () => {
  const spawn = () => ({ error: new Error('ENOENT') });
  assert.throws(() => vm(repo, ['list'], { spawn }), /could not run .*dev: ENOENT/);
});
