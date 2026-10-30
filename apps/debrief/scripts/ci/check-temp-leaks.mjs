#!/usr/bin/env node
// Fails when test runs leave `debrief-*` / `imt-*` folders in the OS temp directory (docs/testing/temp-leaks.md).
//   node scripts/ci/check-temp-leaks.mjs --snapshot leaks.json   (before the tests: records what is already there)
//   node scripts/ci/check-temp-leaks.mjs --check leaks.json      (after the tests: exits 1 naming the new ones)
//   node scripts/ci/check-temp-leaks.mjs                         (no baseline: every matching folder counts)
// `--dir <path>` scans that folder instead of os.tmpdir(); `--socket-dir <path>` also scans that folder for endpoint-shaped
// names (CI: the job's own temp folder and socket base, conformance/ci/job-tmpdir.sh); `--shared` scans the shared socket
// folders too, as without `--dir` (only where /tmp is the job's own: a Linux container).

import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isMain } from '../../../../conformance/src/util/is-main.mjs';

export const PATTERN = /^(debrief|imt)-/;

/**
 * The name of a private endpoint folder (`<prefix>-<12 hex digits>`, crates/debrief-ipc/src/endpoint.rs): the only shape
 * looked for in the shared folders (`/tmp`, `$XDG_RUNTIME_DIR`) a socket falls back to when the temp folder is too long,
 * which hold other programs' files too. A confined test folder (`dbw` and six characters) counts as well.
 */
export const ENDPOINT_PATTERN = /^(debrief(-[a-z0-9]+)+-[0-9a-f]{12}|dbw[A-Za-z0-9]{6})$/;

/** The shared folders endpoints can fall back to, besides the temp folder. */
export function sharedDirs(env = process.env, temp = tmpdir()) {
  const dirs = ['/tmp', env['XDG_RUNTIME_DIR']].filter((d) => typeof d === 'string' && d !== '');
  return [...new Set(dirs)].filter((d) => d !== temp);
}

/** `<dir>/<name>` for the endpoint-shaped entries of `dir`, sorted; an unreadable folder has none. */
export function listEndpointLeaks(dir) {
  try {
    return readdirSync(dir)
      .filter((n) => ENDPOINT_PATTERN.test(n))
      .sort()
      .map((n) => `${dir}/${n}`);
  } catch {
    return [];
  }
}

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

export function main(argv, log = console, shared = sharedDirs, platform = process.platform) {
  const opt = (name) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const dir = opt('--dir') ?? tmpdir();
  // Without `--dir`, or with `--shared`, the shared folders are scanned too, for endpoint-shaped names only (kept in the list
  // as full paths): a socket folder falls back to them when the temp folder's path is too long.
  const socketDir = opt('--socket-dir');
  // CI passes "$PADDOCK_SOCKET_BASE": empty means the job's socket folder was never made, and the leak check would look in
  // one folder fewer than the tests use. Windows has no socket folder (named pipes).
  if (argv.includes('--socket-dir') && !socketDir && platform !== 'win32') {
    log.error('--socket-dir is empty: the job has no socket folder (PADDOCK_SOCKET_BASE, conformance/ci/job-tmpdir.sh)');
    return 2;
  }
  const extra = [
    ...(socketDir ? [socketDir] : []),
    ...(argv.includes('--dir') && !argv.includes('--shared') ? [] : shared(process.env, dir).filter((d) => d !== dir && d !== socketDir)),
  ];
  const current = [...listLeaks(dir), ...extra.flatMap((d) => listEndpointLeaks(d))];
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

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
