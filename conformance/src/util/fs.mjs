// Read-only helpers over an app checkout. Paths are POSIX-style and relative to the root, so findings read the same everywhere.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const SKIP_DIRS = new Set(['node_modules', '.git', 'target', 'dist', '.angular', 'coverage', 'test-results', 'playwright-report']);

/** Every file under `root` (relative POSIX paths, sorted), skipping build output and dependencies. */
export function walk(root, dir = root, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name)) walk(root, join(dir, e.name), out);
    } else if (e.isFile()) out.push(relative(root, join(dir, e.name)).split(sep).join('/'));
  }
  return out.sort();
}

/** The inspected tree: lists files once, reads them on demand. */
export function openTree(root) {
  let files = null;
  const cache = new Map();
  return {
    root,
    files: () => (files ??= walk(root)),
    exists: (p) => existsSync(join(root, p)),
    isDir: (p) => {
      try {
        return statSync(join(root, p)).isDirectory();
      } catch {
        return false;
      }
    },
    read: (p) => {
      if (!cache.has(p)) {
        try {
          cache.set(p, readFileSync(join(root, p), 'utf8'));
        } catch {
          cache.set(p, null);
        }
      }
      return cache.get(p);
    },
    readBuffer: (p) => readFileSync(join(root, p)),
  };
}

/** 1-based line of the first match of `re` in `text` (1 when none). */
export function lineOf(text, re) {
  const m = re.exec(text);
  if (!m) return 1;
  return text.slice(0, m.index).split('\n').length;
}

/** `path` against a waiver or filter pattern: exact, or a prefix ending in `*`. */
export function pathMatches(pattern, path) {
  if (pattern === '*') return true;
  return pattern.endsWith('*') ? path.startsWith(pattern.slice(0, -1)) : pattern === path;
}
