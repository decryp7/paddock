# @decryp7/paddock-e2e

The Playwright half of the Paddock shell test kit (ADR 0012 §7, plan S16; the TestBed half is
`@decryp7/paddock-ui/shell/testing`). A plain Node package (ES modules with type declarations), versioned with
`@decryp7/paddock-ui`. Peer dependencies: `@playwright/test` and `@axe-core/playwright`.

```ts
// playwright.config.ts
import { defineConfig, devices } from '@playwright/test';
import { assertChromeForTesting, e2ePort } from '@decryp7/paddock-e2e';

const PORT = e2ePort('MYAPP_E2E_PORT', 4300); // required on CI
export default assertChromeForTesting(
  defineConfig({
    reporter: [['list'], ['@decryp7/paddock-e2e/retried-reporter']],
    use: { ...devices['Desktop Chrome'], baseURL: `http://localhost:${PORT}` },
    // ...
  }),
);
```

```ts
// e2e/home.spec.ts
import { axeViolations, expect, test, THEMES } from '@decryp7/paddock-e2e';

for (const colorScheme of THEMES) {
  test(`home (${colorScheme})`, async ({ app, page, pageErrors }) => {
    await page.emulateMedia({ colorScheme });
    await app.frame.open('#/home');
    await app.statusBar.jobs.click();
    await expect(app.jobs.items).toHaveCount(0);
    expect(await axeViolations(page)).toEqual([]);
    expect(pageErrors).toEqual([]);
  });
}
```

| Export | What it is |
| --- | --- |
| `test`, `expect` | Playwright's, with the fixtures `app` (page objects), `pageErrors` (uncaught errors; `test.use({ consoleErrors: true })` adds console errors) and a worker check that refuses an installed browser |
| `assertChromeForTesting(config)` | Throws when the config or a project would launch the installed Chrome or Edge (by channel or path, rule 7). Playwright's Chromium is Chrome for Testing; a pinned binary may be named in `PADDOCK_E2E_CHROME` |
| `e2ePort(name, fallback)` | The port from the environment; on CI a missing one is an error |
| `ShellApp` and its parts | `frame` (heading, toggle, sidebar mode, regions, `focusBy`), `sidebar`, `statusBar` (items by id, chip, bell, jobs, "…"), `notifications` (toasts, centre, notices by cause, service details), `jobs`, `menus`, `dialogs` (window URLs, result, fit history, in-page dialogs), `settings` |
| `dialogContracts(path)` | The dialogs of an app's contract file, to boot every dialog window by URL |
| `axeViolations(page)`, `THEMES`, `WCAG_TAGS` | axe against WCAG 2.2 AA, as `rule: targets` lines |
| `LAYOUT_WIDTHS`, `WINDOW_SIZES`, `expectNoHorizontalScroll`, `expectWithinWidth`, `resize` | Layout scenarios (rule 18) |
| `installTauriFake(page, options)` | A fake `__TAURI_INTERNALS__`: replies by command, held until released, recorded calls, events delivered to the page's listeners, a played platform (`mac` has the native menu bar) |
| `waitForStableFrame`, `captureStable` | Screenshots once the fonts are loaded, nothing animates and the layout held still between two looks (works under `page.clock`) |
| `@decryp7/paddock-e2e/retried-reporter` | Lists tests that passed only after a retry (console, `playwright-retried.md`, `$GITHUB_STEP_SUMMARY`); never changes the exit code |

Build: `npm run build:e2e` in `ui/` (writes `ui/dist/paddock-e2e`). Its unit tests run with `npm run test:scripts`.
