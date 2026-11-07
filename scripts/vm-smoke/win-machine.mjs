// The per-machine install's host-side steps (Dyno 1.1.0, family ADR 0053): an update announced as the installed version is refused by
// `paddock-setup.exe apply-update` (exit 27, "not newer", nothing left in updates\), and the uninstall removes the programs, the machine-wide
// entries and the start-at-login values. Run as the VM's administrator through the SSH admin key; the install itself (the real UAC prompt) is the suite's.
import { ssh, answerUac, winMcpStep } from './win-helper.mjs';
import { connect, tunnel } from './win-cdp.mjs';
import { quitStopStep } from './mcp-judges.mjs';
import { hungPageStep } from './hung-page.mjs';
import { Actor } from './win-act.mjs';
import { APP_LOG_PS } from './win-tray-icons.mjs';
import { judgeColdUninstall, coldUninstallAccepted, judgeInstalled, judgeKilledInstall } from './win-judges.mjs';
import { NOT_DRIVEN, ROW_TITLES, notDriven } from './release-rows.mjs';
import { swapNotDriven } from './upgrade-swap-row.mjs';
import { winSwapRow } from './win-swap.mjs';
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

export function sameVersionRefused() {
  const checks = [];
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
  return result('Windows: an update from the same version is refused (not newer)', checks, [...out.split('\n').slice(0, 10), '--- setup.log tail', ...tail], 'apply-update refused the same version (exit 27) and left nothing');
}

export async function uninstall() {
  const checks = [];
  ssh('Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
  const un = `${PROG}\\uninstall.exe`;
  const hasState = triState(ssh(`Test-Path '${un}'`, { admin: true }));
  const has = hasState === 'yes';
  checks.push({ ok: has, ...(hasState === 'unreadable' ? { kind: 'check' } : {}), detail: has ? 'uninstall.exe is in the install folder' : hasState === 'unreadable' ? 'the install folder could not be read (SSH printed nothing)' : 'no uninstall.exe in the install folder' });
  if (!has) return result('Windows: uninstall removes the programs and the machine-wide entries', checks);
  // The uninstaller's own exit code and log are kept (a silent uninstall that does nothing must be explained): started as before over the administrator's ssh, waited for up to 60 s.
  const sshRun = ssh(`$p = Start-Process '${un}' -ArgumentList '/S' -PassThru; $null = $p.WaitForExit(60000); 'EXIT=' + $(if ($p.HasExited) { $p.ExitCode } else { 'still-running' })`, { admin: true, ms: 120_000 });
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
  const evidence = [`uninstall.exe /S over the administrator's ssh: ${sshRun.trim().split('\n').at(-1)}`];
  if (facts.folder !== 'no') {
    evidence.push('--- setup.log tail', ...ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 30 -ErrorAction SilentlyContinue`, { admin: true }).split('\n').slice(-30));
    evidence.push('--- temp uninstaller leftovers', ssh("Get-ChildItem $env:TEMP,'C:\\Windows\\Temp' -Filter '~nsu*' -ErrorAction SilentlyContinue | ForEach-Object FullName", { admin: true }));
    // The second way: the same uninstaller from the interactive session of the test user, with its own UAC prompt answered over VNC (the GUI-automation path).
    const w = await interactiveUninstall();
    evidence.push(`uninstall.exe /S from the interactive session: ${w}`);
    checks.push({ ok: true, info: true, detail: `second way (interactive session + UAC): ${w}; folder afterwards: ${triState(ssh(`Test-Path '${PROG}'`, { admin: true }))}` });
  }
  checks.push(...judgeUninstall(facts));
  return result('Windows: uninstall removes the programs and the machine-wide entries', checks, evidence, 'the programs, the uninstall entry and the start-at-login values are gone');
}

const SETUP = 'C:\\paddock-test\\vmcheck\\setup.exe';

/** The uninstaller started from the test user's interactive session (scheduled task, /it) with a UAC prompt answered over VNC; returns its exit code text. */
async function interactiveUninstall() {
  ssh(`Remove-Item C:\\paddock-test\\vmcheck\\un.out -ErrorAction SilentlyContinue; Set-Content C:\\paddock-test\\vmcheck\\un.ps1 '$p = Start-Process "${PROG}\\uninstall.exe" -ArgumentList "/S" -PassThru; $null = $p.WaitForExit(90000); "EXIT=" + $(if ($p.HasExited) { $p.ExitCode } else { "still-running" }) | Set-Content C:\\paddock-test\\vmcheck\\un.out'; schtasks /end /tn DynoUninstall 2>$null | Out-Null; schtasks /create /tn DynoUninstall /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\un.ps1" | Out-Null; schtasks /run /tn DynoUninstall | Out-Null`);
  const uac = await answerUac('uninstall-interactive');
  let out = '';
  for (let i = 0; i < 40 && !/EXIT=/.test(out); i++) { await sleep(3000); out = ssh('Get-Content C:\\paddock-test\\vmcheck\\un.out -ErrorAction SilentlyContinue'); }
  await sleep(10_000);
  return `${uac.ok ? 'UAC answered' : uac.detail}; ${out.trim() || 'no exit code'}`;
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
  startInstaller();
  const uacK = await answerUac('kill-installer');
  await sleep(2500);
  const killed = installerRunning();
  ssh('Get-Process -Name setup -ErrorAction SilentlyContinue | Stop-Process -Force');
  await sleep(4000);
  steps.push(result(ROW_TITLES.killInstaller, [{ ok: uacK.ok, detail: uacK.detail }, ...judgeKilledInstall({ killed, ...facts() })], [uacK.detail], 'the machine reads Not installed after the installer was killed'));
  // 3. Install again.
  ssh(`Remove-Item '${PROG}' -Recurse -Force -ErrorAction SilentlyContinue`, { admin: true });
  const b = await installOnce('install-again');
  steps.push(result(ROW_TITLES.installAgain, [{ ok: b.uac.ok, detail: b.uac.detail }, ...judgeInstalled('install again', { exit: b.exit, ...facts() })], [b.uac.detail], 'install again works'));
  return steps;
}

/** The first uninstall after a boot (run at the end of the restart run, when the guest has just come up): see judgeColdUninstall. Accepts what fix/uninstall-cold-leftover defines. */
export async function coldUninstallStep() {
  ssh('Get-Process dyno-app,dyno-agent,dyno-bench -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
  ssh(`Start-Process '${PROG}\\uninstall.exe' -ArgumentList '/S'`, { admin: true });
  let folder = 'unreadable';
  for (let i = 0; i < 20; i++) { await sleep(3000); folder = triState(ssh(`Test-Path '${PROG}'`, { admin: true })); if (folder === 'no') break; }
  const names = ssh(`(Get-ChildItem '${PROG}' -Force -Name -ErrorAction SilentlyContinue) -join ','`, { admin: true }).split(',').filter(Boolean);
  const log = ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 40 -ErrorAction SilentlyContinue`, { admin: true });
  const f = { folder, onlyUninstallExe: folder === 'yes' ? names.filter((n) => !/^(uninstall\.exe|updates)$/i.test(n)).length === 0 : null, noteLogged: /uninstall.*(left|leftover|holder|lock)|XtaCache/i.test(log), reinstallOk: null };
  if (f.folder === 'yes' && f.onlyUninstallExe && f.noteLogged) { const r = await installOnce('cold-uninstall-reinstall'); f.reinstallOk = r.exit === 0 && facts().app === 'yes'; }
  const checks = judgeColdUninstall(f);
  const res = result(ROW_TITLES.winFirstUninstall, checks, [JSON.stringify(f), `left: ${names.join(', ') || '(nothing)'}`, ...log.split('\n').slice(-12)], 'gone after the retries, or the noted fallback with a working reinstall');
  return coldUninstallAccepted(f) ? { ...res, status: 'pass', summary: 'the first uninstall after boot left nothing, or only the noted leftover and a reinstall works' } : res;
}

export const notDrivenWinRows = () => [notDriven(ROW_TITLES.secondUser, NOT_DRIVEN.secondUser)];

const NT = `Add-Type -Namespace N -Name P -MemberDefinition '[DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr h); [DllImport("ntdll.dll")] public static extern int NtResumeProcess(IntPtr h);'; `;
const renderers = `$ids = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match '--type=renderer' -and $_.CommandLine -match 'dyno' } | ForEach-Object { $_.ProcessId }); `;

/** A hung or paused visible page on Windows: the WebView2 renderer of dyno-app is suspended (NtSuspendProcess, no debugger); the question is read from the window list. Keep Waiting cannot be pressed here (no button lookup in the actor), so the doubling stays a broken check. */
export async function winHung() {
  ssh('Set-Content C:\\paddock-test\\vmcheck\\launch-hung.ps1 \'Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"\'; schtasks /create /tn DynoHungLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch-hung.ps1" | Out-Null; schtasks /run /tn DynoHungLaunch | Out-Null');
  await sleep(30_000);
  const actor = new Actor(); actor.start(); await sleep(6000);
  const r = await hungPageStep({
    now: () => Date.now() / 1000, sleep,
    suspend: async () => { const out = ssh(`${NT}${renderers}foreach ($i in $ids) { [void][N.P]::NtSuspendProcess((Get-Process -Id $i).Handle) }; $ids.Count`); if (!(Number(out.trim().split('\\n').at(-1)) > 0)) throw new Error('no WebView2 renderer of dyno-app was found'); },
    resume: async () => { ssh(`${NT}${renderers}foreach ($i in $ids) { [void][N.P]::NtResumeProcess((Get-Process -Id $i).Handle) }`); },
    logLines: async () => ssh(APP_LOG_PS).split('\\n'),
    questions: async (phase) => { const n = (await actor.send('windows').catch(() => '')).split('\\n').filter((l) => /not responding|Keep Waiting/i.test(l)).length; return phase === 'open' && n === 0 ? null : n; },
    keepWaiting: async () => false, shot: async (name) => actor.shot(name),
  }, ROW_TITLES.hungPage);
  await actor.send('quit', 5000).catch(() => {});
  ssh('Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Stop-Process -Force');
  return r;
}

/** "Quit and stop measuring" as the LAST step of the run (it ends the app and the agent): menu click over the DevTools port, 60 s of no Dyno process, reopen. The reboot half is not tried here. */
export async function winQuitStop() {
  const launch = () => ssh('Set-Content C:\\paddock-test\\vmcheck\\launch-qs.ps1 \'$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"\'; schtasks /create /tn DynoQsLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch-qs.ps1" | Out-Null; schtasks /run /tn DynoQsLaunch | Out-Null');
  const count = async () => Number(ssh('(Get-Process dyno-app,dyno-agent -ErrorAction SilentlyContinue | Measure-Object).Count')) || 0;
  return quitStopStep({
    now: () => Date.now(), sleep,
    trigger: async () => {
      launch(); await sleep(25_000);
      const t = tunnel();
      try {
        const browser = await connect();
        const main = browser.contexts().flatMap((c) => c.pages()).find((p) => !p.url().includes('dialog?id='));
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
    count, reopen: async () => { launch(); await sleep(30_000); return count(); }, reboot: async () => null,
  });
}

export const afterQuitSteps = [() => sameVersionRefused(), () => winMcpStep(), () => uninstall(), () => installRows(), () => winSwapRow({ oldStaged: process.env.PADDOCK_OLD_SETUP === '1' }), () => notDrivenWinRows(), () => winHung(), () => winQuitStop()];
