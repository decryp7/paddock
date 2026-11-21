/**
 * The browser launcher: a small wrapper that starts the test browser in an internal folder (ER-14).
 *
 * Playwright starts the browser with no `cwd` (playwright-core `launchProcess`), so it runs in the folder `playwright test` was
 * started in, which on the dev machine is the checkout on an external volume, and macOS then asks the user for access to a
 * removable volume. `launchOptions` has no `cwd`, so the browser's `executablePath` is a wrapper that does
 * `cd "<internal folder>" && exec "<the real browser>" "$@"` (`exec`, so Playwright's pid is the browser's). Only macOS has the
 * prompt, so only macOS gets the wrapper; Linux and Windows run the real browser as before (a Windows `.cmd` cannot be spawned
 * without a shell, which Playwright does not use, so there is none).
 *
 * The wrapper is written when the config loads and finds the real browser only when a browser starts (a config for WebKit only, or
 * with no Chromium installed, loads without a browser): the pinned binary (`CHROMIUM_TEST_PATH`, baked in), else by the arguments
 * Playwright gives it, the headless shell for `--headless` (found with `playwright install --dry-run`, the public way to ask where
 * it lives) and the full Chromium (`chromium.executablePath()`) for a headed run. The answer is kept in a file of the scratch root
 * (per Playwright version and browsers folder), so only the first launch pays the 0.4 s of the dry run. A browser that cannot be
 * found, or a wrapper that cannot be written, prints an `::error::` line with the cause and the next step; the launch never falls
 * back to the checkout as the working folder.
 */
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { browserScratchRoot, type BrowserUse, internalBrowserLaunch, isInstalledBrowser, isRemovableVolume, LAUNCHER_PREFIX, pinnedChrome, registerLauncher } from './browser.ts';

/** A launcher problem, worded as a CI error annotation: the cause and the next step. */
export class LauncherError extends Error {
  constructor(cause: string, next: string) {
    super(`::error::browser launcher: ${cause}. Next: ${next}`);
    this.name = 'LauncherError';
  }
}

/** A string as one POSIX shell word: single quotes, a quote inside one closed and reopened. */
export function shellQuote(text: string): string {
  return `'${text.replace(/'/g, `'\\''`)}'`;
}

/** What the wrapper needs to find the real browser when it starts. */
export interface LauncherSpec {
  /** The folder the browser starts in. */
  readonly cwd: string;
  /** The named browser (`CHROMIUM_TEST_PATH`): started as it is, in every mode. */
  readonly pinned?: string;
  /** Where the answer of a look for Playwright's own browser is kept (a folder; made when needed). */
  readonly cache: string;
  /** The Node program and Playwright's `cli.js` and `playwright-core` entry file, by absolute path. */
  readonly node: string;
  readonly cli: string;
  readonly core: string;
}

const FAIL = (cause: string, next: string) => `{ echo "::error::browser launcher: ${cause}. Next: ${next}" >&2; exit 127; }`;

/** The wrapper's text. Not a function of the environment: what it needs is in `spec`, so one name (a hash of the text) is one wrapper. */
export function launcherScript(spec: LauncherSpec): string {
  const enter = `cd ${shellQuote(spec.cwd)} || { echo "::error::browser launcher: cannot enter the internal folder. Next: make PADDOCK_JOB_TEMP (or the OS temp folder) writable" >&2; exit 126; }`;
  const head = `#!/bin/sh\n# Written by paddock-e2e (browser-launcher.ts): the browser starts in an internal folder, never the checkout (ER-14).\n${enter}\n`;
  if (spec.pinned) return `${head}exec ${shellQuote(spec.pinned)} "$@"\n`;
  return `${head}mode=chromium
for a in "$@"; do
  case "$a" in --headless|--headless=old) mode=headless-shell ;; esac
done
cache=${shellQuote(spec.cache)}
real=$(cat "$cache/$mode" 2>/dev/null)
if [ ! -x "$real" ]; then
  if [ "$mode" = headless-shell ]; then
    out=$(${shellQuote(spec.node)} ${shellQuote(spec.cli)} install --dry-run chromium-headless-shell 2>&1) || ${FAIL('Playwright could not say where its headless shell is', 'run npx playwright install chromium-headless-shell, or name a pinned browser in CHROMIUM_TEST_PATH')}
    loc=$(printf '%s\\n' "$out" | sed -n 's/^[[:space:]]*Install location:[[:space:]]*//p' | head -n 1)
    real=$(ls -d "$loc"/*/chrome-headless-shell 2>/dev/null | head -n 1)
  else
    real=$(${shellQuote(spec.node)} -e 'console.log(require(process.argv[1]).chromium.executablePath())' ${shellQuote(spec.core)} 2>/dev/null)
  fi
  [ -x "$real" ] || ${FAIL('the Playwright $mode is not installed ("$real")', 'run npx playwright install chromium (headed) or npx playwright install chromium-headless-shell (PLAYWRIGHT_BROWSERS_PATH names the folder), or name a pinned browser in CHROMIUM_TEST_PATH')}
  mkdir -p "$cache" && printf '%s\\n' "$real" > "$cache/$mode.$$" && mv "$cache/$mode.$$" "$cache/$mode"
fi
exec "$real" "$@"
`;
}

/**
 * Writes the wrapper for `spec` into `dir` and returns its path. The name holds a hash of its content, so every worker that writes
 * it writes the same bytes; each writes a private file first and renames it into place (an atomic step), so a worker that starts
 * at the same moment never executes a half-written script. `spec.cwd` is made if it is missing.
 */
export function writeLauncher(dir: string, spec: LauncherSpec): string {
  const text = launcherScript(spec);
  const path = join(dir, `${LAUNCHER_PREFIX}${createHash('sha256').update(text).digest('hex').slice(0, 16)}.sh`);
  try {
    mkdirSync(dir, { recursive: true });
    mkdirSync(spec.cwd, { recursive: true });
    if (!(existsSync(path) && readFileSync(path, 'utf8') === text)) {
      const own = `${path}.${process.pid}.${Math.random().toString(36).slice(2)}.tmp`;
      writeFileSync(own, text, { mode: 0o755 });
      chmodSync(own, 0o755);
      renameSync(own, path);
    }
  } catch (e) {
    throw new LauncherError(`cannot write ${path} (${(e as Error).message})`, 'make the browser scratch folder writable (PADDOCK_JOB_TEMP, or the OS temp folder)');
  }
  registerLauncher(path);
  return path;
}

/** The Playwright files and the cache key of the Playwright this process runs under (found from the current folder upwards, as the tests' own). */
export type PlaywrightFiles = Pick<LauncherSpec, 'node' | 'cli' | 'core'> & { readonly key: string };

function playwrightFiles(env: Readonly<Record<string, string | undefined>>): PlaywrightFiles {
  try {
    const here = createRequire(join(process.cwd(), 'noop.js'));
    const dir = dirname(here.resolve('playwright/package.json'));
    const version = (JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) as { version?: string }).version ?? '?';
    const core = createRequire(join(dir, 'noop.js')).resolve('playwright-core');
    const key = createHash('sha256').update(`${version}\n${env['PLAYWRIGHT_BROWSERS_PATH'] ?? ''}`).digest('hex').slice(0, 12);
    return { node: process.execPath, cli: join(dir, 'cli.js'), core, key };
  } catch (e) {
    throw new LauncherError(`Playwright was not found from ${process.cwd()} (${(e as Error).message})`, 'run the tests from inside the repository after `npm ci`');
  }
}

/**
 * The wrapper to launch the test browser through: written into `root`, starting the browser in `root/cwd`. `undefined` where the
 * wrapper is not needed (not macOS) and when `root` is on a removable volume (nothing is written there; the guard in browser.ts then
 * refuses the folder with its own message). Nothing about the browser is looked up here (see the file's comment).
 */
export function browserLauncher(
  root: string,
  env: Readonly<Record<string, string | undefined>> = process.env,
  platform: string = process.platform,
  isRemovable: (path: string) => boolean = isRemovableVolume,
  files?: PlaywrightFiles,
): string | undefined {
  if (platform !== 'darwin' || isRemovable(root)) return undefined;
  const pinned = pinnedChrome(env);
  if (pinned && isInstalledBrowser(pinned)) throw new LauncherError(`${pinned} is the installed browser (rule 7)`, 'name a Chrome for Testing build in CHROMIUM_TEST_PATH');
  const found: PlaywrightFiles = pinned ? { node: '', cli: '', core: '', key: 'pinned' } : (files ?? playwrightFiles(env));
  return writeLauncher(join(root, 'launcher'), { cwd: join(root, 'cwd'), cache: join(root, 'browsers', found.key), pinned, node: found.node, cli: found.cli, core: found.core });
}

/**
 * The Playwright `use` for a Chromium project: the browser's writable folders on the internal disk ({@link internalBrowserLaunch}) and,
 * on macOS, the wrapper as its `executablePath` so that it also starts in an internal folder. Spread it into the project's `use`.
 * It looks for no browser (see {@link browserLauncher}), so a config for WebKit only loads without Chromium.
 */
export function internalBrowserUse(
  env: Readonly<Record<string, string | undefined>> = process.env,
  launcher: 'auto' | 'off' = 'auto',
): { launchOptions: NonNullable<BrowserUse['launchOptions']> } {
  const root = browserScratchRoot(env);
  // Without the wrapper the pinned browser, when one is named, is launched directly as before.
  const executablePath = (launcher === 'off' ? undefined : browserLauncher(root, env)) ?? pinnedChrome(env);
  return { launchOptions: { ...internalBrowserLaunch(root, env), ...(executablePath ? { executablePath } : {}) } };
}
