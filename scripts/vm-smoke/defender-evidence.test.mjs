import test from 'node:test';
import assert from 'node:assert/strict';
import { parseEvents, parseFields, originLine, eventLines, fieldOf, NOT_IN_EVENT, FIELDS, mpLogWindow, mpLogPs, mpLogLines, judgeControl, summarizeRate, planRateRun, runRate, installDetected, detectedName, EICAR, controlPs, eventsPs, nowPs, parseNow, versionsLine, threatPs, detectionTime } from './defender-evidence.mjs';

// The shape of Defender's rendered 1116 and 1117 as captured from the Paddock test VM on 2026-11-08 (an EICAR drop; labels exactly as Windows prints
// them: Name, ID, Severity, Category, Path, Detection Origin, Detection Type, Detection Source, User, Process Name, [1117: Action, Action Status, Error
// Code, Error description,] Security intelligence Version, Engine Version; there is no "Signature ID" line). The values are invented.
const msg1116 = (path, extra = '') => `Microsoft Defender Antivirus has detected malware or other potentially unwanted software.
 For more information please see the following:
 https://go.microsoft.com/fwlink/?linkid=37020&name=Trojan:Win32/Example.A!ml&threatid=2147000000&enterprise=0
 	Name: Trojan:Win32/Example.A!ml
 	ID: 2147000000
 	Severity: Severe
 	Category: Trojan
 	Path: file:_${path}
 	Detection Origin: Local machine
 	Detection Type: Concrete
 	Detection Source: Real-Time Protection
 	User: EXAMPLE\\someone
 	Process Name: C:\\Windows\\System32\\cmd.exe
 	Security intelligence Version: AV: 1.463.99.0, AS: 1.463.99.0, NIS: 1.463.99.0
 	Engine Version: AM: 1.1.24000.1, NIS: 1.1.24000.1${extra}`;
const msg1117 = (path) => `Microsoft Defender Antivirus has detected malware or potentially unwanted software using Defender security intelligence and applied the remediation action defined in the security settings.
 For more information please see the following:
https://go.microsoft.com/fwlink/?linkid=37020&name=Trojan:Win32/Example.A!ml&threatid=2147000000&enterprise=0
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
 	Action: Quarantine
 	Action Status:  No additional actions required
 	Error Code: 0x00000000
 	Error description: The operation completed successfully.
 	Security intelligence Version: AV: 1.463.99.0, AS: 1.463.99.0, NIS: 1.463.99.0
 	Engine Version: AM: 1.1.24000.1, NIS: 1.1.24000.1`;
const raw = (...evs) => evs.map((e) => `@@EVENT ${e.t} id=${e.id}\n${e.m}\n@@END`).join('\n') + '\nDONE';
const T = (min, sec = 0) => `2026-11-08T04:${String(min).padStart(2, '0')}:${String(sec).padStart(2, '0')}.0000000Z`;
const SETUP = 'C:\\Program Files\\Dyno\\paddock-setup.exe';
const CTRL = 'C:\\paddock-test\\eicar\\eicar-control.com';

test('the full 1116 text is kept, beyond 400 characters, with the five fields', () => {
  const ev = parseEvents(raw({ t: T(3, 50), id: 1116, m: msg1116(SETUP) }));
  assert.equal(ev.length, 1);
  assert.ok(ev[0].text.length > 400);
  assert.match(ev[0].text, /Security intelligence Version/);
  assert.deepEqual(FIELDS, ['Detection Origin', 'Detection Type', 'Detection Source', 'Process Name', 'ID']);
  assert.equal(fieldOf(ev[0], 'Detection Origin'), 'Local machine');
  assert.equal(fieldOf(ev[0], 'Detection Source'), 'Real-Time Protection');
  assert.equal(fieldOf(ev[0], 'ID'), '2147000000');
  assert.match(originLine(ev), /^Defender: origin Local machine \/ type Concrete \/ source Real-Time Protection \/ process C:\\Windows\\System32\\cmd\.exe \/ threat id 2147000000 \(newest in the log/);
  assert.ok(eventLines(ev).some((l) => /Security intelligence Version/.test(l)));
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
test('the scripts read the full message, end with a marker, and are bounded', () => {
  assert.doesNotMatch(eventsPs(), /Substring/);
  assert.match(eventsPs(), /DONE/);
  assert.doesNotMatch(eventsPs(), /StartTime/);
  assert.match(eventsPs(20, '2026-11-08T04:03:50Z'), /StartTime = \[datetime\]::Parse\('2026-11-08T04:03:50\.000Z'\)/);
  assert.throws(() => eventsPs(20, 'not a time'), /not a time/);
  assert.match(threatPs(), /Select-Object -First 50/);
  assert.match(threatPs(), /cut at 2000 chars/);
});
test('the guest clock is read from its own output', () => {
  assert.match(nowPs(), /ToUniversalTime/);
  assert.equal(parseNow('2026-11-08T04:03:50.1234567Z\nDONE'), '2026-11-08T04:03:50.1234567Z');
  assert.equal(parseNow('2026-11-08T04:03:50.1234567Z'), null, 'no end marker');
  assert.equal(parseNow('garbage\nDONE'), null);
});
test('an origin line names only a detection since the run started', () => {
  const ev = parseEvents(raw({ t: T(1), id: 1116, m: msg1116(SETUP) }));
  assert.match(originLine(ev, T(2)), /^Defender: no detection event \(1116\) since /);
  assert.match(originLine(ev, T(0)), /threat id 2147000000 since /);
});
test('the MPLog window and script (streamed, ordinal compare, capped lines, stamp count)', () => {
  const w = mpLogWindow('2026-11-08T04:03:50.0000000Z', 30);
  assert.deepEqual(w, { lo: '2026-11-08T04:03:20.000', hi: '2026-11-08T04:04:20.000' });
  assert.equal(mpLogWindow('x'), null);
  const ps = mpLogPs(w);
  assert.match(ps, /MPLog-\*\.log/);
  assert.match(ps, /CompareOrdinal\(\$t, '2026-11-08T04:03:20\.000'\)/);
  assert.match(ps, /\[Text\.Encoding\]::Unicode/);
  assert.match(ps, /'ReadWrite'/);
  assert.match(ps, /cut at 2000 chars/);
  assert.match(ps, /STAMPED=/);
  assert.deepEqual(mpLogLines('@@LOG MPLog-1.log\nline a\r\nSTAMPED=40\nDONE', w).slice(1), ['@@LOG MPLog-1.log', 'line a']);
  assert.equal(mpLogLines('@@LOG MPLog-1.log\nSTAMPED=40\nDONE', w).at(-1), '(no MPLog lines in the window)');
  assert.match(mpLogLines('@@LOG MPLog-1.log\nSTAMPED=0\nDONE', w).at(-1), /no MPLog line carried an ISO timestamp/);
  assert.equal(mpLogLines('cut off', w)[1], '(unreadable)');
  assert.equal(detectionTime(parseEvents(raw({ t: 'T1', id: 1116, m: 'x' }))), 'T1');
});
test('the versions line is printed only when the output is complete', () => {
  const ok = 'AMEngineVersion=1.1.24000.1 AMProductVersion=4.18 signatures updated 11/2/2026\nMAPS=2 SubmitSamples=1 CloudBlockLevel=0\nDONE';
  assert.match(versionsLine(ok), /AMEngineVersion=1\.1\.24000\.1.*\| MAPS=2/);
  assert.equal(versionsLine('AMEngineVersion=1.1\nDONE'), null, 'the preference half is missing');
  assert.equal(versionsLine('AMEngineVersion=1.1 AMProductVersion=4\nMAPS=2'), null, 'no end marker');
  assert.equal(versionsLine(''), null);
});
test('the EICAR control is built in pieces and is the standard string', () => {
  assert.equal(EICAR.length, 68);
  assert.match(controlPs(), /REMOVED_BY_AV/);
  assert.match(controlPs(), /DROP=0x/);
});
test('control: ok only when its 1116 names the file and shows every field', () => {
  const good = parseEvents(raw({ t: 't', id: 1116, m: msg1116(CTRL) }));
  assert.equal(judgeControl(good, 'REMOVED_BY_AV=1\nDONE').ok, true);
  const other = parseEvents(raw({ t: 't', id: 1116, m: msg1116('C:\\x\\other.exe') }));
  assert.equal(judgeControl(other, '').ok, false);
  assert.equal(judgeControl([], '').ok, false);
  assert.equal(judgeControl(null, '').ok, false);
  const cut = parseEvents(raw({ t: 't', id: 1116, m: `Path: file:_${CTRL}\n Detection Origin: Local machine` }));
  const r = judgeControl(cut, '');
  assert.equal(r.ok, false);
  assert.match(r.reason, /Detection Type/);
});
test('control: a write Defender blocked is a detection when its 1116 is there, broken when it is not', () => {
  const good = parseEvents(raw({ t: 't', id: 1116, m: msg1116(CTRL) }));
  const drop = 'DROP=0x800700E1 Operation did not complete successfully because the file contains a virus or potentially unwanted software.\nREMOVED_BY_AV=1\nDONE';
  const seen = judgeControl(good, drop);
  assert.equal(seen.ok, true);
  assert.match(seen.reason, /blocked at write/);
  const none = judgeControl([], drop);
  assert.equal(none.ok, false);
  assert.match(none.reason, /could not be written \(0x800700E1.*no 1116 names/);
  assert.equal(judgeControl([], 'DROP=0x80070005 Access is denied').ok, false);
});
test('installDetected matches the program or the installer, in 1116 only, and only since the run started', () => {
  const ev = parseEvents(raw({ t: T(5), id: 1116, m: msg1116(SETUP) }, { t: T(5), id: 1117, m: 'nothing' }));
  assert.equal(installDetected(ev), true);
  assert.equal(detectedName(ev), 'paddock-setup.exe');
  const inst = parseEvents(raw({ t: T(5), id: 1116, m: msg1116('C:\\paddock-test\\vmcheck\\Dyno_1.1.1_x64-setup.exe') }));
  assert.equal(detectedName(inst, ['paddock-setup.exe', 'x64-setup.exe']), 'x64-setup.exe');
  assert.equal(installDetected([]), false);
  assert.equal(installDetected(null), null);
  assert.equal(installDetected(ev, ['paddock-setup.exe'], T(6)), false, 'an earlier detection is not this run');
  assert.equal(installDetected(ev, ['paddock-setup.exe'], T(4)), true);
});
test('summary: no detection without a good control is check broken', () => {
  const runs = [{ detected: false }, { detected: false }];
  assert.equal(summarizeRate(runs, { ok: false, reason: 'blind' }).verdict, 'check broken');
  assert.match(summarizeRate(runs, { ok: true, reason: 'seen' }).verdict, /no detection in 2 runs/);
  const s = summarizeRate([{ detected: true }, { detected: false }, { detected: null }], { ok: true, reason: 'seen' });
  assert.deepEqual([s.detected, s.measured, s.notMeasured, s.rate], [1, 2, 1, 0.5]);
  assert.equal(summarizeRate([{ detected: null }], { ok: true, reason: '' }).verdict, 'not measured');
});

// A fake guest that keeps ONE cumulative event log (like Defender's), so a detection stays visible to every later read unless the read is bounded.
function fakeGuest({ detectAt = [], revertClears = true, controlEvents = 'good' } = {}) {
  const log = [];
  let clock = 0;
  const g = {
    log,
    calls: [],
    revert: async () => { g.calls.push('revert'); if (revertClears) log.length = 0; },
    versions: async () => 'v',
    control: async () => { g.calls.push('control'); return { events: controlEvents === 'good' ? parseEvents(raw({ t: T(0), id: 1116, m: msg1116(CTRL) })) : [], out: '' }; },
    now: async () => { clock += 1; return T(10 + clock); },
    install: async (i) => { g.calls.push(`install${i}`); if (detectAt.includes(i)) log.push({ time: T(10 + clock, 30), id: 1116, text: msg1116(SETUP), fields: parseFields(msg1116(SETUP)) }); },
    readEvents: async (i, since) => (since ? log.filter((e) => Date.parse(e.time) >= Date.parse(since)) : [...log]),
    names: ['paddock-setup.exe'],
    lines: [],
  };
  g.logfn = (l) => g.lines.push(l);
  return g;
}

test('the run starts every install from a reverted snapshot, after the control, and counts one detection once', async () => {
  const g = fakeGuest({ detectAt: [3] });
  const res = await runRate({ ...g, installs: 10, log: g.logfn });
  assert.deepEqual(g.calls.slice(0, 4), ['revert', 'control', 'revert', 'install1']);
  assert.equal(g.calls.filter((c) => c === 'revert').length, 11, 'one for the control and one per install');
  assert.equal(res.runs.length, 10);
  assert.equal(res.summary.detected, 1);
  assert.equal(res.summary.verdict, '1/10 detected');
  assert.ok(res.runs[2].detected && res.runs.filter((r) => r.detected).length === 1);
});
test('even without a revert clearing the log, only events since the install started count (the cumulative-log bug)', async () => {
  const g = fakeGuest({ detectAt: [3], revertClears: false });
  const res = await runRate({ ...g, installs: 10, log: g.logfn });
  assert.equal(res.summary.detected, 1, 'a detection at install 3 must not show again in installs 4 to 10');
  assert.match(res.runs[3].origin, /^Defender: no detection event/);
});
test('a blind control makes the whole run check broken, and phases are logged with durations', async () => {
  const g = fakeGuest({ controlEvents: 'none' });
  const res = await runRate({ ...g, installs: 2, log: g.logfn });
  assert.equal(res.summary.verdict, 'check broken');
  assert.ok(g.lines.some((l) => /control verdict: BROKEN/.test(l)));
  assert.ok(g.lines.some((l) => /install 2\/2 install: done in \d+\.\d s/.test(l)));
  assert.match(g.lines.at(-1), /summary: check broken/);
});
test('a failing phase is logged with its cause and the run stops', async () => {
  const g = fakeGuest();
  g.install = async () => { throw new Error('the installer did not start'); };
  await assert.rejects(runRate({ ...g, installs: 2, log: g.logfn }), /did not start/);
  assert.ok(g.lines.some((l) => /install 1\/2 install: FAILED after .*did not start/.test(l)));
});
test('plan lists 10 installs by default, each from a reverted snapshot', () => {
  const p = planRateRun({ snapshot: 's', installer: 'i' });
  assert.equal(p.installs, 10);
  assert.ok(p.steps.slice(1, 11).every((s) => /revert, start/.test(s)));
});

test('the real event order (newest first, the 1117 before its 1116) still finds the detection and its fields', () => {
  const ev = parseEvents(raw({ t: T(3, 51), id: 1117, m: msg1117(SETUP) }, { t: T(3, 50), id: 1116, m: msg1116(SETUP) }));
  assert.deepEqual(ev.map((e) => e.id), [1117, 1116]);
  assert.equal(fieldOf(ev[0], 'Action'), 'Quarantine');
  assert.equal(fieldOf(ev[0], 'Action Status'), 'No additional actions required');
  assert.match(originLine(ev), /threat id 2147000000/);
  assert.equal(detectionTime(ev), T(3, 50));
  assert.equal(detectedName(ev), 'paddock-setup.exe');
});
test('MPLog lines as the guest writes them: a 23-character UTC ISO stamp, then the text; continuation lines have none', () => {
  // Captured shape (2026-11-08): "2026-11-08T21:49:01.910 SDN:Issuing SDN query for ...", "Resource Path:C:\\..." with no stamp.
  const w = mpLogWindow('2026-11-08T21:49:02.717Z', 30);
  const stamp = '2026-11-08T21:49:01.910';
  assert.equal(stamp.length, 23);
  assert.ok(stamp >= w.lo && stamp <= w.hi, 'a line 1 s before the detection is inside the window');
  assert.ok('2026-11-08T21:50:02.000' > w.hi && '2026-11-08T21:48:00.000' < w.lo);
  const out = `@@LOG MPLog-20261029-174420.log\n${stamp} [RTP] [Mini-filter] Blocked file(#1): \\Device\\HarddiskVolume3\\x\n2026-11-08T21:49:02.717 DETECTIONEVENT MPSOURCE_REALTIME Virus:DOS/Example file:C:\\x;\nSTAMPED=5666\nDONE`;
  const lines = mpLogLines(out, w);
  assert.equal(lines.length, 4);
  assert.match(lines[3], /DETECTIONEVENT MPSOURCE_REALTIME/);
});
