#!/usr/bin/env node
// Gate and report of the ESLint rule `paddock/e2e-ready-reads` (ER-41; eslint/src/rules/e2e-ready-reads.cjs): a read right after a bare
// visibility wait in an e2e spec. The lint commands run with --max-warnings 0 and the rule is off in them, so this runs eslint by itself over
// every app's and the ui's e2e folder with the rule at `warn`, prints one line per site and a count per project, and writes the table to the step
// summary. The sites that exist are a generated baseline (scripts/e2e-ready-baseline.json: sites per spec file) that only shrinks (ER-46):
//   - a file with more sites than its baseline is a NEW site: `::error::` naming the file and the helper to use;
//   - a file with fewer is a fixed site: `::error::` asking for `--update`, which rewrites the baseline and refuses growth;
//   - a baseline entry that grew or appeared against the merge base (git, `--base`, default origin/main) fails with "needs the user's approval";
//   - the waivers (`// conformance: e2e-ready — <why>`) are counted per spec file in scripts/e2e-ready-waivers.json the same way: a new waiver or one in a
//     new place fails with "needs the user's approval" (it is an exception list in comment form, ER-46); fewer is fine (`--update`);
//   - a project eslint could not measure fails (ER-33: not measured never passes).
// Blocking (ER-41): the findings exit 1 only when scripts/e2e-ready-gate.json names `blockingSinceRun` (the CI run that was green on the real tree
// with this gate in report-only mode). Until then they are printed as `::error::` lines and the script exits 0. To flip: write that run id there.
//
//   node scripts/e2e-ready-report.mjs [project ...]     default: every app with an e2e folder, and ui
//   node scripts/e2e-ready-report.mjs --update          shrink the baseline to the current counts (never grows it); --init creates it
//   options: --waivers <file>  --baseline <file>  --gate <file>  --base <git ref>
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const { WAIVER } = createRequire(import.meta.url)('../eslint/src/rules/e2e-ready-reads.cjs');

const RULE = 'paddock/e2e-ready-reads';

/** The sites of the rule in an ESLint JSON result: [{ file, line, message }]. */
export function sitesOf(results) {
  return results.flatMap((f) => f.messages.filter((m) => m.ruleId === RULE).map((m) => ({ file: f.filePath, line: m.line, message: m.message })));
}

/**
 * The markdown table for the step summary: project, sites, and a "not measured" row for each project where eslint failed (ER-33: a failed project is
 * never a project with no sites), with the count of unmeasured projects.
 */
export function summaryTable(counts, unmeasured = []) {
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  return [
    '### e2e ready-reads (report-only)',
    '',
    '| project | sites |',
    '| --- | ---: |',
    ...Object.entries(counts).map(([p, n]) => `| ${p} | ${n} |`),
    ...unmeasured.map((p) => `| ${p} | not measured |`),
    `| **total (measured projects)** | **${total}** |`,
    `| **projects not measured** | **${unmeasured.length}** |`,
    '',
  ].join('\n');
}

/** Sites per spec file, sorted by path: { file: count } from `sites` ([{ file }]) with `root` stripped from each path. */
export function countsByFile(sites, root) {
  const counts = {};
  for (const s of sites) {
    const f = s.file.startsWith(`${root}/`) ? s.file.slice(root.length + 1) : s.file;
    counts[f] = (counts[f] ?? 0) + 1;
  }
  return Object.fromEntries(Object.entries(counts).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

/** Against the baseline: `added` (files with more sites than recorded, or not recorded) and `fixed` (files with fewer, or none now). */
export function compareToBaseline(counts, baseline) {
  const added = [];
  const fixed = [];
  for (const [f, n] of Object.entries(counts)) if (n > (baseline[f] ?? 0)) added.push({ file: f, now: n, recorded: baseline[f] ?? 0 });
  for (const [f, n] of Object.entries(baseline)) if ((counts[f] ?? 0) < n) fixed.push({ file: f, now: counts[f] ?? 0, recorded: n });
  return { added, fixed };
}

/** Entries of `head` that grew or are new against `base` (the merge base's baseline): a baseline only shrinks (ER-46). */
export function baselineGrowth(base, head) {
  return Object.entries(head).filter(([f, n]) => n > (base[f] ?? 0)).map(([f, n]) => ({ file: f, base: base[f] ?? 0, head: n }));
}

/** Waivers per spec file under `dir` (recursive, .ts): { relative path: count }, sorted. `rel` is the folder's path under the root. */
export function waiversByFile(dir, rel) {
  const out = {};
  const walk = (d, r) => {
    for (const name of readdirSync(d).sort()) {
      const full = join(d, name);
      const rr = `${r}/${name}`;
      if (name === 'node_modules' || name.endsWith('-snapshots')) continue;
      if (statSync(full).isDirectory()) walk(full, rr);
      else if (name.endsWith('.ts')) {
        const n = readFileSync(full, 'utf8').split('\n').filter((l) => WAIVER.test(l)).length;
        if (n) out[rr] = n;
      }
    }
  };
  if (existsSync(dir)) walk(dir, rel);
  return out;
}

/** The findings as `::error::` lines (and nothing else). */
export function findings({ added, fixed, grown, unmeasured, waiverAdded = [], waiverFixed = [], waiverGrown = [] }) {
  return [
    ...waiverAdded.map((a) => `::error file=${a.file}::e2e ready-reads: ${a.now - a.recorded} new waiver(s) \`conformance: e2e-ready\` (${a.recorded} recorded, ${a.now} now): a new waiver, or one in a new place, needs the user's approval (ER-46). Wait with a ready helper instead.`),
    ...waiverFixed.map((a) => `::error file=${a.file}::e2e ready-reads: ${a.recorded - a.now} waiver(s) removed (${a.recorded} recorded, ${a.now} now). Shrink the list: node scripts/e2e-ready-report.mjs --update`),
    ...waiverGrown.map((g) => `::error file=${g.file}::e2e ready-reads waivers grew against the merge base (${g.base} to ${g.head}): the list only shrinks; growth needs the user's approval (ER-46).`),
    ...added.map((a) => `::error file=${a.file}::e2e ready-reads: ${a.now - a.recorded} new read(s) right after a bare visibility wait (${a.recorded} recorded, ${a.now} now). Wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) before the read; do not add a baseline entry.`),
    ...fixed.map((a) => `::error file=${a.file}::e2e ready-reads: ${a.recorded - a.now} site(s) fixed (${a.recorded} recorded, ${a.now} now). Shrink the baseline: node scripts/e2e-ready-report.mjs --update`),
    ...grown.map((g) => `::error file=${g.file}::e2e ready-reads baseline grew against the merge base (${g.base} to ${g.head}): a baseline only shrinks; growth needs the user's approval (ER-46).`),
    ...unmeasured.map((p) => `::error::e2e ready-reads: ${p} not measured; a project that was not measured never passes (ER-33). Next step: run eslint in ${p} and read its error.`),
  ];
}

function readJson(path, fallback) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (e) {
    if (e.code === 'ENOENT' && fallback !== undefined) return fallback;
    throw e;
  }
}

/** The baseline at `ref` (the merge base with it), or null when git cannot say (no ref, no file at the base: reported, never a pass for a growth). */
function baselineAtBase(root, ref, relPath) {
  try {
    const base = execFileSync('git', ['merge-base', 'HEAD', ref], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
    return JSON.parse(execFileSync('git', ['show', `${base}:${relPath}`], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }));
  } catch (e) {
    console.log(`::notice::e2e ready-reads: no baseline at the merge base with ${ref} (${String(e.message).split('\n')[0]}); the growth check is not measured`);
    return null;
  }
}

function projects(root) {
  const apps = existsSync(join(root, 'apps')) ? readdirSync(join(root, 'apps')).map((a) => `apps/${a}/app`).filter((d) => existsSync(join(root, d, 'e2e'))) : [];
  return [...apps, 'ui'];
}

function option(argv, name) {
  const i = argv.indexOf(name);
  if (i < 0) return null;
  const [, v] = argv.splice(i, 2);
  return v;
}

function main(argv) {
  const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
  const baselinePath = resolve(root, option(argv, '--baseline') ?? 'scripts/e2e-ready-baseline.json');
  const waiversPath = resolve(root, option(argv, '--waivers') ?? 'scripts/e2e-ready-waivers.json');
  const gatePath = resolve(root, option(argv, '--gate') ?? 'scripts/e2e-ready-gate.json');
  const baseRef = option(argv, '--base') ?? 'origin/main';
  const update = argv.includes('--update');
  const init = argv.includes('--init');
  const names = argv.filter((a) => !a.startsWith('--'));
  const counts = {};
  const unmeasured = [];
  const all = [];
  for (const project of names.length ? names : projects(root)) {
    let out = '[]';
    try {
      // The rule's severity comes from the environment (eslint/src/e2e.cjs): a `--rule` override cannot find the plugin where it is registered only inside
      // the e2e block ("could not find plugin paddock" in every app but two), because the override also applies to files the block ignores.
      out = execFileSync('npx', ['eslint', 'e2e/**/*.ts', '-f', 'json'], { cwd: join(root, project), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: { ...process.env, PADDOCK_E2E_READY_LINT: 'warn' } });
    } catch (e) {
      // eslint exits 1 when it finds errors of other rules and still prints the JSON; anything else is reported and counted as unmeasured.
      out = e.stdout?.toString() || '';
      if (!out.trim().startsWith('[')) {
        console.log(`::warning::e2e ready-reads: eslint failed in ${project} (${String(e.message).split('\n')[0]}); not measured`);
        unmeasured.push(project);
        continue;
      }
    }
    const sites = sitesOf(JSON.parse(out));
    counts[project] = sites.length;
    all.push(...sites);
    for (const s of sites) console.log(`${s.file.replace(`${root}/`, '')}:${s.line}: ${s.message.split(':')[0]}`);
  }
  const table = summaryTable(counts, unmeasured);
  console.log(table);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${table}\n`);

  const byFile = countsByFile(all, root);
  const waiverCounts = {};
  for (const project of names.length ? names : projects(root)) Object.assign(waiverCounts, waiversByFile(join(root, project, 'e2e'), `${project}/e2e`));
  const relOf = (p) => (p.startsWith(`${root}/`) ? p.slice(root.length + 1) : p);
  const rel = relOf(baselinePath);
  const baseline = readJson(baselinePath, init ? {} : undefined);
  const waivers = readJson(waiversPath, init ? {} : undefined);
  if (update || init) {
    if (unmeasured.length) {
      console.log(`::error::e2e ready-reads: not rewriting the baseline, ${unmeasured.join(', ')} not measured`);
      return 1;
    }
    const { added } = compareToBaseline(byFile, baseline);
    const { added: waiverAdded } = compareToBaseline(waiverCounts, waivers);
    if ((added.length || waiverAdded.length) && !init) {
      console.log(findings({ added, fixed: [], grown: [], unmeasured: [], waiverAdded }).join('\n'));
      console.log('::error::e2e ready-reads: --update only shrinks the baseline; a new site is fixed with a ready helper, never recorded');
      return 1;
    }
    writeFileSync(baselinePath, `${JSON.stringify(byFile, null, 2)}\n`);
    writeFileSync(waiversPath, `${JSON.stringify(waiverCounts, null, 2)}\n`);
    console.log(`e2e ready-reads: baseline written (${Object.values(byFile).reduce((a, b) => a + b, 0)} sites in ${Object.keys(byFile).length} files)`);
    return 0;
  }
  const { added, fixed } = compareToBaseline(byFile, baseline);
  const base = baselineAtBase(root, baseRef, rel);
  const grown = base ? baselineGrowth(base, baseline) : [];
  const { added: waiverAdded, fixed: waiverFixed } = compareToBaseline(waiverCounts, waivers);
  const waiverBase = baselineAtBase(root, baseRef, relOf(waiversPath));
  const waiverGrown = waiverBase ? baselineGrowth(waiverBase, waivers) : [];
  const lines = findings({ added, fixed, grown, unmeasured, waiverAdded, waiverFixed, waiverGrown });
  for (const l of lines) console.log(l);
  const since = readJson(gatePath, {}).blockingSinceRun ?? null;
  console.log(`e2e ready-reads: ${lines.length} finding(s); ${since ? `blocking since run ${since}` : 'report-only (ER-41): blockingSinceRun is not set in scripts/e2e-ready-gate.json'}`);
  return lines.length && since ? 1 : 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
