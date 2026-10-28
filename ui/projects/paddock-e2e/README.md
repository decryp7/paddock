# @decryp7/paddock-e2e

The Playwright half of the Paddock shell test kit (ADR 0012 §7, plan S16; the TestBed half is
`@decryp7/paddock-ui/shell/testing`). A plain Node package (ES modules with type declarations), versioned with
`@decryp7/paddock-ui`. Peer dependencies: `@playwright/test` and `@axe-core/playwright`.

The accent colour check is the unit-test half of accessibility: `accentProblems(cssRules(tokensCss), 'myaccent')` in `@decryp7/paddock-ui/shell`
(contrast of the accent as text, focus ring and fill, the text on the fill, colour-vision simulation, distance from the warning hue and,
if asked, from other accents and the status colours). Conformance check C33 requires both halves.

```ts
// playwright.config.ts
import { paddockConfig } from '@decryp7/paddock-e2e';

export default paddockConfig({
  app: 'myapp',
  portEnv: 'MYAPP_E2E_PORT', // required on CI, one per worktree
  defaultPort: 4300,
  serve: (port) => `npx ng serve --port ${port}`,
});
```

`paddockConfig` is the whole shared part: projects `chromium` and `webkit` (a branch runs `--project=chromium`, rule 3), a
`screenshots` project only when `SCREENSHOTS=1`, `retries: 0`, a trace kept on failure, one worker, the browser guard, the
port from the environment, and the web server. `CHROMIUM_TEST_PATH` names a pinned Chrome for Testing. Sharding: run
`PADDOCK_E2E_SHARD=1-of-4 PADDOCK_E2E_BLOB=1 npx playwright test --shard=1/4` in each job (blob report in `blob-report/`), then
`npx playwright merge-reports --reporter list,html blob-report` in the merge job. Anything else goes in `extend`
(`use`, extra `projects`, `webServer`, ...); `reporters` adds the retried reporter after `list`.

```ts
// e2e/screenshots.spec.ts: the documentation screenshots, run with `SCREENSHOTS=1 npx playwright test --project=screenshots`
import { captureDocScreens } from '@decryp7/paddock-e2e';

captureDocScreens({
  appSelector: 'app-shell',
  screens: [
    { name: 'dashboard', url: '/?latency=0#/', ready: async (page) => { /* open what the image documents */ } },
    { name: 'report', url: '/#/report', frame: 'page' }, // a long page is shown whole
    { name: 'tray', url: '/#/tray', size: { width: 440, height: 660 }, themes: ['dark'] },
  ],
});
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
| `paddockConfig(options)` | The shared Playwright config (see above); `SCREENSHOT_SPECS` is the pattern of screenshot-only specs (`screenshots.spec.ts`, `*.shot.ts`) |
| `captureDocScreens(options)` | The documentation screenshot runner: screens x both themes at 1440 x 900 @2x, a frozen clock (2026-10-22 14:30 UTC), UTC and `en-GB`, reduced motion, settled frames, written to `docs/screenshots/<theme>/<name>.png` (`layout: 'flat'` keeps `<name>-<theme>.png`) with `hashes.json` (SHA-256 per image, merged) beside them for the review gate (C24). `planShots`, `recordHashes`, `sha256` are the pure parts |
| `assertChromeForTesting(config)` | Throws when the config or a project would launch the installed Chrome or Edge (by channel or path, rule 7). Playwright's Chromium is Chrome for Testing; a pinned binary may be named in `CHROMIUM_TEST_PATH` (or the older `PADDOCK_E2E_CHROME`) |
| `e2ePort(name, fallback)` | The port from the environment; on CI a missing one is an error |
| `ShellApp` and its parts | `frame` (heading, toggle, sidebar mode, regions, `focusBy`), `sidebar`, `statusBar` (items by id, chip, bell, jobs, "…"), `notifications` (toasts, centre, notices by cause, service details), `jobs`, `menus`, `dialogs` (window URLs, result, fit history, in-page dialogs), `settings` |
| `dialogContracts(path)` | The dialogs of an app's contract file, to boot every dialog window by URL |
| `axeViolations(page)`, `THEMES`, `WCAG_TAGS` | axe against WCAG 2.2 AA, as `rule: targets` lines |
| `axeInThemes(page)`, `axeTargets(page, targets)` | axe in each theme (`theme: rule: targets`); or over a list of windows/screens (URL plus a ready check), returned by name so a failure names the window |
| `axeEveryDialog(page, contractsPath, { inputs, skip })`, `dialogTargets`, `dialogWindowUrl` | axe over the window of every dialog in the app's `dialog-contracts.json`, both themes: `expect(await axeEveryDialog(page, path)).toEqual({})`. A new dialog is covered the moment it is in the file; one that cannot open alone is named in `skip` with a reason; an empty list is an error |
| `tabWalk`, `expectTabOrder`, `expectReachableByKeyboard`, `expectEscapeReturnsFocus` | Keyboard-only flows (WCAG 2.1.1, 2.1.2, 2.4.3, 2.4.7): walk the Tab order (ends `left`, `cycled`, `trapped` or `limit`; `walkStep` is the pure rule), fail on a trap or a stop with no visible focus, prove named elements are reachable, prove Escape closes a popup and returns the focus to its trigger |
| `LAYOUT_WIDTHS`, `WINDOW_SIZES`, `expectNoHorizontalScroll`, `expectWithinWidth`, `resize` | Layout scenarios (rule 18) |
| `installTauriFake(page, options)` | A fake `__TAURI_INTERNALS__`: replies by command, held until released, recorded calls, events delivered to the page's listeners, a played platform (`mac` has the native menu bar). The shell plugin's commands (`plugin:paddock-shell\|<command>`, `SHELL_PLUGIN_PREFIX`) are recorded and answered by their plain names |
| `waitForStableFrame`, `captureStable` | Screenshots once the fonts are loaded, nothing animates and the layout held still between two looks (works under `page.clock`) |
| `readVersion(path)`, `RELEASE_VERSION` | The app's version from its single source (`package.json` or `Cargo.toml`), checked for a release-number shape (rule 6) |
| `@decryp7/paddock-e2e/retried-reporter` | Lists tests that passed only after a retry (console, `playwright-retried.md`, `$GITHUB_STEP_SUMMARY`); never changes the exit code |

Build: `npm run build:e2e` in `ui/` (writes `ui/dist/paddock-e2e`). Its unit tests run with `npm run test:scripts`; the screenshot runner is
also run for real on `data:` pages: `npx playwright test -c projects/paddock-e2e/selftest/playwright.config.ts --project=screenshots`.

## Screenshots and layout checks (`capture.ts`)

`captureScreen(page, path, { fullPage?: 'viewport' | 'main', scrollTop?, allow?, busy? })` is the only way to take a review or
documentation image of an app page: it waits until nothing is loading (`pd-skeleton`, `.skeleton`, `[aria-busy=true]`), fonts are
ready, every `img` is decoded, no animation runs in main and the layout holds for two looks; it scrolls only the main scroller,
fails when the frame scrolled, an element is cut at the bottom edge (rows cut by an inner scroller, such as a virtual list, are its own business), or an opaque, empty cover sits over the app (any CSS colour, pseudo-elements, fixed and sticky bars; `allow` never excuses a cover). To shoot a running state ("Still working"), emulate `reducedMotion: 'reduce'` or name the spinner in `animated`. A whole page (`fullPage: 'main'`) grows the window up to `FRAME_MAX_HEIGHT`. `captureDocScreens` uses it, so its screens are held to the same rules. `expectFullyVisible(locator)`
(for primary actions) and `expectPageFillsHost(page, { page?, fill? })` (the real page component fills the host, no rendered virtual viewport has 0 height; `fill: false` for content-high pages) are the assertions behind it; both poll until the layout holds. See [docs/plans/shell-adoption-checklist.md](../../../docs/plans/shell-adoption-checklist.md).
