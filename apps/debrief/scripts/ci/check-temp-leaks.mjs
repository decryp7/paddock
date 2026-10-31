#!/usr/bin/env node
// Fails when test runs leave `debrief-*` / `imt-*` folders in the OS temp directory (docs/testing/temp-leaks.md).
//   node scripts/ci/check-temp-leaks.mjs --snapshot leaks.json   (before the tests: records what is already there)
//   node scripts/ci/check-temp-leaks.mjs --check leaks.json      (after the tests: exits 1 naming the new ones)
//   node scripts/ci/check-temp-leaks.mjs                         (no baseline: every matching folder counts)
// `--dir <path>` scans that folder instead of os.tmpdir(); `--socket-dir <path>` also scans that folder for endpoint-shaped
// names (CI: the job's own temp folder and socket base, conformance/ci/job-tmpdir.sh); `--shared` scans the shared socket
// folders too, as without `--dir` (only where /tmp is the job's own: a Linux container). On a leak it names, per offender, the
// source lines (and the test around them) that spell the folder's name, searched under the app (`--source <dir>`, repeatable,
// overrides), and ends with one `::error::` line naming them (rule 56).

import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { formatError } from '../../../../conformance/ci/ci-log.mjs';
import { isMain } from '../../../../conformance/src/util/is-main.mjs';

/** The app's own folder: where the sources that create the scratch folders live (`--source` overrides it). */
const APP_DIR = resolve(fileURLToPath(new URL('../..', import.meta.url)));
/** Folders never searched for sources: build output, dependencies, VCS. */
const SKIP_DIRS = new Set(['node_modules', 'target', '.git', 'dist', '.angular', 'coverage', 'test-results']);
const SOURCE_EXT = /\.(rs|ts|mts|mjs|js)$/;
/** Bounds of the source search (rule: bounded resources): files read, bytes per file, places named per pattern. */
const MAX_FILES = 20000;
const MAX_BYTES = 2 * 1024 * 1024;
const MAX_HITS = 5;
/** This checker and its test spell example names; they never create a folder. */
const SELF = basename(fileURLToPath(import.meta.url)).replace(/\.mjs$/, '');

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

/** The source files under `root` that can create scratch folders, sorted; bounded by MAX_FILES. */
export function sourceFiles(root) {
  const out = [];
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      if (out.length >= MAX_FILES) return;
      if (e.isDirectory()) {
        if (!SKIP_DIRS.has(e.name)) walk(join(dir, e.name));
      } else if (e.isFile() && SOURCE_EXT.test(e.name) && !e.name.startsWith(SELF)) out.push(join(dir, e.name));
    }
  };
  walk(root);
  return out;
}

/** The name of the test around line `i`: the nearest `fn name` (Rust) or `it('...'` / `test('...'` (TS) above it. */
export function enclosingTest(lines, i) {
  for (let j = i; j >= 0 && j > i - 300; j--) {
    const rust = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/.exec(lines[j]);
    if (rust) return rust[1];
    const ts = /\b(?:it|test)\(\s*(['"`])(.+?)\1/.exec(lines[j]);
    if (ts) return ts[2];
  }
  return '';
}

/**
 * Where a leak pattern's folders are created: `path:line (in test)` for the source lines that spell the pattern's stem
 * (`debrief-tools-perfmon-*` -> `debrief-tools-perfmon-` followed by a placeholder, a quote or a digit, so a longer name such as
 * `debrief-tools-perfmon-ow-` is not blamed), falling back to any line containing the stem. At most MAX_HITS places.
 */
export function findCreators(pattern, files, root = APP_DIR) {
  const stem = pattern.endsWith('*') ? pattern.slice(0, -1) : pattern;
  const escaped = stem.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const strict = new RegExp(`${escaped}(?![a-z])`);
  const scan = (re) => {
    const hits = [];
    for (const f of files) {
      let text;
      try {
        if (statSync(f).size > MAX_BYTES) continue;
        text = readFileSync(f, 'utf8');
      } catch {
        continue;
      }
      if (!text.includes(stem)) continue;
      const lines = text.split('\n');
      for (let i = 0; i < lines.length && hits.length < MAX_HITS; i++) {
        if (!re.test(lines[i])) continue;
        const name = enclosingTest(lines, i);
        const rel = isAbsolute(f) ? relative(root, f) : f;
        hits.push(`${rel}:${i + 1}${name ? ` (in ${name})` : ''}`);
      }
      if (hits.length >= MAX_HITS) break;
    }
    return hits;
  };
  const exact = scan(strict);
  return exact.length ? exact : scan(new RegExp(escaped));
}

/** A leaked entry's modification time (ISO, UTC), to line it up with the JUnit report's test times; '' when unreadable. */
function modified(path) {
  try {
    return statSync(path).mtime.toISOString();
  } catch {
    return '';
  }
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
  const sources = argv.flatMap((a, i) => (a === '--source' && argv[i + 1] ? [argv[i + 1]] : []));
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
  const groups = groupLeaks(leaked);
  const roots = sources.length ? sources : [APP_DIR];
  const files = roots.flatMap((r) => sourceFiles(r));
  log.error(`Offenders, with the source lines that create them (searched ${files.length} files under ${roots.join(', ')}):`);
  const blamed = [];
  for (const { pattern, count } of groups) {
    log.error(`  ${String(count).padStart(5)}  ${pattern}`);
    const where = findCreators(pattern, files, roots[0]);
    if (where.length === 0) log.error('         no source line spells this name: it is built at run time; grep for its parts');
    for (const w of where) log.error(`         ${w}`);
    if (where[0]) blamed.push(`${pattern} from ${where[0]}`);
  }
  log.error('Folders (modified, UTC; match the times with the JUnit report):');
  for (const n of leaked) log.error(`  ${n}  ${modified(n.startsWith('/') ? n : join(dir, n))}`);
  log.log(
    formatError(
      `${leaked.length} temp folder(s) left behind by the tests: ${groups.map((g) => g.pattern).join(', ')}`,
      blamed.length
        ? `make these tests clean up on every exit, also on panic (tempfile::TempDir or a drop guard): ${blamed.join('; ')}`
        : 'find the tests that create these names and make them clean up on every exit (tempfile::TempDir or a drop guard)',
    ),
  );
  return 1;
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
