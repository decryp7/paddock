#!/usr/bin/env node
// The Windows guest of the VM release checks (local only, never CI): reverts the Windows 11 test VM (VMware Fusion, `vm-win.sh`) to its
// `gui-session-tools` snapshot, starts it, waits for the interactive session, stages the installer and win-suite.ps1, runs the suite there
// (a scheduled task in the paddock-test session), polls for its result file with a bounded wait, and writes the Markdown result in the
// vm-smoke format. The VM is reverted and stopped at the end, also after a failure. Every start, revert, stop and .vmx edit goes through
// the VM broker (`./dev vm`, broker.mjs): one lease per VM, a preflight, headless only (ADR 0054 §1.5). Only `ssh` and `ip` of vm-win.sh remain.
//
//   node scripts/vm-smoke/run-win.mjs --app dyno (--installer <setup.exe> | --run <ci run id>) [--out <dir>]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderResult, verdict } from './judge.mjs';
import { runUiPhase } from './win-ui.mjs';
import { helperSteps } from './win-helper.mjs';
import { runRebootPhase } from './win-reboot.mjs';
import { BrokerError, Lease } from './broker.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
const VMWIN = process.env.PADDOCK_VM_WIN ?? join(REPO, 'scripts', 'vm-win.sh');
const VM_NAME = 'paddock-win11-test';
const VMDIR = process.env.PADDOCK_WIN_VM_DIR ?? '/Volumes/d7sst7/vms/paddock-win11-test';
const SNAPSHOT = 'gui-session-tools';
export const VNC_PORT = 5995;
const GUEST = 'C:\\paddock-test\\vmcheck';
const VNC_PW = process.env.PADDOCK_VNC_PW ?? Math.random().toString(36).slice(2, 10); // a per-run password, never printed
process.env.PADDOCK_VNC_PW ??= VNC_PW; // win-vnc.mjs reads it
const log = (m) => console.error(`vm-smoke(win): ${m}`);

export function parseArgs(argv) {
  const o = { out: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--app') o.app = v(); else if (a === '--installer') o.installer = v(); else if (a === '--run') o.run = v(); else if (a === '--out') o.out = v(); else if (a === '--keep') o.keep = true; else if (a === '--only') o.only = v(); else if (a === '--ui') o.ui = true; else if (a === '--vnc') o.vnc = true; else if (a === '--scale') o.scale = Number(v()); else if (a === '--reboot-check') o.reboot = true;
    else throw new Error(`unknown option ${a}`);
  }
  if (o.app !== 'dyno') throw new Error('--app dyno is the only app with a Windows suite');
  if (!o.installer === !o.run) throw new Error('give exactly one of --installer <setup.exe> or --run <CI run id>');
  if (o.run && !/^\d+$/.test(o.run)) throw new Error('--run takes a number');
  return o;
}

/** A bounded host command; the child is ours, so a guard ends it by handle. */
function sh(cmd, args, timeoutMs, input) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: timeoutMs, input, env: process.env });
  return { code: r.status ?? (r.signal ? 124 : 1), stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
}
// vm-win.sh is used for `ssh` and `ip` only; the VM itself is driven by the broker lease.
const vm = (args, timeoutMs = 120_000) => sh('bash', [VMWIN, ...args], timeoutMs);
const ssh = (cmd, timeoutMs = 90_000, admin = false) => vm(['ssh', ...(admin ? ['--admin'] : []), cmd], timeoutMs);
const clean = (s) => s.split('\n').filter((l) => !l.startsWith('**')).join('\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function download(run, name, dest) {
  const token = process.env.GITEA_TOKEN;
  if (!token) throw new Error('GITEA_TOKEN is not set (source ~/.zshenv)');
  const zip = join(dest, `${name}.zip`);
  const url = `${process.env.PADDOCK_GITEA ?? 'https://dev.decryptology.net'}/decryp7/paddock/actions/runs/${run}/artifacts/${name}`;
  const r = sh('curl', ['-sS', '-L', '--fail', '-o', zip, '--config', '-'], 900_000, `header = "Authorization: token ${token}"\nurl = "${url}"\n`);
  if (r.code !== 0) throw new Error(`downloading ${name} of run ${run} failed (curl exit ${r.code})`);
  const dir = join(dest, 'unzipped');
  mkdirSync(dir);
  if (sh('unzip', ['-q', '-o', zip, '-d', dir], 120_000).code !== 0) throw new Error('unzip failed');
  const exe = readdirSync(dir).find((f) => f.endsWith('setup.exe'));
  if (!exe) throw new Error(`no *setup.exe in ${name}`);
  return join(dir, exe);
}

async function main(argv) {
  const o = parseArgs(argv);
  const out = resolve(o.out ?? process.env.PADDOCK_VM_SMOKE_OUT ?? join(REPO, 'vm-smoke-results'));
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(join(tmpdir(), 'vm-smoke-win-'));
  let lease = null;
  let started = false;
  let vnc = false;
  const cleanup = () => {
    if (lease && started && o.keep) {
      log(`--keep: the VM is left running with Dyno up (CDP port 9222 in the guest); the lease is released. Revert it with: vm-win.sh stop; vm-win.sh revert ${SNAPSHOT}`);
    } else if (lease && started) {
      log(`reverting the VM to ${SNAPSHOT}`);
      // Ask Windows to shut down first (the broker powers off hard after the grace period), then the parsed .vmx edit, then the revert.
      vm(['ssh', '--admin', 'shutdown /s /t 0'], 30_000);
      try { lease.stop({ graceSec: 120 }); } catch (e) { log(`stop: ${e.message}`); }
      if (vnc) { try { lease.vncOff(); log('vmx: VNC lines removed from the current file (no old copy is ever restored)'); } catch (e) { log(`vnc off: ${e.message}`); } }
      try { lease.revert(SNAPSHOT); } catch (e) { log(`revert: ${e.message}`); }
    }
    lease?.release();
    rmSync(scratch, { recursive: true, force: true });
  };
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => { cleanup(); process.exit(130); });
  try {
    const exe = o.installer ? resolve(o.installer) : await download(o.run, 'dyno-Windows', scratch);
    if (!existsSync(exe)) throw new Error(`${exe} is not a file`);
    const source = o.installer ?? `CI run ${o.run}, artifact dyno-Windows, ${basename(exe)}`;
    // One lease per VM. A VM that is leased, or that runs without a lease (another job), is refused here and never touched.
    lease = Lease.take(REPO, VM_NAME, `vm-smoke win ${o.app} ${source}`, { waitSec: Number(process.env.PADDOCK_VM_LEASE_WAIT ?? 0) });
    log(`lease ${lease.id}`);
    started = true;
    log('revert to the snapshot (the broker runs its preflight on the .vmx, the boot disk and leftover lock folders)');
    lease.revert(SNAPSHOT);
    if (o.vnc) {
      // VNC for the UAC consent (secure desktop). Only between revert and start: the broker adds the RemoteDisplay.vnc.* lines to the CURRENT .vmx.
      vnc = true;
      lease.vncOn({ port: VNC_PORT, passwordEnv: 'PADDOCK_VNC_PW' });
      log(`vmx: VNC on port ${VNC_PORT} for this run`);
    }
    log('start (headless, bounded 10 min)'); lease.start();
    if (vm(['ip'], 60_000).code !== 0) throw new Error('the VM started but has no address yet');
    const ip = vm(['ip'], 30_000).stdout.trim().split('\n')[0];
    log(`guest ${ip}; waiting for the console session (bounded 6 min)`);
    let session = false;
    for (let i = 0; i < 90 && !session; i++) {
      session = vm(['ssh', 'if ((query user) -match "console\\s+\\d+\\s+Active") {exit 0} else {exit 1}'], 30_000).code === 0;
      if (!session) await sleep(4000);
    }
    if (!session) throw new Error('no console session after 6 minutes (auto-logon is set in the gui-session-tools snapshot; check the VM window)');
    const scp = (...a) => sh('scp', ['-q', '-i', join(VMDIR, 'keys', 'paddock-test'), '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', `UserKnownHostsFile=${join(VMDIR, 'keys', 'known_hosts')}`, ...a], 600_000);
    if (o.scale && o.scale !== 100) {
      // Display scale for the test user (registry + restart; the live change fails on the VMware display): 125 % = 120 dpi, 150 % = 144, 200 % = 192.
      const dpi = Math.round(96 * o.scale / 100);
      log(`setting the display scale to ${o.scale} % (${dpi} dpi) and restarting (bounded)`);
      ssh(`Set-ItemProperty 'HKCU:\\Control Panel\\Desktop' LogPixels ${dpi} -Type DWord; Set-ItemProperty 'HKCU:\\Control Panel\\Desktop' Win8DpiScaling 1 -Type DWord; Set-ItemProperty 'HKCU:\\Control Panel\\Desktop\\WindowMetrics' AppliedDPI ${dpi} -Type DWord`);
      ssh('Set-ItemProperty "HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon" AutoAdminLogon 1; shutdown /r /t 3', 60_000, true);
      await sleep(45_000);
      session = false;
      for (let i = 0; i < 90 && !session; i++) { session = vm(['ssh', 'if ((query user) -match "console\\s+\\d+\\s+Active") {exit 0} else {exit 1}'], 30_000).code === 0; if (!session) await sleep(4000); }
      if (!session) throw new Error('no console session after the scale restart');
      await sleep(30_000);
    }
    ssh(`Remove-Item ${GUEST} -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force ${GUEST}\\out | Out-Null`);
    const guestExe = `${GUEST}\\setup.exe`;
    if (scp(exe, `paddock-test@${ip}:C:/paddock-test/vmcheck/setup.exe`).code !== 0) throw new Error('copying the installer failed');
    if (scp(join(HERE, 'win-suite.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-suite.ps1`).code !== 0) throw new Error('copying win-suite.ps1 failed');
    log('running the suite in the interactive session (bounded 40 min)');
    ssh(`schtasks /create /tn VmCheck /sc once /st 00:00 /ru paddock-test /it /f /tr 'powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${GUEST}\\win-suite.ps1 -Installer ${guestExe} -Out ${GUEST}\\out${o.keep || o.ui || o.reboot ? ' -KeepRunning' : ''}${o.only ? ` -Only ${o.only}` : o.reboot ? ' -Only start' : ''}'; schtasks /run /tn VmCheck`);
    let done = false;
    const answered = new Set();
    for (let i = 0; i < 480 && !done; i++) {
      // The suite asks for administrator-only readings (powercfg) through request files; this host has the paddock-admin key.
      const listing = clean(ssh(`Get-ChildItem ${GUEST}\\out -Name -Filter 'req-*.txt' -ErrorAction SilentlyContinue; if (Test-Path ${GUEST}\\out\\done.txt) { 'DONE' }`, 60_000).stdout);
      for (const f of listing.split(/\r?\n/).map((x) => x.trim()).filter((x) => /^req-\d+\.txt$/.test(x))) {
        if (answered.has(f)) continue;
        answered.add(f);
        const cmd = clean(ssh(`Get-Content ${GUEST}\\out\\${f}`, 30_000).stdout).trim();
        if (!/^powercfg \/(requests|waketimers|a)$/.test(cmd)) { ssh(`Set-Content ${GUEST}\\out\\${f.replace('req', 'resp')} 'refused: ${cmd.replace(/'/g, '')}'`, 30_000); continue; }
        // The administrator's answer goes back through the standard user's own shell (base64), so the file is the user's and readable by the suite.
        const answer = clean(ssh(`${cmd} | Out-String`, 60_000, true).stdout);
        const b64 = Buffer.from(answer, 'utf8').toString('base64');
        const w = ssh(`[IO.File]::WriteAllBytes('${GUEST}\\out\\${f.replace('req', 'resp')}', [Convert]::FromBase64String('${b64}'))`, 30_000);
        log(`answered ${f} (${cmd}): ${answer.split('\n').length} lines, write exit ${w.code}`);
      }
      done = listing.includes('DONE');
      if (!done) await sleep(2000);
    }
    let uiSteps = [];
    if (o.ui && done) {
      scp(join(HERE, 'win-act.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-act.ps1`);
      log('UI phase: snap check, screenshot check on the WebView2 DOM, Quit (bounded)');
      process.env.PADDOCK_WIN_SHOTS = join(out, `${new Date().toISOString().replace(/[:.]/g, '-')}-win-dom-shots`);
      uiSteps = await runUiPhase({ log, extra: o.vnc ? helperSteps : [] });
    }
    if (o.reboot && done) {
      scp(join(HERE, 'win-act.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-act.ps1`);
      const waitConsole = async () => { for (let i = 0; i < 90; i++) { if (vm(['ssh', 'if ((query user) -match "console\\s+\\d+\\s+Active") {exit 0} else {exit 1}'], 30_000).code === 0) return true; await sleep(4000); } return false; };
      const launchDyno = () => {
        ssh(`Set-Content C:\\paddock-test\\vmcheck\\launch.ps1 '$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; Start-Process "$env:LOCALAPPDATA\\Dyno\\dyno-app.exe"'; schtasks /create /tn DynoLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch.ps1" | Out-Null; schtasks /run /tn DynoLaunch | Out-Null`);
      };
      log('restart check: helper on, record, real restart, record again without Repair');
      uiSteps = await runRebootPhase({ vm, waitConsole, launchDyno, log });
    }
    const dest = join(scratch, 'out');
    mkdirSync(dest);
    scp('-r', `paddock-test@${ip}:C:/paddock-test/vmcheck/out/*`, dest);
    const stepsFile = join(dest, 'steps.json');
    if (!existsSync(stepsFile)) throw new Error('the suite wrote no steps.json (see run.log: ' + (existsSync(join(dest, 'run.log')) ? readFileSync(join(dest, 'run.log'), 'utf8').slice(-300) : 'absent') + ')');
    const raw = JSON.parse(readFileSync(stepsFile, 'utf8').replace(/^\uFEFF/, ''));
    const steps = (Array.isArray(raw) ? raw : [raw]).map((s) => ({ ...s, checks: s.checks ?? [], evidence: [].concat(s.evidence ?? []).map(String) }));
    if (!done) steps.push({ title: 'Suite finished', status: 'fail', summary: 'the suite did not finish within 40 minutes', checks: [], evidence: [] });
    steps.push(...uiSteps);
    steps.push({ title: 'Tray right-click menu, dialogs at 150 % (manual)', status: 'manual', summary: 'Not automated yet: the tray menu and the 150 % scale need native input and a live display-scale change that the VMware display does not allow.' });
    const started_ = new Date().toISOString();
    const md = renderResult({ ...lease.stamp({ snapshot: SNAPSHOT, command: `node scripts/vm-smoke/run-win.mjs ${argv.join(' ')}` }), app: 'Dyno', version: steps[0]?.evidence?.[0]?.replace(/^version /, ''), installer: source, guest: 'VMware Fusion Windows 11 Arm (paddock-win11-test, snapshot gui-session-tools), interactive session, x64 installer under emulation', started: started_, commit: sh('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD']).stdout.trim(), steps });
    const file = join(out, `${started_.replace(/[:.]/g, '-')}-dyno-win.md`);
    writeFileSync(file, md);
    for (const f of readdirSync(dest)) if (f.endsWith('.png')) writeFileSync(join(out, `${started_.replace(/[:.]/g, '-')}-win-${f}`), readFileSync(join(dest, f)));
    log(`result: ${file}`);
    console.log(file);
    return verdict(steps).blocksTag ? 1 : 0;
  } catch (e) {
    log(`${e instanceof BrokerError && e.couldNotRun ? 'could not run' : 'error'}: ${e.message}`);
    return 2;
  } finally {
    cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
