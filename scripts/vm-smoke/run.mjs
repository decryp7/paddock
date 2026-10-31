#!/usr/bin/env node
// Scripted VM start check (rule 54's "prove the release path"; docs: scripts/vm-smoke/README.md). On a fresh Tart clone, headless,
// installs the app's macOS installer, cold-starts it and checks the process tree, the discovery files and Quit with evidence, then
// writes one Markdown result file and deletes the clone. Local only: CI never starts a VM.
//
//   node scripts/vm-smoke/run.mjs --app dyno (--installer <file.dmg> | --run <ci run id>) [--out <dir>] [--base <tart vm>]
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statfsSync, writeFileSync, cpSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Check, runAll } from './checks.mjs';
import { GuiCheck, runGui } from './gui.mjs';
import { renderResult } from './judge.mjs';
import { runUpdateTest } from './update.mjs';
import { startFeed } from './update-feed.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..');
export const MACOS_IMAGE = 'ghcr.io/cirruslabs/macos-tahoe-base@sha256:4aaa751fa7538902d73c767f27a066fc87e83f183069da4e4b5022863abd4c77';

/** The local Tart VM name of the GUI test image built from a pinned base (gui-image.sh): one image per base digest. */
export function guiImageName(baseRef) {
  const m = /\/macos-([a-z]+)-base@sha256:([0-9a-f]{8})/.exec(baseRef);
  if (!m) throw new Error(`not a pinned cirruslabs macOS base reference: ${baseRef}`);
  return `vmgui-base-${m[1]}-${m[2]}`;
}
const GITEA = process.env.PADDOCK_GITEA ?? 'https://dev.decryptology.net';
const REPO_PATH = 'decryp7/paddock';
const SHARE = '/Volumes/My Shared Files/stage';
const MANUAL_GUI_NOTE = 'Automated in the GUI mode: run the same command with --gui (README, "GUI mode").';
const MANUAL = [
  { title: 'Gatekeeper first launch (--gui)', summary: `Gatekeeper is enabled in the guest before the install, but this headless run installs without the quarantine flag, so its prompt is not exercised here. ${MANUAL_GUI_NOTE}` },
  { title: 'Login Items approval, login start, Finder double-click (--gui)', summary: `Not exercised headless. ${MANUAL_GUI_NOTE}` },
];

export function parseArgs(argv) {
  const o = { guest: 'macos', out: null, base: null, keep: false, maxLoad: Number(process.env.PADDOCK_VM_MAX_LOAD ?? 16) };
  const take = (i, name) => { if (i + 1 >= argv.length) throw new Error(`${name} needs a value`); return argv[i + 1]; };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--app') o.app = take(i++, a);
    else if (a === '--installer') o.installer = take(i++, a);
    else if (a === '--run') o.run = take(i++, a);
    else if (a === '--artifact') o.artifact = take(i++, a);
    else if (a === '--out') o.out = take(i++, a);
    else if (a === '--base') o.base = take(i++, a);
    else if (a === '--guest') o.guest = take(i++, a);
    else if (a === '--keep-vm') o.keep = true;
    else if (a === '--gui') o.gui = true;
    else if (a === '--update-test') { o.updateTest = take(i++, a); o.gui = true; }
    else throw new Error(`unknown option ${a}`);
  }
  if (!o.app) throw new Error('--app <id> is needed (dyno, spotter, pitcrew, pitwall, debrief)');
  if (!/^[a-z][a-z-]*$/.test(o.app)) throw new Error(`--app ${o.app} is not an app id`);
  if (o.updateTest && (o.installer || o.run)) throw new Error('--update-test takes its installer from the assets folder (spotter-update-assets.sh); give no --installer or --run');
  if (!o.updateTest && !o.installer === !o.run) throw new Error('give exactly one of --installer <file.dmg> or --run <CI run id>');
  if (o.run && !/^\d+$/.test(o.run)) throw new Error('--run takes a number');
  if (o.guest !== 'macos') throw new Error(`--guest ${o.guest}: only macos is scripted here. Windows guests: scripts/vm-win.sh on origin/tools/win-test-vm; Linux: tools/macos-vm-smoke.sh --guest linux (see README)`);
  return o;
}

/** The guest's default gateway from `route -n get default`, which is the host's address on the VM bridge; null otherwise. */
export function parseGateway(text) {
  const g = /^\s*gateway: (\d+\.\d+\.\d+\.\d+)\s*$/m.exec(String(text ?? ''))?.[1];
  return g && /^192\.168\.\d+\.1$/.test(g) ? g : null;
}

/** The VNC URL `tart run --no-graphics --vnc-experimental` prints ("VNC server is running at vnc://:<pw>@127.0.0.1:<port>"). */
export function parseVncUrl(text) {
  const m = /vnc:\/\/:?([^@\s]*)@(127\.0\.0\.1|localhost):(\d+)/.exec(String(text ?? ''));
  return m ? { password: decodeURIComponent(m[1]), host: m[2], port: Number(m[3]) } : null;
}

/**
 * vncdotool commands for a drag as a hand does it: one click on empty space of the window first (so it is key; without it the
 * press only selected the icon, measured), then press on the icon, a few moves past the drag threshold, hover on the target,
 * release.
 */
export function vncDragCommands(from, to) {
  const blank = { x: Math.round((from.x + to.x) / 2), y: from.y + 150 };
  const mid = Math.round((from.x + to.x) / 2);
  return ['move', blank.x, blank.y, 'pause', '0.3', 'click', '1', 'pause', '1', 'move', from.x, from.y, 'pause', '0.3', 'mousedown', '1', 'pause', '0.3',
    'move', from.x - 17, from.y + 5, 'pause', '0.1', 'move', mid, from.y, 'pause', '0.1', 'move', to.x, to.y, 'pause', '1', 'mouseup', '1'].map(String);
}

/** vncdotool commands for a double-click. */
export function vncDoubleClickCommands(at) {
  return ['move', at.x, at.y, 'pause', '0.3', 'click', '1', 'pause', '0.1', 'click', '1'].map(String);
}

/**
 * What this checkout lacks for `./dev` (paddock-dev, which holds the host-wide VM slot): the paths its inputs list names
 * (tools/paddock-dev/inputs.txt: the tool and the shared crates it is built from). A sparse checkout without `crates/` used to
 * fail later as "could not take a VM slot"; this names the gap and the fix instead. Returns the missing paths ([] = complete).
 */
export function checkoutGaps(repo) {
  const list = join(repo, 'tools', 'paddock-dev', 'inputs.txt');
  if (!existsSync(list)) return ['tools/paddock-dev'];
  return readFileSync(list, 'utf8').split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#')).filter((p) => !existsSync(join(repo, p)));
}

const log = (m) => console.error(`vm-smoke: ${m}`);

const children = new Set(); // every host child in flight, so a signal can end them (cleanup)

/** Runs a host command with a guard; resolves { code, stdout, stderr }. The child is ours, so it is ended by handle. */
export function hostRun(cmd, args, { timeoutMs = 120_000, input, env, cwd } = {}) {
  return new Promise((res) => {
    const child = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'], env: env ?? process.env, cwd });
    children.add(child);
    let stdout = '', stderr = '', done = false;
    const timer = setTimeout(() => { stderr += `\n(guard: ${cmd} ran longer than ${timeoutMs / 1000} s and was ended)`; child.kill('SIGTERM'); setTimeout(() => child.kill('SIGKILL'), 3000).unref(); }, timeoutMs);
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => { stderr += String(e); if (!done) { done = true; clearTimeout(timer); res({ code: 127, stdout, stderr }); } });
    child.on('close', (code, sig) => { children.delete(child); if (!done) { done = true; clearTimeout(timer); res({ code: code ?? (sig ? 143 : 1), stdout, stderr }); } });
    // A child that exits without reading its stdin (`true`, a fast `./dev` answer) closes the pipe: EPIPE is not an error here.
    child.stdin.on('error', () => {});
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
  let slot = null, runChild = null, cloned = false, cleaning = null, server = null;

  // Runs once, whoever calls it (the normal end, an error, a signal): ends every host child, stops and deletes the clone, gives
  // the slot back, removes the scratch folders.
  const cleanup = () => {
    cleaning ??= (async () => {
      for (const c of children) c.kill('SIGTERM');
      server?.close();
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
    const gaps = checkoutGaps(REPO);
    if (gaps.length) throw new Error(`this checkout lacks ${gaps.join(', ')}, which ./dev (paddock-dev, the VM slot) is built from. Use a full checkout, or: git sparse-checkout add tools crates .cargo`);

    // The installer.
    const plan = o.updateTest ? JSON.parse(readFileSync(join(resolve(o.updateTest), 'plan.json'), 'utf8')) : null;
    let dmg = o.installer ? resolve(o.installer) : plan ? findDmg(join(resolve(o.updateTest), 'installer')) : null;
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
    if (plan) cpSync(join(resolve(o.updateTest), 'ca', 'ca.pem'), join(stage, 'ca.pem'));

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

    const base = o.base ?? (o.gui ? guiImageName(MACOS_IMAGE) : MACOS_IMAGE);
    if (o.gui && !base.startsWith('ghcr') && !(await tart(['list', '--quiet'])).stdout.split('\n').includes(base)) throw new Error(`the GUI test image ${base} does not exist: build it with scripts/vm-smoke/gui-image.sh`);
    const tools = o.gui ? Object.fromEntries((await hostRun(join(HERE, 'gui', 'tools.sh'), [], { timeoutMs: 900_000, env: tartEnv })).stdout.trim().split('\n').map((l) => l.split('=')).filter((kv) => kv.length === 2)) : {};
    if (o.gui && !tools.vncdo) throw new Error('scripts/vm-smoke/gui/tools.sh could not provide vncdo');
    log(`cloning ${base.startsWith('ghcr') ? 'the pinned macOS base image' : base} as ${vm}`);
    if (base.startsWith('ghcr')) {
      const pulled = await tart(['pull', base], { timeoutMs: 3_600_000 });
      if (pulled.code !== 0) throw new Error(`pulling the base image failed: ${pulled.stderr.trim().slice(0, 200)}`);
    }
    cloned = true; // before the clone: a signal during `tart clone` still deletes whatever it left
    const c = await tart(['clone', base, vm], { timeoutMs: 1_800_000 });
    if (c.code !== 0) throw new Error(`tart clone failed: ${c.stderr.trim().slice(0, 300)}`);

    log('booting headless');
    // CI=1 as well as --no-graphics: Tart then prints the VNC URL instead of opening a viewer on the host (Run.swift). The URL
    // (with its password) is only kept in memory, never logged; the VNC server is the VM's own, on 127.0.0.1.
    let runOut = '';
    runChild = spawn(tartBin, ['run', '--no-graphics', ...(o.gui ? ['--vnc-experimental'] : []), `--dir=stage:${stage}:ro`, vm], { env: { ...tartEnv, CI: '1' }, stdio: ['ignore', 'pipe', 'pipe'] });
    runChild.stdout.on('data', (d) => { runOut += d; });
    runChild.stderr.on('data', (d) => { runOut += d; });
    const exec = async (cmd, opts = {}) => {
      const r = await tart(['exec', ...(opts.input === undefined ? [] : ['-i']), vm, 'env', 'PADDOCK_DISPOSABLE_VM=1', 'bash', '-c', cmd], { timeoutMs: opts.timeoutMs ?? 120_000, input: opts.input });
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
    const started = new Date().toISOString();
    let steps, version;
    if (o.gui) {
      if (!config.gui) throw new Error(`apps/${o.app}/vm-smoke.json has no "gui" block`);
      const vnc = parseVncUrl(runOut);
      if (!vnc) throw new Error('tart did not report the VM\'s VNC server');
      // The installer over http from the host, bound to the VM bridge only (the guest's default gateway), one file.
      const gw = parseGateway((await exec('/sbin/route -n get default')).stdout); // /sbin is not on tart exec's PATH
      if (!gw) throw new Error('the guest has no default gateway on the VM bridge (192.168.x.1): not serving the installer');
      const file = basename(dmg);
      server = createServer((req, res) => {
        if (req.url !== `/${encodeURIComponent(file)}`) { res.writeHead(404).end(); return; }
        res.writeHead(200, { 'content-type': 'application/x-apple-diskimage', 'content-length': statSync(dmg).size });
        createReadStream(dmg).pipe(res);
      });
      await new Promise((r, j) => { server.once('error', j); server.listen(0, gw, r); });
      const vncdo = (cmds) => hostRun(tools.vncdo, ['-s', `${vnc.host}::${vnc.port}`, '-p', vnc.password, ...cmds], { timeoutMs: 120_000 });
      const host = {
        installerUrl: `http://${gw}:${server.address().port}/${encodeURIComponent(file)}`,
        installerSha256: createHash('sha256').update(readFileSync(dmg)).digest('hex'),
        drag: async (from, to) => { await vncdo(vncDragCommands(from, to)); },
        doubleClick: async (at) => { await vncdo(vncDoubleClickCommands(at)); },
        // A reboot inside the guest; the image logs admin in again. Waits until commands run in an Aqua session again.
        reboot: async () => {
          await exec('sudo /sbin/shutdown -r now', { timeoutMs: 30_000 });
          const t = Date.now();
          while (Date.now() - t < 120_000 && (await tart(['exec', vm, 'true'], { timeoutMs: 10_000 })).code === 0) await sleepMs(2000);
          while (Date.now() - t < 600_000) {
            if ((await tart(['exec', vm, 'launchctl', 'managername'], { timeoutMs: 15_000 })).stdout.trim() === 'Aqua') return true;
            await sleepMs(4000);
          }
          return false;
        },
      };
      const check = new GuiCheck(guest, config, host, { log });
      if (plan) {
        // The local HTTPS feed of the update test: the VM bridge address only, the test CA's server certificate, the current case's files.
        if (gw !== plan.host) throw new Error(`the guest's gateway is ${gw} but the old build has ${plan.host} built in as its feed; rebuild the assets with --host ${gw}`);
        const feed = await startFeed({ assets: resolve(o.updateTest), host: gw, port: plan.port });
        children.add({ kill: () => feed.close() }); // closed with the other host children at cleanup
        ({ steps, version } = await runUpdateTest(check, feed, plan));
      } else ({ steps, version } = await runGui(check));
      // The screenshots (taken inside the guest) next to the result file.
      const shots = join(out, `${started.replace(/[:.]/g, '-')}-${o.app}-screens`);
      mkdirSync(shots, { recursive: true });
      for (const f of check.shots) {
        const r = spawnSync(tartBin, ['exec', vm, 'cat', `/Users/admin/vmsmoke-ev/${f}`], { env: tartEnv, maxBuffer: 64 * 1024 * 1024 });
        if (r.status === 0) writeFileSync(join(shots, f), r.stdout);
      }
      steps.find((s) => s.title.startsWith('Screenshots')).summary += ` in ${shots}`;
    } else {
      const check = new Check(guest, config, { log });
      ({ steps, version } = await runAll(check, { stagedDmg: `${SHARE}/installer.dmg`, manual: MANUAL }));
    }
    const commit = spawnSync('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).stdout.trim();
    const md = renderResult({ app: config.app, version, installer: source, guest: `Tart macOS (${base.startsWith('ghcr') ? 'pinned base image' : base}), fresh clone ${vm}, headless${o.gui ? ', GUI mode (AX inside the guest, the VM\'s own VNC for pointer drags)' : ''}`, started, commit, steps });
    const file = join(out, `${started.replace(/[:.]/g, '-')}-${o.app}.md`);
    writeFileSync(file, md);
    log(`result: ${file}`);
    console.log(file);
    return steps.some((s) => s.status === 'fail') ? 1 : 0;
  } catch (e) {
    log(`error: ${e.message}`);
    return 2;
  } finally {
    await cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
