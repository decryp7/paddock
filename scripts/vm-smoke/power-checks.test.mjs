import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PowerChecks, TEST_MAC } from './power-checks.mjs';
import { dynoBlocksLinux, dynoHoldsMac, dynoRequestsWindows, parseMagicPacket, parsePmsetAssertions, parsePmsetSched, parsePowercfgRequests, parseSleepStatesWindows, parseSystemdInhibit, parseWakeTimers } from './power-probes.mjs';

// Same-shape text of each tool (invented data: no real machine's output).
const PMSET_HELD = `Assertion status system-wide:
   PreventUserIdleSystemSleep     1
Listed by owning process:
   pid 412(powerd): [0x0001000c000186a0] 00:10:00 PreventUserIdleSystemSleep named: "Powerd - Prevent sleep while display is on"
   pid 1830(dyno-agent): [0x0002a1b3000189c1] 00:00:12 PreventUserIdleSystemSleep named: "Dyno keep awake"
\tTimeout will fire in 3587 secs Action=TimeoutActionRelease
`;
const PMSET_FREE = `Assertion status system-wide:
   PreventUserIdleSystemSleep     0
Listed by owning process:
   pid 412(powerd): [0x0001000c000186a0] 00:10:00 PreventUserIdleDisplaySleep named: "Powerd - Prevent sleep while display is on"
`;
const SCHED = (at) => `Repeating power events:\nScheduled power events:\n [0]  wake at ${at} by 'net.decryptology.dyno.vm-check'\n [1]  wake at 12/01/2026 03:00:00 by 'com.apple.alarm.user-visible-foo'\n`;
const WIN_REQ = `DISPLAY:\nNone.\n\nSYSTEM:\n[PROCESS] \\Device\\HarddiskVolume3\\Users\\paddock-test\\AppData\\Local\\Dyno\\dyno-agent.exe\nDyno keep awake\n\nAWAYMODE:\nNone.\n`;
const INHIBIT = `WHO         UID  USER PID  COMM         WHAT  WHY             MODE\ndyno-agent  1000 test 2210 dyno-agent   sleep Dyno keep awake block\nGNOME Shell 1000 test 1100 gnome-shell  sleep GNOME needs it  delay\n\n2 inhibitors listed.\n`;
const mac = TEST_MAC.split(':').map((h) => h.toLowerCase());
const hex = (bytes) => bytes.reduce((rows, b, i) => { if (i % 16 === 0) rows.push([]); rows.at(-1).push(b.toString(16).padStart(2, '0')); return rows; }, []).map((r, i) => `\t0x${(i * 16).toString(16).padStart(4, '0')}:  ${r.join('').replace(/(.{4})/g, '$1 ').trim()}`).join('\n');
const packet = (m, port = 9) => {
  const mb = m.map((h) => parseInt(h, 16));
  const eth = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f, 0x08, 0x00];
  const ip = [0x45, 0, 0, 130, 0, 0, 0, 0, 64, 17, 0, 0, 192, 168, 64, 5, 255, 255, 255, 255];
  const udp = [0xd4, 0x31, 0, port, 0, 110, 0, 0];
  const body = [...Array(6).fill(0xff), ...Array(16).fill(mb).flat()];
  return `12:00:00.000000 IP 192.168.64.5.54321 > 255.255.255.255.${port}: UDP, length 102\n${hex([...eth, ...ip, ...udp, ...body])}\n`;
};

test('pmset assertions: Dyno holds, powerd alone does not', () => {
  assert.equal(dynoHoldsMac(parsePmsetAssertions(PMSET_HELD)).length, 1);
  assert.equal(dynoHoldsMac(parsePmsetAssertions(PMSET_FREE)).length, 0);
});
test('pmset sched keeps the owner and time', () => {
  const e = parsePmsetSched(SCHED('11/04/2026 06:10:00'));
  assert.deepEqual(e.map((x) => x.owner), ['net.decryptology.dyno.vm-check', 'com.apple.alarm.user-visible-foo']);
  assert.equal(e[0].at.hour, 6);
});
test('powercfg requests, wake timers, sleep states', () => {
  assert.equal(dynoRequestsWindows(parsePowercfgRequests(WIN_REQ)).length, 1);
  assert.equal(dynoRequestsWindows(parsePowercfgRequests('DISPLAY:\nNone.\n\nSYSTEM:\nNone.\n')).length, 0);
  assert.equal(parseWakeTimers('There are no active wake timers in the system.').length, 0);
  const t = parseWakeTimers('Timer set by [PROCESS] \\Device\\X\\dyno-agent.exe expires at 6:10:00 AM on 11/4/2026.\n  Reason: Dyno wake\n');
  assert.equal(t.length, 1);
  assert.equal(t[0].reason, 'Dyno wake');
  assert.deepEqual(parseSleepStatesWindows('The following sleep states are available on this system:\n    Standby (S0 Low Power Idle)\n\nThe following sleep states are not available on this system:\n    Hibernate\n'), ['Standby (S0 Low Power Idle)']);
  assert.deepEqual(parseSleepStatesWindows('The following sleep states are not available on this system:\n    Standby (S3)\n'), []);
});
test('systemd-inhibit: only a blocking Dyno row counts', () => {
  assert.equal(dynoBlocksLinux(parseSystemdInhibit(INHIBIT)).length, 1);
  assert.equal(dynoBlocksLinux(parseSystemdInhibit(INHIBIT.replace('block', 'delay'))).length, 0);
});
test('magic packet: mac, port, and a wrong payload', () => {
  const p = parseMagicPacket(packet(mac));
  assert.deepEqual([p.magic, p.mac, p.dstPort, p.dstAddress], [true, TEST_MAC, 9, '255.255.255.255']);
  const bad = parseMagicPacket(packet(mac).replace(/ffff ffff ffff 0a1b/, 'ffff ffff ffff 0a1b'.replace('ffff ffff ffff', 'ffff ffff fffe')));
  assert.equal(bad.dstPort, 9);
  assert.equal(parseMagicPacket('12:00:00 IP a.1 > b.9: UDP, length 3\n\t0x0000:  0102 03\n').magic, false);
});

/** A fake macOS guest with Dyno's CLI and pmset/tcpdump state; `bug` seeds a fault. */
function fakeGuest({ bug = null, helper = true } = {}) {
  let clock = 0, held = false, wake = null, dump = '';
  const ok = (stdout = '') => ({ code: 0, stdout, stderr: '' });
  const exec = async (cmd) => {
    if (cmd.includes('/dyno\' awake on')) { if (bug !== 'never-held') held = true; return ok('Keeping awake for 1 hour'); }
    if (cmd.includes('/dyno\' awake status')) return ok(held ? 'awake' : 'not holding');
    if (cmd.includes('/dyno\' awake off')) { if (bug !== 'not-released') held = false; return ok('released'); }
    if (cmd === 'pmset -g assertions') return ok(held ? PMSET_HELD : PMSET_FREE);
    if (cmd.startsWith('date -d')) return { code: 0, stdout: '06:10\n', stderr: '' };
    if (cmd.startsWith('date \'+%Y')) return ok('2026 11 04 06 05\n');
    if (cmd.includes('wake-schedule add')) { if (!helper) return { code: 1, stdout: 'Scheduled wake needs the helper. Turn on the helper first.', stderr: '' }; wake = '11/04/2026 06:10:00'; return ok('added'); }
    if (cmd.includes('wake-schedule cancel')) { if (bug !== 'wake-stays') wake = null; return ok('cancelled'); }
    if (cmd === 'pmset -g sched') return ok(wake ? SCHED(wake) : 'Repeating power events:\nScheduled power events:\n [0]  wake at 12/01/2026 03:00:00 by \'com.apple.alarm.user-visible-foo\'\n');
    if (cmd.startsWith('route -n get')) return ok('en0\n');
    if (cmd.startsWith('sudo sh -c') && cmd.includes('tcpdump')) { dump = 'tcpdump: listening on en0, link-type EN10MB\n'; return ok('4242\n'); }
    if (cmd.includes('grep -q \'listening on\'')) return dump.includes('listening on') ? ok() : { code: 1, stdout: '', stderr: '' };
    if (cmd.includes('/dyno\' wake ')) { dump += packet(bug === 'wrong-mac' ? mac.map((h, i) => (i === 5 ? 'ef' : h)) : mac, bug === 'wrong-port' ? 4000 : 9); return ok('sent'); }
    if (cmd.startsWith('cat /tmp/vmsmoke-wol.txt')) return ok(dump);
    return ok();
  };
  return { exec, now: () => clock, sleep: async (ms) => { clock += ms; } };
}
const power = (o) => new PowerChecks(fakeGuest(o), 'macos', { bundle: '/Applications/Dyno.app' });
const status = async (o, step) => (await power(o)[step]()).status;

test('keep awake: pass, never held, not released', async () => {
  assert.equal(await status({}, 'keepAwake'), 'pass');
  assert.equal(await status({ bug: 'never-held' }, 'keepAwake'), 'fail');
  assert.equal(await status({ bug: 'not-released' }, 'keepAwake'), 'fail');
});
test('scheduled wake: pass, left behind, needs the helper (skip, not pass)', async () => {
  const r = await power({}).scheduledWake();
  assert.equal(r.status, 'pass');
  assert.ok(r.checks.some((c) => /5 min ahead|4 min ahead|\d+ min ahead/.test(c.detail)));
  assert.equal(await status({ bug: 'wake-stays' }, 'scheduledWake'), 'fail');
  assert.equal(await status({ helper: false }, 'scheduledWake'), 'skip');
});
test('wake-on-lan: pass, wrong mac, wrong port; Windows has no capture wired and says so', async () => {
  const r = await power({}).wakeOnLan();
  assert.equal(r.status, 'pass');
  assert.equal(await status({ bug: 'wrong-mac' }, 'wakeOnLan'), 'fail');
  assert.equal(await status({ bug: 'wrong-port' }, 'wakeOnLan'), 'fail');
  assert.equal((await new PowerChecks(fakeGuest(), 'windows', { bundle: 'C:\\x' }).wakeOnLan()).status, 'skip');
});
