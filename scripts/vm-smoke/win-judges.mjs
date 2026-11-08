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
export function judgeAutostart({ boot, procs, expectOn, control = null }) {
  const out = judgeAutostartCore({ boot, procs, expectOn });
  if (!control || !expectOn || out.every((c) => c.ok)) return out;
  // Why nothing started: the StartupApproved state of the "Dyno" value (disabled there = Windows was told not to run it) and whether ANY other Run-key program started at
  // this logon (none = the guest did not process the Run key at all: the measurement's, not Dyno's).
  if (control.approved === 'disabled') return [...out, chk(false, 'StartupApproved\\Run marks the Dyno value disabled: Windows was told not to run it at logon')];
  if (!control.othersStarted?.length) return [...out.map((c) => (c.ok ? c : { ...c, kind: 'check', detail: `${c.detail} (no other Run-key program started at this logon either: the guest did not process the Run key)` })), chk(false, 'control: no Run-key program (OneDrive, Edge auto-launch) started after this logon', 'check')];
  return [...out, { ok: true, info: true, detail: `control: other Run-key programs started at this logon (${control.othersStarted.join(', ')}), so the Run key was processed` }];
}

function judgeAutostartCore({ boot, procs, expectOn }) {
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
/**
 * The app's own Shell_NotifyIcon state after `since` (ISO "s", the boot): its "tray diagnose" lines, which report the shell's answer per icon ("shell holds the icon true")
 * and the count ("N of M icon(s) shown"). null when there is no such line after `since`.
 */
export function parseTrayDiagnosis(lines, since) {
  const after = [].concat(lines ?? []).map(String).filter((l) => /tray diagnose:/.test(l)).filter((l) => { const t = /(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)/.exec(l)?.[1]; return t && (!since || t > since); });
  if (!after.length) return null;
  const counts = after.map((l) => /(\d+) of (\d+) icon\(s\) shown/.exec(l)).filter(Boolean);
  const last = counts.at(-1);
  const icons = after.filter((l) => /shell holds the icon (true|false)/.test(l));
  return { shown: last ? Number(last[1]) : null, of: last ? Number(last[2]) : null, held: icons.filter((l) => /shell holds the icon true/.test(l)).length, notHeld: icons.filter((l) => /shell holds the icon false/.test(l)).length, lines: after.slice(-6) };
}

export function judgeTrayCount({ metrics, visible, overflow, measured = true, running = true, registered = null, diagnosis = null }) {
  // Second probe when UI Automation reads nothing: the app's own Shell_NotifyIcon diagnosis after the boot (the shell's answer per icon).
  if (!measured && diagnosis && diagnosis.of !== null && metrics.length) {
    const ok = diagnosis.shown === metrics.length && diagnosis.of === metrics.length && diagnosis.notHeld === 0;
    return [chk(ok, ok ? `the app's Shell_NotifyIcon diagnosis after the boot: ${diagnosis.shown} of ${diagnosis.of} icon(s) shown, the shell holds each (${diagnosis.held} confirmed); UI Automation could not read the notification area on this guest` : `the app's Shell_NotifyIcon diagnosis after the boot: ${diagnosis.shown} of ${diagnosis.of} shown, ${diagnosis.notHeld} not held by the shell, for ${metrics.length} created`)];
  }
  // The fallback when UI Automation sees no notification-area button at all (the Windows 11 Arm guest): the shell's own registry list of icons
  // (HKCU\Control Panel\NotifyIconSettings) proves only that icons were REGISTERED, not that they are on screen, so it is information and the row stays "not measured".
  if (!measured) return [chk(false, `not measured: the notification area could not be read by UI Automation on this guest (no notification-area button at all)${registered === null ? '' : `; fallback read: ${registered} icon(s) registered with the shell in HKCU NotifyIconSettings for ${metrics.length} created (registered, not seen on screen)`}`, 'check')];
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

/** Occurrences of the product's info-level tray log line for a right click in the app log text. */
export const rightClicksInLog = (log) => (String(log ?? '').match(/tray click: Right Up/g) ?? []).length;

/**
 * The tray right-click probe (ER-33): the click is proven to have REACHED the tray icon only by the app's own log line `tray click: Right Up` (one more than before the
 * click); the menu is read through UI Automation. A log line without a menu is a probe that cannot see a native menu unless a screenshot shows it: reported as a broken
 * check (the screenshot is saved for a person), never as a product failure; no log line means the click did not arrive at all, also a broken check.
 * @param {{logBefore:number, logAfter:number, menuItems:number}} f
 */
export function judgeTrayClick({ logBefore, logAfter, menuItems }) {
  const arrived = logAfter > logBefore;
  if (!arrived) return [chk(false, 'the app logged no "tray click: Right Up": the right click did not reach the tray icon (the probe, not the product, is not working)', 'check')];
  if (menuItems === 0) return [chk(true, 'the app logged "tray click: Right Up"'), chk(false, 'the click arrived but UI Automation sees no menu: either the menu did not open or this probe cannot see a native menu; see the screenshot taken while it was open', 'check')];
  return [chk(true, 'the app logged "tray click: Right Up"'), chk(true, `the right click opened a menu (${menuItems} item(s) read)`)];
}

/**
 * The first uninstall after a boot (fix/uninstall-cold-leftover, b48df1818: the shared include retries the uninstall.exe delete for up to 5 s while XtaCache still reads it).
 * Pass: the install folder is gone. A folder that stays is accepted ONLY as the noted fallback: just uninstall.exe is left, the lock past 5 s was logged, and a reinstall works.
 * @param {{folder:'yes'|'no'|'unreadable', onlyUninstallExe:boolean|null, noteLogged:boolean, reinstallOk:boolean|null}} f
 */
export function judgeColdUninstall(f) {
  if (f.folder === 'unreadable') return [chk(false, 'the install folder could not be read after the uninstall', 'check')];
  if (f.folder === 'no') return [chk(true, 'the install folder is gone after the first uninstall (the retries removed uninstall.exe)')];
  const checks = [chk(false, `the install folder is still there after the first uninstall${f.onlyUninstallExe ? ' (only uninstall.exe)' : ' (more than uninstall.exe)'}`)];
  if (f.onlyUninstallExe) {
    checks.push(chk(f.noteLogged, f.noteLogged ? 'the lock past 5 s was reported with its note' : 'the leftover was not reported with a note'));
    checks.push(f.reinstallOk === null ? chk(false, 'a reinstall over the leftover was not tried', 'check') : chk(f.reinstallOk, f.reinstallOk ? 'a reinstall over the leftover works' : 'a reinstall over the leftover failed'));
  }
  return checks;
}

/** The fallback is a pass only when every check of it holds: gone, or (only uninstall.exe, noted, reinstall works). */
export const coldUninstallAccepted = (f) => f.folder === 'no' || (f.folder === 'yes' && f.onlyUninstallExe === true && f.noteLogged && f.reinstallOk === true);

/** How many "InstallTool finished" and "InstallTool failed" lines a helper.log text holds. */
export const installEvents = (text) => ({ finished: (String(text).match(/InstallTool finished/g) ?? []).length, failed: (String(text).match(/InstallTool failed/g) ?? []).length });

/**
 * The outcome of the install this step started, from the WHOLE log counted before and now: a new "finished" or "failed" line. The old probe looked at the last 8 lines, and the
 * helper's own "processes answered" lines (two every 5 s) pushed the finished line out before the next look (0ad round 2: "install: timeout" with smartctl.exe in place).
 * @returns {'finished'|'failed'|null} null: nothing new yet
 */
export function installOutcomeSince(before, nowText) {
  const n = installEvents(nowText);
  if (n.failed > before.failed) return 'failed';
  if (n.finished > before.finished) return 'finished';
  return null;
}

/**
 * The guest-side watcher of the kill-the-installer row. The host cannot time a kill (an SSH round trip and a UAC answer take longer than the install, 0ad round 2: "the installer
 * ended before the kill"), so a script in the guest polls every 100 ms for the point the install is part-way (paddock-setup.exe's own log says `stop-all end`: the old files are
 * stopped and the new ones are being copied, the uninstall entry is not written yet) and ends the installer there, at once. The log is read with a shared-write open (the installer holds it open while it runs; Get-Content found nothing in 0ad round 4: NEVER). It writes KILLED (with what it ended and the log lines
 * it saw), TOO-LATE (the log reached `check-version` first) or NEVER (nothing within maxSec) to `out`. Run as SYSTEM, which may end the elevated installer.
 */
export function killWatcherScript({ log, out, maxSec = 150, point = 'phase=stop-all end', late = 'phase=check-version' }) {
  return `$ErrorActionPreference='SilentlyContinue'; Remove-Item '${out}'; $end=(Get-Date).AddSeconds(${maxSec}); while((Get-Date) -lt $end){ $t=''; try { $fs=[IO.File]::Open('${log}','Open','Read','ReadWrite,Delete'); $sr=New-Object IO.StreamReader($fs); $t=$sr.ReadToEnd(); $sr.Close() } catch { }; ` +
    `if($t -match '${late}'){ 'TOO-LATE' | Set-Content '${out}'; exit }; ` +
    `if($t -match '${point}'){ $v=@(Get-Process setup,paddock-setup | Where-Object { $_.Id -ne $PID }); $names=($v | ForEach-Object { $_.ProcessName + ':' + $_.Id }) -join ','; $v | Stop-Process -Force; ('KILLED ' + $names + ' at ' + (Get-Date).ToString('o')) | Set-Content '${out}'; exit }; Start-Sleep -Milliseconds 100 }; ('NEVER exists=' + (Test-Path '${log}') + ' len=' + $t.Length + ' setup-processes=' + (@(Get-Process setup,paddock-setup).Count)) | Set-Content '${out}'`;
}

/** The watcher's answer to the `killed` fact: true only when it ended at least one installer process at the point; otherwise why not. */
export function killWatcherOutcome(text) {
  const t = String(text ?? '').trim();
  const m = /^KILLED (\S*)/.exec(t);
  if (m && m[1]) return { killed: true, detail: `the installer was ended part-way (${t})` };
  if (m) return { killed: false, detail: `the point was reached but no installer process was running to end (${t})` };
  if (/^TOO-LATE/.test(t)) return { killed: false, detail: 'the install had passed the kill point (check-version in setup.log) before the watcher acted' };
  if (/^NEVER/.test(t)) return { killed: false, detail: `the installer never reached the kill point (no stop-all line in setup.log) within the watcher's time (${t})` };
  return { killed: false, detail: `the watcher wrote no answer (${t || 'empty'})` };
}

/**
 * `consent.exe` rows as the guest prints them (`pid=N start=ISO parent=N cmd=...`, one per process) to [{pid, start, parent, cmd}].
 */
export const parseConsent = (text) => String(text ?? '').split('\n').map((l) => /^pid=(\d+) start=(\S+) parent=(\d+)(?: cmd=(.*))?$/.exec(l.trim())).filter(Boolean).map((m) => ({ pid: Number(m[1]), start: m[2], parent: Number(m[3]), cmd: m[4] ?? '' }));

/**
 * Whether a prompt still up after the answer is a SECOND elevation or the first one lingering (0ad round 2 saw "the UAC prompt was still there 60 s after the answer" while the
 * install ended 0). Same pid and start time as the answered prompt: the first one (the answer did not take, or consent.exe was slow to leave); another pid or start: a second elevation.
 * @returns {{second: boolean, detail: string}}
 */
export function judgeConsentAfterAnswer(answered, later) {
  const key = (c) => `${c.pid}@${c.start}`;
  const was = new Set(answered.map(key));
  if (!answered.length) return { second: false, detail: 'no consent.exe was listed when the prompt was answered, so a later one cannot be told from the first' };
  const fresh = later.filter((c) => !was.has(key(c)));
  if (fresh.length) return { second: true, detail: `a different consent.exe (${fresh.map((c) => `pid ${c.pid}, parent ${c.parent}, started ${c.start}`).join('; ')}) appeared after the answer: a second elevation` };
  return { second: false, detail: later.length ? `the answered consent.exe (${answered.map((c) => `pid ${c.pid}`).join(', ')}) was still listed, no new one: the first prompt lingered` : 'no consent.exe after the answer' };
}

/** The actor's `rclickwatch` answer: `SEEN <ms>` / `NONE <ms>`, then `--windows` rows (handle, process, rect) and `--items` rows (name, centre). */
export function parseMenuWatch(text) {
  const t = String(text ?? '');
  const head = /^(SEEN|NONE)\s+(\d+)/m.exec(t);
  const part = (name, next) => { const i = t.indexOf(`--${name}`); if (i < 0) return ''; const j = next ? t.indexOf(`--${next}`, i) : -1; return t.slice(i + name.length + 2, j < 0 ? undefined : j).trim(); };
  const windows = part('windows', 'items').split('\n').map((l) => l.split('\t')).filter((c) => c.length >= 3).map(([handle, proc, rect]) => ({ handle, proc, rect: rect.trim() }));
  const items = part('items').split('\n').map((l) => l.split('\t')).filter((c) => c[0] && c[1]).map(([name, at]) => ({ name: name.trim(), at: at.trim().replace(',', ' ') }));
  return { seen: head?.[1] === 'SEEN', ms: head ? Number(head[2]) : null, windows, items, valid: Boolean(head) };
}

/**
 * The native tray menu rows (ER-33): the probe's own control (a menu of the harness's own notification icon) must be SEEN first, or nothing about Dyno is judged
 * (check broken). With a seen control, Dyno's right click that the app logged but that opened no native menu in both tries is a product failure; a click the app did not log is
 * the probe's.
 * @param {{control: ReturnType<typeof parseMenuWatch>|null, controlIcon: boolean, attempts: {logged: boolean, watch: ReturnType<typeof parseMenuWatch>}[]}} f
 */
export function judgeNativeMenu({ control, controlIcon, attempts }) {
  const checks = [];
  const ctlSeen = Boolean(control?.seen && control.items.length > 0);
  checks.push(chk(ctlSeen, ctlSeen ? `control: the probe sees the native menu of the harness's own notification icon (${control.items.map((i) => i.name).join(', ')}; first seen ${control.ms} ms after the click)` : !controlIcon ? 'control: the harness\'s own notification icon could not be found, so the probe was not shown to see a native tray menu' : `control: the probe did not see the native menu of the harness's own icon (${control?.seen ? 'a menu window, no items read' : 'no menu window'}), so it cannot judge Dyno's`, 'check'));
  const seenAny = attempts.find((a) => a.watch.seen);
  const logged = attempts.some((a) => a.logged);
  if (!logged) checks.push(chk(false, 'the app logged no "tray click: Right Up" for any attempt: the right click did not reach the tray icon (the probe, not the product)', 'check'));
  else if (seenAny) checks.push(chk(true, `the right click opened a native menu (first seen ${seenAny.watch.ms} ms after the click, ${seenAny.watch.items.length} item(s) read)`));
  else checks.push({ ...chk(false, `the app logged the right click ${attempts.filter((a) => a.logged).length} time(s) but no native menu window (class #32768) appeared within 6 s of either click; the screenshot after the click is saved`), ...(ctlSeen ? {} : { kind: 'check' }) });
  return checks;
}
