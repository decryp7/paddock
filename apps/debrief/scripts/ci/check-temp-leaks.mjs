#!/usr/bin/env node
// Fails when test runs leave `debrief-*` / `imt-*` folders in the OS temp directory (docs/testing/temp-leaks.md).
//   node scripts/ci/check-temp-leaks.mjs --snapshot leaks.json   (before the tests: records what is already there)
//   node scripts/ci/check-temp-leaks.mjs --check leaks.json      (after the tests: exits 1 naming the new ones)
//   node scripts/ci/check-temp-leaks.mjs                         (no baseline: every matching folder counts)
// `--dir <path>` scans that folder instead of os.tmpdir().

import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';

export const PATTERN = /^(debrief|imt)-/;

/** Names in `dir` that look like Debrief test scratch folders, sorted; an unreadable folder has none. */
export function listLeaks(dir = tmpdir()) {
  try {
    return readdirSync(dir).filter((n) => PATTERN.test(n)).sort();
  } catch {
    return [];
  }
}

/** The names in `current` that are not in `baseline`. */
export function newLeaks(current, baseline = []) {
  const known = new Set(baseline);
  return current.filter((n) => !known.has(n));
}

/** One name's trailing segment is per-run noise: a pid, a counter, a random hex id or a tempfile suffix. */
function isNoise(segment) {
  return /\d/.test(segment) || (/^[A-Za-z0-9]{6}$/.test(segment) && /[A-Z]/.test(segment));
}

/**
 * The fixture a leaked name comes from: the name without its per-run suffixes (`debrief-ai-find-36441-0` ->
 * `debrief-ai-find-*`), so a long list reads as a few offenders to fix.
 */
export function leakPattern(name) {
  const parts = name.split('-');
  let keep = parts.length;
  while (keep > 1 && isNoise(parts[keep - 1])) keep--;
  return keep === parts.length ? name : `${parts.slice(0, keep).join('-')}-*`;
}

/** The offenders: each pattern with how many folders it left, most first (ties by name). */
export function groupLeaks(names) {
  const counts = new Map();
  for (const n of names) {
    const p = leakPattern(n);
    counts.set(p, (counts.get(p) ?? 0) + 1);
  }
  return [...counts]
    .map(([pattern, count]) => ({ pattern, count }))
    .sort((a, b) => b.count - a.count || a.pattern.localeCompare(b.pattern));
}

function readBaseline(file) {
  try {
    const v = JSON.parse(readFileSync(file, 'utf8'));
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

export function main(argv, log = console) {
  const opt = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const dir = opt('--dir') ?? tmpdir();
  const current = listLeaks(dir);
  const snapshot = opt('--snapshot');
  if (snapshot) {
    writeFileSync(snapshot, JSON.stringify(current));
    log.log(`recorded ${current.length} existing debrief-*/imt-* folder(s) in ${dir}`);
    return 0;
  }
  const check = opt('--check');
  const leaked = newLeaks(current, check ? readBaseline(check) : []);
  if (leaked.length === 0) {
    log.log(`no debrief-*/imt-* folders left in ${dir}`);
    return 0;
  }
  log.error(`${leaked.length} test folder(s) left in ${dir} (tests must use tempfile::TempDir or a guard that cleans on panic).`);
  log.error('Offenders (grep the tests for the name before the *):');
  for (const { pattern, count } of groupLeaks(leaked)) log.error(`  ${String(count).padStart(5)}  ${pattern}`);
  log.error('Folders:');
  for (const n of leaked) log.error(`  ${n}`);
  return 1;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) process.exit(main(process.argv.slice(2)));
