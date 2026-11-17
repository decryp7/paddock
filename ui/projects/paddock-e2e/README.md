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

## A fixed page clock for every spec: `pinnedClockTest`

A mock backend stamps its items relative to the page clock and the app groups them by local calendar day, so a run just after
midnight had no "Today" (CI run 8154). `pinnedClockTest(base, options)` returns `base` with the page clock installed before the
first navigation of every context, at one fixed instant (default 2026-10-22 14:30 UTC, the instant of the documentation screenshots), in one zone (`UTC`) and locale
(`en-US`). The clock ticks on from there, so timers and animation frames keep working. Wrap once per app and export the result
as the app's `test`; specs import `test` and `expect` from that file, never from `@playwright/test`.

```ts
// e2e/test.ts
import { expect, test as base } from '@playwright/test'; // or the kit's `test` from '@decryp7/paddock-e2e'
import { pinnedClockTest } from '@decryp7/paddock-e2e';

export const test = pinnedClockTest(base, { now: '2026-10-22T14:30:00Z', timezoneId: 'UTC', locale: 'en-US' });
export { expect };
```

- Options are all optional: `now` (a `Date`, epoch milliseconds or ISO string), `timezoneId`, `locale`. The constants
  `PINNED_NOW`, `PINNED_ZONE` and `PINNED_LOCALE` hold the defaults.
- `PADDOCK_E2E_NOW=<ISO instant>` moves the pin for a whole run (for example `2026-11-16T00:19:00Z` to see which specs depend on the
  time of day). An unreadable value throws, naming the variable. `pinnedInstant(options)` reads the pin the same way.
- A spec that needs the real time says so: `test.use({ pinnedClock: false })` in the file or a `describe`, with a one-line reason
  next to it. The zone and locale defaults stay then.
- A spec that installs its own clock still can: a second `page.clock.install({ time })` or `setFixedTime` wins over the pin
  (Playwright 1.64.1, measured on Chromium and WebKit). Drop a bare `page.clock.install()` instead, because it restarts the
  clock at the real time; the pinned clock is already installed and `runFor` works on it.
- What the fake clock changes (measured on Playwright 1.64.1, Chromium and WebKit, 2026-11-16): timers and `requestAnimationFrame` keep
  running and CSS transitions are not affected, but a `setTimeout(0)` takes about twice as long (Chromium 8 ms against 3.8 ms, WebKit 13
  against 6), so a spec that reads the page right after an action must wait on the condition. `performance.now` and
  `requestAnimationFrame` are the fake ones, so a spec that measures real frame cost opts out (`test.use({ pinnedClock: false })`, with
  its reason on the line above). Playwright's clock also catches an exception thrown by a timer callback (`setTimeout`, `setInterval`,
  `requestAnimationFrame`, `requestIdleCallback`): `_updateRealTimeTimer` in its `clock.ts` ends in `.catch(console.error)` and `_runTo`
  keeps only the first error of a batch, so the page saw no `error` event and no `pageerror`. The fixture fixes that: after the install
  it wraps the four timer functions so each callback's exception goes to `reportError`, as in a real browser (`reportTimerErrors`,
  selftest `pinned-clock.spec.ts`). A `pageErrors` guard or an error handler sees a timer's throw again.
- It covers contexts the fixtures create (`page`, `context`). A context made with `browser.newContext()` is not pinned: a spec that needs a
  context of its own (a window size, a user agent, a colour scheme) calls `newPinnedContext(browser, { ...contextOptions, pin })`, which installs the
  same clock, zone, locale and timer-error reporting (`pin: false` is the plain context). A conformance check over the e2e specs flags a bare `browser.newContext(`.

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
| `captureDocScreens(options)` | The documentation screenshot runner: screens x both themes at 1440 x 900 @2x, a frozen clock (2026-10-22 14:30 UTC), UTC and `en-GB`, reduced motion, settled frames, written to `docs/screenshots/<theme>/<name>.png` (`layout: 'flat'` keeps `<name>-<theme>.png`) with `hashes.json` (SHA-256 per image, merged) beside them for the review gate (C24). Before every picture it stops the page's live updates (`pauseLive`, option `pauseLiveBeforeShot`, default on), turns animations off and hides the caret; `alignToBucketMs` runs the clock to the middle of a time bucket first (for a mock that rounds its polled replies to that bucket, as Dyno's `?stable=60000`), so two runs of one tree are byte-identical (Dyno: 92 pictures, measured 2026-11-16). A screen can set `before` (steps before the page opens), `transparent` (a window on nothing) and `element`/`clip`. `planShots`, `recordHashes`, `sha256` are the pure parts |
| `assertChromeForTesting(config)` | Throws when the config or a project would launch the installed Chrome or Edge (by channel or path, rule 7). Playwright's Chromium is Chrome for Testing; a pinned binary may be named in `CHROMIUM_TEST_PATH` (or the older `PADDOCK_E2E_CHROME`) |
| `e2ePort(name, fallback)` | The port from the environment; on CI a missing one is an error |
| `ShellApp` and its parts | `frame` (heading, toggle, sidebar mode, regions, `focusBy`), `sidebar`, `statusBar` (items by id, chip, bell, jobs, "…"), `notifications` (toasts, centre, notices by cause, service details), `jobs`, `menus`, `dialogs` (window URLs, result, fit history, in-page dialogs), `settings` |
| `dialogContracts(path)` | The dialogs of an app's contract file, to boot every dialog window by URL |
| `axeViolations(page)`, `THEMES`, `WCAG_TAGS` | axe against WCAG 2.2 AA, as `rule: targets` lines; the page is held still for the scan (no transitions and finite animations at their end on every element and pseudo-element, reduced motion emulated, both undone after); `axeInThemes` and `axeTargets` wait until the switched colour scheme is live before scanning |
| `axeInThemes(page)`, `axeTargets(page, targets)` | axe in each theme (`theme: rule: targets`); or over a list of windows/screens (URL plus a ready check), returned by name so a failure names the window |
| `axeEveryDialog(page, contractsPath, { inputs, skip })`, `dialogTargets`, `dialogWindowUrl` | axe over the window of every dialog in the app's `dialog-contracts.json`, both themes: `expect(await axeEveryDialog(page, path)).toEqual({})`. A new dialog is covered the moment it is in the file; one that cannot open alone is named in `skip` with a reason; an empty list is an error |
| `tabWalk`, `expectTabOrder`, `expectReachableByKeyboard`, `expectEscapeReturnsFocus` | Keyboard-only flows (WCAG 2.1.1, 2.1.2, 2.4.3, 2.4.7): walk the Tab order (ends `left`, `cycled`, `trapped` or `limit`; `walkStep` is the pure rule), fail on a trap or a stop with no visible focus, prove named elements are reachable, prove Escape closes a popup and returns the focus to its trigger |
| `LAYOUT_WIDTHS`, `WINDOW_SIZES`, `expectNoHorizontalScroll`, `expectWithinWidth`, `resize` | Layout scenarios (rule 18) |
| `expectHeaderOnTop(grid, parts?)`, `headerCovers`, `isOpaqueColor` | A grid's sticky header stays on top: scrolls the body (or the page the header sticks to), hovers the row under the header and probes `elementFromPoint` across the header row; fails on a cover, a translucent header or a body that never scrolled (ADR 0027) |
| `installTauriFake(page, options)` | A fake `__TAURI_INTERNALS__`: replies by command, held until released, recorded calls, events delivered to the page's listeners, a played platform (`mac` has the native menu bar). The shell plugin's commands (`plugin:paddock-shell\|<command>`, `SHELL_PLUGIN_PREFIX`) are recorded and answered by their plain names |
| `waitForStableFrame`, `captureStable` | Screenshots once the fonts are loaded, nothing animates and the layout held still between two looks (works under `page.clock`) |
| `startFrameCost(page)`, `stopFrameCost(page)` | Frame cost while something happens (a drag): per frame, from its `requestAnimationFrame` callback to the first task after rendering; the sampler is a measurement, not a wait (C71) |
| `readVersion(path)`, `RELEASE_VERSION` | The app's version from its single source (`package.json` or `Cargo.toml`), checked for a release-number shape (rule 6) |
| `@decryp7/paddock-e2e/retried-reporter` | Lists tests that passed only after a retry (console, `playwright-retried.md`, `$GITHUB_STEP_SUMMARY`); never changes the exit code |

Build: `npm run build:e2e` in `ui/` (writes `ui/dist/paddock-e2e`). Its unit tests run with `npm run test:scripts`; the screenshot runner is
also run for real on `data:` pages: `npx playwright test -c projects/paddock-e2e/selftest/playwright.config.ts --project=screenshots`, and the
axe helpers in both engines with `--project=chromium --project=webkit`.
