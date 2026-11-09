#!/usr/bin/env node
// The Defender detection-rate run on the Windows test VM (Dyno 1.1.1 WP-4): N installs of one installer, each from a reverted snapshot, with an EICAR
// positive control first (ER-33: without a control that Defender saw, "no detection" is `check broken`). LOCAL ONLY, never CI; it drives only the VM
// that scripts/vm-win.sh allows (paddock-win11-test). The control string exists only in the guest command line and the guest.
//
//   node scripts/vm-smoke/defender-rate.mjs --installer <Dyno_x_x64-setup.exe> [--installs 10] [--snapshot <name>] [--out <folder>] [--settle 120] [--force] [--plan]
//
// --plan prints the schedule and exits (no VM is touched). The install is silent (`/S`) under the administrator account over SSH, not the GUI/UAC path
// of run-win.mjs: the installer's parent process differs from a user's, so a rate from here is a rate for this path.
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { holdVmLock } from './vm-lock-reexec.mjs';
import { isMain } from '../../conformance/src/util/is-main.mjs';
import { VMWIN } from './vm-win-path.mjs';
import { checkWinVmTarget } from './win-vm-target.mjs';
import { DEFAULT_SETTLE_S, CONTROL_NEXT_STEP, controlPs, eventsPs, nowPs, parseNow, parseEvents, planRateRun, runRate, threatPs, versionsPs, versionsLine, DEFAULT_INSTALLS } from './defender-evidence.mjs';

const GUEST_DIR = 'C:\\paddock-test\\vmcheck';
const GUEST_EXE = `${GUEST_DIR}\\setup.exe`;

export function parseArgs(argv) {
  const o = { installs: DEFAULT_INSTALLS, snapshot: process.env.PADDOCK_WIN_SNAPSHOT ?? 'gui-session-tools-no-dyno-2026-11-08', plan: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === '--plan') { o.plan = true; continue; }
    if (k === '--force') { o.force = true; continue; }
    if (!['--installer', '--installs', '--snapshot', '--out', '--settle'].includes(k)) throw new Error(`unknown option: ${k}`);
    if (argv[i + 1] === undefined) throw new Error(`${k} needs a value`);
    o[k.slice(2)] = argv[++i];
  }
  o.installs = Number(o.installs);
  o.settleS = o.settle === undefined ? DEFAULT_SETTLE_S : Number(o.settle);
  if (!Number.isFinite(o.settleS) || o.settleS < 0 || o.settleS > 900) throw new Error('--settle must be 0 to 900 seconds');
  if (!Number.isInteger(o.installs) || o.installs < 1 || o.installs > 50) throw new Error('--installs must be a whole number from 1 to 50');
  if (!o.installer) throw new Error('--installer <setup.exe> is needed');
  return o;
}

/** The guest adapter over scripts/vm-win.sh (which refuses every VM but paddock-win11-test). Each call returns the command's stdout. */
export function makeAdapter({ vmwin = VMWIN, snapshot, installer, run = (args, ms) => spawnSync('bash', [vmwin, ...args], { encoding: 'utf8', timeout: ms }) }) {
  const vm = (args, ms = 120_000) => {
    const r = run(args, ms);
    if (r.status !== 0) throw new Error(`vm-win.sh ${args[0]} failed (exit ${r.status ?? 'timeout'}): ${(r.stderr || r.stdout || '').trim().split('\n').slice(-2).join(' | ')}`);
    return (r.stdout ?? '').split('\n').filter((l) => !l.startsWith('**')).join('\n').trim();
  };
  const admin = (cmd, ms = 120_000) => vm(['ssh', '--admin', cmd], ms);
  return {
    // Revert (the VM is then off), start (waits for SSH), and stage the installer: a clean guest per run.
    revert: async () => {
      vm(['revert', snapshot], 300_000);
      vm(['start'], 600_000);
      admin(`New-Item -ItemType Directory -Force '${GUEST_DIR}' | Out-Null`);
      vm(['put', 'paddock-admin', installer, GUEST_EXE], 600_000);
    },
    versions: async () => versionsLine(admin(versionsPs())) ?? '(unreadable or partial)',
    control: async () => {
      const since = parseNow(admin(nowPs()));
      const out = admin(controlPs(), 180_000);
      return { events: parseEvents(admin(eventsPs(20, since))), out };
    },
    now: async () => {
      const t = parseNow(admin(nowPs()));
      if (!t) throw new Error('the guest clock could not be read');
      return t;
    },
    install: async () => {
      const out = admin(`$p = Start-Process '${GUEST_EXE}' -ArgumentList '/S' -Wait -PassThru -ErrorAction Stop; 'EXIT=' + $p.ExitCode; 'DONE'`, 900_000);
      const m = /EXIT=(-?\d+)/.exec(out);
      if (!m) throw new Error(`the install did not report an exit code: ${out.slice(-200)}`);
      return { exit: Number(m[1]) };
    },
    readEvents: async (_i, since) => parseEvents(admin(eventsPs(50, since))),
    threats: async () => admin(threatPs()),
  };
}

async function main(argv) {
  const o = parseArgs(argv);
  const installer = resolve(o.installer);
  const plan = planRateRun({ snapshot: o.snapshot, installs: o.installs, installer: basename(installer), settleS: o.settleS });
  console.log(JSON.stringify(plan, null, 2));
  if (o.plan) return 0;
  if (!existsSync(installer)) throw new Error(`no such installer: ${installer}`);
  checkWinVmTarget();
  const sha = createHash('sha256').update(readFileSync(installer)).digest('hex');
  console.log(`installer ${basename(installer)} sha256 ${sha}`);
  const g = makeAdapter({ snapshot: o.snapshot, installer });
  const res = await runRate({ installs: o.installs, settleS: o.settleS, force: o.force, ...g, names: ['paddock-setup.exe', basename(installer), 'setup.exe'] });
  if (o.out) {
    mkdirSync(o.out, { recursive: true });
    const file = join(o.out, `defender-rate-${new Date().toISOString().replace(/[:.]/g, '-')}.json`);
    writeFileSync(file, JSON.stringify({ installer: basename(installer), sha256: sha, snapshot: o.snapshot, ...res }, null, 2));
    console.log(`wrote ${file}`);
  }
  console.log(`VERDICT: ${res.summary.verdict}`);
  if (res.summary.verdict === 'check broken') console.error(`::error::check broken: ${res.summary.note}. Next step: ${CONTROL_NEXT_STEP}`);
  return res.summary.verdict === 'check broken' || res.summary.verdict === 'not measured' ? 3 : 0;
}

if (isMain(import.meta.url)) {
  (process.argv.includes('--plan') ? undefined : holdVmLock('defender-rate')) ?? main(process.argv.slice(2)).then((c) => process.exit(c), (e) => { console.error(`::error::${e.message}. See the phase lines above for where it stopped; the VM is left as it was (revert it with scripts/vm-win.sh revert <snapshot>).`); process.exit(1); });
}
