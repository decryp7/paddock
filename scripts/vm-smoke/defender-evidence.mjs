// Windows Defender evidence for the install row (Dyno 1.1.1 WP-4, research note R1). Pure parsers and script builders; the SSH calls stay in win-machine.mjs.
// What it answers: which Defender layer convicted a file (Detection Origin, Type, Source, Process Name, the threat ID), and how often (a rate over N installs).
// ER-33: a "no detection" counts only when an EICAR positive control, run the same way, showed every field in full; otherwise the verdict is `check broken`.

/**
 * The 1116 fields the origin question needs, in the order they are printed. The rendered 1116 message has Name, ID (the threat id), Severity, Category,
 * Path, Detection Origin, Detection Type, Detection Source, User, Process Name, Security intelligence Version and Engine Version; there is no
 * signature line (captured on the test VM, 2026-11-08), so the threat `ID` stands in for it; the signature version is in the message text.
 */
export const FIELDS = ['Detection Origin', 'Detection Type', 'Detection Source', 'Process Name', 'ID'];
export const NOT_IN_EVENT = '(not in event)';

/** Event ids: 1116 detected, 1117 action taken, 1006 malware found (older engines), 1118/1119 action failed. */
export const EVENT_IDS = [1116, 1117, 1006, 1118, 1119];

const BEGIN = '@@EVENT';
const END = '@@END';

/** PowerShell printing the guest's UTC clock (ISO), taken just before an install so its events can be told from earlier ones. */
export function nowPs() {
  return "(Get-Date).ToUniversalTime().ToString('o'); 'DONE'";
}

/** The guest clock from nowPs output, or null. */
export function parseNow(raw) {
  const m = /^(\d{4}-\d\d-\d\dT[\d:.]+Z?)\s*$/m.exec(raw ?? '');
  return m && /DONE\s*$/.test(raw) && !Number.isNaN(Date.parse(m[1])) ? m[1] : null;
}

/**
 * PowerShell that prints every event with its FULL message (line breaks kept), each between @@EVENT and @@END marker lines. With `since` (an ISO
 * time) only events created at or after it are listed: an earlier detection is never attributed to a later install.
 */
export function eventsPs(max = 20, since = null) {
  if (since !== null && Number.isNaN(Date.parse(since))) throw new Error(`not a time: ${since}`);
  const start = since ? `; StartTime = [datetime]::Parse('${new Date(since).toISOString()}').ToUniversalTime()` : '';
  return `Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-Windows Defender/Operational'; Id = ${EVENT_IDS.join(',')}${start} } -MaxEvents ${max} -ErrorAction SilentlyContinue | ForEach-Object { '${BEGIN} ' + $_.TimeCreated.ToUniversalTime().ToString('o') + ' id=' + $_.Id; $_.Message; '${END}' }; 'DONE'`;
}

/** Parse eventsPs output into [{ time, id, fields, text }]. `text` is the full message. Returns null when the script did not reach DONE (unreadable). */
export function parseEvents(raw) {
  if (!/DONE\s*$/.test(raw)) return null;
  const events = [];
  const re = new RegExp(`${BEGIN} (\\S+) id=(\\d+)\\r?\\n([\\s\\S]*?)\\r?\\n?${END}`, 'g');
  for (const m of raw.matchAll(re)) {
    const text = m[3].replace(/\r/g, '').trim();
    events.push({ time: m[1], id: Number(m[2]), text, fields: parseFields(text) });
  }
  return events;
}

/** "Key: value" lines of a Defender message. Keys are kept as printed; the first occurrence wins. */
export function parseFields(text) {
  const fields = {};
  for (const line of text.split('\n')) {
    const m = /^\s*([A-Za-z][A-Za-z0-9 ]{1,40}?):\s*(.*?)\s*$/.exec(line);
    if (m && !(m[1] in fields)) fields[m[1]] = m[2];
  }
  return fields;
}

/** A field's value, or NOT_IN_EVENT. Labels are matched exactly as Windows prints them; no other spelling is guessed. */
export const fieldOf = (ev, name) => (ev.fields[name] !== undefined && ev.fields[name] !== '' ? ev.fields[name] : NOT_IN_EVENT);

/** The five fields of one detection event, with NOT_IN_EVENT where missing. */
export function originOf(ev) {
  return Object.fromEntries(FIELDS.map((f) => [f, fieldOf(ev, f)]));
}

/** The events at or after `since` (all of them when it is null). */
export const eventsSince = (events, since) => (since === null || since === undefined || events === null ? events : events.filter((e) => Date.parse(e.time) >= Date.parse(since)));

/**
 * One line: "Defender: origin X / type Y / source Z / process P / threat id N", or the reason there is none. Only a detection at or after `since` is
 * named; without `since` the line says it is the newest one in the log, not necessarily this run's.
 */
export function originLine(events, since = null) {
  if (events === null) return 'Defender: events unreadable';
  const det = eventsSince(events, since).find((e) => e.id === 1116);
  const scope = since ? ` since ${since}` : ' (newest in the log, not bounded to a run)';
  if (!det) return `Defender: no detection event (1116)${since ? ` since ${since}` : ''}`;
  const o = originOf(det);
  return `Defender: origin ${o['Detection Origin']} / type ${o['Detection Type']} / source ${o['Detection Source']} / process ${o['Process Name']} / threat id ${o.ID}${scope}`;
}

/** Evidence lines for the row: the origin line, then every event with its full message (never cut). */
export function eventLines(events, since = null) {
  if (events === null) return ['--- Defender Operational 1116/1117/1006', '(unreadable)'];
  const out = [`--- ${originLine(events, since)}`, '--- Defender Operational events, full text'];
  if (!events.length) return [...out, '(no events)'];
  for (const e of events) out.push(`${e.time} id=${e.id}:`, ...e.text.split('\n').map((l) => `    ${l.trim()}`).filter((l) => l.trim()));
  return out;
}

/** Get-MpThreatDetection with the fields the note asks for. */
export function threatPs(max = 50) {
  return `Get-MpThreatDetection -ErrorAction SilentlyContinue | Select-Object -First ${max} | ForEach-Object { '' + $_.InitialDetectionTime.ToUniversalTime().ToString('o') + ' threat ' + $_.ThreatID + ' process ' + $_.ProcessName + ' sourceTypeId ' + $_.DetectionSourceTypeID + ' action ' + $_.ActionSuccess + ' ' + (($_.Resources -join ',') -replace '^(.{2000}).+$', '$1 ... [cut at 2000 chars]') }; 'DONE'`;
}

/** Get-MpComputerStatus versions: engine, product, signatures (version and date), and the cloud and behaviour switches that change the verdict path. */
export function versionsPs() {
  return "$s = Get-MpComputerStatus; 'AMEngineVersion=' + $s.AMEngineVersion + ' AMProductVersion=' + $s.AMProductVersion + ' AntivirusSignatureVersion=' + $s.AntivirusSignatureVersion + ' signatures updated ' + $s.AntivirusSignatureLastUpdated + ' RealTimeProtection=' + $s.RealTimeProtectionEnabled + ' BehaviorMonitor=' + $s.BehaviorMonitorEnabled + ' IoavProtection=' + $s.IoavProtectionEnabled + ' NIS=' + $s.NISEnabled; $p = Get-MpPreference; 'MAPS=' + $p.MAPSReporting + ' SubmitSamples=' + $p.SubmitSamplesConsent + ' CloudBlockLevel=' + $p.CloudBlockLevel; 'DONE'";
}

/** The versions line, or null when the output is partial (a missing end marker or a missing part: never printed as if complete). */
export function versionsLine(raw) {
  const t = String(raw ?? '');
  if (!/DONE\s*$/.test(t) || !/AMEngineVersion=\S/.test(t) || !/MAPS=/.test(t)) return null;
  return t.replace(/DONE\s*$/, '').split('\n').map((l) => l.trim()).filter(Boolean).join(' | ');
}

/** The window [lo, hi] (ISO prefixes, UTC) around a detection time. MPLog lines start with "2026-11-08T04:03:50.123Z". */
export function mpLogWindow(whenIso, seconds = 30) {
  const t = Date.parse(whenIso);
  if (Number.isNaN(t)) return null;
  const f = (ms) => new Date(ms).toISOString().slice(0, 23);
  return { lo: f(t - seconds * 1000), hi: f(t + seconds * 1000) };
}

export const MPLOG_LINE_CAP = 2000;

/**
 * PowerShell printing the MPLog lines of the newest log files within the window (at most `cap` lines per file, each cut at MPLOG_LINE_CAP characters
 * with a visible mark). The file is streamed (Defender keeps writing it; MPLog is UTF-16 with a BOM). `STAMPED=n` says how many lines carried an
 * ISO timestamp at all: 0 means the window could not be applied (the format is read, not assumed). Lines are Defender's own; paths may appear.
 */
export function mpLogPs({ lo, hi }, cap = 300) {
  return `$d = Join-Path $env:ProgramData 'Microsoft\\Windows Defender\\Support'; Get-ChildItem $d -Filter 'MPLog-*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 3 | ForEach-Object { '@@LOG ' + $_.Name; $n = 0; $stamped = 0; try { $fs = [IO.File]::Open($_.FullName, 'Open', 'Read', 'ReadWrite'); $sr = New-Object IO.StreamReader($fs, [Text.Encoding]::Unicode, $true); while ($null -ne ($l = $sr.ReadLine())) { if ($l.Length -ge 23 -and $l -match '^\\d{4}-\\d\\d-\\d\\dT') { $stamped++; $t = $l.Substring(0,23); if ([string]::CompareOrdinal($t, '${lo}') -ge 0 -and [string]::CompareOrdinal($t, '${hi}') -le 0 -and $n -lt ${cap}) { $n++; if ($l.Length -gt ${MPLOG_LINE_CAP}) { $l.Substring(0, ${MPLOG_LINE_CAP}) + ' ... [cut at ${MPLOG_LINE_CAP} chars]' } else { $l } } } } } catch { 'READ-ERROR ' + $_.Exception.Message } finally { if ($sr) { $sr.Dispose() } }; 'STAMPED=' + $stamped }; 'DONE'`;
}

/** MPLog output to evidence lines. Unfinished output is reported; an empty window and a log without ISO timestamps are told apart. */
export function mpLogLines(raw, window) {
  const head = `--- Defender MPLog ${window ? `${window.lo}..${window.hi}` : '(no detection time, window unknown)'}`;
  if (!/DONE\s*$/.test(raw ?? '')) return [head, '(unreadable)'];
  const lines = raw.replace(/DONE\s*$/, '').split('\n').map((l) => l.replace(/\r$/, '')).filter((l) => l.trim());
  const body = lines.filter((l) => !/^STAMPED=\d+$/.test(l));
  const stamped = lines.filter((l) => /^STAMPED=\d+$/.test(l)).map((l) => Number(l.slice(8)));
  const notes = [];
  if (stamped.length && stamped.every((n) => n === 0)) notes.push('(no MPLog line carried an ISO timestamp: the window could not be applied; the log format is not what this probe assumes)');
  else if (!body.some((l) => !l.startsWith('@@LOG'))) notes.push('(no MPLog lines in the window)');
  return [head, ...body, ...notes];
}

/** The newest 1116 time, ISO, or null. */
export const detectionTime = (events) => events?.find((e) => e.id === 1116)?.time ?? null;

// --- The positive control (ER-33) ---------------------------------------------------------------------------------------------------------------------------
// The EICAR test string, split so that this file itself is never flagged by an on-access scanner on the host.
export const EICAR = ['X5O!P%@AP[4\\PZX54(P^)7CC)7}$', 'EICAR-STANDARD-ANTIVIRUS-TEST-FILE!', '$H+H*'].join('');
export const CONTROL_DIR = 'C:\\paddock-test\\eicar';
export const CONTROL_FILE = 'eicar-control.com';

/**
 * Guest script: drop the control file and wait for its 1116 (up to `waitS`). Real-time protection often blocks the write itself (ERROR_VIRUS_INFECTED,
 * 0x800700E1): that is `DROP=<hresult> <message>` and counts as a detection only if a 1116 names the file (judgeControl). The file is removed afterwards.
 */
export function controlPs(waitS = 60) {
  const f = `${CONTROL_DIR}\\${CONTROL_FILE}`;
  return `$ErrorActionPreference='Continue'; New-Item -ItemType Directory -Force '${CONTROL_DIR}' | Out-Null; try { Set-Content -Path '${f}' -Value '${EICAR.replace(/'/g, "''")}' -NoNewline -ErrorAction Stop } catch { 'DROP=0x' + ('{0:X8}' -f $_.Exception.HResult) + ' ' + $_.Exception.Message }; $end = (Get-Date).AddSeconds(${waitS}); while ((Get-Date) -lt $end -and (Test-Path '${f}')) { Start-Sleep -Milliseconds 500 }; 'REMOVED_BY_AV=' + [int](-not (Test-Path '${f}')); Remove-Item '${f}' -Force -ErrorAction SilentlyContinue; 'DONE'`;
}

/**
 * Judge the control, events first. It shows the probe can see a detection only when a 1116 names the control file AND all five fields are present in
 * full. A write that Defender blocked (`DROP=0x800700E1`, or a message naming a virus) is a detection at write time: it is a good control when its
 * 1116 is there, and a broken one only when no 1116 names the file. Returns { ok, reason }; a not-ok control makes every "no detection" `check broken`.
 */
export function judgeControl(events, controlOut) {
  if (events === null) return { ok: false, reason: 'the Defender events were unreadable after the control' };
  const drop = /DROP=(.*)/.exec(controlOut ?? '')?.[1]?.trim() ?? null;
  const blocked = drop !== null && /0x800700E1|virus|potentially unwanted/i.test(drop);
  const ev = events.find((e) => e.id === 1116 && e.text.toLowerCase().includes(CONTROL_FILE));
  if (!ev) {
    if (drop !== null) return { ok: false, reason: `the control file could not be written (${drop}) and no 1116 names ${CONTROL_FILE}` };
    return { ok: false, reason: `no 1116 names ${CONTROL_FILE}: real-time protection did not see the control, or the probe cannot see detections` };
  }
  const missing = FIELDS.filter((f) => fieldOf(ev, f) === NOT_IN_EVENT);
  if (missing.length) return { ok: false, reason: `the control's 1116 lacks ${missing.join(', ')}: the probe does not show every field in full` };
  return { ok: true, reason: `control seen${blocked ? ' (blocked at write)' : ''}: ${originLine([ev])}` };
}

// --- The rate run -------------------------------------------------------------------------------------------------------------------------------------------
export const DEFAULT_INSTALLS = 10;
/** After an install, events are polled this long for a delayed (cloud or ML) verdict, stopping early on a 1116. */
export const DEFAULT_SETTLE_S = 120;
export const DEFAULT_POLL_S = 10;

/** The plan, printed before the run so the VM schedule is explicit. Every install starts from a reverted snapshot (installs are independent: no upgrade over an earlier one, no cached verdict). */
export function planRateRun({ snapshot, installs = DEFAULT_INSTALLS, installer, settleS = DEFAULT_SETTLE_S }) {
  return { snapshot, installs, installer, settleS, steps: [
    'revert to snapshot, start; read versions and run the EICAR control (it must be detected, in full)',
    ...Array.from({ length: installs }, (_, i) => `install ${i + 1}/${installs}: revert, start, copy the installer, note the guest clock, install, poll the Defender events since the clock for up to ${settleS} s (stop at the first 1116)`),
    'summary: detections/installs, per-run origin lines',
  ] };
}

/** Which of `names` a 1116 at or after `since` names (the first match, as written in `names`), null when none, undefined when the events are unreadable. */
export function detectedName(events, names = ['paddock-setup.exe', 'setup.exe'], since = null) {
  if (events === null) return undefined;
  const hits = eventsSince(events, since).filter((e) => e.id === 1116);
  for (const n of names) if (hits.some((e) => e.text.toLowerCase().includes(n.toLowerCase()))) return n;
  return null;
}

/** Whether a set of events holds a detection naming the program or the installer since `since`: true, false, or null when unreadable. */
export function installDetected(events, names, since = null) {
  const m = detectedName(events, names, since);
  return m === undefined ? null : m !== null;
}

/**
 * Summarise runs [{ detected: true|false|null, origin }] with the control verdict. Without a good control the verdict is `check broken`, never `no detection`.
 * `null` runs (events unreadable) are `not measured` and excluded from the rate.
 */
export function summarizeRate(runs, control) {
  const measured = runs.filter((r) => r.detected !== null);
  const detected = measured.filter((r) => r.detected).length;
  const notMeasured = runs.length - measured.length;
  const base = { installs: runs.length, measured: measured.length, detected, notMeasured, rate: measured.length ? detected / measured.length : null };
  if (!control.ok) return { ...base, verdict: 'check broken', note: control.reason };
  if (!measured.length) return { ...base, verdict: 'not measured', note: 'no run could be read' };
  return { ...base, verdict: detected ? `${detected}/${measured.length} detected` : `no detection in ${measured.length} runs (control ok)`, note: control.reason };
}

const stamp = () => new Date().toISOString();
const realSleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * The run itself, with the VM steps injected so it is testable without a guest: `revert()` (revert the snapshot and start; the guest is then clean),
 * `versions()`, `control()` -> { events, out }, `install(i)` -> { exit } (the installer's exit code), `now()` (the guest's UTC clock),
 * `readEvents(i, since)`. The control runs first: with a blind control the run stops there (`force` runs on). Then each install runs from its own
 * reverted snapshot; only events at or after the clock taken just before it are counted, and they are polled for `settleS` seconds (early stop at the first
 * 1116 naming the program or installer) because a cloud or ML verdict can come after the install returns. An install that exited non-zero with no
 * detection is `not measured`. Every phase is logged with its time and duration (ER-27).
 */
export async function runRate({ installs = DEFAULT_INSTALLS, revert, versions, control, install, now, readEvents, names, settleS = DEFAULT_SETTLE_S, pollS = DEFAULT_POLL_S, sleep = realSleep, force = false, log = (l) => console.log(l) }) {
  const phase = async (label, fn) => {
    const t0 = Date.now();
    log(`${stamp()} ${label}: start`);
    try {
      const r = await fn();
      log(`${stamp()} ${label}: done in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
      return r;
    } catch (e) {
      log(`${stamp()} ${label}: FAILED after ${((Date.now() - t0) / 1000).toFixed(1)} s: ${e.message}`);
      throw e;
    }
  };
  await phase('revert for the control', revert);
  const ver = await phase('versions', versions);
  const controlRaw = await phase('control', control);
  const verdict = judgeControl(controlRaw.events, controlRaw.out);
  log(`${stamp()} control verdict: ${verdict.ok ? 'ok' : 'BROKEN'}: ${verdict.reason}`);
  if (!verdict.ok && !force) {
    const summary = summarizeRate([], verdict);
    log(`${stamp()} stopped after the control: check broken. Next step: ${CONTROL_NEXT_STEP}`);
    return { versions: ver, control: verdict, runs: [], summary, stopped: 'control' };
  }
  const runs = [];
  for (let i = 1; i <= installs; i++) {
    const tag = `install ${i}/${installs}`;
    await phase(`${tag} revert`, revert);
    const since = await phase(`${tag} clock`, now);
    const done = (await phase(`${tag} install`, () => install(i))) ?? {};
    const exit = Number.isInteger(done.exit) ? done.exit : null;
    // Poll for a delayed verdict: stop at the first 1116 naming a target, else after the settle period.
    const t0 = Date.now();
    let events = null;
    let settledS = 0;
    const polls = Math.max(1, Math.ceil(settleS / pollS));
    for (let k = 0; k < polls; k++) {
      events = await phase(`${tag} events (poll ${k + 1}/${polls})`, () => readEvents(i, since));
      if (detectedName(events, names, since)) break;
      if (k < polls - 1) await sleep(pollS * 1000);
      settledS = (k + 1) * pollS;
    }
    const matched = detectedName(events, names, since);
    const failedNoDetection = !matched && exit !== null && exit !== 0;
    const detected = matched === undefined || failedNoDetection ? null : matched !== null;
    const run = { i, since, exit, detected, matched: matched ?? null, settledS: matched ? Math.round((Date.now() - t0) / 1000) : settledS, origin: originLine(events, since) };
    if (failedNoDetection) run.note = `install failed (exit ${exit}) with no detection`;
    log(`${stamp()} ${tag}: ${detected === null ? `not measured (${failedNoDetection ? run.note : 'events unreadable'})` : detected ? `DETECTED (${run.matched})` : `no detection after ${settledS} s`}; exit ${exit ?? 'unknown'}; ${run.origin}`);
    runs.push(run);
  }
  const summary = summarizeRate(runs, verdict);
  log(`${stamp()} summary: ${summary.verdict} (${summary.detected}/${summary.measured} measured, ${summary.notMeasured} not measured); ${summary.note}`);
  return { versions: ver, control: verdict, runs, summary };
}

export const CONTROL_NEXT_STEP = 'the probe cannot see a detection; check that real-time protection is on in the snapshot and that the control file is not excluded, then run again (--force only to see the installs anyway)';
