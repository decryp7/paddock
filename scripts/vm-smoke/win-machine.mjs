// The per-machine install's host-side steps (Dyno 1.1.0, family ADR 0053): an update announced as the installed version is refused by
// `paddock-setup.exe apply-update` (exit 27, "not newer", nothing left in updates\), and the uninstall removes the programs, the machine-wide
// entries and the start-at-login values. Run as the VM's administrator through the SSH admin key; the install itself (the real UAC prompt) is the suite's.
import { ssh, answerUac, winMcpStep } from './win-helper.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { quitStopStep } from './mcp-judges.mjs';
import { hungPageStep } from './hung-page.mjs';
import { Actor } from './win-act.mjs';
import { APP_LOG_PS } from './win-tray-icons.mjs';
import { killWatcherScript, killWatcherOutcome, judgeColdUninstall, coldUninstallAccepted, judgeInstalled, judgeKilledInstall } from './win-judges.mjs';
import { NOT_DRIVEN, ROW_TITLES, notDriven } from './release-rows.mjs';
import { swapNotDriven } from './upgrade-swap-row.mjs';
import { winSwapRow } from './win-swap.mjs';
import { hiddenStartStep } from './login-start-rows.mjs';
import { makeResult } from './verdict.mjs';
import { judgeUninstall as judgeUninstallFacts, triState } from './win-judges.mjs';

/** What the uninstall left, judged from tri-state facts (yes, no, unreadable): see win-judges.mjs. */
export const judgeUninstall = (f) => judgeUninstallFacts(f, PROG);

const PROG = 'C:\\Program Files\\Dyno';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const result = makeResult;

/** `apply-update` printed "EXIT=<n>" last. Returns the code, or null. */
export function exitCodeOf(text) {
  const m = /EXIT=(-?\d+)\s*$/.exec(text.trim());
  return m ? Number(m[1]) : null;
}

/** The first three numbers of a Windows ProductVersion ("1.1.0.0" is "1.1.0"). */
export function semverOf(productVersion) {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(productVersion.trim());
  return m ? `${m[1]}.${m[2]}.${m[3]}` : null;
}

/**
 * paddock-setup.exe, the program the installer's POSTINSTALL runs (check-version, clean-run) and apply-update and the uninstaller use. On the 7d06 round it went missing
 * seconds after the install (app log: error 1812, then 2), so every row that needs it first proves it is there, with FileVersion 1.1.0, and that setup.log shows the
 * POSTINSTALL steps; otherwise the row is a broken check. Windows Defender's view is always attached (Get-MpThreatDetection, Operational events 1116, 1117, 1006): does
 * Defender hold or remove the unsigned exe?
 */
export function setupExeFacts(label) {
  const raw = ssh(`$ErrorActionPreference='Continue'; $f='${PROG}\\paddock-setup.exe'; $e=$null; try { $i = Get-Item $f -ErrorAction Stop; 'PRESENT=1'; 'VERSION=' + $i.VersionInfo.FileVersion } catch { 'PRESENT=0'; 'ERROR=' + $_.Exception.Message }; $log = Get-Content '${PROG}\\updates\\setup.log' -ErrorAction SilentlyContinue; 'CHECKVERSION=' + [int][bool]($log | Select-String 'check-version'); 'CLEANRUN=' + [int][bool]($log | Select-String 'clean-run'); if ($Error.Count) { 'LASTERROR=' + ($Error[0] | Out-String).Trim().Replace([char]10,' ') }`, { admin: true });
  const v = (k) => new RegExp(`^${k}=(.*)$`, 'm').exec(raw)?.[1]?.trim() ?? null;
  const present = v('PRESENT') === '1';
  const version = v('VERSION');
  const okVersion = /^1\.1\.0(\.0)?$/.test(version ?? '');
  const setupLog = { checkVersion: v('CHECKVERSION') === '1', cleanRun: v('CLEANRUN') === '1' };
  const defender = defenderEvidence();
  const evidence = [`--- paddock-setup.exe (${label}): ${present ? `present, FileVersion ${version}` : `MISSING (${v('ERROR') ?? 'no error text'})`}; setup.log check-version ${setupLog.checkVersion ? 'yes' : 'no'}, clean-run ${setupLog.cleanRun ? 'yes' : 'no'}${v('LASTERROR') ? `; $Error[0]: ${v('LASTERROR').slice(0, 300)}` : ''}`, ...defender];
  const checks = [
    { ok: present && okVersion, kind: 'check', detail: present ? (okVersion ? `paddock-setup.exe is there (FileVersion ${version})` : `paddock-setup.exe has FileVersion ${version}, not 1.1.0`) : `paddock-setup.exe is missing from ${PROG} (${label}); see the Defender evidence` },
    { ok: setupLog.checkVersion && setupLog.cleanRun, kind: 'check', detail: setupLog.checkVersion && setupLog.cleanRun ? 'setup.log shows check-version and clean-run (POSTINSTALL ran)' : `setup.log lacks ${[!setupLog.checkVersion && 'check-version', !setupLog.cleanRun && 'clean-run'].filter(Boolean).join(' and ')}: POSTINSTALL did not run as designed` },
  ];
  return { present, version, ok: checks.every((c) => c.ok), checks, evidence };
}

/** Windows Defender: detections and the Operational log's 1116 (detected), 1117 (action taken), 1006 (malware found), newest first. */
export function defenderEvidence() {
  const det = ssh("Get-MpThreatDetection -ErrorAction SilentlyContinue | ForEach-Object { '' + $_.InitialDetectionTime + ' threat ' + $_.ThreatID + ' action ' + $_.ActionSuccess + ' ' + ($_.Resources -join ',') }; 'END'", { admin: true });
  const ev = ssh("Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-Windows Defender/Operational'; Id = 1116,1117,1006 } -MaxEvents 20 -ErrorAction SilentlyContinue | ForEach-Object { $_.TimeCreated.ToString('s') + ' id=' + $_.Id + ': ' + (($_.Message -replace '\\s+',' ').Substring(0, [Math]::Min(400, $_.Message.Length))) }; 'END'", { admin: true });
  const ver = ssh("$s = Get-MpComputerStatus; 'AMEngineVersion=' + $s.AMEngineVersion + ' AMProductVersion=' + $s.AMProductVersion + ' AntivirusSignatureVersion=' + $s.AntivirusSignatureVersion + ' signatures updated ' + $s.AntivirusSignatureLastUpdated", { admin: true }).trim();
  const lines = (t) => t.replace(/END\s*$/, '').trim().split('\n').map((l) => l.trim()).filter(Boolean);
  return [`--- Defender versions: ${ver || '(unreadable)'}`, '--- Defender Get-MpThreatDetection', ...(/END\s*$/.test(det) ? (lines(det).length ? lines(det) : ['(no detections)']) : ['(unreadable)']), '--- Defender Operational 1116/1117/1006', ...(/END\s*$/.test(ev) ? (lines(ev).length ? lines(ev) : ['(no events)']) : ['(unreadable)'])];
}

export function sameVersionRefused() {
  const checks = [];
  const pre = setupExeFacts('before apply-update');
  if (!pre.present) return result('Windows: an update from the same version is refused (not newer)', pre.checks, pre.evidence);
  const ver = semverOf(ssh(`(Get-Item '${PROG}\\dyno-app.exe').VersionInfo.ProductVersion`));
  checks.push({ ok: Boolean(ver), detail: ver ? `installed version ${ver}` : 'the installed version could not be read' });
  const hasSig = ssh('Test-Path C:\\paddock-test\\vmcheck\\setup.exe.sig') === 'True';
  if (!ver || !hasSig) {
    if (!hasSig) checks.push({ ok: false, detail: 'the installer has no .sig next to it, so the signed update cannot be built' });
    return result('Windows: an update from the same version is refused (not newer)', checks);
  }
  const cmd = `$sig = (Get-Content C:\\paddock-test\\vmcheck\\setup.exe.sig -Raw).Trim(); & '${PROG}\\paddock-setup.exe' apply-update --file C:\\paddock-test\\vmcheck\\setup.exe --signature $sig --version ${ver} --restart 2>&1 | Out-String; "EXIT=$LASTEXITCODE"`;
  const out = ssh(cmd, { admin: true, ms: 120_000 });
  const code = exitCodeOf(out);
  checks.push({ ok: code === 27, detail: code === 27 ? 'apply-update exit 27 (not newer than the installed version)' : `apply-update exit ${code} (expected 27 "not newer")` });
  const left = ssh(`Get-ChildItem '${PROG}\\updates' -Name -ErrorAction SilentlyContinue | Where-Object { $_ -ne 'setup.log' }`, { admin: true }).split('\n').map((l) => l.trim()).filter(Boolean);
  checks.push({ ok: left.length === 0, detail: left.length ? `a refused update left ${left.join(', ')} in updates\\` : 'a refused update leaves nothing in updates\\' });
  const tail = ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 8 -ErrorAction SilentlyContinue`, { admin: true }).split('\n');
  return result('Windows: an update from the same version is refused (not newer)', [...pre.checks, ...checks], [...out.split('\n').slice(0, 10), '--- setup.log tail', ...tail, ...pre.evidence], 'apply-update refused the same version (exit 27) and left nothing');
}

export async function uninstall() {
  const checks = [];
  ssh('Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
  const un = `${PROG}\\uninstall.exe`;
  const hasState = triState(ssh(`Test-Path '${un}'`, { admin: true }));
  const has = hasState === 'yes';
  checks.push({ ok: has, ...(hasState === 'unreadable' ? { kind: 'check' } : {}), detail: has ? 'uninstall.exe is in the install folder' : hasState === 'unreadable' ? 'the install folder could not be read (SSH printed nothing)' : 'no uninstall.exe in the install folder' });
  if (!has) return result('Windows: uninstall removes the programs and the machine-wide entries', checks);
  const pre = setupExeFacts('before the uninstall');
  // The uninstaller runs from the interactive session (see interactiveUninstall); its own exit code, the ~nsu copy's pid and exit code, and setup.log are kept.
  const runText = await interactiveUninstall('uninstall');
  let facts = {};
  // Three-state reads (yes, no, unreadable): a dead SSH prints nothing and must not read as "removed". The start-at-login value of the standard
  // user is read over the user's own SSH (that is paddock-test's HKCU, not the administrator's).
  const adminBool = (cmd) => triState(ssh(cmd, { admin: true }));
  for (let i = 0; i < 40; i++) {
    await sleep(3000);
    const count = ssh('(Get-Process dyno-app,dyno-agent,dyno-helper,dyno-bench -ErrorAction SilentlyContinue | Measure-Object).Count', { admin: true });
    const runMachine = ssh("(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*Dyno*' } | ForEach-Object { $_.Name }", { admin: true });
    const runUser = ssh("(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -ErrorAction SilentlyContinue).PSObject.Properties | Where-Object { $_.Value -like '*Dyno*' } | ForEach-Object { $_.Name }; 'END'");
    facts = {
      folder: adminBool(`Test-Path '${PROG}'`),
      folderFiles: ssh(`(Get-ChildItem '${PROG}' -Name -ErrorAction SilentlyContinue) -join ', '`, { admin: true }),
      uninstallKey: adminBool("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Dyno'"),
      service: adminBool('[bool](Get-Service DynoHelper -ErrorAction SilentlyContinue)'),
      processes: /^\d+$/.test(count.trim()) ? Number(count) : 'unreadable',
      // Machine-wide Run: an empty answer from a live SSH means none; the end marker of the user read tells a dead SSH from "no values".
      runMachine: adminBool("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run'") === 'unreadable' ? 'unreadable' : runMachine.trim() ? 'yes' : 'no',
      runMachineNames: runMachine.trim(),
      runUser: /END\s*$/.test(runUser) ? (runUser.replace(/END\s*$/, '').trim() ? 'yes' : 'no') : 'unreadable',
      runUserNames: runUser.replace(/END\s*$/, '').trim(),
    };
    if (facts.folder === 'no' && facts.uninstallKey === 'no') break;
  }
  const evidence = [`uninstall.exe /S from the interactive session: ${runText}`];
  if (facts.folder !== 'no') evidence.push('--- setup.log tail', ...ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 30 -ErrorAction SilentlyContinue`, { admin: true }).split('\n').slice(-30));
  // Without paddock-setup.exe (or with a POSTINSTALL that never ran) the uninstaller cannot do its part: what it leaves is then not a product verdict.
  const judged = judgeUninstall(facts).map((c) => (pre.present ? c : { ...c, ...(c.ok ? {} : { kind: 'check', detail: `${c.detail} (paddock-setup.exe was missing before the uninstall)` }) }));
  checks.push(...pre.checks, ...judged);
  evidence.push(...pre.evidence, ...defenderEvidence().map((l) => `[after the uninstall] ${l}`));
  return result('Windows: uninstall removes the programs and the machine-wide entries', checks, evidence, 'the programs, the uninstall entry and the start-at-login values are gone');
}

const SETUP = 'C:\\paddock-test\\vmcheck\\setup.exe';

/**
 * The uninstaller started from the test user's interactive session (scheduled task /it) with its UAC prompt answered over VNC. The NSIS uninstaller copies itself to
 * %TEMP%\~nsuN.tmp\Un_A.exe and exits 0 at once, so the exit code of the first process proves nothing: the copy is found by its path, waited for, and its pid and exit code are logged.
 * (An admin ssh session ends the copy with the session: the 2026-11-07 rounds saw exit 0 and nothing removed there.) Returns the log text.
 */
async function interactiveUninstall(label = 'uninstall-interactive') {
  const script = `$p = Start-Process '${PROG}\\uninstall.exe' -ArgumentList '/S' -PassThru; $null = $p.WaitForExit(90000); $first = 'first process pid ' + $p.Id + ' exit ' + $(if ($p.HasExited) { $p.ExitCode } else { 'still-running' }); $seen = @{}; $end = (Get-Date).AddSeconds(150); $quiet = 0; while ((Get-Date) -lt $end -and $quiet -lt 4) { $n = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*~nsu*' }); if ($n.Count -eq 0) { $quiet++ } else { $quiet = 0; foreach ($q in $n) { if (-not $seen.ContainsKey($q.Id)) { $seen[$q.Id] = $q } } }; Start-Sleep -Seconds 2 }; $copies = ($seen.Values | ForEach-Object { 'copy pid ' + $_.Id + ' exit ' + $(if ($_.HasExited) { $_.ExitCode } else { 'unknown' }) }) -join '; '; ($first + '; ' + $(if ($copies) { $copies } else { 'no ~nsu copy seen' })) | Set-Content C:\\paddock-test\\vmcheck\\un.out`;
  ssh(`Remove-Item C:\\paddock-test\\vmcheck\\un.out -ErrorAction SilentlyContinue; Set-Content C:\\paddock-test\\vmcheck\\un.ps1 '${script.replace(/'/g, "''")}'; schtasks /end /tn DynoUninstall 2>$null | Out-Null; schtasks /create /tn DynoUninstall /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\un.ps1" | Out-Null; schtasks /run /tn DynoUninstall | Out-Null`);
  const uac = await answerUac(label);
  let out = '';
  for (let i = 0; i < 80 && !/first process/.test(out); i++) { await sleep(3000); out = ssh('Get-Content C:\\paddock-test\\vmcheck\\un.out -ErrorAction SilentlyContinue'); }
  return `${uac.ok ? 'UAC answered' : uac.detail}; ${out.trim() || 'no result written'}`;
}

/**
 * Starts the staged installer in the test user's console session. A task cannot run a requireAdministrator program directly (error 740, no prompt): the task runs a
 * PowerShell script that starts it with Start-Process (ShellExecute, which shows the UAC prompt), waits, and writes the installer's exit code to a file.
 */
function startInstaller() {
  ssh(`Remove-Item C:\\paddock-test\\vmcheck\\reinstall.out -ErrorAction SilentlyContinue; Set-Content C:\\paddock-test\\vmcheck\\reinstall.ps1 '$p = Start-Process ${SETUP} -ArgumentList "/S" -PassThru; $p.WaitForExit(); $p.ExitCode | Set-Content C:\\paddock-test\\vmcheck\\reinstall.out'; schtasks /end /tn DynoReinstall 2>$null | Out-Null; schtasks /create /tn DynoReinstall /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\reinstall.ps1" | Out-Null; schtasks /run /tn DynoReinstall | Out-Null`);
}
const installerRunning = () => ssh('[bool](Get-Process -Name setup -ErrorAction SilentlyContinue)') === 'True';
const facts = () => ({
  app: triState(ssh(`Test-Path '${PROG}\\dyno-app.exe'`, { admin: true })),
  key: triState(ssh("Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Dyno'", { admin: true })),
});

/** One full install of the staged installer with its own UAC prompt answered over VNC; returns { exit, uac }. */
async function installOnce(label) {
  startInstaller();
  const uac = await answerUac(label);
  let ended = false;
  for (let i = 0; i < 100 && !ended; i++) { await sleep(3000); ended = !installerRunning(); }
  const exitText = ssh('Get-Content C:\\paddock-test\\vmcheck\\reinstall.out -ErrorAction SilentlyContinue');
  return { uac, exit: ended && /^-?\d+$/.test(exitText.trim()) ? Number(exitText.trim()) : null };
}

/**
 * The install rows after the uninstall (each with a later UAC prompt answered over VNC): reinstall over a planted leftover uninstall.exe, the installer killed
 * part-way, and install again. The text "Not installed" is judged by the machine state the app reads (the machine-wide uninstall entry), not read from the window.
 */
export async function installRows() {
  const steps = [];
  // 1. Reinstall over a leftover uninstall.exe: planted by the administrator where the uninstaller goes.
  ssh(`New-Item -ItemType Directory -Force '${PROG}' | Out-Null; Set-Content '${PROG}\\uninstall.exe' 'LEFTOVER-PLANTED'`, { admin: true });
  const a = await installOnce('reinstall-over-leftover');
  const planted = triState(ssh(`(Get-Content '${PROG}\\uninstall.exe' -Raw -ErrorAction SilentlyContinue) -match 'LEFTOVER-PLANTED'`, { admin: true }));
  steps.push(result(ROW_TITLES.winReinstall, [{ ok: a.uac.ok, detail: a.uac.detail }, ...judgeInstalled('reinstall over a leftover', { exit: a.exit, ...facts(), uninstallIsPlanted: planted })], [a.uac.detail], 'the install over a leftover uninstall.exe worked and replaced it'));
  // 2. Uninstall, then kill the installer part-way.
  ssh(`Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Process '${PROG}\\uninstall.exe' -ArgumentList '/S' -Wait`, { admin: true, ms: 180_000 });
  for (let i = 0; i < 20 && facts().key !== 'no'; i++) await sleep(3000);
  // The kill is made in the guest at a fixed point (killWatcherScript), not timed from here.
  const WATCH = 'C:\\paddock-test\\vmcheck\\kill-watch';
  ssh(`Remove-Item '${PROG}\\updates\\setup.log' -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force '${WATCH}' | Out-Null; Set-Content '${WATCH}\\w.ps1' '${killWatcherScript({ log: `${PROG}\\updates\\setup.log`, out: `${WATCH}\\k.txt` }).replace(/'/g, "''")}'; Remove-Item '${WATCH}\\k.txt' -ErrorAction SilentlyContinue; schtasks /create /tn DynoKillWatch /sc once /st 00:00 /ru SYSTEM /rl HIGHEST /f /tr "powershell -NoProfile -ExecutionPolicy Bypass -File ${WATCH}\\w.ps1" | Out-Null; schtasks /run /tn DynoKillWatch | Out-Null`, { admin: true });
  startInstaller();
  const uacK = await answerUac('kill-installer');
  let watched = '';
  for (let i = 0; i < 60 && !/^(KILLED|TOO-LATE|NEVER)/.test(watched.trim()); i++) { await sleep(3000); watched = ssh(`Get-Content '${WATCH}\\k.txt' -ErrorAction SilentlyContinue`, { admin: true }); }
  const w = killWatcherOutcome(watched);
  await sleep(4000);
  ssh('schtasks /delete /tn DynoKillWatch /f 2>$null | Out-Null', { admin: true });
  steps.push(result(ROW_TITLES.killInstaller, [{ ok: uacK.ok, detail: uacK.detail }, { ok: w.killed, kind: w.killed ? undefined : 'check', detail: w.detail }, ...judgeKilledInstall({ killed: w.killed, ...facts() })], [uacK.detail, w.detail], 'the machine reads Not installed after the installer was killed'));
  // 3. Install again.
  ssh(`Remove-Item '${PROG}' -Recurse -Force -ErrorAction SilentlyContinue`, { admin: true });
  const b = await installOnce('install-again');
  steps.push(result(ROW_TITLES.installAgain, [{ ok: b.uac.ok, detail: b.uac.detail }, ...judgeInstalled('install again', { exit: b.exit, ...facts() })], [b.uac.detail], 'install again works'));
  return steps;
}

/** The first uninstall after a boot (run at the end of the restart run, when the guest has just come up): see judgeColdUninstall. Accepts what fix/uninstall-cold-leftover defines. */
export async function coldUninstallStep() {
  ssh('Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
  // From the interactive session (an admin ssh session ends the NSIS copy with the session); the ~nsu copy's pid and exit code are logged and waited for.
  const runText = await interactiveUninstall('cold-uninstall');
  let folder = 'unreadable';
  for (let i = 0; i < 20; i++) { await sleep(3000); folder = triState(ssh(`Test-Path '${PROG}'`, { admin: true })); if (folder === 'no') break; }
  const names = ssh(`(Get-ChildItem '${PROG}' -Force -Name -ErrorAction SilentlyContinue) -join ','`, { admin: true }).split(',').filter(Boolean);
  const log = ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 40 -ErrorAction SilentlyContinue`, { admin: true });
  const f = { folder, onlyUninstallExe: folder === 'yes' ? names.filter((n) => !/^(uninstall\.exe|updates)$/i.test(n)).length === 0 : null, noteLogged: /uninstall.*(left|leftover|holder|lock)|XtaCache/i.test(log), reinstallOk: null };
  if (f.folder === 'yes' && f.onlyUninstallExe && f.noteLogged) { const r = await installOnce('cold-uninstall-reinstall'); f.reinstallOk = r.exit === 0 && facts().app === 'yes'; }
  const checks = judgeColdUninstall(f);
  const res = result(ROW_TITLES.winFirstUninstall, checks, [JSON.stringify(f), `uninstall: ${runText}`, `left: ${names.join(', ') || '(nothing)'}`, ...log.split('\n').slice(-12)], 'gone after the retries, or the noted fallback with a working reinstall');
  return coldUninstallAccepted(f) ? { ...res, status: 'pass', summary: 'the first uninstall after boot left nothing, or only the noted leftover and a reinstall works' } : res;
}

export const notDrivenWinRows = () => [notDriven(ROW_TITLES.secondUser, NOT_DRIVEN.secondUser)];

const NT = `Add-Type -Namespace N -Name P -MemberDefinition '[DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr h); [DllImport("ntdll.dll")] public static extern int NtResumeProcess(IntPtr h);'; `;
/**
 * The WebView2 renderers of dyno-app: msedgewebview2.exe processes of `--type=renderer` that have a dyno-app.exe among their ancestors (renderer, then the WebView2 browser
 * process, then the app). Two earlier filters read the command line (the user-data folder, then `--webview-exe-name`) and found none in 0ad rounds 2 and 4: the ancestry is the one
 * fact that does not depend on which switches Edge passes on.
 */
export const RENDERERS_PS = `$all = @(Get-CimInstance Win32_Process); $byId = @{}; foreach ($q in $all) { $byId[[int]$q.ProcessId] = $q }; function UnderDyno($p) { $c = $p; for ($i = 0; $i -lt 8 -and $c; $i++) { $c = $byId[[int]$c.ParentProcessId]; if ($c -and $c.Name -eq 'dyno-app.exe') { return $true } }; return $false }; $ids = @($all | Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match '--type=renderer' -and (UnderDyno $_) } | ForEach-Object { $_.ProcessId }); `;
/** What the guest has, for the evidence when no renderer is found: every WebView2 process with its parent, its type switch and its embedding program. */
export const WEBVIEW_LISTING_PS = `Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | ForEach-Object { 'pid=' + $_.ProcessId + ' parent=' + $_.ParentProcessId + ' ' + (($_.CommandLine -split ' ' | Where-Object { $_ -match '^--(type|webview-exe-name)=' }) -join ' ') }`;
const renderers = RENDERERS_PS;

/** A hung or paused visible page on Windows: the WebView2 renderer of dyno-app is suspended (NtSuspendProcess, no debugger); the question is read from the window list. Keep Waiting cannot be pressed here (no button lookup in the actor), so the doubling stays a broken check. */
// CIM (Win32_Process) answers "Access denied" to the standard user over SSH (a network logon): every process query of this row runs as the administrator key (0ad round 6).
export async function winHung() {
  ssh('Set-Content C:\\paddock-test\\vmcheck\\launch-hung.ps1 \'Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"\'; schtasks /create /tn DynoHungLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch-hung.ps1" | Out-Null; schtasks /run /tn DynoHungLaunch | Out-Null');
  // Condition wait (up to 90 s) for the app and a WebView2 renderer under it, never a fixed sleep; when none comes, what the launch task did is the evidence.
  let up = false;
  for (let i = 0; i < 30 && !up; i++) { await sleep(3000); up = Number(ssh(`${NT}${renderers}$ids.Count`, { admin: true }).trim().split('\n').at(-1)) > 0; }
  const launchDiag = up ? 'a WebView2 renderer of dyno-app was up' : `no renderer after 90 s; task: ${ssh("schtasks /query /tn DynoHungLaunch /v /fo list | Select-String 'Last Result|Status|Last Run'").replace(/\s*\n\s*/g, ' | ')}; script: ${ssh('Get-Content C:\\paddock-test\\vmcheck\\launch-hung.ps1 -ErrorAction SilentlyContinue')}; dyno processes: ${ssh("(Get-Process dyno* -ErrorAction SilentlyContinue | ForEach-Object { $_.Name + ':' + $_.Id }) -join ','")}`;
  const actor = new Actor(); actor.start(); await sleep(6000);
  // The guest's clock against this host's, for dating log lines the polls missed.
  const gMs = Number(ssh('[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()').trim().split('\n').at(-1)); const hMs = Date.now();
  const r = await hungPageStep({
    clockOffsetMs: Number.isFinite(gMs) ? hMs - gMs : undefined,
    now: () => Date.now() / 1000, sleep,
    suspend: async () => { const out = ssh(`${NT}${renderers}foreach ($i in $ids) { [void][N.P]::NtSuspendProcess((Get-Process -Id $i).Handle) }; $ids.Count`, { admin: true }); if (!(Number(out.trim().split('\\n').at(-1)) > 0)) throw new Error(`no WebView2 renderer of dyno-app was found (dyno processes: ${ssh("Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^dyno' } | ForEach-Object { $_.Name + ' pid=' + $_.ProcessId + ' session=' + $_.SessionId + ' ' + $_.CommandLine }", { admin: true }).replace(/\s*\n\s*/g, ' | ').slice(0, 500) || 'none'}; WebView2 processes: ${ssh(WEBVIEW_LISTING_PS, { admin: true }).replace(/\s*\n\s*/g, ' | ').slice(0, 600) || 'none'})`); },
    resume: async () => { ssh(`${NT}${renderers}foreach ($i in $ids) { [void][N.P]::NtResumeProcess((Get-Process -Id $i).Handle) }`, { admin: true }); },
    // Only the watchdog's lines, filtered in the guest: reading the whole log three times over SSH took longer than the watchdog's questions come (0ad round 6: the line was in the log, the poll never returned it).
    logLines: async () => ssh(`Get-ChildItem -Path "$env:LOCALAPPDATA\\Dyno","$env:APPDATA\\Dyno" -Recurse -Depth 3 -Filter 'dyno.*.log' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike 'dyno-agent*' } | ForEach-Object { Select-String -Path $_.FullName -Pattern 'stopped responding|has not answered for a long time|Not-responding question|responds again' | ForEach-Object { $_.Line } }`, { ms: 120_000 }).split('\\n'),
    questions: async (phase) => { const n = (await actor.send('windows').catch(() => '')).split('\\n').filter((l) => /not responding|Keep Waiting/i.test(l)).length; return phase === 'open' && n === 0 ? null : n; },
    // After the run: the whole filtered log once more, so a line the polls missed is named in the evidence.
    finalLines: async () => ssh(`Get-ChildItem -Path "$env:LOCALAPPDATA\\Dyno","$env:APPDATA\\Dyno" -Recurse -Depth 3 -Filter 'dyno.*.log' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike 'dyno-agent*' } | ForEach-Object { Select-String -Path $_.FullName -Pattern 'has not answered for a long time' | ForEach-Object { $_.Line } }`, { ms: 120_000 }).split('\\n'),
    keepWaiting: async () => false, shot: async (name) => actor.shot(name),
  }, ROW_TITLES.hungPage);
  await actor.send('quit', 5000).catch(() => {});
  ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force');
  r.evidence = [...(r.evidence ?? []), `launch: ${launchDiag}`];
  return r;
}

/** "Quit and stop measuring" as the LAST step of the run (it ends the app and the agent): menu click over the DevTools port, 60 s of no Dyno process, reopen. The reboot half is not tried here. */
export async function winQuitStop() {
  const launch = () => ssh('Set-Content C:\\paddock-test\\vmcheck\\launch-qs.ps1 \'$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"\'; schtasks /create /tn DynoQsLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch-qs.ps1" | Out-Null; schtasks /run /tn DynoQsLaunch | Out-Null');
  const count = async () => Number(ssh('(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count')) || 0;
  return quitStopStep({
    now: () => Date.now(), sleep,
    trigger: async () => {
      // The hidden-start row before this left a Dyno without the DevTools port running; a launch would hand over to it. Start a fresh one with the port.
      ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force'); await sleep(4000);
      launch();
      const t = tunnel();
      try {
        // Condition wait (up to 120 s) for the DevTools port and the app's main page, not a fixed 25 s (0ad round 7: no page yet on a loaded VM, "reading 'evaluate'" of undefined).
        let browser = null; let main = null;
        for (let i = 0; i < 40 && !main; i++) {
          await sleep(3000);
          try { browser = await connect(); main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id=') && p.url() !== 'about:blank') ?? null; } catch { main = null; }
        }
        if (!main) throw new Error('the app\'s main page did not appear on the DevTools port within 120 s');
        const clicked = await main.evaluate(async () => {
          const items = () => [...document.querySelectorAll('[role=menuitem]')];
          if (!items().some((e) => e.textContent.trim().startsWith('Quit and stop'))) { [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'File')?.click(); await new Promise((r) => setTimeout(r, 600)); }
          const it = items().find((e) => e.textContent.trim().startsWith('Quit and stop'));
          if (!it) return false;
          it.click(); return true;
        }).catch(() => true); // the page goes away with the app
        if (!clicked) throw new Error('no "Quit and stop measuring" item in the File menu');
      } finally { t.close(); }
    },
    count, reopen: async () => { launch(); await sleep(30_000); return count(); }, // the reboot half is the restart run's (win-reboot.mjs)
  });
}

const RUN_KEY = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run';
/** The entry's own command (HKCU Run "Dyno") started in the interactive session; judged: the agent answers and dyno-app has no main window. */
export async function winHiddenStart() {
  const cmd = ssh(`(Get-ItemProperty '${RUN_KEY}' -ErrorAction SilentlyContinue).Dyno`).trim().split('\n').at(-1) || `"${PROG}\\dyno-app.exe" --hidden`;
  const r = await hiddenStartStep({
    sleep,
    stop: async () => { ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force'); await sleep(4000); },
    startEntry: async () => { ssh(`Set-Content C:\\paddock-test\\vmcheck\\run-entry.cmd '${cmd.replace(/'/g, "''")}'; schtasks /create /tn DynoRunEntry /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c C:\\paddock-test\\vmcheck\\run-entry.cmd" | Out-Null; schtasks /run /tn DynoRunEntry | Out-Null`); },
    agentUp: async () => /"cpuPercent"/.test(ssh(`& '${PROG}\\dyno.exe' status --json 2>$null`)),
    windows: async () => ssh("Get-Process dyno-app -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | ForEach-Object { 'pid ' + $_.Id + ' title ' + $_.MainWindowTitle }").split('\n').map((l) => l.trim()).filter(Boolean),
  });
  r.evidence = [...(r.evidence ?? []), `entry command: ${cmd}`];
  return r;
}
/** The `--only` rows that run after Quit (win-rows.mjs), each a function returning a step or steps. */
export const afterRowFns = {
  'hung-page': () => winHung(),
  'install-rows': () => installRows(),
  'upgrade-swap': () => winSwapRow({ oldStaged: process.env.PADDOCK_OLD_SETUP === '1' }),
  'second-user': () => notDrivenWinRows(),
};
export const afterRowSteps = (ids) => ids.map((k) => afterRowFns[k]);
export const afterQuitSteps = [() => sameVersionRefused(), () => winMcpStep(), () => uninstall(), () => installRows(), () => winSwapRow({ oldStaged: process.env.PADDOCK_OLD_SETUP === '1' }), () => notDrivenWinRows(), () => winHung(), () => winHiddenStart(), () => winQuitStop()];
