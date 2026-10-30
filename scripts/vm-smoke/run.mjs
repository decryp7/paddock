#!/usr/bin/env node
// Scripted VM start check (rule 54's "prove the release path"; docs: scripts/vm-smoke/README.md). On a fresh Tart clone, headless,
// installs the app's macOS installer, cold-starts it and checks the process tree, the discovery files and Quit with evidence, then
// writes one Markdown result file and deletes the clone. Local only: CI never starts a VM.
//
//   node scripts/vm-smoke/run.mjs --app dyno (--installer <file.dmg> | --run <ci run id>) [--out <dir>] [--base <tart vm>]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync, cpSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
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
  { title: 'Gatekeeper first launch (manual)', summary: 'A downloaded, quarantined copy needs right-click > Open once; this check installs without the quarantine flag, so that prompt is not exercised. Do it in the VM window of the user\'s own test VM.' },
  { title: 'Login Items approval (manual)', summary: 'macOS may ask to allow the item under System Settings > General > Login Items & Extensions after Launch at login is switched on. That click is not automated and is not driven through any desktop.' },
];

export function parseArgs(argv) {
  const o = { guest: 'macos', out: null, base: null, keep: false, maxLoad: Number(process.env.PADDOCK_VM_MAX_LOAD ?? 30) };
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

/** Runs a host command with a guard; resolves { code, stdout, stderr }. The child is ours, so it is ended by handle. */
function hostRun(cmd, args, { timeoutMs = 120_000, input, env } = {}) {
  return new Promise((res) => {
    const child = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'], env: env ?? process.env });
    let stdout = '', stderr = '', done = false;
    const timer = setTimeout(() => { stderr += `\n(guard: ${cmd} ran longer than ${timeoutMs / 1000} s and was ended)`; child.kill('SIGTERM'); setTimeout(() => child.kill('SIGKILL'), 3000).unref(); }, timeoutMs);
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (e) => { stderr += String(e); if (!done) { done = true; clearTimeout(timer); res({ code: 127, stdout, stderr }); } });
    child.on('close', (code, sig) => { if (!done) { done = true; clearTimeout(timer); res({ code: code ?? (sig ? 143 : 1), stdout, stderr }); } });
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

const load1 = () => Number(/load averages?: ([\d.,]+)/.exec(spawnSync('uptime', { encoding: 'utf8' }).stdout)?.[1].replace(',', '.') ?? 0);

async function main(argv) {
  const o = parseArgs(argv);
  const config = JSON.parse(readFileSync(join(REPO, 'apps', o.app, 'vm-smoke.json'), 'utf8'));
  const tartHome = process.env.TART_HOME ?? '/Volumes/d7sst7/dev-targets/tart-home';
  const tartEnv = { ...process.env, TART_HOME: tartHome };
  const tart = (args, opts = {}) => hostRun(process.env.TART ?? 'tart', args, { env: tartEnv, ...opts });
  const out = resolve(o.out ?? process.env.PADDOCK_VM_SMOKE_OUT ?? join(REPO, 'vm-smoke-results'));
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(join(tmpdir(), 'vm-smoke-'));
  const vm = `paddock-vmsmoke-${o.app}-${new Date().toISOString().replace(/\D/g, '').slice(0, 14)}-${process.pid}`;
  const lock = join(tartHome, '.paddock-vm-smoke.lock');
  let haveLock = false, runChild = null, cloned = false;

  const cleanup = async () => {
    if (runChild) runChild.kill('SIGTERM');
    if (cloned && !o.keep) { await tart(['stop', vm], { timeoutMs: 120_000 }); const d = await tart(['delete', vm], { timeoutMs: 120_000 }); if (d.code !== 0) log(`WARNING: could not delete ${vm}; run: tart delete ${vm}`); }
    else if (cloned) log(`--keep-vm: ${vm} is still there; delete it with: tart delete ${vm}`);
    if (haveLock) rmSync(lock, { recursive: true, force: true });
    rmSync(scratch, { recursive: true, force: true });
  };
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => { cleanup().finally(() => process.exit(130)); });

  try {
    if (o.guest === 'macos' && process.platform !== 'darwin') throw new Error('the macOS guest needs a Mac host with Tart');
    if ((await hostRun('which', ['tart'])).code !== 0 && !process.env.TART) throw new Error('tart is needed (brew install cirruslabs/cli/tart)');
    const l = load1();
    if (l >= o.maxLoad) throw new Error(`the 1-minute load is ${l} (limit ${o.maxLoad}); not starting a VM`);

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

    // One VM at a time, host-wide (the same lock tools/macos-vm-smoke.sh uses).
    mkdirSync(tartHome, { recursive: true });
    try { mkdirSync(lock); } catch { throw new Error(`another VM run holds ${lock} (one VM at a time); try again when it is done`); }
    haveLock = true;
    writeFileSync(join(lock, 'pid'), String(process.pid));

    const base = o.base ?? MACOS_IMAGE;
    log(`cloning ${base.startsWith('ghcr') ? 'the pinned macOS base image' : base} as ${vm}`);
    const c = await tart(['clone', base, vm], { timeoutMs: 1_800_000 });
    if (c.code !== 0) throw new Error(`tart clone failed: ${c.stderr.trim().slice(0, 300)}`);
    cloned = true;

    log('booting headless');
    runChild = spawn(process.env.TART ?? 'tart', ['run', '--no-graphics', `--dir=stage:${stage}:ro`, vm], { env: tartEnv, stdio: 'ignore' });
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
