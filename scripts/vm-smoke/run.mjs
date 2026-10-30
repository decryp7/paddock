#!/usr/bin/env node
// Scripted VM start check (rule 54's "prove the release path"; docs: scripts/vm-smoke/README.md). On a fresh Tart clone, headless,
// installs the app's macOS installer, cold-starts it and checks the process tree, the discovery files and Quit with evidence, then
// writes one Markdown result file and deletes the clone. Local only: CI never starts a VM.
//
//   node scripts/vm-smoke/run.mjs --app dyno (--installer <file.dmg> | --run <ci run id>) [--out <dir>] [--base <tart vm>]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statfsSync, writeFileSync, cpSync, statSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Check, runAll } from './checks.mjs';
import { renderResult } from './judge.mjs';

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
    else throw new Error(`unknown option ${a}`);
  }
  if (!o.app) throw new Error('--app <id> is needed (dyno, spotter, pitcrew, pitwall, debrief)');
  if (!/^[a-z][a-z-]*$/.test(o.app)) throw new Error(`--app ${o.app} is not an app id`);
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
    cloned = true; // before the clone: a signal during `tart clone` still deletes whatever it left
    const c = await tart(['clone', base, vm], { timeoutMs: 1_800_000 });
    if (c.code !== 0) throw new Error(`tart clone failed: ${c.stderr.trim().slice(0, 300)}`);

    log('booting headless');
    runChild = spawn(tartBin, ['run', '--no-graphics', `--dir=stage:${stage}:ro`, vm], { env: tartEnv, stdio: 'ignore' });
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
    const check = new Check(guest, config, { log });
    const started = new Date().toISOString();
    const { steps, version } = await runAll(check, { stagedDmg: `${SHARE}/installer.dmg`, manual: MANUAL });
    const commit = spawnSync('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).stdout.trim();
    const md = renderResult({ app: config.app, version, installer: source, guest: `Tart macOS (${base.startsWith('ghcr') ? 'pinned base image' : base}), fresh clone ${vm}, headless`, started, commit, steps });
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
