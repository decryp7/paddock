#!/usr/bin/env node
'use strict';
// The report step of the report-only lint rules (ER-41): `node <repo>/eslint/bin/lint-report.cjs <patterns...>` from an app's folder
// (its `npm run lint:report`). It runs ESLint with the app's own config and PADDOCK_BULK_LINT=warn (src/bulk.cjs), so
// `paddock/no-backend-call-in-loop` and the app's other report-only rules (Debrief's DEBRIEF_LINT_REPORT=1) report as warnings; prints
// each warning and error as `file:line rule message`; appends a summary (counts per rule, the first findings) to $GITHUB_STEP_SUMMARY
// when CI sets it; and exits 0 whatever it found, because these rules do not block yet. ESLint failing to run (a broken config, a parse
// crash) is an `::error::` annotation and exit 2: a report that could not look is not a green report (ER-33).
// PADDOCK_LINT_REPORT_CONFIG names another config file (tests).
const { appendFileSync } = require('node:fs');
const { relative } = require('node:path');

const MAX_LISTED = 50;

/** The markdown summary of `results` (ESLint's), for the job summary. */
function summarise(results, cwd) {
  const byRule = new Map();
  const lines = [];
  for (const r of results) {
    for (const m of r.messages) {
      const rule = m.ruleId ?? '(parser)';
      byRule.set(rule, (byRule.get(rule) ?? 0) + 1);
      lines.push(`${relative(cwd, r.filePath)}:${m.line} ${rule}: ${m.message}`);
    }
  }
  const head = `### Report-only lint rules (${relative(process.cwd(), cwd) || '.'})\n\n`;
  if (lines.length === 0) return { text: `${head}no findings\n`, lines };
  const counts = [...byRule.entries()].sort((a, b) => b[1] - a[1]).map(([rule, n]) => `- ${rule}: ${n}`).join('\n');
  const listed = lines.slice(0, MAX_LISTED).map((l) => `    ${l}`).join('\n');
  const more = lines.length > MAX_LISTED ? `\n\n(${lines.length - MAX_LISTED} more in the job log)` : '';
  return { text: `${head}${lines.length} findings (report-only, not blocking):\n\n${counts}\n\n${listed}${more}\n`, lines };
}

async function main() {
  const patterns = process.argv.slice(2);
  if (patterns.length === 0) {
    console.log('::error::lint-report needs the folders to lint (for example `src e2e`). Next: name them in the app\'s lint:report script.');
    return 2;
  }
  process.env.PADDOCK_BULK_LINT = 'warn';
  const started = Date.now();
  let results;
  try {
    const { ESLint } = require(require.resolve('eslint', { paths: [process.cwd(), __dirname] }));
    const overrideConfigFile = process.env.PADDOCK_LINT_REPORT_CONFIG || undefined;
    results = await new ESLint({ cwd: process.cwd(), overrideConfigFile }).lintFiles(patterns);
  } catch (e) {
    console.log(`::error::lint-report: ESLint could not run in ${process.cwd()} (${String(e?.message ?? e).split('\n')[0]}). Next: run \`npx eslint ${patterns.join(' ')}\` there and fix the config.`);
    return 2;
  }
  const { text, lines } = summarise(results, process.cwd());
  for (const l of lines) console.log(l);
  console.log(`lint-report: ${lines.length} findings in ${results.length} files (${Date.now() - started} ms); report-only, the step stays green.`);
  if (process.env.GITHUB_STEP_SUMMARY) {
    try {
      appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${text}\n`);
    } catch (e) {
      console.log(`::warning::lint-report: the job summary could not be written (${e.message}); the findings are in the log above.`);
    }
  }
  return 0;
}

if (require.main === module) main().then((code) => process.exit(code));

module.exports = { summarise };
