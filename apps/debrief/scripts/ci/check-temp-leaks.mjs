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
/** Bounds of the source search (rule: bounded resources): files listed, bytes read per file, places named per pattern. */
export const LIMITS = Object.freeze({ maxFiles: 20000, maxBytes: 2 * 1024 * 1024, maxHits: 5 });
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

/** The source files under `root` that can create scratch folders, sorted; at most `maxFiles`. */
export function sourceFiles(root, { maxFiles = LIMITS.maxFiles } = {}) {
  const out = [];
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries.sort((x, y) => x.name.localeCompare(y.name))) {
      if (out.length >= maxFiles) return;
      if (e.isDirectory()) {
        if (!SKIP_DIRS.has(e.name)) walk(join(dir, e.name));
      } else if (e.isFile() && SOURCE_EXT.test(e.name) && !e.name.startsWith(SELF)) out.push(join(dir, e.name));
    }
  };
  walk(root);
  return out;
}

/** The function or test around line `i`: the nearest `fn name` (Rust) or `it('...'` / `test('...'` (TS) above it. */
function enclosing(lines, i) {
  for (let j = i; j >= 0 && j > i - 300; j--) {
    const rust = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/.exec(lines[j]);
    if (rust) return { name: rust[1], kind: 'rust', at: j };
    const ts = /\b(?:it|test)\(\s*(['"`])(.+?)\1/.exec(lines[j]);
    if (ts) return { name: ts[2], kind: 'ts', at: j };
  }
  return { name: '', kind: '', at: -1 };
}

/** The name of the test around line `i`: the nearest `fn name` (Rust) or `it('...'` / `test('...'` (TS) above it. */
export function enclosingTest(lines, i) {
  return enclosing(lines, i).name;
}

/** A test file by its path: under a `tests/` or `e2e/` folder, or a spec, test or `_test.rs` file. */
export function isTestPath(path) {
  return /(^|[\\/])(tests|e2e)[\\/]/.test(path) || /(\.spec|\.test)\.[cm]?[jt]s$|_tests?\.rs$/.test(path);
}

/**
 * Whether line `i` is test code: a test file, a line after `#[cfg(test)]` (Rust test modules sit at the end of a file), a
 * Rust fn with a `#[...test]` attribute just above it, or inside a TS `it(`/`test(`.
 */
export function inTest(path, lines, i) {
  if (isTestPath(path)) return true;
  for (let j = i; j >= 0; j--) if (/^\s*#\[cfg\(test\)\]/.test(lines[j])) return true;
  const e = enclosing(lines, i);
  if (e.kind === 'ts') return true;
  if (e.kind === 'rust') for (let j = e.at - 1; j >= 0 && j >= e.at - 5; j--) if (/^\s*#\[[\w:]*test\b/.test(lines[j])) return true;
  return false;
}

/** The source-name stem of a leak pattern: the folder name without `*` and without the folder it was found in. */
function stemOf(pattern) {
  return basename(pattern.endsWith('*') ? pattern.slice(0, -1) : pattern);
}

/**
 * Where each leak pattern's folders are created, reading every source file once. A hit is `strict` when the line spells the
 * stem followed by a positional placeholder, quote or digit (`debrief-tools-perfmon-` blames `"debrief-tools-perfmon-{}"`,
 * never the longer `debrief-tools-perfmon-ow-` nor `debrief-{kind}-`), otherwise a loose hit (the name is probably built at
 * run time). Hits are ranked: strict
 * in a test, strict elsewhere, loose in a test, loose elsewhere; at most `maxHits` per pattern.
 * `files` are paths (relative to `root`) or `{ root, path }` entries, each shown relative to its own root. Each file is read
 * once (`read`, injectable for tests) whatever the number of patterns; a file over `maxBytes` is skipped.
 * @returns {Map<string, {where: string, strict: boolean, test: boolean}[]>}
 */
export function findAllCreators(patterns, files, { root = APP_DIR, maxBytes = LIMITS.maxBytes, maxHits = LIMITS.maxHits, read = (f) => readFileSync(f, 'utf8') } = {}) {
  const wanted = patterns.map((p) => {
    const stem = stemOf(p);
    const escaped = stem.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return { p, stem, strict: new RegExp(`${escaped}(?![a-z]|\\{[A-Za-z_])`), hits: [] };
  });
  const keep = maxHits * 20;
  for (const entry of files) {
    const path = typeof entry === 'string' ? entry : entry.path;
    const base = typeof entry === 'string' ? root : entry.root;
    let text;
    try {
      if (statSync(path).size > maxBytes) continue;
      text = read(path);
    } catch {
      continue;
    }
    const here = wanted.filter((w) => w.hits.length < keep && text.includes(w.stem));
    if (here.length === 0) continue;
    const lines = text.split('\n');
    const rel = isAbsolute(path) ? relative(base, path) : path;
    for (let i = 0; i < lines.length; i++) {
      for (const w of here) {
        if (w.hits.length >= keep || !lines[i].includes(w.stem)) continue;
        const name = enclosingTest(lines, i);
        w.hits.push({ where: `${rel}:${i + 1}${name ? ` (in ${name})` : ''}`, strict: w.strict.test(lines[i]), test: inTest(rel, lines, i) });
      }
    }
  }
  const rank = (h) => (h.strict ? 0 : 2) + (h.test ? 0 : 1);
  return new Map(wanted.map((w) => [w.p, [...w.hits].sort((x, y) => rank(x) - rank(y)).slice(0, maxHits)]));
}

/** `findAllCreators` for one pattern: its ranked places as `path:line (in test)` strings. */
export function findCreators(pattern, files, root = APP_DIR, limits = {}) {
  return (findAllCreators([pattern], files, { root, ...limits }).get(pattern) ?? []).map((h) => h.where);
}

/**
 * What to blame for one pattern: a single culprit only when exactly one strict hit is inside a test; otherwise the
 * candidates, and loose hits only as "possibly". '' when nothing spells the name.
 */
export function verdict(pattern, hits) {
  const strict = hits.filter((h) => h.strict);
  const strictTests = strict.filter((h) => h.test);
  if (strictTests.length === 1) return `${pattern} from ${strictTests[0].where}`;
  if (strict.length > 0) return `${pattern}: candidates: ${strict.slice(0, 3).map((h) => h.where).join(', ')}`;
  if (hits.length > 0) return `${pattern}: possibly (name built at run time): ${hits.slice(0, 3).map((h) => h.where).join(', ')}`;
  return '';
}

/** How one hit is listed under its pattern. */
function describeHit(h) {
  if (!h.strict) return `${h.where}  (possibly: name built at run time)`;
  return h.test ? h.where : `${h.where}  (not in a test)`;
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
  const files = roots.flatMap((r) => sourceFiles(r).map((path) => ({ root: r, path })));
  log.error(`Offenders, with the source lines that create them (searched ${files.length} files under ${roots.join(', ')}):`);
  const found = findAllCreators(groups.map((g) => g.pattern), files);
  const blamed = [];
  for (const { pattern, count } of groups) {
    log.error(`  ${String(count).padStart(5)}  ${pattern}`);
    const hits = found.get(pattern) ?? [];
    if (hits.length === 0) log.error('         no source line spells this name: it is built at run time; grep for its parts');
    for (const h of hits) log.error(`         ${describeHit(h)}`);
    const v = verdict(pattern, hits);
    if (v) blamed.push(v);
  }
  log.error('Folders (modified, UTC; match the times with the JUnit report):');
  for (const n of leaked) log.error(`  ${n}  ${modified(isAbsolute(n) ? n : join(dir, n))}`);
  log.log(
    formatError(
      `${leaked.length} temp folder(s) left behind by the tests: ${groups.map((g) => g.pattern).join(', ')}`,
      blamed.length
        ? `make the tests that create them clean up on every exit, also on panic (tempfile::TempDir or a drop guard): ${blamed.join('; ')}`
        : 'find the tests that create these names and make them clean up on every exit (tempfile::TempDir or a drop guard)',
    ),
  );
  return 1;
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
