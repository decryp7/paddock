// One mechanism against e2e runs on stale code (docs/lessons.md, "Stale servers and stale builds"). Plain CommonJS (Playwright loads
// TypeScript configs as CJS, where import.meta does not exist) with no dependencies, so every Playwright config in the repo (ui and apps) can import it by relative path.
//
// 1. reuseServer(): a running dev server is reused only when PW_REUSE_SERVER=1 is set explicitly (default: never).
// 2. worktreePort(): the default port of a config is derived from the repo root path, so two worktrees do not collide.
// 3. staleBuilds()/assertFreshBuilds(): fail fast (as Playwright globalSetup) when ui/dist or the generated
//    ui/projects/paddock-ui/css/*.css is older than the newest source it is built from.
'use strict';
const { existsSync, readdirSync, statSync } = require('node:fs');
const { join } = require('node:path');

/** The repository root (the parent of scripts/). */
const REPO_ROOT = join(__dirname, '..');
/** The first port of the derived range; a worktree gets a block of {@link PORT_SLOTS} slots above it. */
const PORT_RANGE_START = 20_000;
/** The number of distinct worktree slots (each config adds its own small offset: its default port minus 4300). */
const PORT_SLOTS = 4_000;
/** The command the failure message names. */
const REBUILD_HINT = 'rebuild ui: npm run build -w ui';

/** Whether a running server may be reused: only when `PW_REUSE_SERVER` is switched on (not empty, not `0`). */
function reuseServer(env = process.env) {
  const v = env['PW_REUSE_SERVER'];
  return v !== undefined && v !== '' && v !== '0';
}

/** A stable 32-bit FNV-1a hash of a string. */
function hashString(text) {
  let h = 0x811c9dc5;
  for (const ch of Buffer.from(text, 'utf8')) {
    h ^= ch;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/**
 * The default port of a config whose fixed default is `base` (4300, 4352, ...): the same offset above a worktree block
 * derived from the repo root path. On CI the fixed `base` is returned, so CI stays deterministic (and its jobs name their
 * ports through the existing environment variables anyway).
 */
function worktreePort(base, root = REPO_ROOT, env = process.env) {
  if (env['CI']) return base;
  return PORT_RANGE_START + (hashString(root) % PORT_SLOTS) + (base - 4300);
}

/** The port of a config: the environment variable `name` when set (validated), else {@link worktreePort}. */
function e2ePortFor(name, base, env = process.env, root = REPO_ROOT) {
  const raw = env[name];
  if (raw === undefined || raw === '') return worktreePort(base, root, env);
  const port = Number(raw);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error(`${name} is not a port: ${JSON.stringify(raw)}`);
  return port;
}

const SKIP_DIRS = new Set(['node_modules', '.angular']);
const TEST_FILE = /\.(spec|test)\.[cm]?[jt]s$/;

/** The newest mtime (ms) of the files under `dir` that `keep(relativePath)` accepts; 0 when there are none. */
function newestMtime(dir, keep = () => true, rel = '') {
  let newest = 0;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const e of entries) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) {
      if (SKIP_DIRS.has(e.name)) continue;
      newest = Math.max(newest, newestMtime(join(dir, e.name), keep, r));
    } else if (keep(r)) {
      newest = Math.max(newest, statSync(join(dir, e.name)).mtimeMs);
    }
  }
  return newest;
}

/**
 * What is stale under `uiRoot` (the ui/ folder): a list of problems, empty when fresh.
 * - the generated css/*.css against scss/** (always checked);
 * - ui/dist against everything in projects/paddock-ui except tests (only when `checkDist`, for the apps that consume it).
 */
function staleBuilds(uiRoot = join(REPO_ROOT, 'ui'), { checkDist = false } = {}) {
  const pkg = join(uiRoot, 'projects', 'paddock-ui');
  const problems = [];
  const scss = newestMtime(join(pkg, 'scss'));
  const cssDir = join(pkg, 'css');
  const cssFiles = existsSync(cssDir) ? readdirSync(cssDir).filter((f) => f.endsWith('.css')) : [];
  if (cssFiles.length === 0) problems.push(`${cssDir} has no generated css`);
  for (const f of cssFiles) {
    if (statSync(join(cssDir, f)).mtimeMs < scss) problems.push(`css/${f} is older than scss/**`);
  }
  if (checkDist) {
    const dist = join(uiRoot, 'dist');
    const built = newestMtime(dist);
    const sources = newestMtime(pkg, (r) => !TEST_FILE.test(r));
    if (built === 0) problems.push(`${dist} is missing or empty`);
    else if (built < sources) problems.push('ui/dist is older than ui/projects/paddock-ui/**');
  }
  return problems;
}

/** Throws the fail-fast error when {@link staleBuilds} finds anything; `PW_SKIP_BUILD_GUARD=1` skips the check. */
function assertFreshBuilds(uiRoot, options, env = process.env) {
  if (env['PW_SKIP_BUILD_GUARD'] === '1') return;
  const problems = staleBuilds(uiRoot, options);
  if (problems.length > 0) throw new Error(`${REBUILD_HINT}\nStale build, tests would run against old code:\n- ${problems.join('\n- ')}`);
}

module.exports = { REPO_ROOT, PORT_RANGE_START, PORT_SLOTS, REBUILD_HINT, reuseServer, hashString, worktreePort, e2ePortFor, newestMtime, staleBuilds, assertFreshBuilds };
