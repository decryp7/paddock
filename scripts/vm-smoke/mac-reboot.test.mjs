import assert from 'node:assert/strict';
import { test } from 'node:test';
import { autosaveNames, controlCenterFacts, loginSwitchMode, trayLogFacts, trayShown } from './mac-reboot.mjs';

const log = [
  '{"@m":"tray icon dyno-number created (metric CPU, autosave name net.decryptology.dyno.menubar.number; on Windows 11 a new icon)","@t":"2026-01-01T00:00:01Z"}',
  '{"@m":"tray icon dyno-number-2 created (metric Memory, autosave name net.decryptology.dyno.menubar.number-2; on Windows 11)","@t":"2026-01-01T00:00:01Z"}',
  '{"@m":"start signal: open event: login item (lgit); sender loginwindow; flag no (UptimeMs 9000)","@t":"2026-01-01T00:00:01Z"}',
  '{"@m":"tray check #0: icon dyno-number is not shown (window visible true, occluded true, frame 0,0 53x0, app active false, policy NSApplicationActivationPolicy(0)); adding it again","@t":"2026-01-01T00:00:02Z"}',
  '{"@m":"tray check #0: 2 wanted, 1 shown, 1 added again, 0 still missing","@t":"2026-01-01T00:00:02Z"}',
  '{"@m":"crash report on record: dyno-app-2026-01-01-000000.ips written 2026-01-01T00:00:00Z (5 s before this start)","@t":"2026-01-01T00:00:03Z"}',
  '{"@m":"tray diagnose: 2 of 2 icon(s) shown 5 s after they were made","@t":"2026-01-01T00:00:07Z"}',
].join('\n');

test('autosave names come from the log, once each', () => {
  assert.deepEqual(autosaveNames(log + '\n' + log), ['net.decryptology.dyno.menubar.number', 'net.decryptology.dyno.menubar.number-2']);
});

test('shown test: only an on-screen Control Center window with a size counts', () => {
  const tsv = [
    'Control Center\t447\t25\tnet.decryptology.dyno.menubar.number\t749,0,53x30',
    'Control Center\t447\t25\tnet.decryptology.dyno.menubar.number-2\t696,0,53x0',
    'Dyno\t440\t0\tDyno\t0,30,1024x680',
    'Control Center\t447\t25\tnet.decryptology.dyno.menubar.number-9\t600,0,40x30',
  ].join('\n');
  const s = trayShown(tsv, autosaveNames(log));
  assert.deepEqual(s.map((r) => [r.name.split('.').pop(), r.shown, Boolean(r.unnamed)]), [['number', true, false], ['number-2', false, false], ['number-9', true, true]]);
  assert.equal(trayShown('', ['net.decryptology.dyno.menubar.number'])[0].shown, false);
});

test('log facts: the start signal, every check and the last summary', () => {
  const f = trayLogFacts(log);
  assert.match(f.startSignal, /login item \(lgit\)/);
  assert.equal(f.checks.length, 2);
  assert.match(f.lastSummary, /2 wanted, 1 shown/);
  assert.equal(trayLogFacts('').startSignal, null);
  // The app's own context lines: the sync summary, the diagnose summary and the crash reports on record.
  assert.equal(f.context.length, 2);
  assert.match(f.context[0], /^crash report on record: dyno-app-/);
  assert.match(f.context[1], /^tray diagnose: 2 of 2/);
});

test('ControlCenter facts count tracked, ephemeral and blocked lines about Dyno only', () => {
  const cc = [
    '2026 Df ControlCenter[447:a65] [com.apple.controlcenter:appStatusItems] Starting to track host; (bid:net.decryptology.dyno-x-445)',
    '2026 Df ControlCenter[447:a65] [com.apple.controlcenter:appStatusItems] Created ephemaral instance DisplayableId(1) for (bid:net.decryptology.dyno-x-445) with positioning .ephemeral',
    '2026 Df ControlCenter[447:a65] [com.apple.controlcenter:appStatusItems] Moving host to blocked list; (bid:net.decryptology.dyno-x-445)',
    '2026 Df ControlCenter[447:a65] [com.apple.controlcenter:appStatusItems] Moving host to blocked list; (bid:com.example.other-0-1)',
  ].join('\n');
  const f = controlCenterFacts(cc);
  assert.equal(f.lines, 3);
  assert.equal(f.tracked, 1);
  assert.equal(f.ephemeral, 1);
  assert.equal(f.blocked.length, 1);
});

test('the login switch mode is press by default, leave on request, and nothing else', () => {
  assert.equal(loginSwitchMode({}), 'press');
  assert.equal(loginSwitchMode({ VM_SMOKE_LOGIN_SWITCH: 'leave' }), 'leave');
  assert.throws(() => loginSwitchMode({ VM_SMOKE_LOGIN_SWITCH: 'on' }), /press \(default\) or leave/);
});
