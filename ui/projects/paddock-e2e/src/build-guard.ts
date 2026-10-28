import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { isOn } from './env.ts';

/**
 * Fail-fast check that e2e and screenshot runs do not test old code (docs/lessons.md, "Stale servers and stale builds"):
 * `ui/dist` and the generated `ui/projects/paddock-ui/css/*.css` carry a `.source-hash` stamp written by the build
 * ({@link stampBuild}: `npm run build:css`, `npm run build -w ui`); the check recomputes the hash of the sources and
 * compares. A content hash, not mtimes: a rebuild of unchanged sources, a checkout or a touched file never false-fails.
 */

/** The command the failure message names. */
export const REBUILD_HINT = 'rebuild ui: npm run build -w ui';
/** The stamp file inside `css/` and `dist/`. */
export const STAMP_FILE = '.source-hash';
/** The css files `npm run build:css` writes. */
export const CSS_ENTRIES = ['tokens', 'paddock', 'controls', 'material'] as const;

/** What a Playwright config guards: nothing, the generated css (ui), or css and `ui/dist` (an app that maps @decryp7/paddock-ui to it). */
export type BuildGuard = 'none' | 'ui' | 'ui-dist';

// Not sources of the dist: dependencies, caches, the generated css (it has its own stamp), bin/ (CLI scripts and their tests).
const SKIP_DIRS = new Set(['node_modules', '.angular']);
const SKIP_TOP = new Set(['css', 'bin']);
const TEST_FILE = /\.(spec|test)\.[cm]?[jt]s$/;

function walk(dir: string, rel: string, out: string[]): void {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) {
      if (!SKIP_DIRS.has(e.name) && !(rel === '' && SKIP_TOP.has(e.name))) walk(join(dir, e.name), r, out);
    } else if (!e.name.startsWith('.') && !TEST_FILE.test(e.name)) out.push(r);
  }
}

/**
 * The hash of the files a build is made from, under `uiRoot` (the ui/ folder): `scss/**` for the css; for the dist every
 * file under projects/paddock-ui (all entry points, ng-package.json, package.json, tsconfig.lib*.json) except tests, bin/,
 * the generated css/, node_modules and .angular.
 */
export function sourceHash(uiRoot: string, kind: 'css' | 'dist'): string {
  const pkg = join(uiRoot, 'projects', 'paddock-ui');
  const hash = createHash('sha256');
  const files: string[] = [];
  if (kind === 'css') walk(join(pkg, 'scss'), 'scss', files);
  else walk(pkg, '', files);
  for (const f of files) hash.update(`${f}\0`).update(readFileSync(join(pkg, f))).update('\0');
  return hash.digest('hex');
}

/** The folder of a kind's stamp. */
function stampDir(uiRoot: string, kind: 'css' | 'dist'): string {
  return kind === 'css' ? join(uiRoot, 'projects', 'paddock-ui', 'css') : join(uiRoot, 'dist');
}

/** Writes the stamp of a finished build (called by the build scripts); leaves it alone when unchanged. */
export function stampBuild(uiRoot: string, kind: 'css' | 'dist'): void {
  const dir = stampDir(uiRoot, kind);
  mkdirSync(dir, { recursive: true });
  const file = join(dir, STAMP_FILE);
  const hash = sourceHash(uiRoot, kind);
  if (!existsSync(file) || readFileSync(file, 'utf8') !== hash) writeFileSync(file, hash);
}

function stale(uiRoot: string, kind: 'css' | 'dist', what: string): string | null {
  const file = join(stampDir(uiRoot, kind), STAMP_FILE);
  if (!existsSync(file)) return `${what} has no ${STAMP_FILE} (not built from the current sources)`;
  return readFileSync(file, 'utf8') === sourceHash(uiRoot, kind) ? null : `${what} was built from older sources`;
}

/** What is stale under `uiRoot` for `guard`: a list of problems, empty when fresh. */
export function staleBuilds(uiRoot: string, guard: BuildGuard): string[] {
  if (guard === 'none') return [];
  const problems: string[] = [];
  const css = join(uiRoot, 'projects', 'paddock-ui', 'css');
  for (const name of CSS_ENTRIES) if (!existsSync(join(css, `${name}.css`))) problems.push(`css/${name}.css is missing`);
  const cssStale = stale(uiRoot, 'css', 'css/*.css');
  if (cssStale) problems.push(cssStale);
  if (guard === 'ui-dist') {
    const distStale = stale(uiRoot, 'dist', 'ui/dist');
    if (distStale) problems.push(distStale);
  }
  return problems;
}

/** Throws the fail-fast error when {@link staleBuilds} finds anything; `PW_SKIP_BUILD_GUARD=1` skips the check. */
export function assertFreshBuilds(uiRoot: string, guard: BuildGuard, env: Readonly<Record<string, string | undefined>> = process.env): void {
  if (isOn(env['PW_SKIP_BUILD_GUARD'])) return;
  const problems = staleBuilds(uiRoot, guard);
  if (problems.length > 0) throw new Error(`${REBUILD_HINT}\nStale build, tests would run against old code:\n- ${problems.join('\n- ')}`);
}

/** Whether a running server may be reused: only when `PW_REUSE_SERVER` is switched on, and never on CI. */
export function reuseServer(env: Readonly<Record<string, string | undefined>> = process.env): boolean {
  return isOn(env['PW_REUSE_SERVER']) && !isOn(env['CI']);
}

/** The guard an app needs: `ui-dist` when its tsconfig maps @decryp7/paddock-ui to ui/dist, else none (it builds nothing of ui). */
export function appBuildGuardOf(appDir: string): BuildGuard {
  const file = join(appDir, 'tsconfig.json');
  return existsSync(file) && /ui\/dist\/paddock-ui/.test(readFileSync(file, 'utf8')) ? 'ui-dist' : 'none';
}

/** The call of an app's playwright.config.ts: guards what the app consumes (`appDir` is the config's `__dirname`, `uiRoot` the repo's ui/ folder). */
export function appBuildGuard(appDir: string, uiRoot: string, env: Readonly<Record<string, string | undefined>> = process.env): void {
  assertFreshBuilds(uiRoot, appBuildGuardOf(appDir), env);
}

/** The ui workspace's own load-time check: its webServers run `build:css` first, so the css is checked only when a running server is reused. */
export function uiConfigGuard(uiRoot: string, env: Readonly<Record<string, string | undefined>> = process.env): void {
  if (reuseServer(env)) assertFreshBuilds(uiRoot, 'ui', env);
}
