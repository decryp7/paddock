// Release row on Windows: the new setup is run over a RUNNING older Dyno (helper service on); within 60 s the agent and the helper must be the new build and no process
// from before the install may be left (upgrade-swap-row.mjs). The older setup is staged as setup-old.exe (run-win.mjs --old-installer), the new one is setup.exe.
// Installs go through the same scheduled-task launcher and UAC answer over VNC as the install rows. ER-33: no older setup given = NOT MEASURED with that reason.
import { ssh, answerUac } from './win-helper.mjs';
import { upgradeSwapStep, swapNotDriven, versionOf } from './upgrade-swap-row.mjs';

const PROG = 'C:\\Program Files\\Dyno';
const HELPER_EXE = 'C:\\Program Files\\Dyno Helper\\dyno-helper.exe';
const V = 'C:\\paddock-test\\vmcheck';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function runSetup(file, label) {
  ssh(`Remove-Item ${V}\\swap.out -ErrorAction SilentlyContinue; Set-Content ${V}\\swap.ps1 '$p = Start-Process ${V}\\${file} -ArgumentList "/S" -PassThru; $p.WaitForExit(); $p.ExitCode | Set-Content ${V}\\swap.out'; schtasks /end /tn DynoSwapSetup 2>$null | Out-Null; schtasks /create /tn DynoSwapSetup /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${V}\\swap.ps1" | Out-Null; schtasks /run /tn DynoSwapSetup | Out-Null`);
  const uac = await answerUac(label);
  let exit = null;
  for (let i = 0; i < 100 && exit === null; i++) { await sleep(3000); const t = ssh(`Get-Content ${V}\\swap.out -ErrorAction SilentlyContinue`).trim(); if (/^-?\d+$/.test(t)) exit = Number(t); }
  if (!uac.ok) throw new Error(uac.detail);
  if (exit !== 0) throw new Error(`${file} ended with exit ${exit}`);
}

const launchApp = () => ssh(`Set-Content ${V}\\launch-swap.ps1 'Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"'; schtasks /create /tn DynoSwapLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File ${V}\\launch-swap.ps1" | Out-Null; schtasks /run /tn DynoSwapLaunch | Out-Null`);
const statusJson = () => { const t = ssh(`& '${PROG}\\dyno.exe' status --json 2>$null`); try { return JSON.parse(t.slice(t.indexOf('{'))); } catch { return null; } };

export async function winSwapRow({ oldStaged }) {
  if (!oldStaged) return swapNotDriven('no older build was given (--old-installer <older setup.exe>), so there is nothing to install over');
  const prims = {
    now: () => Date.now(), sleep,
    async installOld() {
      ssh('Get-Process dyno-app,dyno-agent,dyno-helper -ErrorAction SilentlyContinue | Stop-Process -Force', { admin: true });
      ssh(`if (Test-Path '${PROG}\\uninstall.exe') { Start-Process '${PROG}\\uninstall.exe' -ArgumentList '/S' -Wait }`, { admin: true, ms: 180_000 });
      for (let i = 0; i < 30 && ssh(`Test-Path '${PROG}'`, { admin: true }).trim() === 'True'; i++) await sleep(3000);
      if (ssh(`Test-Path '${PROG}'`, { admin: true }).trim() === 'True') ssh(`Remove-Item '${PROG}' -Recurse -Force -ErrorAction SilentlyContinue`, { admin: true });
      await runSetup('setup-old.exe', 'swap-old');
    },
    async startOldWithHelper() {
      launchApp();
      let st = null; for (let i = 0; i < 30 && !st; i++) { await sleep(4000); st = statusJson(); }
      const sid = ssh("(New-Object Security.Principal.NTAccount('paddock-test')).Translate([Security.Principal.SecurityIdentifier]).Value").trim().split('\n').at(-1);
      ssh(`& '${PROG}\\dyno-helper.exe' install --for sid:${sid} 2>&1`, { admin: true, ms: 120_000 });
      let on = false; for (let i = 0; i < 20 && !on; i++) { on = /Running/.test(ssh('(Get-Service DynoHelper -ErrorAction SilentlyContinue).Status', { admin: true })); if (!on) await sleep(3000); }
      return { oldVersion: versionOf(st?.agent?.version ?? ''), helperWasOn: on };
    },
    async newVersion() { return versionOf(ssh(`(Get-Item ${V}\\setup.exe).VersionInfo.ProductVersion`)); },
    async buildId() { return ssh(`(Get-FileHash '${PROG}\\dyno-app.exe','${PROG}\\dyno-agent.exe','${PROG}\\dyno-helper.exe' | ForEach-Object Hash) -join ''`, { admin: true }).trim() || null; },
    async installNew() { await runSetup('setup.exe', 'swap-new'); return Date.now(); },
    async openApp() { launchApp(); },
    async agentVersion() { return versionOf(statusJson()?.agent?.version ?? ''); },
    async helperVersion() { const v = versionOf(ssh(`& '${HELPER_EXE}' --version 2>&1`, { admin: true })); return { version: v, readable: v !== null }; },
    async procs() {
      const t = ssh("Get-Process dyno-app,dyno-agent,dyno-helper -ErrorAction SilentlyContinue | ForEach-Object { $_.Name + '|' + $_.Id + '|' + $_.Path + '|' + ([DateTimeOffset]$_.StartTime).ToUnixTimeMilliseconds() }", { admin: true });
      return t.split('\n').map((l) => l.trim().split('|')).filter((f) => f.length === 4 && /^\d+$/.test(f[1])).map(([name, pid, exe, ms]) => ({ name, pid: Number(pid), exe, startMs: Number(ms) }));
    },
  };
  return upgradeSwapStep(prims).catch((e) => swapNotDriven(String(e.message ?? e)));
}
