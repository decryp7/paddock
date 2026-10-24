/**
 * The Chrome for Testing guard (rule 7: tests never use the installed Google Chrome). Playwright's own Chromium (a
 * Chrome for Testing build, `npx playwright install chromium`) is the browser; a pinned binary may be named explicitly
 * in `CHROMIUM_TEST_PATH` (or `PADDOCK_E2E_CHROME`). An installed Chrome or Edge, by channel or by path, is refused: in the config
 * (`assertChromeForTesting`) and again when a worker starts (the kit's `test`).
 */

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
  readonly launchOptions?: { readonly channel?: string; readonly executablePath?: string };
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
