#!/usr/bin/env node
// The Windows guest of the VM release checks (local only, never CI): reverts the Windows 11 test VM (VMware Fusion, `vm-win.sh`) to its
// `gui-session-tools` snapshot, starts it, waits for the interactive session, stages the installer and win-suite.ps1, runs the suite there
// (a scheduled task in the paddock-test session), polls for its result file with a bounded wait, and writes the Markdown result in the
// vm-smoke format. The VM is reverted and stopped at the end, also after a failure.
//
//   node scripts/vm-smoke/run-win.mjs --app dyno (--installer <setup.exe> | --run <ci run id>) [--out <dir>]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { renderResult } from './judge.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
const VMWIN = process.env.PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh';
const VMDIR = process.env.PADDOCK_WIN_VM_DIR ?? '/Volumes/d7sst7/vms/paddock-win11-test';
const SNAPSHOT = 'gui-session-tools';
const GUEST = 'C:\\paddock-test\\vmcheck';
const log = (m) => console.error(`vm-smoke(win): ${m}`);

export function parseArgs(argv) {
  const o = { out: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--app') o.app = v(); else if (a === '--installer') o.installer = v(); else if (a === '--run') o.run = v(); else if (a === '--out') o.out = v();
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
  const cleanup = () => {
    if (started) {
      log(`reverting the VM to ${SNAPSHOT}`);
      vm(['stop'], 200_000);
      vm(['revert', SNAPSHOT], 300_000);
    }
    rmSync(scratch, { recursive: true, force: true });
  };
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => { cleanup(); process.exit(130); });
  try {
    const exe = o.installer ? resolve(o.installer) : await download(o.run, 'dyno-Windows', scratch);
    if (!existsSync(exe)) throw new Error(`${exe} is not a file`);
    const source = o.installer ?? `CI run ${o.run}, artifact dyno-Windows, ${basename(exe)}`;
    started = true;
    log('revert to the snapshot'); if (vm(['revert', SNAPSHOT], 300_000).code !== 0) throw new Error(`revert to ${SNAPSHOT} failed`);
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
    ssh(`Remove-Item ${GUEST} -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force ${GUEST}\\out | Out-Null`);
    const guestExe = `${GUEST}\\setup.exe`;
    if (scp(exe, `paddock-test@${ip}:C:/paddock-test/vmcheck/setup.exe`).code !== 0) throw new Error('copying the installer failed');
    if (scp(join(HERE, 'win-suite.ps1'), `paddock-test@${ip}:C:/paddock-test/vmcheck/win-suite.ps1`).code !== 0) throw new Error('copying win-suite.ps1 failed');
    log('running the suite in the interactive session (bounded 40 min)');
    ssh(`schtasks /create /tn VmCheck /sc once /st 00:00 /ru paddock-test /it /f /tr 'powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ${GUEST}\\win-suite.ps1 -Installer ${guestExe} -Out ${GUEST}\\out'; schtasks /run /tn VmCheck`);
    let done = false;
    for (let i = 0; i < 480 && !done; i++) {
      done = clean(ssh(`Test-Path ${GUEST}\\out\\done.txt`, 60_000).stdout).includes('True');
      if (!done) await sleep(5000);
    }
    const dest = join(scratch, 'out');
    mkdirSync(dest);
    scp('-r', `paddock-test@${ip}:C:/paddock-test/vmcheck/out/*`, dest);
    const stepsFile = join(dest, 'steps.json');
    if (!existsSync(stepsFile)) throw new Error('the suite wrote no steps.json (see run.log: ' + (existsSync(join(dest, 'run.log')) ? readFileSync(join(dest, 'run.log'), 'utf8').slice(-300) : 'absent') + ')');
    const raw = JSON.parse(readFileSync(stepsFile, 'utf8').replace(/^\uFEFF/, ''));
    const steps = (Array.isArray(raw) ? raw : [raw]).map((s) => ({ ...s, checks: s.checks ?? [], evidence: [].concat(s.evidence ?? []).map(String) }));
    if (!done) steps.push({ title: 'Suite finished', status: 'fail', summary: 'the suite did not finish within 40 minutes', checks: [], evidence: [] });
    steps.push({ title: 'Admin helper (UAC), tray menu, dialogs at 100/150 %, snap check, WebView2 pages (manual)', status: 'manual', summary: 'Not automated yet: needs the CDP and VNC drivers (README, "Not built yet").' });
    const started_ = new Date().toISOString();
    const md = renderResult({ app: 'Dyno', version: steps[0]?.evidence?.[0]?.replace(/^version /, ''), installer: source, guest: 'VMware Fusion Windows 11 Arm (paddock-win11-test, snapshot gui-session-tools), interactive session, x64 installer under emulation', started: started_, commit: sh('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD']).stdout.trim(), steps });
    const file = join(out, `${started_.replace(/[:.]/g, '-')}-dyno-win.md`);
    writeFileSync(file, md);
    for (const f of readdirSync(dest)) if (f.endsWith('.png')) writeFileSync(join(out, `${started_.replace(/[:.]/g, '-')}-win-${f}`), readFileSync(join(dest, f)));
    log(`result: ${file}`);
    console.log(file);
    return steps.some((s) => s.status === 'fail') ? 1 : 0;
  } catch (e) {
    log(`error: ${e.message}`);
    return 2;
  } finally {
    cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
