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
 * The real browser is the pinned binary (`CHROMIUM_TEST_PATH`), else Playwright's own headless shell, found through
 * `playwright install --dry-run` (the public way to ask where it lives; `chromium.executablePath()` names the full Chromium,
 * a different browser). A browser that cannot be found, or a wrapper that cannot be written, stops the config with an `::error::`
 * line: it never falls back to the checkout as the working folder.
 */
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { browserScratchRoot, type BrowserUse, internalBrowserLaunch, isInstalledBrowser, isRemovableVolume, LAUNCHER_PREFIX, pinnedChrome } from './browser.ts';

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

/** The wrapper's text: start in `cwd`, then become the real browser with the arguments as they came. */
export function launcherScript(real: string, cwd: string): string {
  return `#!/bin/sh\n# Written by paddock-e2e (browser-launcher.ts): the browser starts in an internal folder, never the checkout (ER-14).\ncd ${shellQuote(cwd)} || { echo "::error::browser launcher: cannot enter ${cwd.replace(/["\\$`]/g, '_')}" >&2; exit 126; }\nexec ${shellQuote(real)} "$@"\n`;
}

/**
 * Writes the wrapper for `real` into `dir` and returns its path. The name holds a hash of its content, so every worker that
 * writes it writes the same bytes; each writes a private file first and renames it into place (an atomic step), so a worker that
 * starts at the same moment never executes a half-written script. `cwd` is made if it is missing.
 */
export function writeLauncher(dir: string, real: string, cwd: string): string {
  const text = launcherScript(real, cwd);
  const path = join(dir, `${LAUNCHER_PREFIX}${createHash('sha256').update(text).digest('hex').slice(0, 16)}.sh`);
  try {
    mkdirSync(dir, { recursive: true });
    mkdirSync(cwd, { recursive: true });
    if (existsSync(path) && readFileSync(path, 'utf8') === text) return path;
    const own = `${path}.${process.pid}.${Math.random().toString(36).slice(2)}.tmp`;
    writeFileSync(own, text, { mode: 0o755 });
    chmodSync(own, 0o755);
    renameSync(own, path);
    return path;
  } catch (e) {
    throw new LauncherError(`cannot write ${path} (${(e as Error).message})`, 'make the browser scratch folder writable (PADDOCK_JOB_TEMP, or the OS temp folder)');
  }
}

/** What `playwright install --dry-run chromium-headless-shell` prints: the install folder in the headless shell's block. */
export function installLocation(dryRun: string): string | undefined {
  const lines = dryRun.split(/\r?\n/);
  const at = lines.findIndex((l) => /headless shell/i.test(l) || /chromium-headless-shell/.test(l));
  if (at < 0) return undefined;
  for (const line of lines.slice(at + 1)) {
    if (/^\S/.test(line)) break;
    const m = /Install location:\s+(.+?)\s*$/.exec(line);
    if (m) return m[1];
  }
  return undefined;
}

/** The headless shell's executable inside its install folder (one folder down, as Playwright unpacks it), or undefined. */
export function headlessShellIn(location: string, exists: (p: string) => boolean = existsSync, list: (p: string) => string[] = (p) => readdirSync(p)): string | undefined {
  const exe = process.platform === 'win32' ? 'chrome-headless-shell.exe' : 'chrome-headless-shell';
  if (!exists(location)) return undefined;
  return list(location)
    .map((d) => join(location, d, exe))
    .find((p) => exists(p));
}

/** Runs `playwright install --dry-run chromium-headless-shell` with the Playwright found from the current folder upwards (the tests' own), and returns what it printed. */
function dryRun(env: Readonly<Record<string, string | undefined>>): string {
  const cli = join(dirname(createRequire(join(process.cwd(), 'noop.js')).resolve('playwright/package.json')), 'cli.js');
  const r = spawnSync(process.execPath, [cli, 'install', '--dry-run', 'chromium-headless-shell'], { env: env as NodeJS.ProcessEnv, encoding: 'utf8', timeout: 60_000 });
  if (r.status !== 0) throw new Error(`exit ${r.status}: ${(r.stderr || r.error?.message || '').trim().slice(0, 300)}`);
  return r.stdout;
}

/** The real browser the wrapper starts: the pinned binary, else Playwright's headless shell. Throws a {@link LauncherError} when there is none. */
export function realBrowser(env: Readonly<Record<string, string | undefined>> = process.env, ask: (env: Readonly<Record<string, string | undefined>>) => string = dryRun): string {
  const pinned = pinnedChrome(env);
  if (pinned) {
    if (isInstalledBrowser(pinned)) throw new LauncherError(`${pinned} is the installed browser (rule 7)`, 'name a Chrome for Testing build in CHROMIUM_TEST_PATH');
    return pinned;
  }
  let out: string;
  try {
    out = ask(env);
  } catch (e) {
    throw new LauncherError(`Playwright could not say where its headless shell is (${(e as Error).message})`, 'run `npx playwright install chromium-headless-shell`, or name a pinned browser in CHROMIUM_TEST_PATH');
  }
  const location = installLocation(out);
  const exe = location ? headlessShellIn(location) : undefined;
  if (!exe) {
    throw new LauncherError(`the Playwright headless shell is not installed${location ? ` in ${location}` : ''}`, 'run `npx playwright install chromium-headless-shell` (PLAYWRIGHT_BROWSERS_PATH names the folder), or name a pinned browser in CHROMIUM_TEST_PATH');
  }
  return exe;
}

/**
 * The wrapper to launch the test browser through: written into `root`, starting the browser in `root/cwd`. `undefined` where the
 * wrapper is not needed (not macOS) and when `root` is on a removable volume (nothing is written there; the guard in browser.ts then
 * refuses the folder with its own message).
 */
export function browserLauncher(
  root: string,
  env: Readonly<Record<string, string | undefined>> = process.env,
  platform: string = process.platform,
  isRemovable: (path: string) => boolean = isRemovableVolume,
  ask?: (env: Readonly<Record<string, string | undefined>>) => string,
): string | undefined {
  if (platform !== 'darwin' || isRemovable(root)) return undefined;
  return writeLauncher(join(root, 'launcher'), realBrowser(env, ask), join(root, 'cwd'));
}

/**
 * The Playwright `use` for a Chromium project: the browser's writable folders on the internal disk ({@link internalBrowserLaunch}) and,
 * on macOS, the wrapper as its `executablePath` so that it also starts in an internal folder. Spread it into the project's `use`.
 * Throws a {@link LauncherError} when the wrapper is needed and cannot be made.
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
