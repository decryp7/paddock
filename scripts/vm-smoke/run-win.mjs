#!/usr/bin/env node
// The Windows guest of the VM release checks (local only, never CI): reverts the Windows 11 test VM (VMware Fusion, `vm-win.sh`) to its
// `gui-session-tools` snapshot, starts it, waits for the interactive session, stages the installer and win-suite.ps1, runs the suite there
// (a scheduled task in the paddock-test session), polls for its result file with a bounded wait, and writes the Markdown result in the
// vm-smoke format. The VM is reverted and stopped at the end, also after a failure.
//
//   node scripts/vm-smoke/run-win.mjs --app dyno (--installer <setup.exe> | --run <ci run id>) [--out <dir>] [--only <rows>]
//   --only names whole release rows (win-rows.mjs: tray-menu, login-jobs, disk-space, exploit, scoping, hung-page, install-rows, upgrade-swap, second-user, restart-tray, or their
//   row numbers, e.g. 10,11,16,21): one install, only those rows, a partial result. Needs --vnc. Without rows, --only is a win-suite.ps1 part (`start`).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildStep, classifyInstallFailure, expectedCommit } from './verdict.mjs';
import { requiredSteps } from './manifest.mjs';
import { commitOfRun, finalizeRun, resultFolder, foundFromPs, psHasCommit, sha256File, withBuildStep } from './report.mjs';
import { runUiPhase } from './win-ui.mjs';
import { helperSteps, answerUac, rowSteps } from './win-helper.mjs';
import { selectRows } from './win-rows.mjs';
import { afterQuitSteps, afterRowSteps } from './win-machine.mjs';
import { runRebootPhase } from './win-reboot.mjs';
import { stripVnc, withVnc, fixDiskName, vmxCheck } from './vmx-edit.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const VMDIR = process.env.PADDOCK_WIN_VM_DIR ?? '/Volumes/d7sst7/vms/paddock-win11-test';
const SNAPSHOT = process.env.PADDOCK_WIN_SNAPSHOT ?? 'gui-session-tools';
const VMX = process.env.PADDOCK_WIN_VMX ?? '/Volumes/d7sst7/vms/paddock-win11-test/vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx';
export const VNC_PORT = 5995;
const GUEST = 'C:\\paddock-test\\vmcheck';
const VNC_PW = process.env.PADDOCK_VNC_PW ?? Math.random().toString(36).slice(2, 10); // a per-run password, never printed
process.env.PADDOCK_VNC_PW ??= VNC_PW; // win-vnc.mjs reads it
/** The vmx is never edited in place: a temporary file next to it, then a rename (the VM is stopped when this is called). */
const writeVmx = (text) => { const tmp = `${VMX}.tmp-${process.pid}`; writeFileSync(tmp, text); renameSync(tmp, VMX); };
const log = (m) => console.error(`vm-smoke(win): ${m}`);

export function parseArgs(argv) {
  const o = { out: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--app') o.app = v(); else if (a === '--installer') o.installer = v(); else if (a === '--run') o.run = v(); else if (a === '--out') o.out = v(); else if (a === '--old-installer') o.oldInstaller = v(); else if (a === '--keep') o.keep = true; else if (a === '--only') o.only = v(); else if (a === '--ui') o.ui = true; else if (a === '--vnc') o.vnc = true; else if (a === '--scale') o.scale = Number(v()); else if (a === '--reboot-check') o.reboot = true; else if (a === '--expect-commit') o.expectCommit = v(); else if (a === '--partial') o.partial = true;
    else throw new Error(`unknown option ${a}`);
  }
  if (o.app !== 'dyno') throw new Error('--app dyno is the only app with a Windows suite');
  if (!o.installer === !o.run) throw new Error('give exactly one of --installer <setup.exe> or --run <CI run id>');
  if (o.run && !/^\d+$/.test(o.run)) throw new Error('--run takes a number');
  o.sel = selectRows(o.only); // throws on a mix of rows and a suite part
  if (o.sel.reboot) o.reboot = true; // restart-tray is the restart phase (its steps all run; the verdict asks only for the tray row)
  if (o.sel.rows.length && !o.vnc) throw new Error('--only rows need --vnc (the installs ask for an administrator)');
  if (o.sel.oldInstaller && !o.oldInstaller) throw new Error('--only upgrade-swap needs --old-installer');
  return o;
}

/** A bounded host command; the child is ours, so a guard ends it by handle. */
function sh(cmd, args, timeoutMs, input) {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: timeoutMs, input, env: process.env });
  return { code: r.status ?? (r.signal ? 124 : 1), stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
}
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
  let started = false;
  let vnc = false;
  const cleanup = () => {
    if (started && o.keep) log('--keep: the VM is left running with Dyno up (CDP port 9222 in the guest); revert it with: vm-win.sh stop; vm-win.sh revert gui-session-tools');
    else if (started) {
      log(`reverting the VM to ${SNAPSHOT}`);
      vm(['stop'], 200_000);
      if (vnc) { writeVmx(fixDiskName(stripVnc(readFileSync(VMX, 'utf8')), readdirSync(dirname(VMX)))); log('vmx: VNC lines removed from the current file (no old copy is ever restored)'); }
      vm(['revert', SNAPSHOT], 300_000);
    }
    rmSync(scratch, { recursive: true, force: true });
  };
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => { cleanup(); process.exit(130); });
  try {
    if (!o.vnc) throw new Error('the per-machine installer asks for an administrator: run with --vnc (the UAC prompt is answered over the VM VNC)');
    const exe = o.installer ? resolve(o.installer) : await download(o.run, 'dyno-Windows', scratch);
    if (!existsSync(exe)) throw new Error(`${exe} is not a file`);
    const source = o.installer ?? `CI run ${o.run}, artifact dyno-Windows, ${basename(exe)}`;
    // UAC and actor screenshots go to the run's evidence folder, next to the result (the host /tmp was overwritten by the next run).
    process.env.PADDOCK_EVIDENCE_DIR = join(resultFolder(out, { run: o.run, installerSha256: sha256File(exe) }), 'evidence');
    // --ui needs @playwright/test (the root npm workspace: `npm ci` in the worktree, or a worktree with node_modules): fail before a VM is touched, not 13 minutes in.
    if (o.ui) { try { await import('@playwright/test'); } catch { throw new Error('--ui needs @playwright/test, which this checkout cannot load: run `npm ci` at the repository root (or use a worktree that has node_modules) first'); } }
    // Never touch a VM somebody else is running: if it is up before this run started it, stop here without a revert or a stop in the cleanup.
    if (/running/.test(vm(['status'], 30_000).stdout)) throw new Error('paddock-win11-test is already running (another job?); refusing to revert or stop it');
    started = true;
    { const cur = readFileSync(VMX, 'utf8'); const fixed = fixDiskName(cur, readdirSync(dirname(VMX))); if (fixed !== cur) { writeVmx(fixed); log('vmx named a missing redo log: pointed at the newest existing one'); } }
    {
      // Preflight, logged: the vmx parses, no key twice, the disk it names exists; stale lock folders are only reported (VMware clears its own on a start; never deleted here).
      const dir = dirname(VMX); const files = readdirSync(dir); const c = vmxCheck(readFileSync(VMX, 'utf8'), files);
      const locks = files.filter((f) => f.endsWith('.lck'));
      log(`vmx check: ${c.lines} lines, ${c.duplicates.length ? `DUPLICATE keys ${c.duplicates.join(', ')}` : 'no duplicate keys'}, disk ${c.disk} ${c.diskExists ? 'exists' : 'MISSING'}; ${locks.length} lock folder(s) present${locks.length ? ` (${locks.join(', ')})` : ''}, left alone`);
      if (c.duplicates.length || !c.diskExists) throw new Error('the vmx is not usable (duplicate keys or a missing disk); not starting the VM');
    }
    log('revert to the snapshot'); if (vm(['revert', SNAPSHOT], 300_000).code !== 0) throw new Error(`revert to ${SNAPSHOT} failed`);
    if (o.vnc) {
      // VNC for the UAC consent (secure desktop). Only between revert and start, on the CURRENT vmx: only the RemoteDisplay.vnc.* lines are added and removed.
      vnc = true;
      vm(['stop'], 200_000);
      writeVmx(withVnc(fixDiskName(readFileSync(VMX, 'utf8'), readdirSync(dirname(VMX))), VNC_PORT, VNC_PW));
      log(`vmx: VNC on port ${VNC_PORT} for this run (backup kept until the end)`);
    }
    log('start (bounded 10 min)'); if (vm(['start'], 600_000).code !== 0) throw new Error('the VM did not start or SSH did not answer');
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
    if (o.oldInstaller) { if (scp(resolve(o.oldInstaller), `paddock-test@${ip}:C:/paddock-test/vmcheck/setup-old.exe`).code !== 0) throw new Error('copying the older installer failed'); process.env.PADDOCK_OLD_SETUP = '1'; }
    if (existsSync(`${exe}.sig`)) scp(`${exe}.sig`, `paddock-test@${ip}:C:/paddock-test/vmcheck/setup.exe.sig`);
    if (scp(join(HERE, 'win-suite.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-suite.ps1`).code !== 0) throw new Error('copying win-suite.ps1 failed');
    // The in-session actor and the menu control are staged for every run (not only --ui), and every script is parsed in the guest first: a script that does not parse never
    // starts (no run.log, no steps.json) and a poll would wait 40 minutes.
    for (const f of ['win-act.ps1', 'win-menu-control.ps1']) if (scp(join(HERE, f), `paddock-test@${ip}:C:/paddock-test/vmcheck/${f}`).code !== 0) throw new Error(`copying ${f} failed`);
    for (const f of ['win-suite.ps1', 'win-act.ps1', 'win-menu-control.ps1']) {
      const parse = clean(ssh(`$e = $null; $t = $null; [void][System.Management.Automation.Language.Parser]::ParseFile('${GUEST}\\${f}', [ref]$t, [ref]$e); foreach ($x in $e) { "line $($x.Extent.StartLineNumber): $($x.Message)" }; 'PARSED'`, 60_000).stdout).trim();
      if (parse !== 'PARSED') throw new Error(`${f} does not parse in the guest: ${parse.replace(/\s*PARSED$/, '').slice(0, 400)}`);
    }
    log('running the suite in the interactive session (bounded 40 min)');
    ssh(`schtasks /create /tn VmCheck /sc once /st 00:00 /ru paddock-test /it /f /tr 'powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${GUEST}\\win-suite.ps1 -Installer ${guestExe} -Out ${GUEST}\\out${o.keep || o.ui || o.reboot || o.sel.rows.length ? ' -KeepRunning' : ''}${o.sel.rows.length ? ' -Only start' : o.only ? ` -Only ${o.only}` : o.reboot ? ' -Only start' : ''}'; schtasks /run /tn VmCheck`);
    let done = false;
    const answered = new Set();
    const uacNotes = [];
    for (let i = 0; i < 480 && !done; i++) {
      // The suite asks for administrator-only readings (powercfg) through request files; this host has the paddock-admin key.
      const listing = clean(ssh(`Get-ChildItem ${GUEST}\\out -Name -ErrorAction SilentlyContinue | Where-Object { $_ -match '^(req|uac)-\\d+\\.txt$' }; if (Test-Path ${GUEST}\\out\\done.txt) { 'DONE' }`, 60_000).stdout);
      for (const f of listing.split(/\r?\n/).map((x) => x.trim()).filter((x) => /^(req|uac)-\d+\.txt$/.test(x))) {
        if (answered.has(f)) continue;
        answered.add(f);
        if (f.startsWith('uac-')) { const u = await answerUac(f.replace(/\.txt$/, '')); log(`${f}: ${u.detail}`); uacNotes.push(u); continue; }
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
    // The suite ends early when the install failed (win-suite.ps1 writes "install failed" into done.txt): no UI, restart or build probe on an empty machine.
    const blocked = done && /install failed/.test(clean(ssh(`Get-Content ${GUEST}\\out\\done.txt -ErrorAction SilentlyContinue`, 30_000).stdout));
    if (blocked) log('the install failed: the UI and restart phases are skipped; dependent steps are not measured');
    // The build binding is read while the app is installed (the uninstall step removes it later).
    const expected = expectedCommit(o.expectCommit) ?? (o.run ? commitOfRun(o.run) : null);
    const buildProbe = expected && done && !blocked ? foundFromPs(clean(ssh(psHasCommit('C:\\Program Files\\Dyno\\dyno-app.exe', expected), 180_000).stdout)) : null;
    // The install row's precondition for every later setup step: paddock-setup.exe is there with FileVersion 1.1.0 and POSTINSTALL ran (check-version, clean-run in setup.log).
    const setupFacts = done && !blocked ? (await import('./win-machine.mjs')).setupExeFacts('right after the install') : null;
    let uiSteps = [];
    if (o.ui && done && !blocked) {
      scp(join(HERE, 'win-act.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-act.ps1`);
      log('UI phase: snap check, screenshot check on the WebView2 DOM, Quit (bounded)');
      process.env.PADDOCK_WIN_SHOTS = join(out, `${new Date().toISOString().replace(/[:.]/g, '-')}-win-dom-shots`);
      uiSteps = await runUiPhase({ log, extra: o.vnc ? helperSteps : [], after: o.vnc ? afterQuitSteps : [] });
    }
    if (o.sel.rows.length && !o.sel.reboot && done && !blocked) {
      scp(join(HERE, 'win-act.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-act.ps1`);
      log(`rows: ${o.sel.rows.join(', ')}`);
      const ids = o.sel.ui.filter((k) => k !== 'tray-menu');
      uiSteps = await runUiPhase({ log, extra: rowSteps(ids, { helper: o.sel.helper }), after: afterRowSteps(o.sel.after), select: { snap: false, tour: false, tray: o.sel.ui.includes('tray-menu') } });
    }
    if (o.reboot && done && !blocked) {
      scp(join(HERE, 'win-act.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-act.ps1`);
      const waitConsole = async (tries = 90) => { for (let i = 0; i < tries; i++) { if (vm(['ssh', 'if ((query user) -match "console\\s+\\d+\\s+Active") {exit 0} else {exit 1}'], 30_000).code === 0) return true; if (i < tries - 1) await sleep(4000); } return false; };
      const launchDyno = () => {
        ssh(`Set-Content C:\\paddock-test\\vmcheck\\launch.ps1 '$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"; Start-Process "$env:ProgramFiles\\Dyno\\dyno-app.exe"'; schtasks /create /tn DynoLaunch /sc once /st 00:00 /ru paddock-test /it /f /tr "cmd /c powershell -NoProfile -ExecutionPolicy Bypass -File C:\\paddock-test\\vmcheck\\launch.ps1" | Out-Null; schtasks /run /tn DynoLaunch | Out-Null`);
      };
      log('restart check: helper on, record, real restart, record again without Repair');
      uiSteps = await runRebootPhase({ vm, waitConsole, launchDyno, log });
    }
    const dest = join(scratch, 'out');
    mkdirSync(dest);
    scp('-r', `paddock-test@${ip}:C:/paddock-test/vmcheck/out/*`, dest);
    const stepsFile = join(dest, 'steps.json');
    if (!existsSync(stepsFile)) {
      const listing = clean(ssh(`Get-ChildItem ${GUEST}\\out -Name; '--- run.log'; Get-Content ${GUEST}\\out\\run.log -Tail 15 -ErrorAction SilentlyContinue`, 60_000).stdout);
      log(`no steps.json; the guest's out folder:\n${listing}`);
    }
    if (!existsSync(stepsFile)) throw new Error('the suite wrote no steps.json (see run.log: ' + (existsSync(join(dest, 'run.log')) ? readFileSync(join(dest, 'run.log'), 'utf8').slice(-300) : 'absent') + ')');
    const raw = JSON.parse(readFileSync(stepsFile, 'utf8').replace(/^\uFEFF/, ''));
    let steps = (Array.isArray(raw) ? raw : [raw]).map((s) => ({ ...s, checks: s.checks ?? [], evidence: [].concat(s.evidence ?? []).map(String) }));
    steps = classifyInstallFailure(steps); // one class, and nothing after a failed install counts
    // The install is per machine: a real UAC prompt must have appeared and been answered (uac-1); none at all is a broken check, not an install.
    if (steps[0] && !uacNotes.some((u) => /^uac-1:/.test(u.detail))) { steps[0].checks.push({ ok: false, kind: 'check', detail: 'no UAC prompt (consent.exe) was seen for the install: the per-machine install was not exercised' }); steps[0].status = 'fail'; }
    if (steps[0]) { steps[0].evidence.push(...uacNotes.map((u) => `UAC: ${u.detail}`)); const badUac = uacNotes.filter((u) => !u.ok); if (badUac.length) { steps[0].status = 'fail'; steps[0].checks.push(...badUac.map((u) => ({ ok: false, detail: u.detail }))); } }
    if (steps[0] && setupFacts) { steps[0].checks.push(...setupFacts.checks); steps[0].evidence.push(...setupFacts.evidence); if (!setupFacts.ok && steps[0].status === 'pass') { steps[0].status = 'check-broken'; steps[0].cls = 'check'; steps[0].summary = `installed, but ${setupFacts.checks.filter((c) => !c.ok).map((c) => c.detail).join('; ')}`.slice(0, 240); } }
    if (!done) steps.push({ title: 'Suite finished', status: 'fail', summary: 'the suite did not finish within 40 minutes', checks: [], evidence: [] });
    steps.push(...uiSteps);
    steps.push({ title: 'Tray right-click menu, dialogs at 150 % (manual)', status: 'manual', summary: 'Not automated yet: the tray menu and the 150 % scale need native input and a live display-scale change that the VMware display does not allow.' });
    const started_ = new Date().toISOString();
    const appLogText = ssh(`Get-ChildItem "$env:LOCALAPPDATA\\Dyno","$env:APPDATA\\Dyno" -Recurse -Depth 3 -Filter 'dyno.*.log' -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike 'dyno-agent*' } | Sort-Object Name | Select-Object -Last 2 | ForEach-Object { "@@ $($_.FullName)"; Get-Content $_.FullName -Tail 600 }`, 90_000).stdout;
    const allSteps = withBuildStep(steps, buildStep({ expected, found: buildProbe }));
    const required = o.sel.rows.length
      ? requiredSteps({ os: 'windows', ui: true, release: false, partial: true }).filter((r) => ['install', 'build-binding'].includes(r.id) || o.sel.manifest.includes(r.id))
      : requiredSteps({ os: 'windows', ui: Boolean(o.ui), reboot: Boolean(o.reboot), release: !o.partial, partial: Boolean(o.partial) });
    const { file, exit } = finalizeRun({
      out, started: started_, meta: { label: o.sel.rows.length ? 'windows-rows' : o.reboot && !o.ui ? 'windows-restart' : 'windows-ui', os: 'windows', run: o.run ?? null },
      run: { app: 'Dyno', version: steps[0]?.evidence?.find((e) => /^version \d/.test(e))?.replace(/^version /, ''), installer: source, installerSha256: sha256File(exe), source, expectedCommit: expected, partial: o.partial || o.sel.rows.length > 0, required, guest: `VMware Fusion Windows 11 Arm (paddock-win11-test, snapshot ${SNAPSHOT}), interactive session, x64 installer under emulation`, started: started_, commit: sh('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD']).stdout.trim(), steps: allSteps },
    });
    mkdirSync(join(dirname(file), 'evidence'), { recursive: true });
    writeFileSync(join(dirname(file), 'evidence', `${started_.replace(/[:.]/g, '-')}-win-app-log.txt`), appLogText); // the app log of every run, pass or fail
    for (const f of readdirSync(dest)) if (f.endsWith('.png') || /^setup-log-.*\.txt$|^run\.log$/.test(f)) writeFileSync(join(dirname(file), 'evidence', `${started_.replace(/[:.]/g, '-')}-win-${f}`), readFileSync(join(dest, f)));
    log(`result: ${file}`);
    console.log(file);
    return exit;
  } catch (e) {
    log(`error: ${e.message}`);
    return 2;
  } finally {
    cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
