import test from 'node:test';
import assert from 'node:assert/strict';
import { parseEvents, parseFields, originLine, eventLines, fieldOf, NOT_IN_EVENT, mpLogWindow, mpLogPs, mpLogLines, judgeControl, summarizeRate, planRateRun, runRate, installDetected, EICAR, controlPs, eventsPs, detectionTime } from './defender-evidence.mjs';

// Same shape as Defender's 1116 text; invented names and ids.
const msg1116 = (path, extra = '') => `Microsoft Defender Antivirus has detected malware or other potentially unwanted software.
 For more information please see the following:
 https://go.microsoft.com/fwlink/?linkid=37020&name=Trojan:Win32/Example.A!ml&threatid=2147000000
 	Name: Trojan:Win32/Example.A!ml
 	ID: 2147000000
 	Severity: Severe
 	Category: Trojan
 	Path: file:_${path}
 	Detection Origin: Local machine
 	Detection Type: Concrete
 	Detection Source: Real-Time Protection
 	User: NT AUTHORITY\\SYSTEM
 	Process Name: C:\\Windows\\System32\\cmd.exe
 	Signature ID: 99887766
 	Signature Version: AV: 1.463.99.0, AS: 1.463.99.0, NIS: 1.463.99.0${extra}`;
const raw = (...evs) => evs.map((e) => `@@EVENT ${e.t} id=${e.id}\n${e.m}\n@@END`).join('\n') + '\nDONE';

test('the full 1116 text is kept, beyond 400 characters, with the five fields', () => {
  const ev = parseEvents(raw({ t: '2026-11-08T04:03:50.0000000Z', id: 1116, m: msg1116('C:\\Program Files\\Dyno\\paddock-setup.exe') }));
  assert.equal(ev.length, 1);
  assert.ok(ev[0].text.length > 400);
  assert.match(ev[0].text, /Signature Version/);
  assert.equal(fieldOf(ev[0], 'Detection Origin'), 'Local machine');
  assert.equal(fieldOf(ev[0], 'Detection Source'), 'Real-Time Protection');
  assert.equal(fieldOf(ev[0], 'Signature ID'), '99887766');
  assert.equal(originLine(ev), 'Defender: origin Local machine / type Concrete / source Real-Time Protection / process C:\\Windows\\System32\\cmd.exe / signature 99887766');
  assert.ok(eventLines(ev).some((l) => /Signature Version/.test(l)));
});
test('a missing field is named, never blank', () => {
  const ev = parseEvents(raw({ t: 't', id: 1116, m: 'Name: x\n Detection Origin: Internet' }));
  assert.equal(fieldOf(ev[0], 'Process Name'), NOT_IN_EVENT);
  assert.match(originLine(ev), /process \(not in event\)/);
});
test('unreadable output is reported, not read as no detection', () => {
  assert.equal(parseEvents('garbage without the end marker'), null);
  assert.match(originLine(null), /unreadable/);
  assert.deepEqual(eventLines(null).slice(-1), ['(unreadable)']);
  assert.match(originLine([]), /no detection event/);
});
test('parseFields keeps the first occurrence', () => {
  assert.equal(parseFields('Path: a\nPath: b').Path, 'a');
});
test('the script reads the full message and ends with a marker', () => {
  assert.doesNotMatch(eventsPs(), /Substring/);
  assert.match(eventsPs(), /DONE/);
});
test('the MPLog window and script', () => {
  const w = mpLogWindow('2026-11-08T04:03:50.0000000Z', 30);
  assert.deepEqual(w, { lo: '2026-11-08T04:03:20.000', hi: '2026-11-08T04:04:20.000' });
  assert.equal(mpLogWindow('x'), null);
  assert.match(mpLogPs(w), /MPLog-\*\.log/);
  assert.match(mpLogPs(w), /2026-11-08T04:03:20\.000/);
  assert.deepEqual(mpLogLines('@@LOG MPLog-1.log\nline a\r\nDONE', w).slice(1), ['@@LOG MPLog-1.log', 'line a']);
  assert.equal(mpLogLines('DONE', w)[1], '(no MPLog lines in the window)');
  assert.equal(mpLogLines('cut off', w)[1], '(unreadable)');
  assert.equal(detectionTime(parseEvents(raw({ t: 'T1', id: 1116, m: 'x' }))), 'T1');
});
test('the EICAR control is built in pieces and is the standard string', () => {
  assert.equal(EICAR.length, 68);
  assert.match(controlPs(), /REMOVED_BY_AV/);
});
test('control: ok only when its 1116 names the file and shows every field', () => {
  const good = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\paddock-test\\eicar\\eicar-control.com') }));
  assert.equal(judgeControl(good, 'REMOVED_BY_AV=1\nDONE').ok, true);
  const other = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\x\\other.exe') }));
  assert.equal(judgeControl(other, '').ok, false);
  assert.equal(judgeControl([], '').ok, false);
  assert.equal(judgeControl(null, '').ok, false);
  assert.equal(judgeControl(good, 'DROP=Access denied').ok, false);
  const cut = parseEvents(raw({ t: 't', id: 1116, m: 'Path: file:_C:\\paddock-test\\eicar\\eicar-control.com\n Detection Origin: Local machine' }));
  const r = judgeControl(cut, '');
  assert.equal(r.ok, false);
  assert.match(r.reason, /Detection Type/);
});
test('installDetected matches on the file name only in 1116', () => {
  const ev = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\Program Files\\Dyno\\paddock-setup.exe') }, { t: 't', id: 1117, m: 'nothing' }));
  assert.equal(installDetected(ev), true);
  assert.equal(installDetected([]), false);
  assert.equal(installDetected(null), null);
});
test('summary: no detection without a good control is check broken', () => {
  const runs = [{ detected: false }, { detected: false }];
  assert.equal(summarizeRate(runs, { ok: false, reason: 'blind' }).verdict, 'check broken');
  assert.match(summarizeRate(runs, { ok: true, reason: 'seen' }).verdict, /no detection in 2 runs/);
  const s = summarizeRate([{ detected: true }, { detected: false }, { detected: null }], { ok: true, reason: 'seen' });
  assert.deepEqual([s.detected, s.measured, s.notMeasured, s.rate], [1, 2, 1, 0.5]);
  assert.equal(summarizeRate([{ detected: null }], { ok: true, reason: '' }).verdict, 'not measured');
});
test('plan lists 10 installs by default; the run drives a fake guest in order', async () => {
  assert.equal(planRateRun({ snapshot: 's', installer: 'i' }).installs, 10);
  const log = [];
  const hit = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\Program Files\\Dyno\\paddock-setup.exe') }));
  const ctl = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\paddock-test\\eicar\\eicar-control.com') }));
  const res = await runRate({
    installs: 10,
    revert: async () => log.push('revert'),
    versions: async () => 'v',
    control: async () => (log.push('control'), { events: ctl, out: '' }),
    install: async (i) => log.push(`install${i}`),
    readEvents: async (i) => (i === 3 ? hit : []),
    names: ['paddock-setup.exe'],
  });
  assert.deepEqual(log.slice(0, 3), ['revert', 'control', 'install1']);
  assert.equal(res.runs.length, 10);
  assert.equal(res.summary.detected, 1);
  assert.equal(res.summary.verdict, '1/10 detected');
});
test('a blind control makes the whole run check broken', async () => {
  const res = await runRate({ installs: 2, revert: async () => {}, versions: async () => '', control: async () => ({ events: [], out: '' }), install: async () => {}, readEvents: async () => [] });
  assert.equal(res.summary.verdict, 'check broken');
});
