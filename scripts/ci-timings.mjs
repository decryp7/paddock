#!/usr/bin/env node
// Measures real CI timings for the Paddock family from the Gitea Actions API (read-only, no dependencies).
//
//   node scripts/ci-timings.mjs [--repos debrief,paddock,pitcrew,pitwall,spotter,dyno] [--limit 30]
//                               [--no-logs] [--cache DIR] [--out docs/research/2026-10-ci-timings.md] [--json FILE]
//
// Token: GITEA_TOKEN from the environment, else read from `zsh -c 'source ~/.zshenv'`. It is never printed or written.
// Per repo and workflow it takes the last N completed runs and reports, per job: wall time, dispatch delay (time
// from the previous job finishing, or the run being created, to the job starting), runner, per-step durations
// (API step timestamps) and, from the job logs, compile vs test-run phases. The critical path of a run is rebuilt
// from timestamps (the API does not expose `needs`): the last job to finish, then repeatedly the job that finished
// last before it started.
//
// With --out, only the region between the two marker comments is replaced, so hand-written text around it survives.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? dflt : args[i + 1];
};
const flag = (name) => args.includes(`--${name}`);
const REPOS = opt('repos', 'debrief,paddock,pitcrew,pitwall,spotter,dyno').split(',');
const LIMIT = Number(opt('limit', '30'));
const WITH_LOGS = !flag('no-logs');
const CACHE = opt('cache', join(tmpdir(), 'ci-timings-cache'));
const BASE = opt('base', 'https://dev.decryptology.net/api/v1/repos/decryp7');
const BEGIN = '<!-- ci-timings:begin -->';
const END = '<!-- ci-timings:end -->';

function token() {
  if (process.env.GITEA_TOKEN) return process.env.GITEA_TOKEN;
  return execFileSync('zsh', ['-c', 'source ~/.zshenv; printf %s "$GITEA_TOKEN"'], { encoding: 'utf8' });
}
const TOKEN = token();
if (!TOKEN) throw new Error('GITEA_TOKEN is not set');

async function get(url, text = false) {
  for (let attempt = 0; ; attempt++) {
    try {
      const res = await fetch(url, { headers: { Authorization: `token ${TOKEN}` } });
      if (res.status === 404) return null;
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return text ? await res.text() : await res.json();
    } catch (e) {
      if (attempt >= 3) throw new Error(`${url.replace(BASE, '')}: ${e.message}`);
      await new Promise((r) => setTimeout(r, 500 * (attempt + 1)));
    }
  }
}

async function pool(items, n, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: n }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i], i);
      }
    }),
  );
  return out;
}

// ---- statistics ----
const sorted = (a) => [...a].sort((x, y) => x - y);
const pct = (a, p) => {
  if (!a.length) return null;
  const s = sorted(a);
  const k = (s.length - 1) * p;
  const lo = Math.floor(k);
  return s[lo] + (s[Math.ceil(k)] - s[lo]) * (k - lo);
};
const median = (a) => pct(a, 0.5);
const p90 = (a) => pct(a, 0.9);
const fmt = (s) => {
  if (s == null) return '-';
  if (s < 90) return `${Math.round(s)}s`;
  const t = Math.round(s);
  return `${Math.floor(t / 60)}m${String(t % 60).padStart(2, '0')}s`;
};
const ts = (x) => (x && !x.startsWith('1970') && !x.startsWith('0001') ? Date.parse(x) : null);
const secs = (a, b) => (a != null && b != null ? (b - a) / 1000 : null);

// ---- step categories (first match wins) ----
const CATEGORIES = [
  ['checkout', /checkout/i],
  ['apt / system libs', /apt|linux dependencies|system dep/i],
  ['cache save', /cache\/save|cache save|save.*cache/i],
  ['cache restore', /cache/i],
  ['toolchain / tools', /toolchain|install-action|rustup|tauri-driver|webdriver|setup-node|libduckdb|nextest|cargo-deny|sccache/i],
  ['playwright install', /playwright install/i],
  ['npm ci', /^install$|npm ci|npm install|node_modules/i],
  ['playwright run (e2e)', /end-to-end|e2e|playwright/i],
  ['vitest / unit tests', /unit test|vitest|ng test/i],
  ['ng build', /frontend build|ng build|translations/i],
  ['lint / format / audit', /lint|clippy|format|audit|licen|deny/i],
  ['rust tests (compile + run)', /^tests$|doc tests|memory budget|cargo test|nextest|rust test/i],
  ['bundle / app build', /bundle|tauri build|build app|app binary|dist|sidecar|installer|dmg|msi|smoke/i],
];
const categorize = (name) => (CATEGORIES.find(([, re]) => re.test(name)) ?? ['other'])[0];

// ---- log metrics (compile vs test-run phases) ----
const ANSI = /\x1b\[[0-9;]*m/g;
function parseLog(text) {
  const m = {};
  const add = (k, v) => (m[k] ??= []).push(v);
  const dur = (n, u) => Number(n) * { ms: 0.001, s: 1, m: 60, h: 3600 }[u];
  for (const raw of text.split('\n')) {
    const line = raw.replace(ANSI, '');
    let r;
    if ((r = /Finished `?([\w-]+)`? profile .* in (?:(\d+)m )?(?:(\d+)h )?([\d.]+)s/.exec(line)))
      add(`cargo build (${r[1]})`, Number(r[2] ?? 0) * 60 + Number(r[3] ?? 0) * 3600 + Number(r[4]));
    else if ((r = /Summary \[\s*([\d.]+)s\]\s+(\d+) tests? run/.exec(line))) add('nextest run', Number(r[1]));
    else if ((r = /\bDuration\s+([\d.]+)(ms|s)\b/.exec(line))) add('vitest', dur(r[1], r[2]));
    else if ((r = /\b(\d+) passed \(([\d.]+)(ms|s|m|h)\)/.exec(line))) add('playwright run', dur(r[2], r[3]));
    else if ((r = /Application bundle generation complete\. \[([\d.]+) seconds\]/.exec(line))) add('ng build bundle', Number(r[1]));
    else if (/^\S+\s+\s*Compiling /.test(line)) m['crates compiled'] = [(m['crates compiled']?.[0] ?? 0) + 1];
  }
  return m;
}

async function jobMetrics(repo, job) {
  mkdirSync(CACHE, { recursive: true });
  const file = join(CACHE, `${repo}-${job.id}.json`);
  if (existsSync(file)) return JSON.parse(readFileSync(file, 'utf8'));
  const text = await get(`${BASE}/${repo}/actions/jobs/${job.id}/logs`, true);
  const m = text ? parseLog(text) : {};
  writeFileSync(file, JSON.stringify(m));
  return m;
}

// ---- collection ----
async function listRuns(repo) {
  const all = [];
  const perWf = {};
  for (let page = 1; page <= 12; page++) {
    const j = await get(`${BASE}/${repo}/actions/runs?limit=50&page=${page}`);
    const runs = j?.workflow_runs ?? [];
    if (!runs.length) break;
    all.push(...runs);
    for (const r of runs) if (r.status === 'completed' && (r.conclusion === 'success' || r.conclusion === 'failure')) perWf[wf(r)] = (perWf[wf(r)] ?? 0) + 1;
    const wfs = Object.keys(perWf);
    if (wfs.length && wfs.every((w) => perWf[w] >= LIMIT)) break;
    if (all.length >= (j.total_count ?? 0)) break;
  }
  return all;
}
const wf = (r) => r.path.split('@')[0];
// Runs that ran installer/release jobs (tags and release candidates) are timed apart from plain check runs.
const kindOf = (x) => (x.jobs.some((j) => /^(bundle|release|build installers)/i.test(j.name)) ? 'with-bundles' : x.run.event === 'pull_request' ? 'pull_request' : 'push');

async function loadJobs(repo, run) {
  const jobs = [];
  for (let page = 1; page <= 4; page++) {
    const j = await get(`${BASE}/${repo}/actions/runs/${run.id}/jobs?limit=50&page=${page}`);
    const batch = j?.jobs ?? [];
    jobs.push(...batch);
    if (batch.length < 50) break;
  }
  const created = ts(run.created_at);
  const norm = jobs
    .map((j) => ({
      id: j.id,
      name: j.name,
      runner: j.runner_name && j.runner_name !== 'undefined' ? j.runner_name : null,
      conclusion: j.conclusion,
      start: ts(j.started_at),
      end: ts(j.completed_at),
      steps: (j.steps ?? []).map((s) => ({ name: s.name, sec: secs(ts(s.started_at), ts(s.completed_at)) })),
    }))
    .filter((j) => j.start != null && j.end != null);
  for (const j of norm) {
    j.wall = secs(j.start, j.end);
    const prev = norm.filter((k) => k !== j && k.end <= j.start + 2000).sort((a, b) => b.end - a.end)[0];
    j.pred = prev?.id ?? null;
    j.delay = secs(prev ? prev.end : created, j.start);
    j.firstWave = !prev; // nothing had finished yet, so the delay is pure queueing (no `needs` to wait for)
  }
  const runEnd = Math.max(...norm.map((j) => j.end), created);
  return { jobs: norm, created, wall: secs(created, runEnd) };
}

function criticalPath(jobs) {
  if (!jobs.length) return [];
  let cur = jobs.reduce((a, b) => (b.end > a.end ? b : a));
  const path = [];
  const byId = new Map(jobs.map((j) => [j.id, j]));
  while (cur) {
    path.unshift(cur);
    cur = cur.pred != null ? byId.get(cur.pred) : null;
  }
  return path;
}

// ---- aggregation + rendering ----
function stat(a) {
  return { n: a.length, med: median(a), p90: p90(a) };
}

async function analyse(repo) {
  const runs = await listRuns(repo);
  const byWf = {};
  for (const r of runs) (byWf[wf(r)] ??= []).push(r);
  const result = { repo, workflows: [] };
  for (const [path, list] of Object.entries(byWf)) {
    const finished = list.filter((r) => r.status === 'completed'); // API lists newest first
    const timed = finished.filter((r) => r.conclusion === 'success' || r.conclusion === 'failure').slice(0, LIMIT);
    const done = finished.filter((r) => r.created_at >= (timed.at(-1)?.created_at ?? ''));
    if (!timed.length) continue;
    const loaded = await pool(timed, 6, async (r) => ({ run: r, ...(await loadJobs(repo, r)) }));
    const useful = loaded.filter((x) => x.jobs.length);
    if (WITH_LOGS) {
      const wanted = useful.flatMap((x) => x.jobs.filter((j) => j.conclusion === 'success' && j.wall > 30));
      await pool(wanted, 6, async (j) => {
        j.metrics = await jobMetrics(repo, j);
      });
    }
    const counts = countBy(done.map((r) => r.conclusion));
    const range = [done.at(-1)?.created_at, done[0]?.created_at].map((d) => d?.slice(0, 10)).join(' to ');
        for (const kind of ['with-bundles', 'pull_request', 'push']) {
      const part = useful.filter((x) => kindOf(x) === kind);
      if (part.length) result.workflows.push({ path, kind, counts, range, runs: part });
    }
  }
  return result;
}
const countBy = (a) => a.reduce((m, k) => ((m[k] = (m[k] ?? 0) + 1), m), {});

function summariseWorkflow(w) {
  const success = w.runs.filter((x) => x.run.conclusion === 'success');
  const ref = success.length >= 3 ? success : w.runs;
  const jobsBy = {};
  for (const x of w.runs)
    for (const j of x.jobs) if (j.conclusion === 'success') (jobsBy[j.name] ??= []).push({ ...j, runId: x.run.id });
  const jobs = Object.entries(jobsBy)
    .map(([name, js]) => {
      const steps = {};
      const cats = {};
      const metrics = {};
      for (const j of js) {
        const catSum = {};
        for (const s of j.steps) {
          if (s.sec == null) continue;
          (steps[s.name] ??= []).push(s.sec);
          const c = categorize(s.name);
          catSum[c] = (catSum[c] ?? 0) + s.sec;
        }
        for (const [c, v] of Object.entries(catSum)) (cats[c] ??= []).push(v);
        for (const [k, v] of Object.entries(j.metrics ?? {})) (metrics[k] ??= []).push(k === 'crates compiled' ? v[0] : v.reduce((a, b) => a + b, 0));
      }
      const runners = countBy(js.map((j) => j.runner ?? 'unknown'));
      const byRunner = Object.fromEntries(
        Object.keys(runners).map((r) => [r, median(js.filter((j) => (j.runner ?? 'unknown') === r).map((j) => j.wall))]),
      );
      return {
        name,
        wall: stat(js.map((j) => j.wall)),
        delay: stat(js.map((j) => j.delay)),
        queue: stat(js.filter((j) => j.firstWave).map((j) => j.delay)),
        runners,
        byRunner,
        cats: Object.entries(cats).map(([c, a]) => [c, median(a)]).sort((a, b) => b[1] - a[1]),
        steps: Object.entries(steps).map(([s, a]) => [s, median(a), a.length]).sort((a, b) => b[1] - a[1]).slice(0, 5),
        metrics: Object.entries(metrics).map(([k, a]) => [k, median(a), a.length]),
      };
    })
    .sort((a, b) => b.wall.med - a.wall.med);

  const walls = ref.map((x) => x.wall);
  const paths = ref.map((x) => criticalPath(x.jobs));
  const onPath = {};
  for (const p of paths) for (const j of p) (onPath[j.name] ??= { wall: [], delay: [] }).wall.push(j.wall), onPath[j.name].delay.push(j.delay);
  const total = paths.length;
  const critical = Object.entries(onPath)
    .map(([name, v]) => ({ name, share: v.wall.length / total, wall: median(v.wall), delay: median(v.delay) }))
    .sort((a, b) => b.share - a.share || b.wall - a.wall);
  const runDelay = ref.map((x) => Math.min(...x.jobs.map((j) => secs(x.created, j.start))));
  const medianChain = median(paths.map((p) => p.reduce((s, j) => s + j.wall, 0)));
  const medianWait = median(paths.map((p) => p.reduce((s, j) => s + (j.delay ?? 0), 0)));
  return { jobs, critical, walls, runDelay, medianChain, medianWait, refCount: ref.length, refKind: ref === success ? 'successful' : 'all finished (fewer than 3 successful)' };
}

function render(results) {
  const L = [];
  const row = (cells) => `| ${cells.join(' | ')} |`;
  L.push(`Generated by \`scripts/ci-timings.mjs\` on ${new Date().toISOString().slice(0, 10)}; last ${LIMIT} successful or failed runs per workflow (cancelled runs in the same span are only counted). `);
  L.push('Times: median / p90. "Delay" is the time from the previous job finishing (or the run being created) to the job starting, so it includes waiting for `needs` as well as queueing. Only successful jobs are timed.\n');
  const overview = [row(['repo', 'workflow (trigger)', 'sample', 'runs timed', 'success/failure/cancelled in window', 'run wall median', 'p90', 'first job starts after (median)', 'critical path jobs-only', 'critical path waits'])];
  overview.push(row(Array(10).fill('---')));
  const body = [];
  for (const res of results) {
    for (const w of res.workflows) {
      const s = summariseWorkflow(w);
      const c = w.counts;
      overview.push(
        row([res.repo, `${w.path.replace('.gitea/workflows/', '')} (${w.kind})`, w.range, String(w.runs.length), `${c.success ?? 0}/${c.failure ?? 0}/${c.cancelled ?? 0}`, fmt(median(s.walls)), fmt(p90(s.walls)), fmt(median(s.runDelay)), fmt(s.medianChain), fmt(s.medianWait)]),
      );
      if (w.runs.length < 3) continue;
      body.push(`### ${res.repo} / ${w.path.replace('.gitea/workflows/', '')} (${w.kind})\n`);
      body.push(`Timed on ${s.refCount} ${s.refKind} runs.\n`);
      body.push('Critical path (rebuilt from timestamps; share = fraction of runs where the job is on it):\n');
      body.push(row(['job', 'share', 'wall (median)', 'delay before it (median)']), row(['---', '---', '---', '---']));
      for (const c2 of s.critical.slice(0, 8)) body.push(row([c2.name, `${Math.round(c2.share * 100)}%`, fmt(c2.wall), fmt(c2.delay)]));
      body.push('\nJobs:\n');
      body.push(row(['job', 'n', 'wall median', 'p90', 'delay median', 'queue, first-wave runs (median; n)', 'runners (count; median wall)', 'log phases (median)', 'biggest steps (median)']), row(Array(9).fill('---')));
      for (const j of s.jobs) {
        const runners = Object.entries(j.runners).map(([r, n]) => `${r.replace('Gitea Action Runner ', '')} ${n}; ${fmt(j.byRunner[r])}`).join(', ');
        const phases = j.metrics.map(([k, v, n]) => (k === 'crates compiled' ? `${Math.round(v)} crates compiled` : `${k} ${fmt(v)}${n < j.wall.n ? ` (${n} runs)` : ''}`)).join('; ') || '-';
        const steps = j.steps.map(([n, v]) => `${n.replace(/^Run /, '')} ${fmt(v)}`).join('; ');
        body.push(row([j.name, j.wall.n, fmt(j.wall.med), fmt(j.wall.p90), fmt(j.delay.med), j.queue.n ? `${fmt(j.queue.med)}; ${j.queue.n}` : '-', runners, phases, steps]));
      }
      body.push('\nStep categories per job (median seconds summed per job run; categories by step name):\n');
      body.push(row(['job', 'categories']), row(['---', '---']));
      for (const j of s.jobs) body.push(row([j.name, j.cats.filter(([, v]) => v >= 3).map(([c, v]) => `${c} ${fmt(v)}`).join('; ')]));
      body.push('');
    }
  }
  const byRunner = {};
  for (const res of results)
    for (const w of res.workflows)
      for (const x of w.runs)
        for (const j of x.jobs) {
          if (j.conclusion !== 'success') continue;
          const r = (byRunner[j.runner ?? 'unknown'] ??= { busy: 0, n: 0, queue: [] });
          r.busy += j.wall;
          r.n++;
          if (j.firstWave) r.queue.push(j.delay);
        }
  const runnerRows = [row(['runner', 'successful jobs', 'busy time', 'first-wave jobs', 'queue median', 'queue p90']), row(Array(6).fill('---'))];
  for (const [name, r] of Object.entries(byRunner).sort((a, b) => b[1].busy - a[1].busy))
    runnerRows.push(row([name, r.n, fmt(r.busy), r.queue.length, fmt(median(r.queue)), fmt(p90(r.queue))]));
  L.push('### Overview\n', ...overview, '', '### Runners (all repos, all timed runs)\n', ...runnerRows, '', ...body);
  return L.join('\n');
}

const results = [];
for (const repo of REPOS) {
  process.stderr.write(`${repo} ...\n`);
  results.push(await analyse(repo));
}
const md = render(results);
if (opt('json')) writeFileSync(opt('json'), JSON.stringify(results.map((r) => ({ ...r, workflows: r.workflows.map((w) => ({ ...w, summary: summariseWorkflow(w) })) })), null, 1));
const out = opt('out');
if (out) {
  const region = `${BEGIN}\n${md}\n${END}`;
  let text = existsSync(out) ? readFileSync(out, 'utf8') : '';
  text = text.includes(BEGIN) ? text.replace(new RegExp(`${BEGIN}[\\s\\S]*?${END}`), () => region) : `${text}\n${region}\n`;
  writeFileSync(out, text);
} else process.stdout.write(`${md}\n`);
