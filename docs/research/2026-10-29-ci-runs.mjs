#!/usr/bin/env node
// The measurement behind docs/research/2026-10-29-ci-runs.md: every Gitea Actions run of a window and its jobs, then one
// row per workflow, job and OS (count, median and p90 duration, wait, pass/fail/cancel, branch/main/other, runner-hours).
// A research tool, not a gate (no tests): read-only API calls, the token from GITEA_TOKEN, never printed.
//
//   node docs/research/2026-10-29-ci-runs.mjs pull  --end 2026-10-29T15:08:41+08:00 --hours 48 > runs.json
//   node docs/research/2026-10-29-ci-runs.mjs table runs.json --end 2026-10-29T15:08:41+08:00 --hours 48
//   node docs/research/2026-10-29-ci-runs.mjs steps runs.json --end ... --hours 48 [--os Windows] [--step stable-workspace]
//   node docs/research/2026-10-29-ci-runs.mjs zombies runs.json --end ... --hours 48
//   GITEA_TOKEN=... node docs/research/2026-10-29-ci-runs.mjs logs runs.json --end ... --hours 48 --job 'crate (windows' > fails.txt
//
// `table` counts jobs that never started apart: skipped (a condition or a skipped need) and cancelled before start.
// `steps`: runner time per step name and OS (passed and failed jobs), with median and p90 per step (the Windows split and
// the stable-workspace median of the note). `zombies`: jobs that started, wrote no log and had every step stamped
// `failure` at the second the job ended (Gitea's stop_zombie_tasks). `logs`: for failed jobs whose name starts with --job,
// the failing nextest and Playwright lines of each log (the flake table).
//
// Wait = job start minus the end of the newest earlier job of the same run that ended before it started (or the run's
// creation): an upper bound on the runner queue for a job that needs only the plan job (every job of ci.yml), a lower one
// where a job needs several. Shard and per-app e2e legs are folded into one row ("(shard)", "e2e shard").
import { readFileSync } from 'node:fs';

const API = process.env.GITEA_API ?? 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock';
if (!API.startsWith('https://dev.decryptology.net/')) console.error(`warning: GITEA_API is ${API}, not dev.decryptology.net: GITEA_TOKEN is sent there`);
const args = process.argv.slice(2);
const opt = (n, d) => (args.includes(`--${n}`) ? args[args.indexOf(`--${n}`) + 1] : d);
const END = Date.parse(opt('end', new Date().toISOString()));
const START = END - Number(opt('hours', '48')) * 3600e3;
const ts = (x) => (x && !x.startsWith('1970') && !x.startsWith('0001') ? Date.parse(x) : null);
const pct = (v, p) => {
  if (!v.length) return null;
  const s = [...v].sort((a, b) => a - b);
  const k = (s.length - 1) * p;
  const lo = Math.floor(k);
  return s[lo] + (s[Math.ceil(k)] - s[lo]) * (k - lo);
};

async function pull() {
  const headers = { Authorization: `token ${process.env.GITEA_TOKEN}` };
  const get = async (p) => {
    for (let i = 0; i < 4; i++) {
      const r = await fetch(API + p, { headers }).catch(() => null);
      if (r?.ok) return r.json();
    }
    throw new Error(`cannot read ${p}`);
  };
  const runs = [];
  for (let page = 1; ; page++) {
    const wr = (await get(`/actions/runs?limit=50&page=${page}`)).workflow_runs ?? [];
    if (!wr.length) break;
    for (const r of wr) if (Date.parse(r.created_at) >= START && Date.parse(r.created_at) <= END) runs.push(r);
    if (wr.some((r) => Date.parse(r.created_at) < START)) break;
  }
  for (const r of runs) {
    for (const k of ['repository', 'head_repository', 'actor', 'trigger_actor']) delete r[k];
    r.jobs = [];
    for (let p = 1; ; p++) {
      const js = (await get(`/actions/runs/${r.id}/jobs?limit=100&page=${p}`)).jobs ?? [];
      r.jobs.push(...js);
      if (js.length < 100) break;
    }
  }
  process.stdout.write(JSON.stringify(runs));
}

function table(file) {
  const runs = JSON.parse(readFileSync(file, 'utf8')).filter((r) => Date.parse(r.created_at) >= START && Date.parse(r.created_at) <= END);
  const os = (j) => (/macos/.test(j.labels.join()) ? 'macOS' : /windows/.test(j.labels.join()) ? 'Windows' : 'Linux');
  const norm = (n) =>
    n
      .replace(/^app (\S+) \((\S+)-latest(, e2e-\d)?\)$/, (m, a, o, e) => `app ${a}${e ? ' e2e shard' : ''}`)
      .replace(/\(shard \d\/\d\)/, '(shard)')
      .replace(/ \((ubuntu|macos|windows)-latest\)$/, '');
  const rows = new Map();
  for (const r of runs) {
    const wf = r.path.split('@')[0];
    const ref = r.path.split('@')[1] ?? '';
    const kind = r.event === 'schedule' ? 'other' : ref === 'refs/heads/main' ? 'main' : ref.startsWith('refs/tags/') ? 'other' : 'branch';
    const jobs = r.jobs.map((j) => ({ j, s: ts(j.started_at), e: ts(j.completed_at) })).filter((x) => x.s && x.e && x.j.labels.length);
    for (const j of r.jobs.filter((y) => !ts(y.started_at) && y.labels.length)) {
      const key = `${wf} | ${norm(j.name)} | ${os(j)}`;
      const e = rows.get(key) ?? { key, dur: [], wait: [], ok: 0, fail: 0, cancel: 0, skipped: 0, unstarted: 0, branch: 0, main: 0, other: 0 };
      e[j.conclusion === 'skipped' ? 'skipped' : 'unstarted']++;
      rows.set(key, e);
    }
    for (const x of jobs) {
      const prev = jobs.filter((y) => y !== x && y.e <= x.s + 2000).reduce((m, y) => Math.max(m, y.e), ts(r.created_at));
      const key = `${wf} | ${norm(x.j.name)} | ${os(x.j)}`;
      const e = rows.get(key) ?? { key, dur: [], wait: [], ok: 0, fail: 0, cancel: 0, skipped: 0, unstarted: 0, branch: 0, main: 0, other: 0 };
      e.dur.push((x.e - x.s) / 1000);
      e.wait.push(Math.max(0, (x.s - prev) / 1000));
      e[x.j.conclusion === 'success' ? 'ok' : x.j.conclusion === 'failure' ? 'fail' : 'cancel']++;
      e[kind]++;
      rows.set(key, e);
    }
  }
  const m = (s) => (s == null ? '-' : s < 90 ? `${Math.round(s)} s` : `${Math.round(s / 60)} min`);
  console.log('| workflow | job | OS | n | median | p90 | wait median | wait p90 | pass | fail | cancelled | skipped | not started | branch/main/other | runner-h |');
  console.log('|---|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|---|--:|');
  for (const e of [...rows.values()].filter((x) => x.dur.length).sort((a, b) => b.dur.reduce((x, y) => x + y, 0) - a.dur.reduce((x, y) => x + y, 0))) {
    const [wf, job, o] = e.key.split(' | ');
    const h = e.dur.reduce((x, y) => x + y, 0) / 3600;
    console.log(`| ${wf} | ${job} | ${o} | ${e.dur.length} | ${m(pct(e.dur, 0.5))} | ${m(pct(e.dur, 0.9))} | ${m(pct(e.wait, 0.5))} | ${m(pct(e.wait, 0.9))} | ${e.ok} | ${e.fail} | ${e.cancel} | ${e.skipped} | ${e.unstarted} | ${e.branch}/${e.main}/${e.other} | ${h.toFixed(1)} |`);
  }
}

const windowRuns = (file) => JSON.parse(readFileSync(file, 'utf8')).filter((r) => Date.parse(r.created_at) >= START && Date.parse(r.created_at) <= END);
const osOf = (j) => (/macos/.test(j.labels.join()) ? 'macOS' : /windows/.test(j.labels.join()) ? 'Windows' : /ubuntu/.test(j.labels.join()) ? 'Linux' : null);

function steps(file) {
  const only = opt('os', null);
  const step = opt('step', null);
  const per = new Map();
  for (const r of windowRuns(file)) {
    for (const j of r.jobs) {
      const o = osOf(j);
      if (!o || (only && o !== only) || !['success', 'failure'].includes(j.conclusion)) continue;
      const jd = (ts(j.completed_at) - ts(j.started_at)) / 1000;
      let inSteps = 0;
      for (const st of j.steps ?? []) {
        const d = (ts(st.completed_at) - ts(st.started_at)) / 1000;
        if (!(d >= 0)) continue;
        inSteps += d;
        const k = `${o} | ${st.name}`;
        (per.get(k) ?? per.set(k, []).get(k)).push(d);
      }
      const k = `${o} | (job time outside steps)`;
      (per.get(k) ?? per.set(k, []).get(k)).push(Math.max(0, jd - inSteps));
    }
  }
  const total = {};
  for (const [k, v] of per) total[k.split(' | ')[0]] = (total[k.split(' | ')[0]] ?? 0) + v.reduce((a, b) => a + b, 0);
  console.log('| OS | step | n | median s | p90 s | hours | share of OS |');
  console.log('|---|---|--:|--:|--:|--:|--:|');
  for (const [k, v] of [...per].sort((a, b) => b[1].reduce((x, y) => x + y, 0) - a[1].reduce((x, y) => x + y, 0))) {
    const [o, name] = k.split(' | ');
    if (step && !name.includes(step)) continue;
    const h = v.reduce((a, b) => a + b, 0);
    console.log(`| ${o} | ${name} | ${v.length} | ${Math.round(pct(v, 0.5))} | ${Math.round(pct(v, 0.9))} | ${(h / 3600).toFixed(2)} | ${Math.round((100 * h) / total[o])} % |`);
  }
}

function zombies(file) {
  for (const r of windowRuns(file)) {
    for (const j of r.jobs) {
      if (j.conclusion !== 'failure' || !ts(j.started_at) || !j.steps?.length) continue;
      if (!j.steps.every((st) => st.conclusion === 'failure' && st.started_at === j.completed_at)) continue;
      console.log(`run ${r.id} job ${j.id} runner ${j.runner_id} ${r.path.split('@')[0]} "${j.name}" ${j.started_at} -> ${j.completed_at} (${Math.round((ts(j.completed_at) - ts(j.started_at)) / 60000)} min)`);
    }
  }
}

async function logs(file) {
  const prefix = opt('job', '');
  const headers = { Authorization: `token ${process.env.GITEA_TOKEN}` };
  const failing = /^\s*(FAIL|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|ABORT) .*\[|^\s+\d+\) \[|^::error::/;
  for (const r of windowRuns(file)) {
    for (const j of r.jobs) {
      if (j.conclusion !== 'failure' || !j.name.startsWith(prefix)) continue;
      const res = await fetch(`${API}/actions/jobs/${j.id}/logs`, { headers });
      const text = res.ok ? await res.text() : '';
      const lines = text.split('\n').map((l) => l.replace(/^\S+Z /, '').replace(/\x1b\[[0-9;]*m/g, '')).filter((l) => failing.test(l));
      console.log(`${j.id}\t${j.name}\t${res.ok ? (lines.length ? [...new Set(lines.map((l) => l.trim().slice(0, 160)))].slice(0, 3).join(' | ') : '(no failing test line)') : `(no log: HTTP ${res.status})`}`);
    }
  }
}

if (args[0] === 'pull') await pull();
else if (args[0] === 'table') table(args[1]);
else if (args[0] === 'steps') steps(args[1]);
else if (args[0] === 'zombies') zombies(args[1]);
else if (args[0] === 'logs') await logs(args[1]);
else console.error('usage: 2026-10-29-ci-runs.mjs pull | table|steps|zombies|logs <runs.json> [--end ISO] [--hours 48]');
