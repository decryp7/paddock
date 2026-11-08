import assert from 'node:assert/strict';
import { test } from 'node:test';
import { judgeColdUninstall, coldUninstallAccepted, rightClicksInLog, judgeTrayClick, judgeInstallOutcome, attemptProbe, classifyAttempt, judgeAutostart, judgeControl, judgeExploitPrecondition, judgeRefusal, judgeRestartService, judgeServed, parseServed, judgeTrayCount, judgeUninstall, judgeWear, parseProcs, parseTrayDiagnosis, serviceState, triState } from "./win-judges.mjs";
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

test('dead state: "no install button" without smartctl.exe, a timeout or a failure is not a passing install', () => {
  assert.equal(judgeInstallOutcome('first', 'no install button', false).ok, false);
  assert.equal(judgeInstallOutcome('first', 'no install button', false).kind, 'check');
  assert.equal(judgeInstallOutcome('first', 'timeout', true).kind, 'check');
  assert.equal(judgeInstallOutcome('first', 'failed', true).ok, false);
  assert.equal(judgeInstallOutcome('first', 'failed', true).kind, undefined);
  assert.equal(judgeInstallOutcome('first', 'finished', false).ok, false);
  assert.equal(judgeInstallOutcome('first', 'finished', true).ok, true);
  assert.equal(judgeInstallOutcome('second', 'no install button', true).ok, true);
});
test('a tray count with Dyno not running is information, never a pass', () => {
  const r = judgeTrayCount({ metrics: ['CPU'], visible: 0, overflow: 0, running: false });
  assert.equal(r[0].info, true);
  assert.equal(makeResult('t', r).status, 'not-measured');
});

const REAL = '1793827772 INFO Helper: served: Info=103 Processes=85 Traffic=1 SocketOwners=0 FileHolders=0 LoginItems=0 Counters=0 MemoryModules=0 Smart=2 InstallTool=1 Firewall=0 Extras=1 Audit=93 LogTail=0 Uninstall=0 ScheduleWake=0 CancelWake=0 ListWakes=0';
test('the real served line (result of a 7534 run) is read per kind', () => {
  assert.equal(parseServed([REAL]).get('processes'), 85);
  assert.equal(bad(judgeServed([REAL], ['Processes', 'Traffic', 'Smart', 'Extras'])).length, 0);
  assert.equal(bad(judgeServed([REAL], ['Processes', 'Firewall'])).length, 1, 'Firewall=0 was not served');
  assert.equal(bad(judgeServed([REAL.replace('Smart=2', 'Smart=0')], ['Smart'])).length, 1);
});

test('dead state: a failed or half install does not pass; a planted leftover still there fails', async () => {
  const { judgeInstalled, judgeKilledInstall } = await import('./win-judges.mjs');
  assert.ok(bad(judgeInstalled('again', { exit: 2, app: 'no', key: 'no' })).length >= 2);
  assert.equal(bad(judgeInstalled('again', { exit: 0, app: 'yes', key: 'yes', uninstallIsPlanted: 'no' })).length, 0);
  assert.equal(bad(judgeInstalled('over', { exit: 0, app: 'yes', key: 'yes', uninstallIsPlanted: 'yes' })).length, 1);
  assert.equal(judgeInstalled('x', { exit: null, app: 'unreadable', key: 'yes' })[0].kind, 'check');
  assert.equal(judgeKilledInstall({ killed: false, key: 'no', app: 'no' })[0].kind, 'check');
  assert.equal(judgeKilledInstall({ killed: true, key: 'yes', app: 'yes' })[0].kind, 'check');
  assert.equal(bad(judgeKilledInstall({ killed: true, key: 'no', app: 'no' })).length, 0);
  assert.equal(bad(judgeKilledInstall({ killed: true, key: 'yes', app: 'no' })).length, 1);
});

test('dead state: a right click with no log line, or a log line with no menu, is a broken probe, never a pass and never a product fail', () => {
  assert.equal(rightClicksInLog('a tray click: Right Up b tray click: Right Up'), 2);
  const none = judgeTrayClick({ logBefore: 1, logAfter: 1, menuItems: 3 });
  assert.equal(none.length, 1);
  assert.equal(none[0].kind, 'check');
  const blind = judgeTrayClick({ logBefore: 1, logAfter: 2, menuItems: 0 });
  assert.equal(blind.find((c) => !c.ok).kind, 'check');
  assert.equal(bad(judgeTrayClick({ logBefore: 1, logAfter: 2, menuItems: 4 })).length, 0);
});

test('first uninstall after boot: gone passes; a leftover passes only as the noted fallback with a working reinstall', () => {
  assert.equal(coldUninstallAccepted({ folder: 'no' }), true);
  assert.equal(coldUninstallAccepted({ folder: 'yes', onlyUninstallExe: true, noteLogged: true, reinstallOk: true }), true);
  assert.equal(coldUninstallAccepted({ folder: 'yes', onlyUninstallExe: true, noteLogged: false, reinstallOk: true }), false);
  assert.equal(coldUninstallAccepted({ folder: 'yes', onlyUninstallExe: false, noteLogged: true, reinstallOk: true }), false);
  assert.equal(judgeColdUninstall({ folder: 'unreadable' })[0].kind, 'check');
  assert.equal(judgeColdUninstall({ folder: 'yes', onlyUninstallExe: true, noteLogged: true, reinstallOk: null }).at(-1).kind, 'check');
  assert.ok(bad(judgeColdUninstall({ folder: 'yes', onlyUninstallExe: true, noteLogged: true, reinstallOk: true })).length === 1, 'the leftover itself is still reported');
});

test('Arm guest fallbacks: an unreadable notification area says what was read instead and stays not measured; the boot-time script lists its sources', async () => {
  const c = judgeTrayCount({ metrics: ['CPU', 'Memory'], visible: 0, overflow: 0, measured: false, registered: 2 })[0];
  assert.equal(c.ok, false);
  assert.equal(c.kind, 'check');
  assert.match(c.detail, /^not measured: .*2 icon\(s\) registered with the shell/);
  const { parseBootTime, BOOT_TIME_PS } = await import('./win-reboot.mjs');
  assert.deepEqual(parseBootTime('2026-11-07T12:00:00|wmic\r\n'), { time: '2026-11-07T12:00:00', source: 'wmic' });
  assert.equal(parseBootTime(''), null);
  for (const s of ['Get-CimInstance', 'Get-WmiObject', 'wmic os get lastbootuptime', 'System Up Time', 'systeminfo']) assert.ok(BOOT_TIME_PS.includes(s), s);
});

// 2026-11-08: two restart rounds saw no dyno-app after the boot with the Run value in place. A guest that started NO Run-key program at that logon (the control: OneDrive,
// Edge's auto-launch) did not process the Run key at all: that is the guest, a broken check. A disabled StartupApproved entry is the product's (or the user's) state: a fail.
test('autostart: no Run-key program started at all is a broken check; others started but not Dyno is a product fail', () => {
  const B = '2026-11-08T04:43:31';
  const none = judgeAutostart({ boot: B, procs: [], expectOn: true, control: { othersStarted: [], approved: 'enabled' } });
  assert.ok(none.filter((c) => !c.ok).every((c) => c.kind === 'check'), JSON.stringify(none));
  const fail = judgeAutostart({ boot: B, procs: [], expectOn: true, control: { othersStarted: ['OneDrive'], approved: 'enabled' } });
  assert.ok(fail.some((c) => !c.ok && c.kind !== 'check'));
  const off = judgeAutostart({ boot: B, procs: [], expectOn: true, control: { othersStarted: [], approved: 'disabled' } });
  assert.ok(off.some((c) => !c.ok && c.kind !== 'check' && /StartupApproved/.test(c.detail)));
});

// 2026-11-08: UI Automation reads no notification-area button on this guest after a restart. The second probe is the app's own Shell_NotifyIcon state: its
// "tray diagnose" lines after the boot ("N of M icon(s) shown", "shell holds the icon true" per icon), which come from the shell's answer, not from the app's wish.
test('tray after restart: the app\'s own Shell_NotifyIcon diagnosis measures the row when UI Automation cannot', () => {
  const log = [
    '2026-11-08T08:18:55.516Z tray diagnose: icon dyno-number shown; shell holds the icon true; TaskbarCreated seen 0 time(s)',
    '2026-11-08T08:18:55.516Z tray diagnose: icon dyno-number-2 shown; shell holds the icon true; TaskbarCreated seen 0 time(s)',
    '2026-11-08T08:18:55.516Z tray diagnose: 2 of 2 icon(s) shown 5 s after they were made; taskbar window owned by pid 856',
  ];
  const ok = judgeTrayCount({ metrics: ['CPU', 'Memory'], visible: 0, overflow: 0, measured: false, registered: 2, diagnosis: parseTrayDiagnosis(log, '2026-11-08T08:18:28') });
  assert.ok(ok.every((c) => c.ok), JSON.stringify(ok));
  const bad = judgeTrayCount({ metrics: ['CPU', 'Memory'], visible: 0, overflow: 0, measured: false, registered: 2, diagnosis: parseTrayDiagnosis([log[0], log[1].replace('true', 'false'), '2026-11-08T08:18:55Z tray diagnose: 1 of 2 icon(s) shown'], '2026-11-08T08:18:28') });
  assert.ok(bad.some((c) => !c.ok && c.kind !== 'check'));
  // lines from before the boot do not count: no diagnosis after the boot stays not measured
  const old = judgeTrayCount({ metrics: ['CPU'], visible: 0, overflow: 0, measured: false, registered: 1, diagnosis: parseTrayDiagnosis(log, '2026-11-08T09:00:00') });
  assert.ok(old.every((c) => c.ok || c.kind === 'check'));
});
