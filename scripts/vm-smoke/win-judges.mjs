// The pure judges of the Windows suite (no I/O, no clock): they take what the guest printed and return checks, so a dead state (a service that is
// stopped, a tray that was not measured, an uninstall whose SSH died) can be put in as a fixture and must not pass. A check with `kind: 'check'`
// says the measurement or its control failed, which is not a statement about the product (verdict.mjs). The runners (win-reboot, win-helper,
// win-machine, win-tray) collect the text.
import { LIFESPAN_VALUE, FAILURE_WORDING } from './helper-readings.mjs';

const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** `name pid=N started=ISO cmd=...` lines (autoStartRecord) to rows. */
export function parseProcs(text) {
  return String(text ?? '').split('\n').map((l) => /^(\S+) pid=(\d+) started=(\S+)(?: cmd=(.*))?$/.exec(l.trim())).filter(Boolean).map((m) => ({ name: m[1], pid: Number(m[2]), started: m[3], cmd: m[4] ?? '' }));
}

/**
 * Autostart at logon (Launch at login ON): `dyno-app` itself must have been started AFTER the boot time, with `--hidden` (the tray start). A
 * running `dyno-agent` alone is not the app: it is what the user's report looked like while the app never came (the audit's false pass).
 * @param {{boot:string|null, procs:ReturnType<typeof parseProcs>, expectOn:boolean}} f boot: LastBootUpTime as ISO "s" text.
 */
export function judgeAutostart({ boot, procs, expectOn }) {
  if (!boot || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$/.test(boot)) return [chk(false, `the boot time could not be read (${boot ?? 'nothing'}), so "started after the boot" cannot be judged`, 'check')];
  const apps = procs.filter((p) => /^dyno-app(\.exe)?$/i.test(p.name));
  const agents = procs.filter((p) => /^dyno-agent(\.exe)?$/i.test(p.name));
  const after = apps.filter((p) => p.started > boot);
  const hidden = after.filter((p) => /(^|\s)--hidden(\s|$)/.test(p.cmd));
  if (!expectOn) return [chk(after.length === 0, after.length ? `Launch at login is off but dyno-app started at logon (pid ${after[0].pid})` : 'Launch at login is off and dyno-app did not start by itself')];
  return [
    chk(after.length > 0, after.length ? `dyno-app pid ${after[0].pid} started ${after[0].started}, after the boot ${boot}` : `no dyno-app started after the boot ${boot}${agents.length ? ` (only dyno-agent pid ${agents[0].pid} is running: the tray app never came)` : ' (nothing of Dyno is running)'}`),
    chk(hidden.length > 0, hidden.length ? 'the autostarted dyno-app carries --hidden (the tray start)' : after.length ? `the autostarted dyno-app was started without --hidden (command: ${after[0].cmd || 'unreadable'})` : 'there is no autostarted dyno-app to carry --hidden'),
  ];
}

/** The service state out of `sc query` text: RUNNING, STOPPED, START_PENDING..., or 'absent' / 'unreachable' (no text at all). */
export function serviceState(text) {
  const t = String(text ?? '').trim();
  if (!t) return 'unreachable';
  return /STATE\s*:\s*\d+\s+(\S+)/.exec(t)?.[1] ?? 'absent';
}

/**
 * The helper service across a restart: LastBootUpTime must have moved (the guest really restarted), and the samples taken from the first moment
 * the guest answered must end RUNNING with no stop after it first ran (a boot-time race). A STOPPED service with start type DEMAND or AUTO is not
 * "fine": the helper is started by nothing then.
 * @param {{bootBefore:string|null, bootAfter:string|null, samples:{t:number, state:string}[]}} f
 */
export function judgeRestartService({ bootBefore, bootAfter, samples }) {
  const checks = [chk(Boolean(bootBefore && bootAfter && bootAfter > bootBefore), bootBefore && bootAfter && bootAfter > bootBefore ? `LastBootUpTime moved from ${bootBefore} to ${bootAfter}: the guest really restarted` : `LastBootUpTime did not move (before ${bootBefore ?? 'unreadable'}, after ${bootAfter ?? 'unreadable'}): this was not a restart, or the boot time could not be read`, bootBefore && bootAfter ? undefined : 'check')];
  const seen = samples.filter((s) => s.state !== 'unreachable');
  if (!seen.length) return [...checks, chk(false, 'no service sample was taken after the restart', 'check')];
  const firstRun = seen.findIndex((s) => s.state === 'RUNNING');
  const last = seen.at(-1);
  checks.push(chk(last.state === 'RUNNING', last.state === 'RUNNING' ? `the DynoHelper service is RUNNING at the end of ${seen.length} samples${firstRun >= 0 ? ` (first RUNNING at +${seen[firstRun].t} s)` : ''}` : `the DynoHelper service is ${last.state} at the end of ${seen.length} samples (it was not started by the restart)`));
  const lost = firstRun >= 0 ? seen.slice(firstRun).filter((s) => s.state !== 'RUNNING') : [];
  checks.push(chk(lost.length === 0, lost.length ? `the service ran and then showed ${lost.map((s) => `${s.state} at +${s.t} s`).join(', ')}: a boot-time race` : 'the service never left RUNNING once it ran'));
  return checks;
}

/**
 * Counts Dyno's tray icons BEFORE any icon is promoted (promoting IsPromoted moves icons into the taskbar and changes what is counted).
 * `metrics` are the icons the app log says it created: none means the log could not be read, which is a broken check, not a product fail.
 * Pass and fail have their own text.
 */
export function judgeTrayCount({ metrics, visible, overflow, measured = true, running = true }) {
  if (!measured) return [chk(false, 'the notification area could not be read (no notification-area button at all): the tray was not measured', 'check')];
  if (!metrics.length) return [chk(false, 'the app log names no tray icon it created, so the number of icons to find is unknown (metrics = 0)', 'check')];
  if (!running) return [{ ok: true, info: true, detail: 'Dyno is not running, so no icons are expected: the tray is not measured here' }];
  const n = visible + overflow;
  return [n === metrics.length
    ? chk(true, `${n} tray icon(s) found (${visible} visible, ${overflow} in the overflow) for ${metrics.length} created`)
    : chk(false, `Dyno is running but ${n} tray icon(s) were found for ${metrics.length} created (${visible} visible, ${overflow} in the overflow)`)];
}

/** An attempt by a standard user, printed by `try { ...; 'OK' } catch [UnauthorizedAccessException] { 'DENIED' } catch { "OTHER:<type>" }`. */
export const attemptProbe = (cmd) => `try { ${cmd}; 'OK' } catch [System.UnauthorizedAccessException] { 'DENIED' } catch { 'OTHER:' + $_.Exception.GetType().Name }`;

/** 'refused' only for access denied; a path that is not there is not a refusal. */
export function classifyAttempt(out) {
  const t = String(out ?? '').trim().split('\n').at(-1) ?? '';
  return t === 'DENIED' ? 'refused' : t === 'OK' ? 'succeeded' : t.startsWith('OTHER:') ? `other:${t.slice(6)}` : 'unreadable';
}

/** One refusal check from the probe text, plus the control's. */
export function judgeRefusal(what, out) {
  const c = classifyAttempt(out);
  if (c === 'refused') return chk(true, `standard user: ${what}: refused (access denied)`);
  if (c === 'succeeded') return chk(false, `standard user: ${what}: SUCCEEDED (must be refused)`);
  return chk(false, `standard user: ${what}: the attempt failed for another reason (${c}), which is not a refusal`, 'check');
}

/** The control: the same probe in a folder the user owns must succeed, else the probe measures nothing. */
export const judgeControl = (out) => chk(classifyAttempt(out) === 'succeeded', classifyAttempt(out) === 'succeeded' ? 'control: the same probe in a user-writable folder succeeded' : `control: the same probe in a user-writable folder did not succeed (${classifyAttempt(out)}), so the denials prove nothing`, classifyAttempt(out) === 'succeeded' ? undefined : 'check');

/** The exploit step needs the install to have finished and smartctl.exe to be there; otherwise it is not measured (not a pass). */
export function judgeExploitPrecondition({ outcome, smartctlExists }) {
  const done = outcome === 'finished';
  return [
    chk(done, done ? 'the install finished' : `the install did not finish (${outcome}); there is no installed tool to attack`, done ? undefined : outcome === 'failed' ? undefined : 'check'),
    chk(smartctlExists === true, smartctlExists ? 'smartctl.exe exists in the tools tree' : 'smartctl.exe does not exist in the tools tree', smartctlExists === true ? undefined : 'check'),
  ];
}

/** `True`/`False`/else to 'yes' | 'no' | 'unreadable': a dead SSH prints nothing and must not read as "removed". */
export function triState(text) {
  const t = String(text ?? '').trim().split('\n').at(-1)?.trim();
  return t === 'True' ? 'yes' : t === 'False' ? 'no' : 'unreadable';
}

/** What the uninstall left, judged from tri-state facts (yes/no/unreadable); an unreadable fact is a broken check. */
export function judgeUninstall(f, prog = 'C:\\Program Files\\Dyno') {
  const gone = (key, okText, badText) => {
    const v = f[key];
    if (v === 'unreadable') return chk(false, `${okText.replace(/ is removed| are gone| is left\b.*/, '')}: the read failed (SSH or PowerShell printed nothing), so "removed" is not known`, 'check');
    return chk(v === 'no', v === 'no' ? okText : badText);
  };
  const procs = f.processes;
  return [
    gone('folder', `${prog} is removed`, `${prog} is still there after the uninstall${f.folderFiles ? ` (${f.folderFiles})` : ''}`),
    gone('uninstallKey', 'the machine-wide uninstall entry is removed', 'the machine-wide uninstall entry is still in the registry'),
    gone('service', 'no DynoHelper service is left', 'the DynoHelper service is still registered'),
    procs === 'unreadable' || procs === null || procs === undefined || Number.isNaN(procs)
      ? chk(false, 'the process count could not be read, so "no Dyno process is left" is not known', 'check')
      : chk(procs === 0, procs === 0 ? 'no Dyno process (app, agent, helper, bench) is left' : `${procs} Dyno process(es) still run`),
    gone('runMachine', 'no machine-wide start-at-login value for Dyno is left', `a machine-wide start-at-login value for Dyno is left: ${f.runMachineNames ?? ''}`),
    gone('runUser', 'no start-at-login value for Dyno is left in paddock-test\'s own HKCU', `a start-at-login value for Dyno is left in paddock-test's HKCU: ${f.runUserNames ?? ''}`),
  ];
}

/** `served:` lines of the helper log to a map of kind to count ("served: processes=12, firewall=1"). */
export function parseServed(lines) {
  const out = new Map();
  for (const l of lines) for (const m of String(l).replace(/^.*served:/i, '').matchAll(/([A-Za-z][\w-]*)\s*[=:]\s*(\d+)/g)) out.set(m[1].toLowerCase(), Math.max(out.get(m[1].toLowerCase()) ?? 0, Number(m[2])));
  return out;
}

/** The served counts per kind: every expected kind needs a count above 0 (a line that merely exists proves no reading was served). */
export function judgeServed(lines, kinds) {
  if (!lines.length) return [chk(false, 'no "served:" line in helper.log after the service was stopped')];
  const m = parseServed(lines);
  if (!m.size) return [chk(false, `the "served:" line has no kind=count pairs (${lines.at(-1).slice(0, 100)}): the format is not the one this check reads`, 'check')];
  return kinds.map((k) => chk((m.get(k.toLowerCase()) ?? 0) > 0, (m.get(k.toLowerCase()) ?? 0) > 0 ? `served ${k}: ${m.get(k.toLowerCase())}` : `served ${k}: ${m.has(k.toLowerCase()) ? 0 : 'not in the line'}`));
}

/** The Windows disk-health step's Lifespan check: a real wear or forecast value, none of the failure wording. */
export function judgeWear(lifeText) {
  const t = String(lifeText ?? '').replace(/\s+/g, ' ');
  const bad = [/not installed/i, ...FAILURE_WORDING].find((re) => re.test(t));
  return [
    chk(LIFESPAN_VALUE.test(t), LIFESPAN_VALUE.test(t) ? 'Lifespan shows a wear or forecast value' : 'Lifespan shows no wear or forecast value (the heading or an intro sentence is not a reading)'),
    chk(!bad, bad ? `Lifespan says ${bad}: the tool did not take the reading` : 'Lifespan has none of the failure wording'),
  ];
}

/**
 * The outcome of pressing "Download and install" (win-helper.mjs `pressInstallAndWait`): finished is the install; "no install button" counts only
 * when smartctl.exe is really there (a tool that is already installed), else nothing was installed and the step proves nothing; `failed` is the
 * product's, `timeout` is not known (a broken check).
 */
export function judgeInstallOutcome(label, outcome, smartctlExists) {
  if (outcome === 'finished') return chk(smartctlExists === true, smartctlExists ? `${label}: the install finished and smartctl.exe is there` : `${label}: the install finished but smartctl.exe is not there`);
  if (outcome === 'no install button') return chk(smartctlExists === true, smartctlExists ? `${label}: the tool is already installed (no install button, smartctl.exe is there)` : `${label}: no install button, yet smartctl.exe is not there: nothing was installed`, smartctlExists === true ? undefined : 'check');
  if (outcome === 'failed') return chk(false, `${label}: the install failed (the helper logged "InstallTool failed")`);
  return chk(false, `${label}: the install did not finish (${outcome})`, 'check');
}

/**
 * An install run as a person would (a second install after an uninstall, or over a planted leftover `uninstall.exe`): the programs and the machine-wide
 * uninstall entry exist and the installer ended 0. With `leftoverReplaced` known, the planted file must be the installer's own again.
 * @param {{exit:number|null, app:'yes'|'no'|'unreadable', key:'yes'|'no'|'unreadable', uninstallIsPlanted?:'yes'|'no'|'unreadable'}} f
 */
export function judgeInstalled(label, f) {
  const unread = [f.app, f.key].includes('unreadable');
  const checks = [
    chk(f.exit === 0, f.exit === 0 ? `${label}: the installer ended 0` : `${label}: the installer ended ${f.exit}`, f.exit === null ? 'check' : undefined),
    chk(f.app === 'yes', f.app === 'yes' ? `${label}: dyno-app.exe is installed` : f.app === 'unreadable' ? `${label}: the install folder could not be read` : `${label}: dyno-app.exe is not installed`, f.app === 'unreadable' ? 'check' : undefined),
    chk(f.key === 'yes', f.key === 'yes' ? `${label}: the machine-wide uninstall entry exists` : f.key === 'unreadable' ? `${label}: the uninstall entry could not be read` : `${label}: the machine-wide uninstall entry is missing`, f.key === 'unreadable' ? 'check' : undefined),
  ];
  if (f.uninstallIsPlanted !== undefined) checks.push(chk(f.uninstallIsPlanted === 'no', f.uninstallIsPlanted === 'no' ? `${label}: the leftover uninstall.exe was replaced by the installer's own` : f.uninstallIsPlanted === 'yes' ? `${label}: the planted leftover uninstall.exe is still there` : `${label}: uninstall.exe could not be read`, f.uninstallIsPlanted === 'unreadable' ? 'check' : undefined));
  void unread;
  return checks;
}

/**
 * The installer killed part-way: nothing half-installed may be presented as installed. The machine-wide uninstall entry is the product's "installed" fact: it must be
 * absent (Not installed), or the install must have completed whole (the kill came too late: a broken check, since nothing was interrupted).
 */
export function judgeKilledInstall({ killed, key, app }) {
  if (!killed) return [chk(false, 'the installer was not running when the kill was due (it had already ended), so nothing was interrupted', 'check')];
  if (key === 'unreadable' || app === 'unreadable') return [chk(false, 'the machine state after the kill could not be read', 'check')];
  if (key === 'yes' && app === 'yes') return [chk(false, 'the install had completed before the kill: nothing was interrupted', 'check')];
  return [chk(key === 'no', key === 'no' ? 'after the kill the machine-wide uninstall entry is absent: the product reads as Not installed' : 'after the kill an uninstall entry exists without a complete install: a half-installed product is presented as installed')];
}
