/**
 * The Chrome for Testing guard (rule 7: tests never use the installed Google Chrome). Playwright's own Chromium (a
 * Chrome for Testing build, `npx playwright install chromium`) is the browser; a pinned binary may be named explicitly
 * in `CHROMIUM_TEST_PATH` (or `PADDOCK_E2E_CHROME`). An installed Chrome or Edge, by channel or by path, is refused: in the config
 * (`assertChromeForTesting`) and again when a worker starts (the kit's `test`).
 */

import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** The variable that may name a pinned browser binary (Chrome for Testing) for `launchOptions.executablePath`. */
export const CHROME_ENV = 'PADDOCK_E2E_CHROME';

/** The same, under the name the apps' CI and docs use; wins over {@link CHROME_ENV} when both are set. */
export const CHROMIUM_TEST_ENV = 'CHROMIUM_TEST_PATH';

/** The pinned browser binary the environment names (Chrome for Testing), or undefined for Playwright's own Chromium. */
export function pinnedChrome(env: Readonly<Record<string, string | undefined>> = process.env): string | undefined {
  return env[CHROMIUM_TEST_ENV] || env[CHROME_ENV] || undefined;
}

/** Playwright channels that launch a browser installed on the machine. */
export const INSTALLED_CHANNELS: readonly string[] = [
  'chrome',
  'chrome-beta',
  'chrome-dev',
  'chrome-canary',
  'msedge',
  'msedge-beta',
  'msedge-dev',
  'msedge-canary',
];

/** Paths of installed Chrome and Edge on macOS, Windows and Linux. */
const INSTALLED_PATHS: readonly RegExp[] = [
  /\/Applications\/Google Chrome[^/]*\.app\//i,
  /\/Applications\/Microsoft Edge[^/]*\.app\//i,
  /[\\/]Google[\\/]Chrome[^\\/]*[\\/]Application[\\/]chrome\.exe$/i,
  /[\\/]Microsoft[\\/]Edge[^\\/]*[\\/]Application[\\/]msedge\.exe$/i,
  /^\/opt\/google\/chrome/i,
  /^\/opt\/microsoft\/msedge/i,
  /^\/usr\/bin\/(google-chrome|microsoft-edge)/i,
];

/** The parts of Playwright's `use` the guard reads. */
export interface BrowserUse {
  readonly channel?: string;
  readonly launchOptions?: {
    readonly channel?: string;
    readonly executablePath?: string;
    readonly downloadsPath?: string;
    readonly tracesDir?: string;
    readonly args?: string[];
    readonly env?: Record<string, string | undefined>;
  };
}

/**
 * Whether `path` is on a removable or external volume. On macOS everything under `/Volumes/` is (the dev machine's
 * `/Volumes/vault-dev` included, though `/` itself shows there as a symlink to the boot disk, which is not matched), and a
 * browser that writes there makes macOS ask the user for access to a removable volume (a TCC window on the desktop, ER-14).
 */
export function isRemovableVolume(path: string): boolean {
  return /^\/Volumes\/[^/]+/.test(path.replace(/\\/g, '/'));
}

/** Switches whose value is a folder the browser writes to. */
const FOLDER_SWITCHES: readonly string[] = ['--user-data-dir=', '--crash-dumps-dir=', '--disk-cache-dir=', '--download-default-directory='];

/** Variables Chrome reads its temp folder from (`MAC_CHROMIUM_TMPDIR` on macOS, which ignores `TMPDIR`; lessons.md, Chrome slow to quit). */
const TEMP_VARIABLES: readonly string[] = ['MAC_CHROMIUM_TMPDIR', 'TMPDIR', 'TMP', 'TEMP'];

/** The internal root for a test browser's writable folders: the job's own temp folder, else the OS temp folder. */
export function browserScratchRoot(env: Readonly<Record<string, string | undefined>> = process.env): string {
  return join(env['PADDOCK_JOB_TEMP'] || tmpdir(), 'paddock-e2e-browser');
}

/**
 * The launch options that keep every folder the test browser writes under the internal temp root (`root`): its temp folder
 * (`MAC_CHROMIUM_TMPDIR`, `TMPDIR`), downloads, traces, crash dumps and disk cache. The profile itself is Playwright's own
 * (a folder in the OS temp folder). Folders are named, not created: the browser and Playwright make them.
 */
export function internalBrowserLaunch(root: string = browserScratchRoot()): NonNullable<BrowserUse['launchOptions']> {
  return {
    env: { MAC_CHROMIUM_TMPDIR: join(root, 'tmp') },
    downloadsPath: join(root, 'downloads'),
    tracesDir: join(root, 'traces'),
    args: [`--crash-dumps-dir=${join(root, 'crashpad')}`, `--disk-cache-dir=${join(root, 'cache')}`],
  };
}

/** The Playwright `use` for a Chromium project that keeps the browser's writable folders on the internal disk (spread it in). */
export function internalBrowserUse(env: Readonly<Record<string, string | undefined>> = process.env): { launchOptions: NonNullable<BrowserUse['launchOptions']> } {
  return { launchOptions: internalBrowserLaunch(browserScratchRoot(env)) };
}

/** Every writable browser folder `launchOptions` names that is on a removable volume. */
export function removableFolderProblems(launchOptions: BrowserUse['launchOptions'], env: Readonly<Record<string, string | undefined>> = process.env): string[] {
  const named: [string, string | undefined][] = [
    ['downloadsPath', launchOptions?.downloadsPath],
    ['tracesDir', launchOptions?.tracesDir],
    ...TEMP_VARIABLES.map((v): [string, string | undefined] => [`env ${v}`, launchOptions?.env?.[v] ?? (launchOptions?.env ? undefined : env[v])]),
    ...(launchOptions?.args ?? []).flatMap((arg): [string, string | undefined][] =>
      FOLDER_SWITCHES.filter((sw) => arg.startsWith(sw)).map((sw): [string, string | undefined] => [sw.slice(0, -1), arg.slice(sw.length)]),
    ),
  ];
  return named
    .filter(([, path]) => path !== undefined && isRemovableVolume(path))
    .map(([what, path]) => `${what} ${path} is on a removable volume; macOS asks the user for access to it (ER-14). Keep it under the job's internal temp folder (internalBrowserLaunch)`);
}

/** Whether `path` is an installed Chrome or Edge. */
export function isInstalledBrowser(path: string): boolean {
  return INSTALLED_PATHS.some((re) => re.test(path));
}

/** What is wrong with the browser `use` would launch (empty: Playwright's Chromium or the pinned binary). */
export function browserProblems(use: BrowserUse | undefined, env: Readonly<Record<string, string | undefined>> = process.env): string[] {
  const problems: string[] = [];
  for (const channel of [use?.channel, use?.launchOptions?.channel]) {
    if (channel && INSTALLED_CHANNELS.includes(channel)) {
      problems.push(`channel "${channel}" launches the installed browser; use Playwright's Chromium (Chrome for Testing) or name a pinned binary in ${CHROMIUM_TEST_ENV}`);
    }
  }
  problems.push(...removableFolderProblems(use?.launchOptions, env));
  const path = use?.launchOptions?.executablePath;
  if (path) {
    if (isInstalledBrowser(path)) problems.push(`executablePath ${path} is the installed browser; tests never use it (rule 7)`);
    else if (path !== pinnedChrome(env)) problems.push(`executablePath ${path} is not the pinned browser named in ${CHROMIUM_TEST_ENV} (or ${CHROME_ENV})`);
  }
  return problems;
}

/** The parts of a Playwright config the guard reads. */
export interface GuardedConfig {
  readonly use?: BrowserUse;
  readonly projects?: readonly { readonly name?: string; readonly use?: BrowserUse }[];
}

/** Throws when the config or one of its projects would launch an installed browser; returns the config otherwise. */
export function assertChromeForTesting<C extends GuardedConfig>(config: C, env: Readonly<Record<string, string | undefined>> = process.env): C {
  const problems = [
    ...browserProblems(config.use, env).map((p) => `config: ${p}`),
    ...(config.projects ?? []).flatMap((project) => browserProblems({ ...config.use, ...project.use }, env).map((p) => `project ${project.name ?? '(unnamed)'}: ${p}`)),
  ];
  if (problems.length) throw new Error(`The e2e would use an installed browser (rule 7):\n- ${[...new Set(problems)].join('\n- ')}`);
  return config;
}
