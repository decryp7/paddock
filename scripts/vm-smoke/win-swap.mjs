// Release row on Windows: the new setup is run over a RUNNING older Dyno (helper service on). Judged as the other OS (upgrade-swap-row.mjs): agent.json `build` = the first 16 hex of the
// SHA-256 of the new bundle's dyno-agent.exe within 60 s, no restart loop; the helper by HASH (the installed Dyno Helper\dyno-helper.exe and the running helper's image equal the bundled
// dyno-helper.exe) and by the helper's log line "refresh: replacing the installed helper" (the installer's POSTINSTALL `dyno-helper refresh`), never by start times.
// The older setup is staged as setup-old.exe (run-win.mjs --old-installer), the new one is setup.exe. Installs go through the scheduled-task launcher and the UAC answer over VNC.
import { ssh, answerUac } from './win-helper.mjs';
import { upgradeSwapStep, swapNotDriven, versionOf } from './upgrade-swap-row.mjs';

const PROG = 'C:\\Program Files\\Dyno';
const HELPER_DIR = 'C:\\Program Files\\Dyno Helper';
const HELPER_LOG_GLOB = 'C:\\ProgramData\\Dyno\\Helper\\helper.log';
const AGENT_JSON = 'C:\\Users\\paddock-test\\AppData\\Local\\Dyno\\agent.json';
const V = 'C:\\paddock-test\\vmcheck';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });
const build16 = (t) => { try { const b = JSON.parse(String(t).slice(String(t).indexOf('{'))).build; return typeof b === 'string' && /^[0-9a-f]{16}$/.test(b) ? b : null; } catch { return null; } };
const guestNowMs = () => Number(ssh('[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()').trim().split('\n').at(-1));
const hash = (path, admin = true) => ssh(`(Get-FileHash '${path}' -Algorithm SHA256 -ErrorAction SilentlyContinue).Hash`, { admin }).trim().split('\n').at(-1).toLowerCase();

async function runSetup(file, label) {
  ssh(`Remove-Item ${V}\\swap.out -ErrorAction SilentlyContinue; Set-Content ${V}\\swap.ps1 '$p = Start-Process ${V}\\${file} -ArgumentList "/S" -PassThru; $p.WaitForExit(); $p.ExitCode | Set-Content ${V}\\swap.out'; schtasks /end /tn DynoSwapSetup 2>$null | Out-Null; schtasks /create /tn DynoSwapSetup /sc once /st 00:00 /ru paddock-test /it /f /tr "powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${V}\\swap.ps1" | Out-Null; schtasks /run /tn DynoSwapSetup | Out-Null`);
  const uac = await answerUac(label);
  let exit = null;
  for (let i = 0; i < 100 && exit === null; i++) { await sleep(3000); const t = ssh(`Get-Content ${V}\\swap.out -ErrorAction SilentlyContinue`).trim(); if (/^-?\d+$/.test(t)) exit = Number(t); }
  if (!uac.ok) throw new Error(uac.detail);
  if (exit !== 0) throw new Error(`${file} ended with exit ${exit}; setup.log tail: ${ssh(`Get-Content '${PROG}\\updates\\setup.log' -Tail 12 -ErrorAction SilentlyContinue`, { admin: true }).replace(/\n/g, ' | ').slice(-500)}`);
}

const launchApp = () => ssh(`Set-Content ${V}\\launch-swap.ps1 'Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"'; schtasks /create /tn DynoSwapLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File ${V}\\launch-swap.ps1" | Out-Null; schtasks /run /tn DynoSwapLaunch | Out-Null`);

export async function winSwapRow({ oldStaged }) {
  let helperBefore = '';
  const swapEvidence = [];
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
      let b = null; for (let i = 0; i < 30 && !b; i++) { await sleep(4000); b = build16(ssh(`Get-Content '${AGENT_JSON}' -Raw -ErrorAction SilentlyContinue`)); }
      const sid = ssh("(New-Object Security.Principal.NTAccount('paddock-test')).Translate([Security.Principal.SecurityIdentifier]).Value").trim().split('\n').at(-1);
      ssh(`& '${PROG}\\dyno-helper.exe' install --for sid:${sid} 2>&1`, { admin: true, ms: 120_000 });
      let on = false; for (let i = 0; i < 20 && !on; i++) { on = /Running/.test(ssh('(Get-Service DynoHelper -ErrorAction SilentlyContinue).Status', { admin: true })); if (!on) await sleep(3000); }
      const v = versionOf(ssh(`(Get-Item '${PROG}\\dyno-app.exe').VersionInfo.ProductVersion`));
      helperBefore = hash(`${HELPER_DIR}\\dyno-helper.exe`);
      return { oldVersion: v, helperWasOn: on };
    },
    async oldAgentBuild16() { return build16(ssh(`Get-Content '${AGENT_JSON}' -Raw -ErrorAction SilentlyContinue`)); },
    async beginInstall() { return guestNowMs(); },
    async installNew() { await runSetup('setup.exe', 'swap-new'); },
    // The new bundle's agent is the file the installer just wrote: its hash is the build the product computes.
    async expectedAgentBuild16() { const h = hash(`${PROG}\\dyno-agent.exe`).slice(0, 16); return /^[0-9a-f]{16}$/.test(h) ? h : null; },
    async openApp() { launchApp(); },
    async agentBuild16() { return build16(ssh(`Get-Content '${AGENT_JSON}' -Raw -ErrorAction SilentlyContinue`)); },
    async agentProcs() {
      const t = ssh("Get-Process dyno-agent -ErrorAction SilentlyContinue | ForEach-Object { '' + $_.Id + '|' + ([DateTimeOffset]$_.StartTime).ToUnixTimeMilliseconds() }", { admin: true });
      return t.split('\n').map((l) => l.trim().split('|')).filter((f) => f.length === 2 && /^\d+$/.test(f[0])).map(([pid, ms]) => ({ pid: Number(pid), startMs: Number(ms) }));
    },
    async extraChecks() {
      const out = [];
      const bundled = hash(`${PROG}\\dyno-helper.exe`);
      const installed = hash(`${HELPER_DIR}\\dyno-helper.exe`);
      out.push(chk(Boolean(bundled) && bundled === installed, bundled && bundled === installed ? `the installed helper (${HELPER_DIR}) is the bundled dyno-helper.exe (SHA-256 ${bundled.slice(0, 16)})` : `the installed helper ${installed.slice(0, 16) || 'unreadable'} is not the bundled ${bundled.slice(0, 16) || 'unreadable'}`));
      const running = ssh("(Get-Process dyno-helper -ErrorAction SilentlyContinue | Select-Object -First 1).Path", { admin: true }).trim().split('\n').at(-1);
      const rh = running ? hash(running) : '';
      out.push(chk(Boolean(rh) && rh === bundled, rh && rh === bundled ? `the running helper image (${running}) is the bundled one` : `the running helper image ${running || '(none)'} hashes ${rh.slice(0, 16) || 'unreadable'}, not the bundled ${bundled.slice(0, 16)}`));
      const unchanged = Boolean(helperBefore) && helperBefore === bundled;
      if (unchanged) out.push(chk(true, `the helper file was already the bundled one before the install (SHA-256 ${bundled.slice(0, 16)}): no helper refresh is expected, so the refresh log line is not required`));
      else {
        // The installer's `dyno-helper refresh` logs "refresh: decided" and "refresh: replacing the installed helper" (dyno-helper setup.rs). Every "refresh:" line of every helper
        // and setup log is kept as evidence. When the exact line is not found, the replacement is judged by the hashes (old file differs, installed and running equal the bundled).
        const lines = ssh(`Get-ChildItem 'C:\\ProgramData\\Dyno','${PROG}\\updates' -Recurse -Filter *.log -ErrorAction SilentlyContinue | Select-String -Pattern 'refresh' -ErrorAction SilentlyContinue | Select-Object -Last 12 | ForEach-Object { $_.Filename + ': ' + $_.Line.Trim() }`, { admin: true }).trim();
        swapEvidence.push('--- "refresh" lines in the helper and setup logs', ...(lines ? lines.split('\n').map((l) => l.trim().slice(0, 300)) : ['(none)']));
        const exact = /refresh: replacing the installed helper/.test(lines);
        const byHash = Boolean(helperBefore) && Boolean(bundled) && helperBefore !== bundled && bundled === installed && bundled === rh;
        out.push(chk(exact || byHash, exact ? 'the helper log has "refresh: replacing the installed helper"' : byHash ? `the helper was replaced, shown by hash (before ${helperBefore.slice(0, 12)}, installed and running now ${bundled.slice(0, 12)} = bundled); the exact refresh line was not found, the log's refresh lines are in the evidence` : `the helper changed (${helperBefore.slice(0, 12)} then ${bundled.slice(0, 12)}) but neither the refresh line nor the hashes show a replacement`));
      }
      const svc = ssh('(Get-Service DynoHelper -ErrorAction SilentlyContinue).Status', { admin: true }).trim().split('\n').at(-1);
      out.push(chk(/Running/.test(svc), `the DynoHelper service is ${svc || 'absent'}`));
      return out;
    },
  };
  const r = await upgradeSwapStep(prims).catch((e) => swapNotDriven(String(e.message ?? e)));
  r.evidence = [...(r.evidence ?? []), `helper before the install: SHA-256 ${helperBefore || '(unread)'}`, ...swapEvidence];
  return r;
}
