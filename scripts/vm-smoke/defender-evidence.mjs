// Windows Defender evidence for the install row (Dyno 1.1.1 WP-4, research note R1). Pure parsers and script builders; the SSH calls stay in win-machine.mjs.
// What it answers: which Defender layer convicted a file (Detection Origin, Type, Source, Process Name, Signature ID), and how often (a rate over N installs).
// ER-33: a "no detection" counts only when an EICAR positive control, run the same way, showed every field in full; otherwise the verdict is `check broken`.

/** The 1116 fields the origin question needs, in the order they are printed. */
export const FIELDS = ['Detection Origin', 'Detection Type', 'Detection Source', 'Process Name', 'Signature ID'];
export const NOT_IN_EVENT = '(not in event)';

/** Event ids: 1116 detected, 1117 action taken, 1006 malware found (older engines), 1118/1119 action failed. */
export const EVENT_IDS = [1116, 1117, 1006, 1118, 1119];

const BEGIN = '@@EVENT';
const END = '@@END';

/** PowerShell that prints every event with its FULL message (line breaks kept), each between @@EVENT and @@END marker lines. */
export function eventsPs(max = 20) {
  return `Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-Windows Defender/Operational'; Id = ${EVENT_IDS.join(',')} } -MaxEvents ${max} -ErrorAction SilentlyContinue | ForEach-Object { '${BEGIN} ' + $_.TimeCreated.ToUniversalTime().ToString('o') + ' id=' + $_.Id; $_.Message; '${END}' }; 'DONE'`;
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

/** A field's value, or NOT_IN_EVENT. "Signature ID" is also printed by some engines as "Signature ID" only; no other spelling is guessed. */
export const fieldOf = (ev, name) => (ev.fields[name] !== undefined && ev.fields[name] !== '' ? ev.fields[name] : NOT_IN_EVENT);

/** The five fields of one detection event, with NOT_IN_EVENT where missing. */
export function originOf(ev) {
  return Object.fromEntries(FIELDS.map((f) => [f, fieldOf(ev, f)]));
}

/** One line: "Defender: origin X / type Y / source Z / process P / signature S", or the reason there is none. */
export function originLine(events) {
  if (events === null) return 'Defender: events unreadable';
  const det = events.find((e) => e.id === 1116);
  if (!det) return 'Defender: no detection event (1116)';
  const o = originOf(det);
  return `Defender: origin ${o['Detection Origin']} / type ${o['Detection Type']} / source ${o['Detection Source']} / process ${o['Process Name']} / signature ${o['Signature ID']}`;
}

/** Evidence lines for the row: the origin line, then every event with its full message (never cut). */
export function eventLines(events) {
  if (events === null) return ['--- Defender Operational 1116/1117/1006', '(unreadable)'];
  const out = [`--- ${originLine(events)}`, '--- Defender Operational events, full text'];
  if (!events.length) return [...out, '(no events)'];
  for (const e of events) out.push(`${e.time} id=${e.id}:`, ...e.text.split('\n').map((l) => `    ${l.trim()}`).filter((l) => l.trim()));
  return out;
}

/** Get-MpThreatDetection with the fields the note asks for. */
export function threatPs() {
  return "Get-MpThreatDetection -ErrorAction SilentlyContinue | ForEach-Object { '' + $_.InitialDetectionTime.ToUniversalTime().ToString('o') + ' threat ' + $_.ThreatID + ' process ' + $_.ProcessName + ' sourceTypeId ' + $_.DetectionSourceTypeID + ' action ' + $_.ActionSuccess + ' ' + ($_.Resources -join ',') }; 'DONE'";
}

/** Get-MpComputerStatus versions: engine, product, signatures (version and date), and the cloud and behaviour switches that change the verdict path. */
export function versionsPs() {
  return "$s = Get-MpComputerStatus; 'AMEngineVersion=' + $s.AMEngineVersion + ' AMProductVersion=' + $s.AMProductVersion + ' AntivirusSignatureVersion=' + $s.AntivirusSignatureVersion + ' signatures updated ' + $s.AntivirusSignatureLastUpdated + ' RealTimeProtection=' + $s.RealTimeProtectionEnabled + ' BehaviorMonitor=' + $s.BehaviorMonitorEnabled + ' IoavProtection=' + $s.IoavProtectionEnabled + ' NIS=' + $s.NISEnabled; $p = Get-MpPreference; 'MAPS=' + $p.MAPSReporting + ' SubmitSamples=' + $p.SubmitSamplesConsent + ' CloudBlockLevel=' + $p.CloudBlockLevel; 'DONE'";
}

/** The window [lo, hi] (ISO prefixes, UTC) around a detection time. MPLog lines start with "2026-11-08T04:03:50.123Z". */
export function mpLogWindow(whenIso, seconds = 30) {
  const t = Date.parse(whenIso);
  if (Number.isNaN(t)) return null;
  const f = (ms) => new Date(ms).toISOString().slice(0, 23);
  return { lo: f(t - seconds * 1000), hi: f(t + seconds * 1000) };
}

/** PowerShell printing the MPLog lines of each log file in the window (at most `cap` lines per file; the lines are Defender's own, no user content beyond paths). */
export function mpLogPs({ lo, hi }, cap = 300) {
  return `$d = Join-Path $env:ProgramData 'Microsoft\\Windows Defender\\Support'; $n = 0; Get-ChildItem $d -Filter 'MPLog-*.log' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 3 | ForEach-Object { '@@LOG ' + $_.Name; Get-Content $_.FullName -ErrorAction SilentlyContinue | Where-Object { $_.Length -ge 23 -and $_.Substring(0,23) -ge '${lo}' -and $_.Substring(0,23) -le '${hi}' } | Select-Object -First ${cap} }; 'DONE'`;
}

/** MPLog output to evidence lines. `null` raw (script did not finish) is reported, an empty window is reported as such. */
export function mpLogLines(raw, window) {
  const head = `--- Defender MPLog ${window ? `${window.lo}..${window.hi}` : '(no detection time, window unknown)'}`;
  if (!/DONE\s*$/.test(raw ?? '')) return [head, '(unreadable)'];
  const lines = raw.replace(/DONE\s*$/, '').split('\n').map((l) => l.replace(/\r$/, '')).filter((l) => l.trim());
  return [head, ...(lines.length ? lines : ['(no MPLog lines in the window)'])];
}

/** The newest 1116 time, ISO, or null. */
export const detectionTime = (events) => events?.find((e) => e.id === 1116)?.time ?? null;

// --- The positive control (ER-33) ---------------------------------------------------------------------------------------------------------------------------
// The EICAR test string, split so that this file itself is never flagged by an on-access scanner on the host.
export const EICAR = ['X5O!P%@AP[4\\PZX54(P^)7CC)7}$', 'EICAR-STANDARD-ANTIVIRUS-TEST-FILE!', '$H+H*'].join('');
export const CONTROL_DIR = 'C:\\paddock-test\\eicar';
export const CONTROL_FILE = 'eicar-control.com';

/** Guest script: drop the control file and wait for its 1116 (up to `waitS`). The file is removed afterwards (a detected one is already gone). */
export function controlPs(waitS = 60) {
  const f = `${CONTROL_DIR}\\${CONTROL_FILE}`;
  return `$ErrorActionPreference='Continue'; New-Item -ItemType Directory -Force '${CONTROL_DIR}' | Out-Null; try { Set-Content -Path '${f}' -Value '${EICAR.replace(/'/g, "''")}' -NoNewline -ErrorAction Stop } catch { 'DROP=' + $_.Exception.Message }; $end = (Get-Date).AddSeconds(${waitS}); while ((Get-Date) -lt $end -and (Test-Path '${f}')) { Start-Sleep -Milliseconds 500 }; 'REMOVED_BY_AV=' + [int](-not (Test-Path '${f}')); Remove-Item '${f}' -Force -ErrorAction SilentlyContinue; 'DONE'`;
}

/**
 * Judge the control. It shows the probe can see a detection only when an event names the control file AND all five fields are present in full.
 * Returns { ok, reason }; a not-ok control makes every "no detection" of the run `check broken`.
 */
export function judgeControl(events, controlOut) {
  if (events === null) return { ok: false, reason: 'the Defender events were unreadable after the control' };
  if (/DROP=/.test(controlOut ?? '')) return { ok: false, reason: `the control file could not be written (${/DROP=(.*)/.exec(controlOut)[1].trim()})` };
  const ev = events.find((e) => e.id === 1116 && e.text.toLowerCase().includes(CONTROL_FILE));
  if (!ev) return { ok: false, reason: `no 1116 names ${CONTROL_FILE}: real-time protection did not see the control, or the probe cannot see detections` };
  const missing = FIELDS.filter((f) => fieldOf(ev, f) === NOT_IN_EVENT);
  if (missing.length) return { ok: false, reason: `the control's 1116 lacks ${missing.join(', ')}: the probe does not show every field in full` };
  return { ok: true, reason: `control seen: ${originLine([ev])}` };
}

// --- The rate run -------------------------------------------------------------------------------------------------------------------------------------------
export const DEFAULT_INSTALLS = 10;

/** The plan, printed before the run so the VM schedule is explicit. */
export function planRateRun({ snapshot, installs = DEFAULT_INSTALLS, installer }) {
  return { snapshot, installs, installer, steps: ['revert to snapshot', 'versions + control (EICAR must be detected in full)', ...Array.from({ length: installs }, (_, i) => `install ${i + 1}/${installs} then read events, threat detections, MPLog window`), 'summary: detections/installs, per-run origin lines'] };
}

/** Whether a set of events (read after one install) holds a detection that names paddock-setup.exe or the installer. */
export function installDetected(events, names = ['paddock-setup.exe']) {
  if (events === null) return null;
  return events.some((e) => e.id === 1116 && names.some((n) => e.text.toLowerCase().includes(n.toLowerCase())));
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

/**
 * The run itself, with the VM steps injected so it is testable without a guest: `revert()`, `readyVersions()`, `runControl()`, `install(i)`, `readEvents()`.
 * Order matters: the control runs first (no point installing if the probe is blind), and events are read after each install.
 */
export async function runRate({ installs = DEFAULT_INSTALLS, revert, versions, control, install, readEvents, names }) {
  await revert();
  const ver = await versions();
  const controlEvents = await control();
  const verdict = judgeControl(controlEvents.events, controlEvents.out);
  const runs = [];
  for (let i = 1; i <= installs; i++) {
    await install(i);
    const events = await readEvents(i);
    runs.push({ i, detected: installDetected(events, names), origin: originLine(events) });
  }
  return { versions: ver, control: verdict, runs, summary: summarizeRate(runs, verdict) };
}
