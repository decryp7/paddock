import test from 'node:test';
import assert from 'node:assert/strict';
import { parseArgs, makeAdapter } from './defender-rate.mjs';

test('arguments: the installer is needed, 10 installs by default, nothing else is accepted', () => {
  assert.equal(parseArgs(['--installer', 'x.exe']).installs, 10);
  assert.equal(parseArgs(['--installer', 'x.exe', '--installs', '3', '--plan']).plan, true);
  assert.throws(() => parseArgs([]), /--installer/);
  assert.throws(() => parseArgs(['--installer', 'x', '--installs', '0']), /1 to 50/);
  assert.throws(() => parseArgs(['--installer', 'x', '--bogus']), /unknown/);
  assert.throws(() => parseArgs(['--installer']), /needs a value/);
});

test('the adapter drives vm-win.sh: revert, start, stage the installer; the install is silent and reports its exit code', async () => {
  const calls = [];
  const run = (args) => {
    calls.push(args);
    const cmd = args.at(-1);
    if (args[0] === 'ssh' && /ToUniversalTime\(\)\.ToString\('o'\)/.test(cmd) && /^\(Get-Date\)/.test(cmd)) return { status: 0, stdout: '2026-11-08T04:00:00.0000000Z\nDONE\n' };
    if (args[0] === 'ssh' && /Start-Process/.test(cmd)) return { status: 0, stdout: 'EXIT=0\nDONE\n' };
    if (args[0] === 'ssh' && /Get-WinEvent/.test(cmd)) return { status: 0, stdout: 'DONE\n' };
    return { status: 0, stdout: '' };
  };
  const g = makeAdapter({ vmwin: 'vm-win.sh', snapshot: 'snap', installer: '/tmp/Dyno_setup.exe', run });
  await g.revert();
  assert.deepEqual(calls.slice(0, 2).map((a) => a.slice(0, 2)), [['revert', 'snap'], ['start']]);
  assert.deepEqual(calls.find((a) => a[0] === 'put'), ['put', 'paddock-admin', '/tmp/Dyno_setup.exe', 'C:\\paddock-test\\vmcheck\\setup.exe']);
  assert.equal(await g.now(), '2026-11-08T04:00:00.0000000Z');
  await g.install(1);
  assert.ok(calls.some((a) => a[0] === 'ssh' && a[1] === '--admin' && /-ArgumentList '\/S'/.test(a[2])));
  assert.deepEqual(await g.readEvents(1, '2026-11-08T04:00:00.0000000Z'), []);
  assert.match(calls.at(-1)[2], /StartTime/, 'events are read since the clock');
});

test('a failing vm-win.sh call stops the run with its reason', async () => {
  const g = makeAdapter({ vmwin: 'vm-win.sh', snapshot: 's', installer: 'i', run: () => ({ status: 2, stdout: '', stderr: 'vm-win: load too high\n' }) });
  await assert.rejects(g.revert(), /revert failed \(exit 2\).*load too high/);
});

test('an install that does not report its exit code is an error, not a no-detection', async () => {
  const g = makeAdapter({ vmwin: 'v', snapshot: 's', installer: 'i', run: () => ({ status: 0, stdout: 'nothing useful' }) });
  await assert.rejects(g.install(1), /did not report an exit code/);
});
