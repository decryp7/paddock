#!/usr/bin/env node
// Report-only run of the ESLint rule `paddock/e2e-ready-reads` (ER-41; eslint/src/rules/e2e-ready-reads.cjs): a read right after a bare
// visibility wait in an e2e spec. The lint commands run with --max-warnings 0 and the rule is off in them, so this runs eslint by itself over
// every app's and the ui's e2e folder with the rule at `warn`, prints one line per site and a count per project, writes the table to the step
// summary and always exits 0 (a CI step with continue-on-error: true; it never blocks). After its first green recorded run the follow-up is
// `error` plus ESLint bulk suppressions (eslint-suppressions.json), which only shrink against the merge base.
//
//   node scripts/e2e-ready-report.mjs [project ...]     default: every app with an e2e folder, and ui
import { execFileSync } from 'node:child_process';
import { appendFileSync, existsSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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

function projects(root) {
  const apps = existsSync(join(root, 'apps')) ? readdirSync(join(root, 'apps')).map((a) => `apps/${a}/app`).filter((d) => existsSync(join(root, d, 'e2e'))) : [];
  return [...apps, 'ui'];
}

function main(argv) {
  const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
  const counts = {};
  const unmeasured = [];
  for (const project of argv.length ? argv : projects(root)) {
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
    for (const s of sites) console.log(`${s.file.replace(`${root}/`, '')}:${s.line}: ${s.message.split(':')[0]}`);
  }
  const table = summaryTable(counts, unmeasured);
  console.log(table);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${table}\n`);
  return 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
