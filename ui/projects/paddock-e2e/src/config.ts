import { defineConfig, devices, type PlaywrightTestConfig, type ReporterDescription } from '@playwright/test';
import { assertChromeForTesting, pinnedChrome } from './browser.ts';
import { e2ePort } from './env.ts';

/**
 * The Playwright config every Paddock app starts from (plan X39): `export default paddockConfig({ app: 'dyno', portEnv:
 * 'PW_PORT', defaultPort: 4352, serve: (port) => `npx ng serve --port ${port}` })`.
 *
 * - Projects `chromium` and `webkit` (rule 3: a branch runs `--project=chromium`; WebKit runs on the combined result and
 *   in CI), plus `screenshots` only when `SCREENSHOTS` is set, so a plain run never rewrites the documentation images.
 * - The port comes from the environment (one per worktree, rule 2) and is required on CI ({@link e2ePort}).
 * - The browser is Playwright's Chromium (Chrome for Testing), or the binary named in `CHROMIUM_TEST_PATH`; the installed
 *   Chrome is refused (rule 7, {@link assertChromeForTesting}).
 * - `retries: 0` (a flake is fixed, not retried, rule 5), traces kept on failure, one worker at a time, `test.only` refused on
 *   CI, and a running server is reused only when `PW_REUSE_SERVER=1` is set (never on CI; a stale one on the port would be tested instead of the build).
 * - Sharding: run `playwright test --shard=i/n` with `PADDOCK_E2E_BLOB=1`; every shard writes a blob report
 *   (`blob-report/`), and the merge job runs `npx playwright merge-reports --reporter list,html blob-report`.
 */
export interface PaddockConfigOptions {
  /** The app's name (the HTML report title and the blob report's name). */
  readonly app: string;
  /** The environment variable that holds the e2e port (default `PW_PORT`). */
  readonly portEnv?: string;
  /** The port when the variable is unset locally (CI requires the variable). */
  readonly defaultPort: number;
  /** The command that serves the app on `port` (`ng serve`, `vite`, ...); omit when the test starts its own server. */
  readonly serve?: (port: number) => string;
  /** How long the server may take to come up (default 180 s). */
  readonly serveTimeoutMs?: number;
  /** The folder of the specs (default `./e2e`). */
  readonly testDir?: string;
  /** Run the specs in WebKit too (default true; Pitcrew-style apps without a Safari target say false). */
  readonly webkit?: boolean;
  /** The window size of the specs (default 1280 x 800). */
  readonly viewport?: { readonly width: number; readonly height: number };
  /** A test's time limit (default 60 s). */
  readonly timeoutMs?: number;
  /** Specs that only take screenshots (default {@link SCREENSHOT_SPECS}); the other projects ignore them. */
  readonly screenshotSpecs?: RegExp;
  /** Extra reporters (after `list`). */
  readonly reporters?: readonly ReporterDescription[];
  /** The environment (default `process.env`); tests pass their own. */
  readonly env?: Readonly<Record<string, string | undefined>>;
  /** Merged over the defaults for anything else (`use`, `projects`, `webServer`, ...). */
  readonly extend?: PlaywrightTestConfig;
}

/** The default pattern of screenshot-only specs: `screenshots.spec.ts` and `*.shot.ts`. */
export const SCREENSHOT_SPECS = /(?:screenshots\.spec|\.shot)\.ts$/;

const DEFAULT_VIEWPORT = { width: 1280, height: 800 } as const;
/** The documentation screenshots' window: 1440 x 900 at twice the pixel density. */
export const DOC_SCREEN_VIEWPORT = { width: 1440, height: 900 } as const;

/** Whether a variable is switched on (set to anything but empty or `0`). */
export function isOn(value: string | undefined): boolean {
  return value !== undefined && value !== '' && value !== '0';
}

/** The config described at {@link PaddockConfigOptions}. */
export function paddockConfig(options: PaddockConfigOptions): PlaywrightTestConfig {
  const env = options.env ?? process.env;
  const port = e2ePort(options.portEnv ?? 'PW_PORT', options.defaultPort, env);
  const viewport = options.viewport ?? DEFAULT_VIEWPORT;
  const shots = options.screenshotSpecs ?? SCREENSHOT_SPECS;
  const executablePath = pinnedChrome(env);
  const chromium = { ...devices['Desktop Chrome'], viewport, ...(executablePath ? { launchOptions: { executablePath } } : {}) };
  const reporter: PlaywrightTestConfig['reporter'] = isOn(env['PADDOCK_E2E_BLOB'])
    ? [['blob', { outputDir: 'blob-report', fileName: `${options.app}-${env['PADDOCK_E2E_SHARD'] ?? 'all'}.zip` }], ['list']]
    : [['list'], ...(options.reporters ?? [])];
  const config = defineConfig({
    testDir: options.testDir ?? './e2e',
    // Inside .angular/, which git ignores.
    outputDir: './.angular/playwright-results',
    fullyParallel: false,
    workers: 1,
    forbidOnly: isOn(env['CI']),
    retries: 0,
    reporter,
    timeout: options.timeoutMs ?? 60_000,
    use: { baseURL: `http://localhost:${port}`, viewport, trace: 'retain-on-failure' },
    projects: [
      { name: 'chromium', testIgnore: shots, use: chromium },
      ...(options.webkit === false ? [] : [{ name: 'webkit', testIgnore: shots, use: { ...devices['Desktop Safari'], viewport } }]),
      // Only on request, so a plain `npx playwright test` never rewrites the documentation images.
      ...(isOn(env['SCREENSHOTS'])
        ? [{ name: 'screenshots', testMatch: shots, use: { ...chromium, viewport: DOC_SCREEN_VIEWPORT, deviceScaleFactor: 2 } }]
        : []),
    ],
    ...(options.serve
      ? { webServer: { command: options.serve(port), url: `http://localhost:${port}`, reuseExistingServer: !isOn(env['CI']) && isOn(env['PW_REUSE_SERVER']), timeout: options.serveTimeoutMs ?? 180_000 } }
      : {}),
    ...options.extend,
  });
  return assertChromeForTesting(config, env);
}
