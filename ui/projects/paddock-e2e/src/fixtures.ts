import { test as base, expect } from '@playwright/test';
import { browserProblems, type BrowserUse } from './browser.ts';
import { ShellApp } from './shell.ts';

/** The kit's fixtures besides Playwright's. */
export interface PaddockFixtures {
  /** The window's page objects. */
  readonly app: ShellApp;
  /** The page's uncaught errors (and, with `consoleErrors`, its console errors), collected from the start. */
  readonly pageErrors: string[];
  /** Whether `pageErrors` also collects `console.error` messages (default false). */
  readonly consoleErrors: boolean;
}

/**
 * Playwright's `test` with the kit's fixtures, and a worker check that refuses an installed browser before any test
 * runs (rule 7): every e2e of an app imports `test` and `expect` from here.
 */
export const test = base.extend<PaddockFixtures, { readonly paddockBrowserGuard: void }>({
  paddockBrowserGuard: [
    // eslint-disable-next-line no-empty-pattern -- Playwright reads the fixtures a function needs from its first parameter, which must be a destructuring pattern
    async ({}, use, workerInfo) => {
      const problems = browserProblems(workerInfo.project.use as BrowserUse);
      if (problems.length) throw new Error(`Refusing the installed browser (rule 7):\n- ${problems.join('\n- ')}`);
      await use();
    },
    { scope: 'worker', auto: true },
  ],
  app: async ({ page }, use) => {
    await use(new ShellApp(page));
  },
  consoleErrors: [false, { option: true }],
  pageErrors: async ({ page, consoleErrors }, use) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    if (consoleErrors) page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
    await use(errors);
  },
});

export { expect };
