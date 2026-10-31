#!/usr/bin/env node
// The scripted Spotter update test on the Windows test VM (README, "Update test"). Host side: reverts the VM to gui-baseline, serves the
// local HTTPS feed on the VM NAT address, installs the OLD test build, then runs the four cases ON the desktop of paddock-test through
// `vm-win.sh job` (UI Automation, win/case.ps1) and writes a Markdown result. Test keys only (spotter-update-assets-windows.sh).
//
//   node scripts/vm-smoke/update-win.mjs --assets <assets-win dir> --vm-win <path to vm-win.sh> [--out <dir>]
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startFeed } from './update-feed.mjs';
import { CASES } from './update-judge.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const log = (m) => console.error(`update-win: ${m}`);
const quiet = (s) => s.split('\n').filter((l) => !/post-quantum|store now, decrypt later|openssh\.com\/pq|^\s*$/.test(l)).join('\n');

export function parseChecks(out) {
  return out.split('\n').map((l) => /^CHECK (ok|FAIL) (.*)$/.exec(l.trim())).filter(Boolean).map((m) => ({ ok: m[1] === 'ok', detail: m[2] }));
}

function main(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i += 2) o[argv[i].replace(/^--/, '')] = argv[i + 1];
  if (!o.assets || !o['vm-win']) throw new Error('usage: update-win.mjs --assets <dir> --vm-win <vm-win.sh> [--out <dir>]');
  const assets = resolve(o.assets);
  const vmwin = resolve(o['vm-win']);
  const plan = JSON.parse(readFileSync(join(assets, 'plan.json'), 'utf8'));
  const out = resolve(o.out ?? join(assets, '..', 'results'));
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(join(tmpdir(), 'update-win-'));
  const vm = (args, opts = {}) => spawnSync('bash', [vmwin, ...args], { encoding: 'utf8', timeout: opts.timeoutMs ?? 600_000, env: { ...process.env, PADDOCK_VM_MAX_LOAD: process.env.PADDOCK_VM_MAX_LOAD ?? '30' } });
  const job = (name, text, secs = 300) => {
    const f = join(scratch, `${name}.ps1`);
    writeFileSync(f, text);
    const r = vm(['job', f, String(secs)], { timeoutMs: (secs + 400) * 1000 });
    return { code: r.status, out: quiet(`${r.stdout}\n${r.stderr}`) };
  };
  const steps = [];
  const push = (title, checks, extra = '') => {
    const ok = checks.length > 0 && checks.every((c) => c.ok);
    steps.push({ title, ok, checks, extra });
    log(`${ok ? 'PASS' : 'FAIL'}  ${title}`);
    return ok;
  };
  let feed;
  const cleanup = async () => { feed?.close(); vm(['stop']); vm(['revert']); rmSync(scratch, { recursive: true, force: true }); };
  return (async () => {
    try {
      log('revert to gui-baseline and start');
      let r = vm(['revert', 'gui-baseline']);
      if (r.status !== 0) throw new Error(`revert failed: ${quiet(r.stderr)}`);
      r = vm(['start']);
      if (r.status !== 0) throw new Error(`start failed: ${quiet(r.stderr)}`);
      for (const [src, dst] of [[join(assets, 'ca', 'ca.cer'), 'C:/paddock-test/ca.cer'], [join(assets, 'installer', `Spotter_${plan.old}_x64-setup.exe`), 'C:/paddock-test/old-setup.exe'],
        [join(HERE, 'win', 'uia.ps1'), 'C:/paddock-test/uia.ps1'], [join(HERE, 'win', 'case.ps1'), 'C:/paddock-test/case.ps1']]) {
        const p = vm(['put', 'paddock-test', src, dst]);
        if (p.status !== 0) throw new Error(`put ${dst} failed: ${quiet(p.stderr)}`);
      }
      feed = await startFeed({ assets, host: plan.host, port: plan.port });
      const trust = job('trust-ca', readFileSync(join(HERE, 'win', 'trust-ca.ps1'), 'utf8'));
      push('Test CA trusted for paddock-test only', parseChecks(trust.out).length ? parseChecks(trust.out) : [{ ok: trust.code === 0, detail: trust.out.trim().split('\n').slice(-2).join(' ') }]);
      const inst = job('install-old', readFileSync(join(HERE, 'win', 'install-old.ps1'), 'utf8').replace('__OLD__', plan.old));
      if (!push('Old test build installed (silent NSIS, per-user)', parseChecks(inst.out))) throw new Error(`install failed: ${inst.out.slice(-300)}`);
      for (const id of ['down', 'badsig', 'wrongkey', 'ok']) {
        const cs = CASES.find((c) => c.id === id);
        feed.setCase(id); feed.clearFeedLog();
        const res = job(`case-${id}`, `$CaseId = '${id}'; $Old = '${plan.old}'; $New = '${plan.new}'\n. C:\\paddock-test\\uia.ps1\n. C:\\paddock-test\\case.ps1\n`, 900);
        const checks = parseChecks(res.out);
        const log_ = feed.feedLog();
        if (id !== 'down') {
          checks.push({ ok: log_.some((l) => /latest\.json/.test(l)), detail: 'the feed saw the manifest request' });
          checks.push({ ok: log_.some((l) => /\.exe /.test(l)) === cs.tarballRequested, detail: `the feed ${cs.tarballRequested ? 'served' : 'did not serve'} the installer (${log_.length} request(s))` });
        }
        push(cs.title, checks, res.out.split('\n').filter((l) => /^(TEXTS|NOTE)/.test(l)).join('\n'));
        for (const f of ['result', 'available', 'after-download', 'relaunched']) {
          vm(['get', 'paddock-test', `C:/paddock-test/${id}-${f}.png`, join(scratch, `${id}-${f}.png`)]);
        }
      }
      return 0;
    } catch (e) {
      log(`error: ${e.message}`);
      steps.push({ title: 'Run', ok: false, checks: [{ ok: false, detail: e.message }], extra: '' });
      return 2;
    } finally {
      const stamp = new Date().toISOString().replace(/[:.]/g, '-');
      const shots = join(out, `${stamp}-spotter-windows-screens`);
      mkdirSync(shots, { recursive: true });
      for (const f of readdirSafe(scratch).filter((n) => n.endsWith('.png'))) copyFileSync(join(scratch, f), join(shots, f));
      const md = [`# Spotter update test: Windows (${plan.old} to ${plan.new})`, '', `Guest: the Windows 11 Arm test VM (gui-baseline, x64 build under Prism), host feed ${plan.host}:${plan.port}. Test keys only. Screens: ${shots}`, '',
        '| Step | Result |', '| --- | --- |', ...steps.map((s) => `| ${s.title} | ${s.ok ? 'PASS' : 'FAIL'} |`), '',
        ...steps.flatMap((s) => [`## ${s.title}`, ...s.checks.map((c) => `- ${c.ok ? 'ok' : 'FAIL'}: ${c.detail}`), ...(s.extra ? ['', '```', s.extra, '```'] : []), ''])].join('\n');
      const file = join(out, `${stamp}-spotter-windows.md`);
      writeFileSync(file, md);
      console.log(file);
      await cleanup();
    }
  })().then((c) => (steps.some((s) => !s.ok) ? 1 : c));
}

function readdirSafe(d) { try { return spawnSync('ls', [d], { encoding: 'utf8' }).stdout.split('\n').filter(Boolean); } catch { return []; } }

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).then((c) => process.exit(c));
