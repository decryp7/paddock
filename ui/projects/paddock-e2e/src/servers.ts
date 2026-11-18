import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { PlaywrightTestConfig } from '@playwright/test';
import type { ServerSpec } from './server-ready.ts';

/**
 * The global setup module next to this file, with this file's own extension: `.ts` from the sources (Playwright and `node --test`
 * run them), `.js` in the published package (the build emits only `.js`).
 */
function globalSetupFile(): string {
  // Playwright loads configs as CommonJS (`__dirname`; `import.meta` is a syntax error there), `node --test` and the built
  // package as ESM (no `__dirname`; the frame of this call names the file by URL).
  let dir: string;
  if (typeof __dirname === 'string') dir = __dirname;
  else {
    const url = /(file:\/\/[^\s)]+?):\d+:\d+/.exec(new Error().stack ?? '')?.[1];
    if (!url) throw new Error('paddock-e2e: cannot locate its own folder for the global setup');
    dir = dirname(fileURLToPath(url));
  }
  for (const ext of ['ts', 'js']) {
    const file = join(dir, `global-setup.${ext}`);
    if (existsSync(file)) return file;
  }
  throw new Error(`paddock-e2e: no global-setup.ts or global-setup.js in ${dir}; the package is incomplete`);
}

/**
 * The servers a run needs, as config fields (`...paddockServers([{ command, url, timeoutMs, reuseExisting }])`). Replaces
 * Playwright's `webServer`: a server that never answers (or ends first) fails the run once at start-up with one `::error::`
 * line naming the URL, the probe, the exit status and the stderr tail, and its output reaches the job log (ER-27, server-ready.ts).
 * Why not `maxFailures`: it would fire after the 2 min limit of its first N tests, and N has no measured basis (ER-12); a
 * readiness check says the cause once, before any test runs.
 */
export function paddockServers(servers: readonly ServerSpec[]): Pick<PlaywrightTestConfig, 'globalSetup' | 'metadata'> {
  return { globalSetup: globalSetupFile(), metadata: { paddockServers: servers } };
}
