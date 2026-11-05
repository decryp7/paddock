import assert from 'node:assert/strict';
import { test } from 'node:test';
import { attemptProbe, classifyAttempt, judgeAutostart, judgeControl, judgeExploitPrecondition, judgeRefusal, judgeRestartService, judgeServed, judgeTrayCount, judgeUninstall, judgeWear, parseProcs, serviceState, triState } from './win-judges.mjs';
import { makeResult } from './verdict.mjs';

const BOOT = '2026-11-05T10:00:00';
const bad = (checks) => checks.filter((c) => !c.ok);

test('dead state: dyno-agent alone running is NOT an autostart (the user\'s exact report)', () => {
  const procs = parseProcs('dyno-agent pid=300 started=2026-11-05T10:00:20 cmd=dyno-agent.exe run');
  const r = judgeAutostart({ boot: BOOT, procs, expectOn: true });
  assert.ok(bad(r).length >= 1);
  assert.match(r[0].detail, /only dyno-agent pid 300 is running/);
});
test('dead state: dyno-app started BEFORE the boot (a hand start survived) or without --hidden does not pass', () => {
  const old = parseProcs('dyno-app pid=1 started=2026-11-05T09:00:00 cmd=dyno-app.exe --hidden');
  assert.ok(bad(judgeAutostart({ boot: BOOT, procs: old, expectOn: true })).length);
  const visible = parseProcs('dyno-app pid=1 started=2026-11-05T10:00:30 cmd="C:\\Program Files\\Dyno\\dyno-app.exe"');
  assert.match(bad(judgeAutostart({ boot: BOOT, procs: visible, expectOn: true }))[0].detail, /without --hidden/);
});
test('autostart passes with dyno-app started after the boot with --hidden; an unreadable boot time is a broken check', () => {
  const procs = parseProcs('dyno-app pid=1 started=2026-11-05T10:00:30 cmd="C:\\Program Files\\Dyno\\dyno-app.exe" --hidden\ndyno-agent pid=2 started=2026-11-05T10:00:31 cmd=x');
  assert.equal(bad(judgeAutostart({ boot: BOOT, procs, expectOn: true })).length, 0);
  assert.equal(judgeAutostart({ boot: null, procs, expectOn: true })[0].kind, 'check');
  assert.equal(bad(judgeAutostart({ boot: BOOT, procs: [], expectOn: false })).length, 0);
  assert.equal(bad(judgeAutostart({ boot: BOOT, procs, expectOn: false })).length, 1);
});
test('dead state: a STOPPED service, an unmoved boot time, or a stop after it ran does not pass', () => {
  const run = (s, t) => ({ t, state: s });
  assert.ok(bad(judgeRestartService({ bootBefore: BOOT, bootAfter: '2026-11-05T10:05:00', samples: [run('STOPPED', 3), run('STOPPED', 60)] })).length);
  assert.ok(bad(judgeRestartService({ bootBefore: BOOT, bootAfter: BOOT, samples: [run('RUNNING', 3)] })).length, 'not a restart');
  assert.ok(bad(judgeRestartService({ bootBefore: BOOT, bootAfter: '2026-11-05T10:05:00', samples: [run('RUNNING', 3), run('STOPPED', 9), run('RUNNING', 15)] })).length, 'race');
  assert.ok(bad(judgeRestartService({ bootBefore: BOOT, bootAfter: '2026-11-05T10:05:00', samples: [{ t: 0, state: 'unreachable' }] })).length, 'no samples');
  assert.equal(bad(judgeRestartService({ bootBefore: BOOT, bootAfter: '2026-11-05T10:05:00', samples: [{ t: 0, state: 'unreachable' }, run('START_PENDING', 6), run('RUNNING', 12), run('RUNNING', 40)] })).length, 0, 'a pending sample before the first RUNNING is not a race');
});
test('serviceState reads sc query text; empty is unreachable, no STATE is absent', () => {
  assert.equal(serviceState('SERVICE_NAME: DynoHelper\n        STATE              : 4  RUNNING'), 'RUNNING');
  assert.equal(serviceState(''), 'unreachable');
  assert.equal(serviceState('[SC] EnumQueryServicesStatus:OpenService FAILED 1060'), 'absent');
});
test('dead state: tray with metrics 0, unmeasured, or the wrong number never passes; pass and fail have different text', () => {
  assert.equal(judgeTrayCount({ metrics: [], visible: 0, overflow: 0 })[0].kind, 'check');
  assert.equal(judgeTrayCount({ metrics: ['CPU'], visible: 0, overflow: 0, measured: false })[0].kind, 'check');
  const fail = judgeTrayCount({ metrics: ['CPU', 'Memory'], visible: 1, overflow: 0 })[0];
  const pass = judgeTrayCount({ metrics: ['CPU', 'Memory'], visible: 1, overflow: 1 })[0];
  assert.equal(fail.ok, false);
  assert.equal(pass.ok, true);
  assert.notEqual(fail.detail, pass.detail);
  assert.doesNotMatch(fail.detail, /and its icons exist/);
});
test('dead state: a path that is not there, or a failing control, is not a refusal', () => {
  assert.equal(judgeRefusal('rename', 'OTHER:ItemNotFoundException').ok, false);
  assert.equal(judgeRefusal('rename', 'OTHER:ItemNotFoundException').kind, 'check');
  assert.equal(judgeRefusal('rename', '').kind, 'check');
  assert.equal(judgeRefusal('rename', 'OK').ok, false);
  assert.equal(judgeRefusal('rename', 'OK').kind, undefined);
  assert.equal(judgeRefusal('rename', 'DENIED').ok, true);
  assert.equal(judgeControl('OK').ok, true);
  assert.equal(judgeControl('DENIED').ok, false);
  assert.equal(classifyAttempt('x\nDENIED\n'), 'refused');
  assert.match(attemptProbe('Set-Content a b'), /UnauthorizedAccessException/);
});
test('dead state: exploit step without a finished install or smartctl.exe is not measured', () => {
  assert.ok(bad(judgeExploitPrecondition({ outcome: 'timeout', smartctlExists: false })).length);
  assert.ok(bad(judgeExploitPrecondition({ outcome: 'finished', smartctlExists: false })).length);
  assert.equal(bad(judgeExploitPrecondition({ outcome: 'finished', smartctlExists: true })).length, 0);
  const r = makeResult('x', judgeExploitPrecondition({ outcome: 'timeout', smartctlExists: false }).map((c) => ({ ...c, kind: 'check' })));
  assert.equal(r.status, 'check-broken');
});
test('dead state: an uninstall read over a dead SSH never reads as removed', () => {
  const dead = { folder: 'unreadable', uninstallKey: 'unreadable', service: 'unreadable', processes: 'unreadable', runMachine: 'unreadable', runUser: 'unreadable' };
  const r = judgeUninstall(dead);
  assert.equal(bad(r).length, 6);
  assert.ok(r.every((c) => c.kind === 'check'));
  assert.equal(makeResult('u', r).status, 'check-broken');
  const clean = { folder: 'no', uninstallKey: 'no', service: 'no', processes: 0, runMachine: 'no', runUser: 'no' };
  assert.equal(bad(judgeUninstall(clean)).length, 0);
  assert.equal(bad(judgeUninstall({ ...clean, folder: 'yes', folderFiles: 'dyno-app.exe', runUser: 'yes', runUserNames: 'Dyno' })).length, 2);
  assert.equal(bad(judgeUninstall({ ...clean, processes: 1 })).length, 1, 'a leftover dyno-bench counts');
});
test('triState', () => { assert.deepEqual(['True', 'False', '', 'garbage'].map(triState), ['yes', 'no', 'unreadable', 'unreadable']); });
test('dead state: a served line counts per kind, and a line with every count 0 does not pass', () => {
  assert.ok(bad(judgeServed([], ['processes'])).length);
  assert.ok(bad(judgeServed(['served: processes=0, firewall=0'], ['processes', 'firewall'])).length === 2);
  assert.ok(bad(judgeServed(['2026 served: processes=12'], ['processes', 'firewall'])).length === 1);
  assert.equal(bad(judgeServed(['served: processes=12, firewall=1'], ['processes', 'firewall'])).length, 0);
  assert.equal(judgeServed(['served: lots'], ['processes'])[0].kind, 'check');
});
test('dead state: disk health needs a real wear value, not "Installed" and a heading', () => {
  assert.ok(bad(judgeWear('Lifespan How long your SSD and battery are likely to last.')).length);
  assert.ok(bad(judgeWear('Lifespan SSD Disk life cannot be estimated')).length);
  assert.ok(bad(judgeWear('Lifespan smartctl is not installed')).length);
  assert.equal(bad(judgeWear('Lifespan SSD About 7.4 years of writes left (5.2 to 10 years)')).length, 0);
});
