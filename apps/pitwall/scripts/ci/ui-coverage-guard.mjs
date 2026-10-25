#!/usr/bin/env node
// Fails when the Angular line coverage (lcov from `ng test --coverage`) is below the threshold (AGENTS.md: >= 80%).
//   node scripts/ci/ui-coverage-guard.mjs [--min 80] [--dir app/coverage]
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Line coverage in percent over every `LF:`/`LH:` pair of an lcov report, or null when it has no lines. */
export function lineCoverage(lcov) {
  let found = 0;
  let hit = 0;
  for (const line of lcov.split(/\r?\n/)) {
    if (line.startsWith('LF:')) found += Number(line.slice(3)) || 0;
    else if (line.startsWith('LH:')) hit += Number(line.slice(3)) || 0;
  }
  return found === 0 ? null : (hit / found) * 100;
}

/** Every lcov.info below a directory. */
export function findLcov(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...findLcov(p));
    else if (name === 'lcov.info') out.push(p);
  }
  return out;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const args = process.argv.slice(2);
  const opt = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const min = Number(opt('--min', '80'));
  const dir = opt('--dir', 'app/coverage');
  const files = findLcov(dir);
  if (files.length === 0) {
    console.error(`No lcov.info under ${dir}: run \`ng test --watch=false --coverage\` first.`);
    process.exit(1);
  }
  const pct = lineCoverage(files.map((f) => readFileSync(f, 'utf8')).join('\n'));
  if (pct === null || pct < min) {
    console.error(`UI line coverage ${pct === null ? 'unknown' : pct.toFixed(2) + '%'} is below ${min}%.`);
    process.exit(1);
  }
  console.log(`UI line coverage ${pct.toFixed(2)}% (>= ${min}%).`);
}
