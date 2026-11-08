#!/usr/bin/env node
// Scripted VM start check (rule 54's "prove the release path"; docs: scripts/vm-smoke/README.md). On a fresh Tart clone, headless,
// installs the app's macOS installer, cold-starts it and checks the process tree, the discovery files and Quit with evidence, then
// writes one Markdown result file and deletes the clone. Local only: CI never starts a VM.
//
//   node scripts/vm-smoke/run.mjs --app dyno (--installer <file.dmg> | --run <ci run id>) [--out <dir>] [--base <tart vm>]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statfsSync, writeFileSync, cpSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Check, runAll } from './checks.mjs';
import { buildStep, expectedCommit } from './verdict.mjs';
import { requiredSteps } from './manifest.mjs';
import { commitOfRun, finalizeRun, foundFromGrep, sha256File, withBuildStep, preflightBase } from './report.mjs';
import { PowerChecks, runPowerSuite } from './power-checks.mjs';
import { MacGui } from './mac-gui.mjs';
import { MacIdleChecks } from './mac-idle.mjs';
import { macDriver, readingSteps } from './helper-readings.mjs';
import { swapNotDriven, upgradeSwapStep } from './upgrade-swap-row.mjs';
import { macSwapPrimitives } from './upgrade-swap-prims.mjs';
import { loginJobsRow, macLoginJobsDriver } from './login-jobs-drivers.mjs';
import { diskSpaceStep, unixAdapter } from './disk-space-row.mjs';
import { discoveryStep, unixDiscoveryPrims } from './discovery-row.mjs';
import { macRebootCheck } from './mac-reboot.mjs';
import { makeMcpGuest } from './mcp-guest.mjs';
import { mcpStep } from './mcp-judges.mjs';
import { NOT_DRIVEN, ROW_TITLES, btmState, keychainOtherBuildStep, judgeLifecycleLog, judgeMacReopen, notDriven, rowStep } from './release-rows.mjs';
import { hungPageStep } from './hung-page.mjs';
import { quitStopStep } from './mcp-judges.mjs';
import { loginSwitchStep, parseBtm } from './login-start-rows.mjs';
import { jobsRestartStep } from './jobs-restart-row.mjs';
import { onlyRequired, parseOnly, rowWanted } from './only.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
const MACOS_IMAGE = 'ghcr.io/cirruslabs/macos-tahoe-base@sha256:4aaa751fa7538902d73c767f27a066fc87e83f183069da4e4b5022863abd4c77';
const GITEA = process.env.PADDOCK_GITEA ?? 'https://dev.decryptology.net';
const REPO_PATH = 'decryp7/paddock';
const SHARE = '/Volumes/My Shared Files/stage';
const MANUAL = [
  { title: 'Gatekeeper first launch (manual)', summary: 'A downloaded, quarantined copy needs right-click > Open once; Gatekeeper is enabled in the guest, but this check installs without the quarantine flag, so that prompt is not exercised. Do it in the VM window of the user\'s own test VM.' },
  { title: 'Login Items approval (manual)', summary: 'macOS may ask to allow the item under System Settings > General > Login Items & Extensions after Launch at login is switched on. That click is not automated and is not driven through any desktop.' },
];

export function parseArgs(argv) {
  const o = { guest: 'macos', suite: 'start', out: null, base: null, keep: false, maxLoad: Number(process.env.PADDOCK_VM_MAX_LOAD ?? 16) };
  const take = (i, name) => { if (i + 1 >= argv.length) throw new Error(`${name} needs a value`); return argv[i + 1]; };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--app') o.app = take(i++, a);
    else if (a === '--installer') o.installer = take(i++, a);
    else if (a === '--installer-before') o.installerBefore = take(i++, a);
    else if (a === '--run') o.run = take(i++, a);
    else if (a === '--artifact') o.artifact = take(i++, a);
    else if (a === '--out') o.out = take(i++, a);
    else if (a === '--base') o.base = take(i++, a);
    else if (a === '--guest') o.guest = take(i++, a);
    else if (a === '--suite') o.suite = take(i++, a);
    else if (a === '--reboot-check') o.reboot = true;
    else if (a === '--expect-commit') o.expectCommit = take(i++, a);
    else if (a === '--partial') o.partial = true;
    else if (a === '--audio-probe-pkg') o.audioProbePkg = take(i++, a);
    else if (a === '--audio-probe-sha256') o.audioProbeSha256 = take(i++, a);
    else if (a === '--only') o.only = parseOnly(take(i++, a));
    else if (a === '--keep-vm') o.keep = true;
    else throw new Error(`unknown option ${a}`);
  }
  if (!['start', 'power', 'helper'].includes(o.suite)) throw new Error(`--suite ${o.suite}: start (default), power or helper`);
  if (o.only && o.suite !== 'power') throw new Error('--only (run just these release rows) needs --suite power');
  if (o.audioProbePkg) {
    if (o.suite !== 'power') throw new Error('--audio-probe-pkg (a virtual audio driver for the guest only) needs --suite power');
    if (!o.audioProbeSha256) throw new Error('--audio-probe-pkg needs --audio-probe-sha256 <64 hex>: the driver is verified before it is installed');
    if (!/^[0-9a-f]{64}$/.test(o.audioProbeSha256)) throw new Error('--audio-probe-sha256 must be 64 hex digits');
  }
  if (o.only) o.partial = true; // a subset is never a release verdict
  if (!o.app) throw new Error('--app <id> is needed (dyno, spotter, pitcrew, pitwall, debrief)');
  if (!/^[a-z][a-z-]*$/.test(o.app)) throw new Error(`--app ${o.app} is not an app id`);
  if (o.installerBefore && o.suite !== 'power') throw new Error('--installer-before (the build that makes the key, for the keychain other-build measurement) needs --suite power');
  if (!o.installer === !o.run) throw new Error('give exactly one of --installer <file.dmg> or --run <CI run id>');
  if (o.run && !/^\d+$/.test(o.run)) throw new Error('--run takes a number');
  if (o.guest !== 'macos') throw new Error(`--guest ${o.guest}: only macos is scripted here. Windows guests: scripts/vm-win.sh on origin/tools/win-test-vm; Linux: tools/macos-vm-smoke.sh --guest linux (see README)`);
  return o;
}

const log = (m) => console.error(`vm-smoke: ${m}`);

const children = new Set(); // every host child in flight, so a signal can end them (cleanup)

/** Runs a host command with a guard; resolves { code, stdout, stderr }. The child is ours, so it is ended by handle. */
function hostRun(cmd, args, { timeoutMs = 120_000, input, env, cwd } = {}) {
  return new Promise((res) => {
    const child = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'], env: env ?? process.env, cwd });
    children.add(child);
    let stdout = '', stderr = '', done = false;
    const timer = setTimeout(() => { stderr += `\n(guard: ${cmd} ran longer than ${timeoutMs / 1000} s and was ended)`; child.kill('SIGTERM'); setTimeout(() => child.kill('SIGKILL'), 3000).unref(); }, timeoutMs);
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => { stderr += String(e); if (!done) { done = true; clearTimeout(timer); res({ code: 127, stdout, stderr }); } });
    child.on('close', (code, sig) => { children.delete(child); if (!done) { done = true; clearTimeout(timer); res({ code: code ?? (sig ? 143 : 1), stdout, stderr }); } });
    child.stdin.on('error', () => {}); // a child that exits before reading its stdin (EPIPE) is not an error of ours
    child.stdin.end(input ?? '');
  });
}

/** Downloads a CI artifact with the Gitea token read from the environment; the token goes to curl on stdin and is never printed. */
async function downloadArtifact(runId, name, dest) {
  const token = process.env.GITEA_TOKEN;
  if (!token) throw new Error('GITEA_TOKEN is not set (source ~/.zshenv)');
  if (!/^[A-Za-z0-9_.-]+$/.test(name)) throw new Error(`bad artifact name ${name}`);
  const url = `${GITEA}/${REPO_PATH}/actions/runs/${runId}/artifacts/${name}`;
  const zip = join(dest, `${name}.zip`);
  const r = await hostRun('curl', ['-sS', '-L', '--fail', '-o', zip, '--config', '-'], { timeoutMs: 900_000, input: `header = "Authorization: token ${token}"\nurl = "${url}"\n` });
  if (r.code !== 0) throw new Error(`downloading ${name} of run ${runId} failed (curl exit ${r.code}): ${r.stderr.replaceAll(token, '***').trim().slice(0, 200)}`);
  const dir = join(dest, 'unzipped');
  mkdirSync(dir);
  const u = await hostRun('unzip', ['-q', '-o', zip, '-d', dir]);
  if (u.code !== 0) throw new Error(`unzip failed: ${u.stderr.trim().slice(0, 200)}`);
  return dir;
}

function findDmg(dir) {
  const walk = (d) => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]));
  const dmgs = walk(dir).filter((f) => f.endsWith('.dmg'));
  if (dmgs.length !== 1) throw new Error(`expected one .dmg in ${dir}, found ${dmgs.length}`);
  return dmgs[0];
}

const sleepMs = (ms) => new Promise((r) => setTimeout(r, ms));
const load1 = () => Number(/load averages?: ([\d.,]+)/.exec(spawnSync('uptime', { encoding: 'utf8' }).stdout)?.[1].replace(',', '.') ?? 0);

async function main(argv) {
  const o = parseArgs(argv);
  const config = JSON.parse(readFileSync(join(REPO, 'apps', o.app, 'vm-smoke.json'), 'utf8'));
  // One TART_HOME default with tools/macos-vm-smoke.sh (its $HOME/.tart); the images here live on d7: export TART_HOME=/Volumes/d7sst7/dev-targets/tart-home
  const tartHome = process.env.TART_HOME ?? join(homedir(), '.tart');
  const tartEnv = { ...process.env, TART_HOME: tartHome };
  const tartBin = process.env.TART ?? 'tart';
  const tart = (args, opts = {}) => hostRun(tartBin, args, { env: tartEnv, ...opts });
  const out = resolve(o.out ?? process.env.PADDOCK_VM_SMOKE_OUT ?? join(REPO, 'vm-smoke-results'));
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(join(tmpdir(), 'vm-smoke-'));
  const owner = mkdtempSync(join(tmpdir(), 'paddock-vm-owner-')); // the VM slot's holder lives as long as this folder
  const vm = `paddock-vmsmoke-${o.app}-${new Date().toISOString().replace(/\D/g, '').slice(0, 14)}-${process.pid}`;
  const slotRoot = join(tartHome, '.paddock-vm');
  const dev = (args) => hostRun('bash', ['./dev', ...args], { timeoutMs: 600_000, env: { ...process.env } , cwd: REPO });
  let slot = null, runChild = null, cloned = false, cleaning = null;

  // Runs once, whoever calls it (the normal end, an error, a signal): ends every host child, stops and deletes the clone, gives
  // the slot back, removes the scratch folders.
  const cleanup = () => {
    cleaning ??= (async () => {
      for (const c of children) c.kill('SIGTERM');
      if (runChild) runChild.kill('SIGTERM');
      if (cloned) {
        if (o.keep) log(`--keep-vm: ${vm} is still there; delete it with: tart delete ${vm}`);
        else {
          await tart(['stop', vm], { timeoutMs: 120_000 });
          const d = await tart(['delete', vm], { timeoutMs: 120_000 });
          if (d.code !== 0 && (await tart(['list', '--quiet'])).stdout.split('\n').includes(vm)) log(`WARNING: could not delete ${vm}; run: tart delete ${vm}`);
        }
      }
      if (slot) await dev(['ci', 'slot', 'release', '--slot', slot.name, '--token', slot.token, '--root', slotRoot]);
      rmSync(owner, { recursive: true, force: true });
      rmSync(scratch, { recursive: true, force: true });
    })();
    return cleaning;
  };
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => { cleanup().finally(() => process.exit(130)); });

  try {
    if (o.guest === 'macos' && process.platform !== 'darwin') throw new Error('the macOS guest needs a Mac host with Tart');
    if ((await hostRun('which', [tartBin])).code !== 0) throw new Error('tart is needed (brew install cirruslabs/cli/tart)');

    // The installer.
    let dmg = o.installer ? resolve(o.installer) : null;
    let source = dmg;
    if (!dmg) {
      const name = o.artifact ?? `${o.app}-macOS`;
      log(`downloading artifact ${name} of CI run ${o.run}`);
      dmg = findDmg(await downloadArtifact(o.run, name, scratch));
      source = `CI run ${o.run}, artifact ${name}, ${basename(dmg)}`;
    }
    if (!existsSync(dmg) || !statSync(dmg).isFile()) throw new Error(`${dmg} is not a file`);
    const stage = join(scratch, 'stage');
    mkdirSync(stage);
    cpSync(dmg, join(stage, 'installer.dmg'));
    if (o.installerBefore) {
      if (!existsSync(o.installerBefore) || !statSync(o.installerBefore).isFile()) throw new Error(`--installer-before ${o.installerBefore} is not a file`);
      cpSync(resolve(o.installerBefore), join(stage, 'before.dmg'));
    }
    if (o.suite === 'power' || o.suite === 'helper') {
      // The GUI driver (guest/axdrive.swift) is compiled on the host, never run there, and cached by the hash of its source; the image's own
      // copy predates `drag`. The guest installs it from the read-only share.
      const src = join(HERE, 'guest', 'axdrive.swift');
      const hash = createHash('sha256').update(readFileSync(src)).digest('hex').slice(0, 16);
      mkdirSync(join(tartHome, '.paddock-vm-gui'), { recursive: true });
      const bin = join(tartHome, '.paddock-vm-gui', `axdrive-${hash}`);
      if (!existsSync(bin)) {
        log('compiling axdrive (once)');
        const c = await hostRun('swiftc', ['-O', '-o', bin, src], { timeoutMs: 600_000 });
        if (c.code !== 0) throw new Error(`swiftc failed for axdrive: ${c.stderr.trim().slice(0, 300)}`);
      }
      cpSync(bin, join(stage, 'axdrive'));
      if (o.audioProbePkg) { cpSync(resolve(o.audioProbePkg), join(stage, 'audio-driver.pkg')); cpSync(join(HERE, 'guest', 'set-default-output.swift'), join(stage, 'set-default-output.swift')); }
    }

    // A VM slot, host-wide, the same one tools/macos-vm-smoke.sh takes (`./dev ci slot`, an OS lock the kernel frees with its holder).
    mkdirSync(tartHome, { recursive: true });
    for (let waited = 0; ; waited += 6) {
      const r = await dev(['ci', 'slot', 'acquire', '--max', process.env.PADDOCK_VM_MAX ?? '1', '--owner', owner, '--root', slotRoot]);
      if (r.code === 0) { const [name, token] = r.stdout.trim().split(/\s+/); slot = { name, token }; break; }
      if (r.code !== 4) throw new Error(`could not take a VM slot (paddock-dev exit ${r.code}): ${r.stderr.trim().slice(0, 200)}`);
      if (waited >= Number(process.env.PADDOCK_VM_SLOT_WAIT ?? 3600)) throw new Error('every VM slot of this Mac stayed held (another smoke run); not starting a VM');
      log('another smoke VM run holds this Mac\'s VM slot; waiting');
      await sleepMs(5000);
    }
    log(`VM slot ${slot.name}`);

    // Disk and load, the same bounds as tools/macos-vm-smoke.sh (60 GB free under TART_HOME, 1-minute load at most 16).
    const fs = statfsSync(tartHome);
    const freeGb = Math.floor((fs.bavail * fs.bsize) / 1024 ** 3);
    const minFree = Number(process.env.PADDOCK_VM_MIN_FREE_GB ?? 60);
    if (freeGb < minFree) throw new Error(`only ${freeGb} GB free under ${tartHome}; at least ${minFree} GB are needed. Free space or set TART_HOME elsewhere.`);
    for (let waited = 0; load1() > o.maxLoad; waited += 6) {
      if (waited >= Number(process.env.PADDOCK_VM_LOAD_WAIT ?? 1800)) throw new Error(`the 1-minute load stayed above ${o.maxLoad}; not starting a VM`);
      log(`load ${load1()} > ${o.maxLoad}; waiting`);
      await sleepMs(5000);
    }

    const base = o.base ?? MACOS_IMAGE;
    log(`cloning ${base.startsWith('ghcr') ? 'the pinned macOS base image' : base} as ${vm}`);
    if (base.startsWith('ghcr')) {
      const pulled = await tart(['pull', base], { timeoutMs: 3_600_000 });
      if (pulled.code !== 0) throw new Error(`pulling the base image failed: ${pulled.stderr.trim().slice(0, 200)}`);
    }
    if (!base.startsWith('ghcr')) {
      const pf = preflightBase((await tart(['list', '--quiet'])).stdout, base);
      if (pf.status !== 'pass') {
        // Reported as a broken check (INCOMPLETE, exit 3) with the reason; nothing was cloned.
        const startedPf = new Date().toISOString();
        const reqPf = o.only ? onlyRequired(o.only) : requiredSteps({ os: 'macos', suite: o.suite, reboot: Boolean(o.reboot), release: !o.partial, partial: Boolean(o.partial) });
        const done = finalizeRun({ out, started: startedPf, meta: { label: o.suite === 'start' ? 'macos-start' : o.suite === 'power' ? 'macos-power' : o.reboot ? 'macos-helper-restart' : 'macos-helper', os: 'macos', suite: o.suite, run: o.run ?? null }, run: { app: config.app, installer: source, installerSha256: sha256File(dmg), source, partial: o.partial, required: reqPf, guest: `Tart ${base} (missing)`, started: startedPf, steps: [pf] } });
        log(`preflight: ${pf.summary}`); console.log(done.file);
        return done.exit;
      }
    }
    cloned = true; // before the clone: a signal during `tart clone` still deletes whatever it left
    const c = await tart(['clone', base, vm], { timeoutMs: 1_800_000 });
    if (c.code !== 0) throw new Error(`tart clone failed: ${c.stderr.trim().slice(0, 300)}`);

    log('booting headless');
    // The power suite also needs the VM's own VNC server (real pointer drags); it listens on 127.0.0.1 only and the URL stays in memory.
    const vnc = o.suite === 'power' || o.suite === 'helper';
    runChild = spawn(tartBin, ['run', '--no-audio', '--no-graphics', ...(vnc ? ['--vnc-experimental'] : []), `--dir=stage:${stage}:ro`, vm], { env: tartEnv, stdio: ['ignore', 'pipe', 'ignore'] });
    let vncUrl = null;
    runChild.stdout.on('data', (d) => { vncUrl ??= /vnc:\/\/\S+/.exec(String(d))?.[0] ?? null; });
    const exec = async (cmd, opts = {}) => {
      const r = await tart(['exec', vm, 'env', 'PADDOCK_DISPOSABLE_VM=1', 'bash', '-c', cmd], { timeoutMs: opts.timeoutMs ?? 120_000 });
      return r;
    };
    const t0 = Date.now();
    for (;;) {
      if ((await tart(['exec', vm, 'true'], { timeoutMs: 20_000 })).code === 0) break;
      if (runChild.exitCode !== null) throw new Error('the VM stopped while booting');
      if (Date.now() - t0 > 300_000) throw new Error('the VM was not ready after the 300 s guard');
      await new Promise((r) => setTimeout(r, 4000));
    }
    log(`VM up after ${Math.round((Date.now() - t0) / 1000)} s`);

    const guest = { exec, now: () => Date.now(), sleep: (ms) => new Promise((r) => setTimeout(r, ms)) };
    if (o.suite === 'power' || o.suite === 'helper') {
      const i = await exec(`sudo cp '${SHARE}/axdrive' /usr/local/bin/axdrive && sudo chmod 755 /usr/local/bin/axdrive && /usr/local/bin/axdrive trusted; echo trusted=$?`);
      log(`axdrive installed in the guest (${i.stdout.trim().split('\n').at(-1)})`);
      if (o.audioProbePkg) { // test tooling inside this clone only (ER-14: the host is never used): a virtual output the guest can play to, verified by hash first
        const ap = await exec(`set -e; echo '${o.audioProbeSha256}  ${SHARE}/audio-driver.pkg' | shasum -a 256 -c - >/dev/null; sudo installer -pkg '${SHARE}/audio-driver.pkg' -target / >/tmp/audio-driver.log 2>&1; sudo killall coreaudiod || true; sleep 6; cp '${SHARE}/set-default-output.swift' /tmp/ && cd /tmp && swiftc set-default-output.swift -o set-default-output && ./set-default-output set; system_profiler SPAudioDataType | grep -B1 -A8 'BlackHole' | grep -E 'BlackHole|Default Output'`, { timeoutMs: 240_000 });
        const okDev = ap.code === 0 && /Default Output Device: Yes/.test(ap.stdout);
        log(`audio probe driver: ${okDev ? 'installed, BlackHole is the default output' : 'NOT ready'} (${ap.stdout.trim().replace(/\s+/g, ' ').slice(0, 200)}${ap.code ? ' exit ' + ap.code : ''})`);
        if (!okDev) throw new Error('check broken: the guest virtual audio driver is not the default output');
      }
    }
    const check = new Check(guest, config, { log });
    const started = new Date().toISOString();
    const staged = `${SHARE}/installer.dmg`;
    const { steps, version } = o.suite === 'power' || o.suite === 'helper'
      ? await (async () => {
        const evDir = join(out, `${started.replace(/[:.]/g, '-')}-${o.app}-power-evidence`);
        mkdirSync(evDir, { recursive: true });
        // A guest file to the host as a file next to the result (PNG through base64: tart exec output is text).
        const fetch = async (path) => {
          const r = await tart(['exec', vm, 'base64', '-i', path], { timeoutMs: 60_000 });
          if (r.code !== 0) return null;
          const dest = join(evDir, basename(path));
          writeFileSync(dest, Buffer.from(r.stdout.replace(/\s+/g, ''), 'base64'));
          return dest;
        };
        const power = new PowerChecks(guest, 'macos', { bundle: `/Applications/${config.bundle}`, log });
        const pw = process.env.PADDOCK_VM_PASSWORD ?? 'admin'; // the pinned image's admin account; never printed
        const gui = new MacGui(guest, { password: pw, fetch, log });
        power.ui = gui;
        const idle = new MacIdleChecks(guest, power, check, { log, ui: gui, audioDevice: o.audioProbePkg ? null : false });
        const { loadRules, MacTour } = await import('./mac-tour.mjs');
        const { macHelperLogStep } = await import('./mac-helper-log.mjs');
        const tour = new MacTour(guest, gui, await loadRules(), { log, check });
        const waitReady = async () => { for (let i = 0; i < 75; i++) { if ((await tart(['exec', vm, 'true'], { timeoutMs: 20_000 })).code === 0 && (await guest.exec('/usr/local/bin/axdrive windows | grep -c Dock')).stdout.trim() !== '0') return true; await sleepMs(4000); } return false; };
        // Rule 71 Quit and stop measuring with its reboot half (the release row) in the restart run: menu item over AX, 60 s with no Dyno process, reopen, then a real
        // restart; the login item must bring the agent back (polled up to 150 s after the guest is ready).
        const bundleR = `/Applications/${config.bundle}`;
        const macQuitStopRestart = async () => {
          const count = async () => Number((await exec('ps -axo comm= | grep -cE "dyno-(app|agent)$"')).stdout.trim()) || 0;
          return quitStopStep({
            now: () => Date.now(), sleep: sleepMs,
            trigger: async () => { await exec(`open -a ${bundleR}`); await sleepMs(15_000); if (!(await gui.press(gui.app, '^Quit and stop measuring', 'AXMenuItem'))) throw new Error('no "Quit and stop measuring" menu item (AX)'); await sleepMs(5000); },
            count,
            reopen: async () => { await exec(`open -a ${bundleR}`); for (let i = 0; i < 10; i++) { await sleepMs(5000); if ((await exec('pgrep -x dyno-agent >/dev/null')).code === 0) break; } return Number((await exec('pgrep -x dyno-agent | wc -l')).stdout.trim()) || 0; },
            reboot: async () => {
              const boot0 = (await exec('sysctl -n kern.boottime')).stdout.trim();
              await exec('(sleep 2; sudo /sbin/reboot) >/dev/null 2>&1 &', { timeoutMs: 30_000 });
              await sleepMs(40_000);
              let up = false;
              for (let i = 0; i < 80 && !up; i++) { up = (await waitReady()) && (await exec('sysctl -n kern.boottime')).stdout.trim() !== boot0; if (!up) await sleepMs(4000); }
              if (!up) return null;
              let n = 0;
              for (let i = 0; i < 30 && n === 0; i++) { n = Number((await exec('pgrep -x dyno-agent | wc -l')).stdout.trim()) || 0; if (!n) await sleepMs(5000); }
              log(`quit-stop restart: boot ${boot0.replace(/\s+/g, ' ')} -> restarted; dyno-agent processes after it: ${n}`);
              return n;
            },
          });
        };
        const readings = o.only ? [] : o.reboot ? [macRebootCheck({ waitReady }), macQuitStopRestart] : readingSteps('macos', async () => macDriver(gui)); // WP-VMH: one step per helper reading
        // Release rows of the power suite (rule 71 MCP, the lifecycle log) and the rows not driven yet, with their reasons.
        const bundle = `/Applications/${config.bundle}`;
        let killNext = false;
        const macRows = async () => {
          const cfg = '$HOME/Library/Application Support/Dyno';
          const mg = makeMcpGuest({
            // launchd restarts a killed agent within about a second (the 0ad round: the call met a new agent), so the kill and the call run in ONE guest command: the agent is
            // killed by pid, its death is waited on, and the call follows at once, before the supervisor's new agent answers.
            os: 'macos', dyno: `${bundle}/Contents/MacOS/dyno`, sh: async (c) => {
              const inline = killNext && /\bmcp\b/.test(c);
              if (inline) killNext = false;
              const r = await exec(inline ? `p=$(pgrep -x dyno-agent | head -1); echo "$p" > /tmp/dyno-killed.pid; [ -n "$p" ] && kill -9 $p; n=0; while kill -0 $p 2>/dev/null && [ $n -lt 200 ]; do n=$((n+1)); sleep 0.01; done; ${c}` : c);
              return { code: r.code, stdout: r.stdout };
            },
            password: pw,
            configDir: async () => ((await exec(`cd "${cfg}" 2>/dev/null && pwd`)).stdout.trim() || null),
            stopApp: async () => { await exec(`osascript -e 'quit app "Dyno"' ; pkill -x dyno-app; true`); await sleepMs(4000); },
            startApp: async () => { await exec(`open -a ${bundle}`); for (let i = 0; i < 30; i++) { if ((await exec(`${bundle}/Contents/MacOS/dyno-agent status >/dev/null 2>&1`)).code === 0) break; await sleepMs(4000); } await sleepMs(8000); },
            // The agent is supervised (launchd job net.decryptology.dyno.agent.ondemand restarts a killed one within seconds): take the job out first, or the "agent is gone" half cannot be seen.
            killAgent: async () => { killNext = true; }, // by pid only, inline with the next call (above): the login job stays loaded (the product's own state); "dead" means the killed pid
            restoreAgent: async () => {},
            agentAlive: async () => (await exec('p=$(cat /tmp/dyno-killed.pid 2>/dev/null); [ -n "$p" ] && kill -0 $p 2>/dev/null')).code === 0,
            agentRestarted: async () => (await exec('p=$(cat /tmp/dyno-killed.pid 2>/dev/null); n=$(pgrep -x dyno-agent | head -1); [ -n "$n" ] && [ "$n" != "$p" ]')).code === 0,
            quitApp: async () => { await exec(`osascript -e 'quit app "Dyno"'; true`); await sleepMs(4000); },
            pidAlive: async (pid) => (await exec(`kill -0 ${pid} 2>/dev/null`)).code === 0,
          });
          const mcp = rowWanted(o.only, 'mcp') ? await mcpStep(mg) : null;
          const lg = await exec(`sudo -u nobody head -5 '/Library/Application Support/Dyno/helper-lifecycle.log' 2>&1; echo "EXIT=$?"`);
          const m = /EXIT=(\d+)\s*$/.exec(lg.stdout);
          // Quit and stop measuring, then reopen (menu item over AX), with the Login Items state before and after and the Background Items notice from the unified log.
          const count = async () => Number((await exec('ps -axo comm= | grep -cE "dyno-(app|agent)$"')).stdout.trim()) || 0;
          const btm = async () => btmState((await exec('sudo sfltool dumpbtm 2>&1 | head -600')).stdout);
          // Precondition of the reopen row: the login item is OFF before Quit and stop. It is switched off the way Dyno offers (Settings > Start Dyno at login, Save changes:
          // SMAppService unregisters it), not in System Settings, which this driver does not drive; the row then shows whether reopening Dyno turns it back on by itself.
          const ld = macLoginJobsDriver(gui);
          let switchedOff = false;
          if ((await ld.readSwitch()) === 'on' && (await ld.pressSwitch())) { const a = await ld.apply(); switchedOff = a.ok; }
          await gui.closeSettings();
          const wasState = await btm();
          const btmDyno = async () => (await exec('sudo sfltool dumpbtm 2>&1 | grep -i -B3 -A14 dyno | head -120')).stdout;
          const dumpBefore = await btmDyno();
          if (o.only && wasState !== 'off') { // ER-33: the precondition is checked first; the Quit and the reopen are not run on a bad start
            if (switchedOff && (await ld.readSwitch()) === 'off' && (await ld.pressSwitch())) await ld.apply();
            await gui.closeSettings();
            return [rowStep(ROW_TITLES.macReopen, [{ ok: false, kind: 'check', detail: `check broken: the Login Items entry is not off before the test (sfltool dumpbtm reads ${wasState}; switch ${switchedOff ? 'saved off' : 'not switched'})` }], 'unused')];
          }
          const t0 = new Date(Date.now() - 5000).toISOString().replace('T', ' ').slice(0, 19);
          const qs = await quitStopStep({
            now: () => Date.now(), sleep: sleepMs,
            trigger: async () => { await exec(`open -a ${bundle}`); await sleepMs(15_000); if (!(await gui.press(gui.app, '^Quit and stop measuring', 'AXMenuItem'))) throw new Error('no "Quit and stop measuring" menu item (AX)'); await sleepMs(5000); },
            count, reopen: async () => { await exec(`open -a ${bundle}`); await sleepMs(30_000); return count(); }, // the reboot half is the helper restart run's (macQuitStopRestart)
          });
          const back = (await exec('pgrep -x dyno-agent >/dev/null')).code === 0;
          const notice = (await exec(`log show --start "${t0}" --info --style compact --predicate 'process != "log" AND (eventMessage CONTAINS[c] "Background Items Added" OR (eventMessage CONTAINS[c] "background item" AND eventMessage CONTAINS[c] "dyno"))' 2>&1 | grep -v -E "^(Timestamp|Filtering|---)" | grep -c -i "background item"`)).stdout.trim();
          const nowState = await btm();
          const dumpAfter = await btmDyno();
          // Source of any "Background Items Added" line: the process, subsystem and message, so the notice can be told from Dyno re-registering its item.
          const noticeLines = (await exec(`log show --start "${t0}" --info --debug --style syslog --predicate 'process != "log" AND (eventMessage CONTAINS[c] "Background Items Added" OR eventMessage CONTAINS[c] "background item" OR subsystem == "com.apple.backgroundtaskmanagement" OR process == "backgroundtaskmanagementd" OR eventMessage CONTAINS[c] "SMAppService")' 2>&1 | tail -n 120`)).stdout;
          const reopen = rowStep(ROW_TITLES.macReopen, judgeMacReopen({ agentBack: back, notificationSeen: /^\d+$/.test(notice) ? Number(notice) > 0 : null, loginItemWasOff: wasState === 'unreadable' ? null : wasState === 'off', loginItemNowOff: nowState === 'unreadable' ? null : nowState === 'off' }), 'the agent is back, the Login Items choice held');
          reopen.evidence = [...(reopen.evidence ?? []), `login item before Quit and stop: ${wasState} (${switchedOff ? 'switched off with Dyno\'s own switch and saved' : 'not switched by this row'}); after the reopen: ${nowState}`];
          reopen.evidence.push(`sfltool dumpbtm (Dyno entries) BEFORE Quit and stop:\n${dumpBefore}`, `sfltool dumpbtm (Dyno entries) AFTER the reopen:\n${dumpAfter}`, `unified log since ${t0} (background task management, SMAppService, Background Items):\n${noticeLines}`);
          // Put Start at login back on for the rows after this one.
          const sw0 = switchedOff ? await ld.readSwitch() : 'n/a';
          const restored = switchedOff && sw0 === 'off' && (await ld.pressSwitch()) ? (await ld.apply()).ok : false;
          await gui.closeSettings();
          let afterRestore = await btm();
          for (let n = 0; n < 12 && restored && afterRestore !== 'on'; n++) { await sleepMs(5000); afterRestore = await btm(); } // dumpbtm lags the save
          if (o.only) check.loginOn = false; // the agent was started by this row's reopen, not by the login job (it adopts it only at the next login): Quit is judged without the login supervision claim
          reopen.evidence = [...(reopen.evidence ?? []), `login item restore: switch read ${sw0}, pressed and saved ${restored}; sfltool dumpbtm then: ${afterRestore}`];
          // Hung or paused page: the WebContent process is stopped; the question is read through AX.
          const hung = !rowWanted(o.only, 'hung-page') ? null : await hungPageStep({
            now: () => Date.now() / 1000, sleep: sleepMs,
            suspend: async () => { if ((await exec('pgrep -f com.apple.WebKit.WebContent >/dev/null')).code !== 0) throw new Error('no WebContent process'); await exec('pkill -STOP -f com.apple.WebKit.WebContent'); },
            resume: async () => { await exec('pkill -CONT -f com.apple.WebKit.WebContent'); },
            logLines: async () => (await exec('cat "$HOME/Library/Logs/Dyno"/dyno.*.log 2>/dev/null | tail -n 400')).stdout.split('\n'),
            questions: async () => (await gui.texts()).filter((t) => /Keep Waiting/.test(t.text)).length, keepWaiting: async () => gui.press(gui.app, '^Keep Waiting', 'AXButton'), shot: async (n) => gui.shot(n),
          }, ROW_TITLES.hungPage);
          return [mcp, ...(o.only ? [] : [qs]), reopen,
            rowStep(ROW_TITLES.macLifecycleLog, judgeLifecycleLog({ code: m ? Number(m[1]) : null, out: lg.stdout.replace(/EXIT=\d+\s*$/, '') }), 'a standard user reads the lifecycle log'),
            hung].filter((x) => x && rowWanted(o.only, x === reopen ? 'mac-reopen' : x === hung ? 'hung-page' : x === mcp ? 'mcp' : 'mac-lifecycle-log'));
        };
        // Launch at login switch (release row): the SMAppService login item (sfltool dumpbtm), the switch pressed and SAVED (Save changes), off then on.
        const macLoginSwitch = async () => {
          const d = macLoginJobsDriver(gui);
          const readEntry = async () => { const b = parseBtm((await exec('sudo sfltool dumpbtm 2>&1 | head -600')).stdout); return { present: b.present, count: b.count }; };
          return loginSwitchStep({ sleep: sleepMs, readSwitch: () => d.readSwitch(), pressSwitch: () => d.pressSwitch(), apply: () => d.apply(), readEntry,
            witness: async () => (await exec('cat "$HOME/Library/Logs/Dyno"/dyno.*.log 2>/dev/null | grep -i -E "login item|start at login|SMAppService" | tail -n 12')).stdout.split('\n') });
        };
        // Release row: the agent is killed (by pid, as the MCP row does), the app relaunched, and the Jobs panel must not keep "could not be read" once the agent is back.
        const macJobsRestart = async () => {
          const ld = macLoginJobsDriver(gui);
          return jobsRestartStep({
            openJobs: () => ld.openJobs(), jobsText: () => ld.jobsText(),
            killAgent: async () => (await exec('p=$(pgrep -x dyno-agent | head -1); [ -n "$p" ] && echo "$p" > /tmp/dyno-killed-jobs.pid && kill -9 $p')).code === 0,
            relaunchApp: async () => { await exec(`osascript -e 'quit app "Dyno"'; pkill -x dyno-app; true`); await sleepMs(4000); await exec(`open -a ${bundle}`); await sleepMs(8000); },
            // a NEW live agent: running, and not the pid that was killed
            agentUp: async () => (await exec('p=$(cat /tmp/dyno-killed-jobs.pid); n=$(pgrep -x dyno-agent | head -1); [ -n "$n" ] && [ "$n" != "$p" ] && "' + bundle + '/Contents/MacOS/dyno-agent" status >/dev/null 2>&1')).code === 0,
          }, { now: () => guest.now(), sleep: (ms) => guest.sleep(ms) });
        };
        const rowFns = o.only ? [...(rowWanted(o.only, 'mac-reopen') || rowWanted(o.only, 'hung-page') || rowWanted(o.only, 'mac-lifecycle-log') ? [macRows] : []), ...(o.only.includes('login-jobs') ? [async () => loginJobsRow(macLoginJobsDriver(gui), { now: () => guest.now(), sleep: (ms) => guest.sleep(ms) })] : []), ...(o.only.includes('jobs-restart') ? [macJobsRestart] : [])] : o.suite === 'power' ? [macRows, macLoginSwitch, async () => loginJobsRow(macLoginJobsDriver(gui), { now: () => guest.now(), sleep: (ms) => guest.sleep(ms) }),
          macJobsRestart,
          async () => diskSpaceStep({ exec: (c) => guest.exec(c, { timeoutMs: 300_000 }), sleep: (ms) => guest.sleep(ms), now: () => guest.now(), adapter: unixAdapter({ os: 'macos', dyno: `/Applications/${config.bundle}/Contents/MacOS/dyno` }) }),
          async () => discoveryStep(unixDiscoveryPrims({ exec: (c) => guest.exec(c), sleep: (ms) => guest.sleep(ms), now: () => guest.now(), file: '$HOME/Library/Application Support/Dyno/agent.json', dyno: `/Applications/${config.bundle}/Contents/MacOS/dyno` }))] : [];
        const suite = await runPowerSuite(check, () => power, { stagedDmg: staged, gui, idle, tour, extra: [...readings, ...rowFns], post: [() => macHelperLogStep(guest)], helperOnly: o.suite === 'helper', only: o.only, manual: o.suite === 'helper' ? [] : undefined });
        // The keychain other-build measurement (not in the manifest, never a pass or a fail): build A (--installer-before) makes a key through the app's own UI, build B
        // (--installer) is installed over it. It runs last because it replaces the installed app.
        if (o.installerBefore && !o.only) {
          const { macKeychainPrimitives } = await import('./mac-keychain-row.mjs');
          const prim = macKeychainPrimitives({ guest, gui, check, share: SHARE, bundle: `/Applications/${config.bundle}`, log });
          const km = await keychainOtherBuildStep(prim).catch((e) => ({ title: ROW_TITLES.keychainOtherBuild, status: 'not-measured', cls: 'check', summary: `not measured: ${e.message} (check broken)`, checks: [{ ok: false, kind: 'check', detail: String(e.message) }], evidence: [] }));
          await prim.cleanup().catch(() => {});
          log(`${km.status.toUpperCase()}  ${km.title}: ${km.summary}`);
          suite.steps.push(km);
        }
        // Release row: the new build installed over a RUNNING older one (needs the older .dmg as --installer-before). Last: it replaces the installed app.
        if (o.suite === 'power' && (!o.only || o.only.includes('upgrade-swap'))) {
          const sw = o.installerBefore
            ? await upgradeSwapStep(macSwapPrimitives({ guest, gui, check, share: SHARE, bundle: `/Applications/${config.bundle}` })).catch((e) => swapNotDriven(String(e.message ?? e)))
            : swapNotDriven('no older build was given (--installer-before <older .dmg>: the 1.0.0 .dmg or an older RC), so there is nothing to install over');
          log(`${sw.status.toUpperCase()}  ${sw.title}: ${sw.summary}`);
          suite.steps.push(sw);
        }
        return suite;
      })()
      : await runAll(check, { stagedDmg: staged, manual: MANUAL });
    const commit = spawnSync('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).stdout.trim();
    // The build binding: the installed binary must contain the commit CI built (12 digits, embedded by build-env).
    const expected = expectedCommit(o.expectCommit) ?? (o.run ? commitOfRun(o.run) : null);
    const appSpec = config.processes.find((p) => p.role === 'app');
    const probe = expected ? await exec(`grep -c -a -F -e ${expected} '/Applications/${config.bundle}/Contents/MacOS/${appSpec.name}' >/dev/null; exit $?`) : null;
    const allSteps = withBuildStep(steps, buildStep({ expected, found: probe ? foundFromGrep(probe.code) : null }));
    const label = o.suite === 'start' ? 'macos-start' : o.suite === 'power' ? 'macos-power' : o.reboot ? 'macos-helper-restart' : 'macos-helper';
    const required = o.only ? onlyRequired(o.only) : requiredSteps({ os: 'macos', suite: o.suite, reboot: Boolean(o.reboot), release: !o.partial, partial: Boolean(o.partial) });
    const { file, exit } = finalizeRun({
      out, started, meta: { label, os: 'macos', suite: o.suite, run: o.run ?? null },
      run: { app: config.app, version, installer: source, installerSha256: sha256File(dmg), source, expectedCommit: expected, partial: o.partial, required, guest: `Tart macOS (${base.startsWith('ghcr') ? 'pinned base image' : base}), fresh clone ${vm}, headless`, started, commit, steps: allSteps },
    });
    log(`result: ${file}`);
    console.log(file);
    return exit;
  } catch (e) {
    log(`error: ${e.message}`);
    return 2;
  } finally {
    await cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
