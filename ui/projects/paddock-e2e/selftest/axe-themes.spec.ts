import { expect, test } from '@playwright/test';
import { axeViolations } from '../src/index.ts';

// Pitwall run 8134 (WebKit): axe read Benchmarks' muted cells on a row half-way through the theme's 100 ms background
// transition (#a0a7b2 on #5c5d60, 2.71:1), although the transitions had been settled before the scan: the transition started
// while axe ran. Whatever starts it (here the theme flips once axe has begun), colours read mid-transition are not a
// measurement: axeViolations sees that a colour moved during the scan, settles and scans again.
const PAGE = `<!doctype html><html lang="en"><title>Themes</title><style>
    td { background: rgb(255, 255, 255); color: rgb(85, 85, 85); transition: background 3s linear; }
    .dark td { background: rgb(17, 17, 17); color: rgb(160, 160, 160); }
  </style>
  <table><tbody><tr><td>muted text</td><td>more muted text</td></tr></tbody></table></html>`;

test('axeViolations does not report colours read while a transition that began during the scan runs', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(PAGE)}`);
  // The first scan of axe flips the theme before it reads anything.
  await page.evaluate(() => {
    type Run = (...a: unknown[]) => Promise<unknown>;
    const w = window as unknown as { flips: number };
    w.flips = 0;
    let real: object | undefined;
    // axe-core assigns window.axe first and its runPartial later: wrap runPartial whenever it is assigned.
    Object.defineProperty(window, 'axe', {
      configurable: true,
      get: () => real,
      set: (axe: object) => {
        let run: Run | undefined;
        const wrapped: Run = async (...args) => {
          // The text turns light at once, the background follows over 3 s: light on white until then.
          if (w.flips++ === 0) document.documentElement.classList.add('dark');
          return (run as Run)(...args);
        };
        Object.defineProperty(axe, 'runPartial', { configurable: true, get: () => (run ? wrapped : undefined), set: (f: Run) => (run = f) });
        real = axe;
      },
    });
  });
  expect(await axeViolations(page)).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as { flips: number }).flips), 'the probe flipped the theme during a scan').toBeGreaterThan(1);
});

test('axeViolations still reports a real contrast failure', async ({ page }) => {
  await page.goto(`data:text/html,${encodeURIComponent(PAGE.replace('rgb(85, 85, 85)', 'rgb(200, 200, 200)'))}`);
  expect(await axeViolations(page, { idsOnly: true })).toEqual(['color-contrast']);
});
